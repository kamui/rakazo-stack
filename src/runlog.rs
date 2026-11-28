//! Run logs (spec §6): one file in the Instance folder's `logs/` per run of `setup`,
//! `update`, `backup` and `restore`. It starts with a header, every line has a UTC time,
//! each command the program runs is logged with its exit code and output, and every line
//! goes through [`Redactions`] on its way to the file.
//!
//! The run's lines wait in memory until [`keep`] names the Instance folder, so a run that
//! stops before it may write anything, such as setup on a rejected key, leaves no file.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::docker::Docker;
use crate::platform::Platform;
use crate::redact::Redactions;
use crate::self_update;

pub const DIR: &str = "logs";
/// The newest run logs kept for each command (spec §21).
pub const KEPT_PER_COMMAND: usize = 20;
/// The last lines of each stream of a command that the log keeps.
const KEPT_LINES: usize = 200;
/// Longer arguments, such as a Node script, are cut short in the logged command line.
const LONGEST_ARGUMENT: usize = 120;

static LOG: Mutex<Option<Log>> = Mutex::new(None);

struct Log {
    command: &'static str,
    platform: Platform,
    started: u64,
    pending: Vec<String>,
    file: Option<File>,
    redactions: Redactions,
}

impl Log {
    fn record(&mut self, text: &str) {
        let stamp = utc_time(now());
        let lines: Vec<&str> = if text.is_empty() {
            vec![""]
        } else {
            text.lines().collect()
        };
        for line in lines {
            let line = format!("{stamp} {line}");
            match &self.file {
                // A failed log write never stops the run.
                Some(file) => {
                    let _ = writeln!(&*file, "{}", self.redactions.apply(&line));
                }
                None => self.pending.push(line),
            }
        }
    }

    /// Writes the header, timed at the start, then the lines so far.
    fn keep(&mut self, folder: &Path, header: Vec<String>) -> io::Result<()> {
        self.redactions.learn_folder(folder);
        let dir = folder.join(DIR);
        let name = format!("{}-{}.log", self.command, utc_stamp(self.started));
        let file = create_private(&dir, &name)?;
        let started = utc_time(self.started);
        let invoked: Vec<String> = std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let header = std::iter::once(format!("rakazo-stack {}", invoked.join(" ")))
            .chain(header)
            .chain([format!("Started: {started}")]);
        for line in header {
            let _ = writeln!(
                &file,
                "{}",
                self.redactions.apply(&format!("{started} {line}"))
            );
        }
        for line in self.pending.drain(..) {
            let _ = writeln!(&file, "{}", self.redactions.apply(&line));
        }
        self.file = Some(file);
        prune(
            &dir,
            &format!("{}-", self.command),
            ".log",
            KEPT_PER_COMMAND,
        );
        Ok(())
    }
}

fn log() -> MutexGuard<'static, Option<Log>> {
    LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Starts the run's log in memory, already replacing what the Instance folder holds and
/// the home folder it sits in.
pub fn begin(command: &'static str, platform: Platform, folder: &Path) {
    *log() = Some(Log {
        command,
        platform,
        started: now(),
        pending: Vec::new(),
        file: None,
        redactions: Redactions::for_folder(folder),
    });
}

/// From here on the run writes `logs/<command>-<UTC time>.log` in the Instance folder,
/// starting with its header and every line so far. Later calls change nothing. The
/// header's Docker version is read here, so a run that never writes its log never asks
/// Docker anything for it.
pub fn keep(folder: &Path) -> io::Result<()> {
    let mut log = log();
    match log.as_mut() {
        Some(log) if log.file.is_none() => {
            let header = header(log.platform);
            log.keep(folder, header)
        }
        _ => Ok(()),
    }
}

/// Adds values to replace in the log, such as a key typed this run.
pub fn learn(add: impl FnOnce(&mut Redactions)) {
    if let Some(log) = log().as_mut() {
        add(&mut log.redactions);
    }
}

/// `println!` that also logs the line.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::runlog::say(&format!($($arg)*))
    };
}

/// Prints a line and logs it.
pub fn say(line: &str) {
    println!("{line}");
    record(line);
}

/// Prints a line on standard error and logs it.
pub fn warn(line: &str) {
    eprintln!("{line}");
    record(line);
}

fn record(line: &str) {
    if let Some(log) = log().as_mut() {
        log.record(line);
    }
}

