//! macOS: the Instance's own `lemond`, run as a LaunchAgent (spec §9, "macOS: native
//! `lemond`"). Which release runs, the download and its SHA-256 check, the plist, the
//! `launchctl` steps, the port check and the health check. The decisions are plain
//! functions; only `Lemond`'s methods and `port_holder` touch the Mac.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::checklist::Checklist;
use crate::github;
use crate::lemonade::{LEMONADE_REPO, LemonadeRelease};
use crate::runlog::RunLog;
use crate::voice;

/// A loopback port of the stack's own, so it can't collide with an upstream Lemonade on
/// 13305. lemond exits rather than pick another one (spec §9).
pub const PORT: u16 = 13315;
const HOST: &str = "127.0.0.1";
/// The relay reaches the Mac's loopback through Docker Desktop's host name.
pub const UPSTREAM_HOST: &str = "host.docker.internal";
/// `--no-broadcast` already stops the discovery beacon; the rest match Linux (spec §9).
const CONFIG: [&str; 3] = [
    "whispercpp.backend=metal",
    "auto_evict=false",
    "auto_check_model_updates=false",
];
const LAUNCHD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const LOG_FILE: &str = "logs/lemond.log";
const LOG_LINES: usize = 10;
const TARBALL_LIMIT: u64 = 256 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const HEALTH_ATTEMPTS: u32 = 30;
const HEALTH_PAUSE: Duration = Duration::from_secs(2);
/// `bootstrap` right after `bootout` can fail while launchd still tears the old job down.
const BOOTSTRAP_ATTEMPTS: u32 = 5;

/// The LaunchAgent's one fixed label. It follows the Compose project, so a test Instance
/// on a Mac never replaces the real Instance's LaunchAgent.
pub fn label(project: &str) -> String {
    format!("com.github.kamui.{project}.lemond")
}

/// `LEMONADE_UPSTREAM` in `.env` (spec §5).
pub fn upstream() -> String {
    format!("http://{UPSTREAM_HOST}:{PORT}")
}

/// Release `vX.Y.Z` publishes `lemonade-embeddable-X.Y.Z-macos-arm64.tar.gz`.
pub fn tarball_name(tag: &str) -> String {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    format!("lemonade-embeddable-{version}-macos-arm64.tar.gz")
}

pub fn download_url(tag: &str) -> String {
    format!(
        "https://github.com/{LEMONADE_REPO}/releases/download/{tag}/{}",
        tarball_name(tag)
    )
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    /// GitHub's own `sha256:<hex>` of the uploaded file.
    digest: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TarballResolution {
    /// The tag, and the tarball's SHA-256 as `sha256:<hex>`.
    pub release: LemonadeRelease,
    /// Newer releases without a published macOS tarball.
    pub skipped: Vec<String>,
}

/// The newest stable release whose macOS tarball is published with a SHA-256.
pub fn choose_tarball(releases_json: &str) -> Result<TarballResolution, String> {
    let tags = github::stable_tags(releases_json).map_err(|err| err.to_string())?;
    let releases: Vec<Release> =
        serde_json::from_str(releases_json).map_err(|err| err.to_string())?;
    let mut skipped = Vec::new();
    for tag in tags {
        let name = tarball_name(&tag);
        let digest = releases
            .iter()
            .filter(|release| release.tag_name == tag)
            .flat_map(|release| &release.assets)
            .find(|asset| asset.name == name)
            .and_then(|asset| asset.digest.clone())
            .filter(|digest| is_sha256(digest));
        match digest {
            Some(digest) => {
                return Ok(TarballResolution {
                    release: LemonadeRelease { tag, digest },
                    skipped,
                });
            }
            None => skipped.push(tag),
        }
    }
    Err("no Lemonade release has a published macOS tarball".to_string())
}

pub fn describe_skipped(tag: &str) -> String {
    format!(
        "Skipped Lemonade {tag}: its macOS tarball isn't published yet. A later `rakazo-stack update` picks it up once it is."
    )
}

fn is_sha256(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Resolves the release on a first run (spec §4).
pub fn resolve(agent: &ureq::Agent, log: &RunLog) -> Result<LemonadeRelease, String> {
    let json = github::fetch_releases(agent, LEMONADE_REPO)
        .map_err(|err| format!("Couldn't check Lemonade's releases: {err}"))?;
    let resolution = choose_tarball(&String::from_utf8_lossy(&json))
        .map_err(|err| format!("Couldn't pick a Lemonade release: {err}"))?;
    for tag in &resolution.skipped {
        log.say(&describe_skipped(tag));
    }
    Ok(resolution.release)
}

/// Refuses a tarball whose SHA-256 isn't the one GitHub published (`sha256:<hex>`).
pub fn verify(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = format!("sha256:{}", sha256_hex(bytes));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "the Lemonade download's SHA-256 ({actual}) isn't the {expected} GitHub published, so it wasn't used"
        ))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `tar` arguments that unpack the tarball's one top folder into `into`. macOS's bsdtar
/// refuses absolute paths and `..` by default.
pub fn unpack_args(tarball: &Path, into: &Path) -> Vec<String> {
    vec![
        "-xzf".into(),
        tarball.display().to_string(),
        "-C".into(),
        into.display().to_string(),
        "--strip-components".into(),
        "1".into(),
    ]
}

/// Unpacks into a hidden folder beside `version_dir` and renames it into place, so a
/// version folder that exists is always complete.
fn unpack(tarball: &Path, version_dir: &Path) -> Result<(), String> {
    let parent = version_dir.parent().expect("a version folder has a parent");
    let name = version_dir
        .file_name()
        .expect("a version folder has a name")
        .to_string_lossy();
    let partial = parent.join(format!(".{name}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&partial).map_err(|err| err.to_string())?;
    let out = command("tar")
        .args(unpack_args(tarball, &partial))
        .output()
        .map_err(|err| format!("tar: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar couldn't unpack it: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    if !partial.join("lemond").is_file() {
        return Err("the tarball has no lemond".to_string());
    }
    fs::rename(&partial, version_dir).map_err(|err| err.to_string())
}

/// Where everything of the Instance's lemond lives (spec §6).
pub struct Layout {
    pub home: PathBuf,
    pub instance: PathBuf,
    pub label: String,
}

impl Layout {
    /// lemond's working folder: `config.json`, its backends and its models.
    pub fn lemonade_dir(&self) -> PathBuf {
        self.instance.join("lemonade")
    }

    pub fn version_dir(&self, tag: &str) -> PathBuf {
        self.lemonade_dir().join("versions").join(tag)
    }

    pub fn hf_home(&self) -> PathBuf {
        self.lemonade_dir().join("models")
    }

    pub fn log(&self) -> PathBuf {
        self.instance.join(LOG_FILE)
    }

    pub fn plist(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", self.label))
    }

    /// The LaunchAgent's command (spec §9). No `--convert`, `-t` or `metal_args`.
    pub fn program_arguments(&self, tag: &str) -> Vec<String> {
        vec![
            self.version_dir(tag).join("lemond").display().to_string(),
            "--no-broadcast".into(),
            "--host".into(),
            HOST.into(),
            "--port".into(),
            PORT.to_string(),
            self.lemonade_dir().display().to_string(),
        ]
    }

    /// The LaunchAgent's plist. It holds the key, so it is written with mode 600.
    pub fn plist_xml(&self, tag: &str, key: &str) -> String {
        let string = |value: &str| format!("<string>{}</string>", xml_escape(value));
        let arguments: String = self
            .program_arguments(tag)
            .iter()
            .map(|arg| format!("\n\t\t{}", string(arg)))
            .collect();
        let hf_home = self.hf_home().display().to_string();
        let log = self.log().display().to_string();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	{label}
	<key>ProgramArguments</key>
	<array>{arguments}
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		{path}
		<key>HF_HOME</key>
		{hf_home}
		<key>LEMONADE_API_KEY</key>
		{key}
	</dict>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>StandardOutPath</key>
	{log}
	<key>StandardErrorPath</key>
	{log}
</dict>
</plist>
"#,
            label = string(&self.label),
            path = string(LAUNCHD_PATH),
            hf_home = string(&hf_home),
            key = string(key),
            log = string(&log),
        )
    }

    /// `lemonade config set` against this lemond, never the default port an upstream
    /// Lemonade uses. The key goes in the environment, not the arguments.
    pub fn config_arguments(&self) -> Vec<String> {
        let mut args: Vec<String> = ["--host", HOST, "--port"]
            .into_iter()
            .map(str::to_string)
            .collect();
        args.push(PORT.to_string());
        args.extend(["config", "set"].map(str::to_string));
        args.extend(CONFIG.map(str::to_string));
        args
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn domain(uid: u32) -> String {
    format!("gui/{uid}")
}

pub fn service(uid: u32, label: &str) -> String {
    format!("gui/{uid}/{label}")
}

/// The `launchctl` calls that bring the LaunchAgent to the plist just written: load it
/// when it isn't loaded, reload it only when the plist changed. Never `unload -w`.
pub fn load_plan(loaded: bool, changed: bool, uid: u32, layout: &Layout) -> Vec<Vec<String>> {
    let bootstrap = vec![
        "bootstrap".to_string(),
        domain(uid),
        layout.plist().display().to_string(),
    ];
    match (loaded, changed) {
        (false, _) => vec![bootstrap],
        (true, true) => vec![
            vec!["bootout".to_string(), service(uid, &layout.label)],
            bootstrap,
        ],
        (true, false) => Vec::new(),
    }
}

/// The running process in `launchctl print gui/<uid>/<label>`; none while launchd waits
/// to respawn it.
pub fn printed_pid(output: &str) -> Option<u32> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = "))
        .and_then(|pid| pid.trim().parse().ok())
}

/// Whether `launchctl print-disabled gui/<uid>` lists the label as disabled, as turning
/// it off in Login Items does. Older macOS prints `true` for disabled.
pub fn is_disabled(output: &str, label: &str) -> bool {
    let quoted = format!("\"{label}\"");
    output.lines().any(|line| {
        line.trim()
            .strip_prefix(&quoted)
            .and_then(|rest| rest.trim().strip_prefix("=>"))
            .is_some_and(|state| matches!(state.trim(), "disabled" | "true"))
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Listener {
    pub pid: u32,
    pub command: String,
}

/// `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fpc`: a `p<pid>` line, then `c<command>`.
pub fn parse_lsof(output: &str) -> Vec<Listener> {
    let mut listeners: Vec<Listener> = Vec::new();
    for line in output.lines() {
        if let Some(pid) = line.strip_prefix('p').and_then(|pid| pid.parse().ok()) {
            listeners.push(Listener {
                pid,
                command: String::new(),
            });
        } else if let (Some(command), Some(last)) = (line.strip_prefix('c'), listeners.last_mut()) {
            last.command = command.to_string();
        }
    }
    listeners
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortHolder {
    Free,
    /// The Instance's own lemond, so a re-run carries on.
    Lemond,
    /// Something else, named for the stop.
    Other(String),
}

/// lemond's port is free, held by the LaunchAgent's own process, or taken.
pub fn decide_port(listening: bool, listeners: &[Listener], lemond_pid: Option<u32>) -> PortHolder {
    if !listening && listeners.is_empty() {
        return PortHolder::Free;
    }
    let others: Vec<String> = listeners
        .iter()
        .filter(|listener| Some(listener.pid) != lemond_pid)
        .map(|listener| format!("{} (process {})", listener.command, listener.pid))
        .collect();
    match (others.is_empty(), listeners.is_empty()) {
        (true, false) => PortHolder::Lemond,
        (true, true) => {
            PortHolder::Other("a process rakazo-stack can't see, perhaps another user's".into())
        }
        (false, _) => PortHolder::Other(others.join(", ")),
    }
}

/// Preflight's look at lemond's port on this Mac (spec §3).
pub fn port_holder(project: &str) -> PortHolder {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], PORT));
    let listening = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok();
    let lsof = command_stdout(
        "lsof",
        &[
            "-nP".into(),
            format!("-iTCP:{PORT}"),
            "-sTCP:LISTEN".into(),
            "-Fpc".into(),
        ],
    )
    .unwrap_or_default();
    let lemond_pid = launchctl_print(uid(), &label(project)).and_then(|out| printed_pid(&out));
    decide_port(listening, &parse_lsof(&lsof), lemond_pid)
}

/// The last lines of lemond's log for a Checklist item, with the key taken out in case
/// lemond ever writes it.
pub fn last_log_lines(log: &str, key: &str) -> Vec<String> {
    let lines: Vec<&str> = log.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(LOG_LINES)..]
        .iter()
        .map(|line| line.replace(key, "<LEMONADE_API_KEY>"))
        .collect()
}

pub fn turned_off_item(label: &str) -> String {
    format!(
        "Voice isn't set up: the Instance's lemond ({label}) is turned off in Login Items. Turn it on in System Settings → General → Login Items & Extensions, under Allow in the Background, then run `rakazo-stack setup` again."
    )
}

pub fn failing_item(label: &str, reason: &str, log_lines: &[String]) -> String {
    let mut item = format!(
        "Voice isn't set up: the Instance's lemond ({label}) {reason}. If it is off in System Settings → General → Login Items & Extensions, turn it on; then run `rakazo-stack setup` again."
    );
    if log_lines.is_empty() {
        item.push_str(&format!(" {LOG_FILE} in ~/rakazo-stack is empty."));
    } else {
        item.push_str(&format!(" The last lines of ~/rakazo-stack/{LOG_FILE}:"));
        for line in log_lines {
            item.push_str(&format!("\n      {line}"));
        }
    }
    item
}

/// Without the program's own `RAKAZO_STACK_*` variables, which can hold the Owner
/// password and pasted keys.
fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"RAKAZO_STACK_") {
            command.env_remove(name);
        }
    }
    command
}

fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

fn command_stdout(program: &str, args: &[String]) -> Option<String> {
    let out = command(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn launchctl_print(uid: u32, label: &str) -> Option<String> {
    command_stdout("launchctl", &["print".into(), service(uid, label)])
}

fn launchctl(args: &[String]) -> Result<(), String> {
    let out = command("launchctl")
        .args(args)
        .output()
        .map_err(|err| format!("launchctl: {err}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(format!(
        "`launchctl {}` failed: {}",
        args.join(" "),
        stderr.trim()
    ))
}

/// Writes through a temporary file with mode 600, so the key is never readable by others.
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .expect("the plist has a name")
        .to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.tmp"));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    fs::rename(temp, path)
}

/// The Instance's lemond on this Mac, as setup's steps 6 and 9 reach it.
pub struct Lemond {
    layout: Layout,
    release: LemonadeRelease,
    key: String,
}

impl Lemond {
    pub fn new(layout: Layout, release: LemonadeRelease, key: &str) -> Self {
        Self {
            layout,
            release,
            key: key.to_string(),
        }
    }

    /// Step 6: install, write the plist, load the LaunchAgent and check its health. A
    /// failure becomes a Checklist item, never another voice setup.
    pub fn set_up(&self, agent: &ureq::Agent, log: &RunLog, checklist: &mut Checklist) -> bool {
        if let Err(reason) = self.install(agent, log) {
            checklist.add(voice::not_set_up(&reason));
            return false;
        }
        let uid = uid();
        match self.load(uid) {
            Ok(()) => {}
            Err(Failure::TurnedOff) => {
                checklist.add(turned_off_item(&self.layout.label));
                return false;
            }
            Err(Failure::Plist(reason)) => {
                checklist.add(voice::not_set_up(&reason));
                return false;
            }
            Err(Failure::Failing(reason)) => {
                checklist.add(self.failing(&reason));
                return false;
            }
        }
        match self.wait_for_health(agent) {
            Ok(()) => {
                log.say(&format!(
                    "Lemonade {} is running natively on 127.0.0.1:{PORT}.",
                    self.release.tag
                ));
                true
            }
            Err(reason) => {
                checklist.add(self.failing(&reason));
                false
            }
        }
    }

    fn failing(&self, reason: &str) -> String {
        let text = fs::read_to_string(self.layout.log()).unwrap_or_default();
        failing_item(
            &self.layout.label,
            reason,
            &last_log_lines(&text, &self.key),
        )
    }

    fn install(&self, agent: &ureq::Agent, log: &RunLog) -> Result<(), String> {
        let tag = &self.release.tag;
        let version_dir = self.layout.version_dir(tag);
        if version_dir.join("lemond").is_file() {
            return Ok(());
        }
        log.say(&format!("Downloading Lemonade {tag} for macOS…"));
        let url = download_url(tag);
        let bytes = download(agent, &url)?;
        verify(&bytes, &self.release.digest)?;
        let versions = version_dir.parent().expect("versions folder");
        fs::create_dir_all(versions).map_err(|err| err.to_string())?;
        let tarball = versions.join(format!(".{}", tarball_name(tag)));
        fs::write(&tarball, &bytes).map_err(|err| err.to_string())?;
        let unpacked = unpack(&tarball, &version_dir);
        let _ = fs::remove_file(&tarball);
        unpacked.map_err(|err| format!("Lemonade {tag} couldn't be unpacked ({err})"))
    }

    fn load(&self, uid: u32) -> Result<(), Failure> {
        let plist = self.layout.plist();
        let xml = self.layout.plist_xml(&self.release.tag, &self.key);
        let changed = fs::read(&plist).ok().as_deref() != Some(xml.as_bytes());
        let write = || -> io::Result<()> {
            fs::create_dir_all(plist.parent().expect("LaunchAgents folder"))?;
            fs::create_dir_all(self.layout.hf_home())?;
            if changed {
                write_private(&plist, xml.as_bytes())?;
            }
            fs::set_permissions(&plist, fs::Permissions::from_mode(0o600))
        };
        write().map_err(|err| Failure::Plist(format!("the LaunchAgent's plist ({err})")))?;
        let disabled = command_stdout("launchctl", &["print-disabled".into(), domain(uid)])
            .is_some_and(|out| is_disabled(&out, &self.layout.label));
        if disabled {
            return Err(Failure::TurnedOff);
        }
        let loaded = launchctl_print(uid, &self.layout.label).is_some();
        for step in load_plan(loaded, changed, uid, &self.layout) {
            let attempts = if step[0] == "bootstrap" {
                BOOTSTRAP_ATTEMPTS
            } else {
                1
            };
            let mut result = launchctl(&step);
            for _ in 1..attempts {
                if result.is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
                result = launchctl(&step);
            }
            result.map_err(Failure::Failing)?;
        }
        Ok(())
    }

    /// `launchctl print` shows it loaded and `/api/v1/health` answers with the key.
    fn wait_for_health(&self, agent: &ureq::Agent) -> Result<(), String> {
        let url = format!("http://{HOST}:{PORT}/api/v1/health");
        let mut last = String::from("didn't answer");
        for attempt in 0..HEALTH_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(HEALTH_PAUSE);
            }
            if launchctl_print(uid(), &self.layout.label).is_none() {
                last = "isn't loaded".to_string();
                continue;
            }
            match agent
                .get(&url)
                .header("authorization", format!("Bearer {}", self.key))
                .call()
            {
                Ok(response) if response.status() == 200 => return Ok(()),
                Ok(response) => {
                    last = format!("answered {url} with HTTP {}", response.status());
                }
                Err(_) => last = format!("didn't answer on {url}"),
            }
        }
        Err(last)
    }

    /// Step 9's settings, through `lemonade config set` against the running lemond.
    pub fn configure(&self) -> Result<(), String> {
        let cli = self.layout.version_dir(&self.release.tag).join("lemonade");
        let out = Command::new(&cli)
            .args(self.layout.config_arguments())
            .env_clear()
            .env("PATH", LAUNCHD_PATH)
            .env("HOME", &self.layout.home)
            .env("LEMONADE_API_KEY", &self.key)
            .output()
            .map_err(|err| format!("{}: {err}", cli.display()))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(stderr
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map_or_else(
                || format!("`lemonade config set` exited with {}", out.status),
                str::to_string,
            ))
    }
}