/// The program, the OS and Docker, for a run log's header and `rakazo-stack logs`.
pub fn header(platform: Platform) -> Vec<String> {
    vec![
        format!(
            "Program: rakazo-stack {} ({})",
            self_update::own_tag(),
            env!("CARGO_PKG_VERSION")
        ),
        format!("OS: {}", os(platform)),
        format!("Docker: {}", docker_version(platform)),
    ]
}

fn os(platform: Platform) -> String {
    let first_line = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    match platform {
        Platform::MacOs => {
            let version = first_line("sw_vers", &["-productVersion"]).unwrap_or_default();
            let build = first_line("sw_vers", &["-buildVersion"]).unwrap_or_default();
            format!("macOS {version} ({build}), {}", std::env::consts::ARCH)
        }
        Platform::Linux | Platform::Wsl => {
            let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
            let name = release
                .lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
                .map_or("Linux", |name| name.trim_matches('"'));
            let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
            let wsl = if platform == Platform::Wsl {
                " in WSL 2"
            } else {
                ""
            };
            format!("{name}{wsl}, kernel {}", kernel.trim())
        }
    }
}

fn docker_version(platform: Platform) -> String {
    let Some(docker) = Docker::find(platform) else {
        return "no Docker CLI was found".into();
    };
    match docker
        .command(["version", "--format", "{{json .}}"])
        .output()
    {
        Ok(out) => describe_docker(&String::from_utf8_lossy(&out.stdout)),
        Err(err) => format!("`docker version` didn't run: {err}"),
    }
}

/// From `docker version --format '{{json .}}'`, which still prints the client half when
/// the engine doesn't answer.
fn describe_docker(json: &str) -> String {
    #[derive(Deserialize)]
    struct Version {
        #[serde(rename = "Client")]
        client: Option<Part>,
        #[serde(rename = "Server")]
        server: Option<Part>,
    }
    #[derive(Deserialize)]
    struct Part {
        #[serde(rename = "Version")]
        version: String,
        #[serde(rename = "Platform")]
        platform: Option<Name>,
    }
    #[derive(Deserialize)]
    struct Name {
        #[serde(rename = "Name")]
        name: String,
    }
    let Ok(version) = serde_json::from_str::<Version>(json.trim()) else {
        return "`docker version` gave no version".into();
    };
    let client = version
        .client
        .map_or_else(|| "unknown".to_string(), |part| part.version);
    match version.server {
        Some(server) => {
            let name = server
                .platform
                .map(|p| format!(" ({})", p.name))
                .unwrap_or_default();
            format!("client {client}, engine {}{name}", server.version)
        }
        None => format!("client {client}, engine not answering"),
    }
}

/// Runs a command with its command line, exit code and output in the run log.
pub trait Logged {
    /// As `output()`. Standard error is logged; standard output, which the program reads
    /// as data, only by its size.
    fn logged_output(&mut self) -> io::Result<Output>;
    /// As `status()`, with both streams shown as they come and logged too.
    fn logged_status(&mut self) -> io::Result<ExitStatus>;
}

impl Logged for Command {
    fn logged_output(&mut self) -> io::Result<Output> {
        let running = Running::start(self);
        let output = self.output();
        match &output {
            Ok(out) => running.finish(Ok(out.status), Stdout::Data(out.stdout.len()), &out.stderr),
            Err(err) => running.finish(Err(err), Stdout::Elsewhere, b""),
        }
        output
    }

    fn logged_status(&mut self) -> io::Result<ExitStatus> {
        self.stdout(Stdio::piped()).stderr(Stdio::piped());
        let (mut child, running) = spawn(self)?;
        let shown = tee(child.stdout.take().expect("stdout is piped"), io::stdout);
        let errors = tee(child.stderr.take().expect("stderr is piped"), io::stderr);
        let status = child.wait();
        let shown = shown.join().unwrap_or_default();
        let errors = errors.join().unwrap_or_default();
        running.finish(status.as_ref().copied(), Stdout::Shown(&shown), &errors);
        status
    }
}

/// Copies a child's stream to ours as it comes, and returns all of it.
fn tee<W: Write + 'static>(
    mut from: impl Read + Send + 'static,
    to: fn() -> W,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut all = Vec::new();
        let mut buffer = [0; 8192];
        while let Ok(read) = from.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let mut to = to();
            let _ = to.write_all(&buffer[..read]);
            let _ = to.flush();
            all.extend_from_slice(&buffer[..read]);
        }
        all
    })
}

/// Spawns a command the caller feeds, reads and waits for; [`Running::finish`] logs its end.
pub fn spawn(command: &mut Command) -> io::Result<(Child, Running)> {
    let running = Running::start(command);
    match command.spawn() {
        Ok(child) => Ok((child, running)),
        Err(err) => {
            running.finish(Err(&err), Stdout::Elsewhere, b"");
            Err(err)
        }
    }
}

/// What became of a command's standard output.
pub enum Stdout<'a> {
    /// The Self-hoster saw it, so the log keeps it.
    Shown(&'a [u8]),
    /// The program read this many bytes as data.
    Data(usize),
    /// It went to a file, a pipe the program reads, or nowhere.
    Elsewhere,
}

/// A command the caller spawns itself, logged when it starts and when it ends.
pub struct Running {
    started: Instant,
}

impl Running {
    pub fn start(command: &Command) -> Self {
        if let Some(log) = log().as_mut() {
            let line = format!("$ {}", command_line(command, &log.redactions));
            log.record(&line);
        }
        Self {
            started: Instant::now(),
        }
    }

    pub fn finish(self, status: Result<ExitStatus, &io::Error>, stdout: Stdout, stderr: &[u8]) {
        if let Stdout::Shown(shown) = stdout {
            record_stream("out", shown);
        }
        record_stream("err", stderr);
        if let Stdout::Data(bytes) = stdout
            && bytes > 0
        {
            record(&format!(
                "  out: {bytes} bytes the program read, not logged"
            ));
        }
        let seconds = self.started.elapsed().as_secs_f64();
        match status {
            Ok(status) => record(&format!("  {status} after {seconds:.1} s")),
            Err(err) => record(&format!("  couldn't run: {err}")),
        }
    }
}

fn record_stream(label: &str, bytes: &[u8]) {
    let (left_out, lines) = last_lines(bytes, KEPT_LINES);
    if left_out > 0 {
        record(&format!("  {label}: … {left_out} earlier lines left out"));
    }
    for line in lines {
        record(&format!("  {label}: {line}"));
    }
}

/// The last `keep` lines, each as a terminal leaves it after any carriage return, and how
/// many came before them.
fn last_lines(bytes: &[u8], keep: usize) -> (usize, Vec<String>) {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.rsplit('\r').next().unwrap_or(line).trim_end())
        .collect();
    let lines: Vec<&str> = match lines.iter().rposition(|line| !line.is_empty()) {
        Some(last) => lines[..=last].to_vec(),
        None => Vec::new(),
    };
    let left_out = lines.len().saturating_sub(keep);
    (
        left_out,
        lines[left_out..].iter().map(|l| l.to_string()).collect(),
    )
}

/// Each argument is replaced before it is cut short, so no cut leaves part of a value.
fn command_line(command: &Command, redactions: &Redactions) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| shown_argument(&redactions.apply(&arg.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shown_argument(arg: &str) -> String {
    let first_line = arg.lines().next().unwrap_or_default();
    if arg.chars().count() > LONGEST_ARGUMENT || first_line.len() < arg.len() {
        let start: String = first_line.chars().take(40).collect();
        return format!("'{start}…' ({} characters)", arg.chars().count());
    }
    if arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || "'\"\\$`".contains(c)) {
        format!("'{}'", arg.replace('\'', "'\\''"))
    } else {
        arg.to_string()
    }
}

/// Creates the folder at 700 and the file in it at 600, appending if it exists.
pub fn create_private(dir: &Path, name: &str) -> io::Result<File> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(dir.join(name))
}

/// Deletes all but the newest `keep` files named `<prefix><UTC time><suffix>`.
pub fn prune(dir: &Path, prefix: &str, suffix: &str, keep: usize) {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .is_some_and(is_utc_stamp)
        })
        .collect();
    names.sort();
    let old = names.len().saturating_sub(keep);
    for name in &names[..old] {
        let _ = fs::remove_file(dir.join(name));
    }
}

fn is_utc_stamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 16
        && bytes[8] == b'T'
        && bytes[15] == b'Z'
        && bytes[..8]
            .iter()
            .chain(&bytes[9..15])
            .all(u8::is_ascii_digit)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `YYYYMMDDTHHMMSSZ`, for file names.