enum Failure {
    TurnedOff,
    Plist(String),
    Failing(String),
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let mut response = agent
        .get(url)
        .config()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .call()
        .map_err(|err| format!("Lemonade's download failed ({url}: {err})"))?;
    if response.status() != 200 {
        return Err(format!(
            "Lemonade's download failed ({url}: HTTP {})",
            response.status()
        ));
    }
    response
        .body_mut()
        .with_config()
        .limit(TARBALL_LIMIT)
        .read_to_vec()
        .map_err(|err| format!("Lemonade's download failed ({url}: {err})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn layout() -> Layout {
        Layout {
            home: PathBuf::from("/Users/example"),
            instance: PathBuf::from("/Users/example/rakazo-stack"),
            label: label("rakazo-stack"),
        }
    }

    /// A tag, whether it is a prerelease, and its assets' names and digests.
    type Entry<'a> = (&'a str, bool, &'a [(&'a str, Option<&'a str>)]);

    fn releases(entries: &[Entry]) -> String {
        let releases: Vec<serde_json::Value> = entries
            .iter()
            .map(|(tag, prerelease, assets)| {
                serde_json::json!({
                    "tag_name": tag,
                    "draft": false,
                    "prerelease": prerelease,
                    "assets": assets
                        .iter()
                        .map(|(name, digest)| serde_json::json!({"name": name, "digest": digest}))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        serde_json::Value::Array(releases).to_string()
    }

    const DIGEST: &str = "sha256:d0a78d095bd6a4c2403993cf5b65d97acc1b311b72663818609062860a38d8cf";

    #[test]
    fn picks_the_newest_stable_release_with_a_macos_tarball() {
        let json = releases(&[
            (
                "candidate-v2026.49.0",
                true,
                &[(
                    "lemonade-embeddable-2026.49.0-macos-arm64.tar.gz",
                    Some(DIGEST),
                )],
            ),
            (
                "v2026.49.1",
                false,
                &[(
                    "lemonade-embeddable-2026.49.1-ubuntu-x64.tar.gz",
                    Some(DIGEST),
                )],
            ),
            (
                "v2026.48.3",
                false,
                &[
                    (
                        "lemonade-embeddable-2026.48.3-ubuntu-x64.tar.gz",
                        Some(DIGEST),
                    ),
                    (
                        "lemonade-embeddable-2026.48.3-macos-arm64.tar.gz",
                        Some(DIGEST),
                    ),
                ],
            ),
        ]);
        let got = choose_tarball(&json).unwrap();
        assert_eq!(got.release.tag, "v2026.48.3");
        assert_eq!(got.release.digest, DIGEST);
        assert_eq!(got.skipped, ["v2026.49.1"]);
        assert!(describe_skipped("v2026.49.1").contains("macOS tarball isn't published yet"));
        assert_eq!(
            download_url("v2026.48.3"),
            "https://github.com/lemonade-sdk/lemonade/releases/download/v2026.48.3/lemonade-embeddable-2026.48.3-macos-arm64.tar.gz"
        );
    }

    #[test]
    fn a_tarball_without_a_sha256_is_skipped() {
        let json = releases(&[
            (
                "v2026.48.3",
                false,
                &[("lemonade-embeddable-2026.48.3-macos-arm64.tar.gz", None)],
            ),
            (
                "v2026.47.1",
                false,
                &[(
                    "lemonade-embeddable-2026.47.1-macos-arm64.tar.gz",
                    Some("sha256:short"),
                )],
            ),
        ]);
        assert!(choose_tarball(&json).is_err());
        assert!(choose_tarball("not json").is_err());
    }

    #[test]
    fn refuses_a_tarball_whose_sha256_doesnt_match() {
        let bytes = b"lemonade";
        let good = format!("sha256:{}", sha256_hex(bytes));
        assert!(verify(bytes, &good).is_ok());
        let err = verify(b"lemonadf", &good).unwrap_err();
        assert!(err.contains("wasn't used"), "{err}");
    }

    #[test]
    fn hashes_with_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn unpacks_the_top_folder_into_the_version_folder() {
        let work = tempfile::tempdir().unwrap();
        let top = work
            .path()
            .join("src/lemonade-embeddable-2026.48.3-macos-arm64");
        fs::create_dir_all(top.join("resources")).unwrap();
        fs::write(top.join("lemond"), b"#!/bin/sh\n").unwrap();
        fs::write(top.join("resources/defaults.json"), b"{}").unwrap();
        let tarball = work.path().join("l.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(work.path().join("src"))
            .arg("lemonade-embeddable-2026.48.3-macos-arm64")
            .status()
            .unwrap();
        assert!(status.success());

        let version_dir = work.path().join("lemonade/versions/v2026.48.3");
        fs::create_dir_all(version_dir.parent().unwrap()).unwrap();
        unpack(&tarball, &version_dir).unwrap();
        assert!(version_dir.join("lemond").is_file());
        assert!(version_dir.join("resources/defaults.json").is_file());
        assert!(
            !work
                .path()
                .join("lemonade/versions/.v2026.48.3.partial")
                .exists()
        );

        let empty = work.path().join("empty.tar.gz");
        fs::create_dir_all(work.path().join("bare/top")).unwrap();
        Command::new("tar")
            .arg("-czf")
            .arg(&empty)
            .arg("-C")
            .arg(work.path().join("bare"))
            .arg("top")
            .status()
            .unwrap();
        let err = unpack(&empty, &work.path().join("lemonade/versions/v0")).unwrap_err();
        assert_eq!(err, "the tarball has no lemond");
        assert!(!work.path().join("lemonade/versions/v0").exists());
    }

    #[test]
    fn the_plist_runs_lemond_on_its_own_port_and_logs_to_the_instance_folder() {
        let xml = layout().plist_xml("v2026.48.3", KEY);
        let expected = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>com.github.kamui.rakazo-stack.lemond</string>
	<key>ProgramArguments</key>
	<array>
		<string>/Users/example/rakazo-stack/lemonade/versions/v2026.48.3/lemond</string>
		<string>--no-broadcast</string>
		<string>--host</string>
		<string>127.0.0.1</string>
		<string>--port</string>
		<string>13315</string>
		<string>/Users/example/rakazo-stack/lemonade</string>
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>/usr/bin:/bin:/usr/sbin:/sbin</string>
		<key>HF_HOME</key>
		<string>/Users/example/rakazo-stack/lemonade/models</string>
		<key>LEMONADE_API_KEY</key>
		<string>{KEY}</string>
	</dict>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>StandardOutPath</key>
	<string>/Users/example/rakazo-stack/logs/lemond.log</string>
	<key>StandardErrorPath</key>
	<string>/Users/example/rakazo-stack/logs/lemond.log</string>
</dict>
</plist>
"#
        );
        assert_eq!(xml, expected);
        assert_eq!(
            xml.matches(KEY).count(),
            1,
            "the key only in the environment"
        );
        for absent in ["--convert", "-t<", "metal_args"] {
            assert!(!xml.contains(absent), "{absent}");
        }
        assert_eq!(
            layout().plist(),
            Path::new(
                "/Users/example/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist"
            )
        );
    }

    #[test]
    fn the_plist_escapes_paths() {
        let layout = Layout {
            home: PathBuf::from("/Users/a&b"),
            instance: PathBuf::from("/Users/a&b/rakazo-stack"),
            label: label("rakazo-stack"),
        };
        let xml = layout.plist_xml("v2026.48.3", KEY);
        assert!(xml.contains("<string>/Users/a&amp;b/rakazo-stack/lemonade</string>"));
        assert!(!xml.contains("a&b"));
    }

    #[test]
    fn writes_the_plist_with_mode_600() {
        let dir = tempfile::tempdir().unwrap();
        let plist = dir.path().join("x.plist");
        write_private(&plist, b"one").unwrap();
        fs::set_permissions(&plist, fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&plist, b"two").unwrap();
        let mode = fs::metadata(&plist).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(fs::read(&plist).unwrap(), b"two");
        assert!(!dir.path().join(".x.plist.tmp").exists());
    }

    #[test]
    fn a_test_project_gets_its_own_label() {
        assert_eq!(
            label("rakazo-test-1"),
            "com.github.kamui.rakazo-test-1.lemond"
        );
        assert_eq!(upstream(), "http://host.docker.internal:13315");
    }

    #[test]
    fn loads_when_unloaded_and_reloads_only_a_changed_plist() {
        let layout = layout();
        let plist =
            "/Users/example/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist";
        let bootstrap = vec!["bootstrap", "gui/501", plist];
        assert_eq!(
            load_plan(false, true, 501, &layout),
            std::slice::from_ref(&bootstrap)
        );
        assert_eq!(
            load_plan(false, false, 501, &layout),
            std::slice::from_ref(&bootstrap)
        );
        assert_eq!(
            load_plan(true, true, 501, &layout),
            [
                vec!["bootout", "gui/501/com.github.kamui.rakazo-stack.lemond"],
                bootstrap
            ]
        );
        assert!(load_plan(true, false, 501, &layout).is_empty());
    }

    #[test]
    fn configures_this_lemond_with_metal() {
        assert_eq!(
            layout().config_arguments(),
            [
                "--host",
                "127.0.0.1",
                "--port",
                "13315",
                "config",
                "set",
                "whispercpp.backend=metal",
                "auto_evict=false",
                "auto_check_model_updates=false",
            ]
        );
    }

    #[test]
    fn reads_launchctl_print() {
        let out = "gui/501/com.github.kamui.rakazo-stack.lemond = {\n\tactive count = 1\n\tpath = /Users/example/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist\n\ttype = LaunchAgent\n\tstate = running\n\n\tprogram = /Users/example/rakazo-stack/lemonade/versions/v2026.48.3/lemond\n\tpid = 4242\n}\n";
        assert_eq!(printed_pid(out), Some(4242));
        let waiting = "gui/501/x = {\n\tstate = spawn scheduled\n\tlast exit code = 1\n}\n";
        assert_eq!(printed_pid(waiting), None);
    }

    #[test]
    fn reads_launchctl_print_disabled() {
        let label = label("rakazo-stack");
        let out = format!(
            "disabled services = {{\n\t\"com.apple.ScriptMenuApp\" => enabled\n\t\"{label}\" => disabled\n}}\n"
        );
        assert!(is_disabled(&out, &label));
        assert!(!is_disabled(&out, "com.apple.ScriptMenuApp"));
        assert!(is_disabled(&format!("\t\"{label}\" => true\n"), &label));
        assert!(!is_disabled(
            &format!("\t\"{label}.other\" => disabled\n"),
            &label
        ));
        assert!(!is_disabled("", &label));
    }

    #[test]
    fn the_port_check_stops_only_for_something_else() {
        let lsof = "p4242\nclemond\np777\ncpython3\n";
        let listeners = parse_lsof(lsof);
        assert_eq!(
            listeners,
            [
                Listener {
                    pid: 4242,
                    command: "lemond".into()
                },
                Listener {
                    pid: 777,
                    command: "python3".into()
                }
            ]
        );
        assert_eq!(decide_port(false, &[], None), PortHolder::Free);
        assert_eq!(
            decide_port(true, &listeners[..1], Some(4242)),
            PortHolder::Lemond
        );
        assert_eq!(
            decide_port(true, &listeners, Some(4242)),
            PortHolder::Other("python3 (process 777)".into())
        );
        // An upstream Lemonade moved to this port is still someone else's lemond.
        assert_eq!(
            decide_port(true, &listeners[..1], None),
            PortHolder::Other("lemond (process 4242)".into())
        );
        assert!(matches!(
            decide_port(true, &[], Some(4242)),
            PortHolder::Other(by) if by.contains("can't see")
        ));
    }

    #[test]
    fn the_checklist_items_quote_the_log_without_the_key() {
        let log = format!("start\n\nloaded key {KEY}\n{}", "line\n".repeat(12));
        let lines = last_log_lines(&log, KEY);
        assert_eq!(lines.len(), 10);
        let all = last_log_lines(&format!("a\nkey {KEY}\n"), KEY);
        assert_eq!(all, ["a", "key <LEMONADE_API_KEY>"]);
        let item = failing_item(&label("rakazo-stack"), "didn't answer", &all);
        assert!(item.contains("Login Items"));
        assert!(item.contains("\n      key <LEMONADE_API_KEY>"));
        assert!(!item.contains(KEY));
        assert!(failing_item("x", "isn't loaded", &[]).contains("is empty"));
        assert!(turned_off_item("x").contains("Allow in the Background"));
    }
}