pub fn utc_stamp(seconds: u64) -> String {
    let [year, month, day, hour, minute, second] = civil(seconds);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// `YYYY-MM-DDTHH:MM:SSZ`, for log lines.
pub fn utc_time(seconds: u64) -> String {
    let [year, month, day, hour, minute, second] = civil(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// From Howard Hinnant's `civil_from_days`.
fn civil(seconds: u64) -> [i64; 6] {
    let days = (seconds / 86_400) as i64;
    let rest = (seconds % 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    [year, month, day, rest / 3_600, rest % 3_600 / 60, rest % 60]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::Password;
    use crate::backup_file::Passphrase;

    /// Held by each test that sets the global log, so none clears another's mid-test.
    static GLOBAL_LOG: Mutex<()> = Mutex::new(());

    fn global_log() -> MutexGuard<'static, ()> {
        GLOBAL_LOG
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn log(command: &'static str) -> Log {
        Log {
            command,
            platform: Platform::Linux,
            started: 1_795_694_461,
            pending: Vec::new(),
            file: None,
            redactions: Redactions::default(),
        }
    }

    fn line_text(line: &str) -> &str {
        let (stamp, text) = line.split_once(' ').unwrap();
        assert_eq!(stamp.len(), 20, "{line}");
        assert!(
            stamp.ends_with('Z') && stamp.as_bytes()[10] == b'T',
            "{line}"
        );
        text
    }

    #[test]
    fn stamps_utc_times() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(951_782_400), "20000229T000000Z");
        assert_eq!(utc_stamp(1_795_694_461), "20261126T120101Z");
        assert_eq!(utc_time(1_795_694_461), "2026-11-26T12:01:01Z");
    }

    #[test]
    fn writes_private_logs_with_every_line_timed_and_replaced() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(
            folder.path().join("secrets.env"),
            "LEMONADE_API_KEY=aaaabbbbccccddddeeeeffff00001111\n",
        )
        .unwrap();
        let mut log = log("backup");
        log.record("before the folder: aaaabbbbccccddddeeeeffff00001111");
        log.keep(folder.path(), vec!["Program: placeholder".into()])
            .unwrap();
        log.redactions.secret("TYPED", "typed-placeholder");
        log.record("after: typed-placeholder\nand a second line");
        log.record("");

        let dir = folder.path().join(DIR);
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let path = dir.join("backup-20261126T120101Z.log");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let text = fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = text.lines().map(line_text).collect();
        assert!(
            text.starts_with("2026-11-26T12:01:01Z rakazo-stack "),
            "{text}"
        );
        assert_eq!(
            &lines[1..],
            [
                "Program: placeholder",
                "Started: 2026-11-26T12:01:01Z",
                "before the folder: <LEMONADE_API_KEY>",
                "after: <TYPED>",
                "and a second line",
                "",
            ]
        );
    }

    #[test]
    fn a_failed_command_logs_its_command_line_exit_code_and_output() {
        let folder = tempfile::tempdir().unwrap();
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "echo shown; echo data >&2; echo failing step >&2; exit 3",
        ]);
        // Through the global log, as the program runs it.
        let _global = global_log();
        *super::log() = Some(log("setup"));
        let status = command.logged_status().unwrap();
        let mut quiet = Command::new("sh");
        quiet
            .args(["-c", "printf '%s\\n' \"$DATA\""])
            .env("DATA", "{\"Name\":\"read as data\"}");
        let out = quiet.logged_output().unwrap();
        keep(folder.path()).unwrap();
        *super::log() = None;
        assert_eq!(status.code(), Some(3));
        assert_eq!(out.stdout, b"{\"Name\":\"read as data\"}\n");

        let text =
            fs::read_to_string(folder.path().join(DIR).join("setup-20261126T120101Z.log")).unwrap();
        // Other tests may log their own commands between these lines.
        let mut lines = text.lines().map(line_text);
        for expected in [
            "$ sh -c 'echo shown; echo data >&2; echo failing step >&2; exit 3'",
            "  out: shown",
            "  err: data",
            "  err: failing step",
            "  exit status: 3 after ",
            "$ sh -c 'printf '\\''%s\\n'\\'' \"$DATA\"'",
            "  out: 24 bytes the program read, not logged",
            "  exit status: 0 after ",
        ] {
            assert!(
                lines.any(|line| line.starts_with(expected)),
                "{expected} in order in:\n{text}"
            );
        }
        assert!(!text.contains("read as data"), "{text}");
    }

    #[test]
    fn replaces_the_owner_password_and_backup_passphrase_once_the_program_has_them() {
        let folder = tempfile::tempdir().unwrap();
        let _global = global_log();
        *super::log() = Some(log("restore"));
        let _password = Password::new("password-placeholder-0001".into()).unwrap();
        let _passphrase = Passphrase::new("passphrase-placeholder".into()).unwrap();
        say("typed: password-placeholder-0001");
        say("typed: passphrase-placeholder");
        keep(folder.path()).unwrap();
        *super::log() = None;

        let text = fs::read_to_string(folder.path().join(DIR).join("restore-20261126T120101Z.log"))
            .unwrap();
        assert!(text.contains("typed: <Owner password>"), "{text}");
        assert!(text.contains("typed: <Backup passphrase>"), "{text}");
        assert!(!text.contains("placeholder-0001"), "{text}");
        assert!(!text.contains("passphrase-placeholder"), "{text}");
    }

    #[test]
    fn keeps_the_last_lines_as_a_terminal_shows_them() {
        assert_eq!(
            last_lines(b"a\nb\r\n 10%\r 50%\r100%\n\n", 2),
            (1, vec!["b".to_string(), "100%".to_string()])
        );
        assert_eq!(last_lines(b"", 5), (0, Vec::new()));
    }

    #[test]
    fn shows_arguments_as_a_shell_would_and_cuts_scripts_short() {
        let mut command = Command::new("docker");
        command.args([
            "compose",
            "exec",
            "-T",
            "api",
            "node",
            "-e",
            "let a = 1;\nrun(a);",
            "it's",
        ]);
        assert_eq!(
            command_line(&command, &Redactions::default()),
            "docker compose exec -T api node -e 'let a = 1;…' (18 characters) 'it'\\''s'"
        );
    }

    #[test]
    fn replaces_the_home_folder_before_cutting_an_argument_short() {
        let root = tempfile::tempdir().unwrap();
        let home = root
            .path()
            .join("a-self-hoster-whose-placeholder-name-is-long");
        let folder = home.join("rakazo-stack");
        let plist = format!(
            "{}/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist",
            home.display()
        );
        assert!(plist.chars().count() > LONGEST_ARGUMENT);
        let _global = global_log();
        begin("setup", Platform::Linux, &folder);
        // Before the log is kept, as setup's key checks run.
        Command::new("true").arg(&plist).logged_output().unwrap();
        keep(&folder).unwrap();
        *super::log() = None;

        let log = fs::read_dir(folder.join(DIR)).unwrap().next().unwrap();
        let text = fs::read_to_string(log.unwrap().path()).unwrap();
        assert!(
            text.contains(
                "$ true ~/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist"
            ),
            "{text}"
        );
        assert!(!text.contains("a-self-hoster"), "{text}");
    }

    #[test]
    fn keeps_the_newest_logs_of_each_command() {
        let dir = tempfile::tempdir().unwrap();
        for day in 10..35 {
            fs::write(
                dir.path().join(format!("setup-202611{day:02}T000000Z.log")),
                "",
            )
            .unwrap();
        }
        fs::write(dir.path().join("update-20261101T000000Z.log"), "").unwrap();
        fs::write(dir.path().join("lemond.log"), "").unwrap();
        prune(dir.path(), "setup-", ".log", KEPT_PER_COMMAND);
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), KEPT_PER_COMMAND + 2);
        assert_eq!(names[0], "lemond.log");
        assert_eq!(names[1], "setup-20261115T000000Z.log");
        assert!(names.contains(&"update-20261101T000000Z.log".to_string()));
    }

    #[test]
    fn describes_docker_with_and_without_its_engine() {
        let up = r#"{"Client":{"Version":"28.1.1"},"Server":{"Version":"28.1.1","Platform":{"Name":"Docker Desktop 4.43.2 (199162)"}}}"#;
        assert_eq!(
            describe_docker(up),
            "client 28.1.1, engine 28.1.1 (Docker Desktop 4.43.2 (199162))"
        );
        let down = r#"{"Client":{"Version":"28.1.1"},"Server":null}"#;
        assert_eq!(describe_docker(down), "client 28.1.1, engine not answering");
    }
}
