//! `install.sh` against a fake release served on localhost. Every run gets its own HOME
//! and a PATH holding only the tools the script may use, so the real `gh` and the real
//! `rakazo-stack` are out of reach.
#![cfg(target_os = "linux")]

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const TAG: &str = "v9.9.9-test";
const LINUX: &str = "rakazo-stack-x86_64-unknown-linux-gnu";
const MACOS: &str = "rakazo-stack-aarch64-apple-darwin";
const PROFILES: [&str; 5] = [
    ".profile",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
];

fn repo_file(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Stands in for the program: records how it was started.
fn fake_program(name: &str) -> String {
    format!(
        "#!/bin/sh\n# {name}\n{{ echo \"args=$*\"; echo \"stdin=$(readlink /proc/$$/fd/0)\"; }} >\"$HOME/program-ran\"\n"
    )
}

struct Release {
    files: tempfile::TempDir,
    base: String,
    requests: Arc<AtomicUsize>,
}

impl Release {
    /// Two fake executables and the `install.sh` baked for them.
    fn new() -> Self {
        let files = tempfile::tempdir().unwrap();
        let dir = files.path().join(TAG);
        fs::create_dir(&dir).unwrap();
        for name in [LINUX, MACOS] {
            fs::write(dir.join(name), fake_program(name)).unwrap();
        }
        let bake = Command::new(repo_file("scripts/bake-install.sh"))
            .arg(TAG)
            .arg(&dir)
            .output()
            .unwrap();
        assert!(bake.status.success(), "{}", text(&bake));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let (root, count) = (files.path().to_path_buf(), requests.clone());
        std::thread::spawn(move || serve(&listener, &root, &count));
        Self {
            files,
            base,
            requests,
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.files.path().join(TAG).join(name)
    }

    fn script(&self) -> PathBuf {
        self.file("install.sh")
    }
}

fn serve(listener: &TcpListener, root: &Path, requests: &AtomicUsize) {
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        let mut lines = BufReader::new(&stream).lines();
        let request = lines.next().unwrap().unwrap();
        for line in lines {
            if line.unwrap().is_empty() {
                break;
            }
        }
        requests.fetch_add(1, Ordering::SeqCst);
        let path = request.split(' ').nth(1).unwrap().trim_start_matches('/');
        let (status, body) = match fs::read(root.join(path)) {
            Ok(body) => ("200 OK", body),
            Err(_) => ("404 Not Found", Vec::new()),
        };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    }
}

struct Machine {
    home: tempfile::TempDir,
    temp: tempfile::TempDir,
    tools: tempfile::TempDir,
}

impl Machine {
    /// x86-64 glibc Linux without `gh`.
    fn new() -> Self {
        let machine = Self {
            home: tempfile::tempdir().unwrap(),
            temp: tempfile::tempdir().unwrap(),
            tools: tempfile::tempdir().unwrap(),
        };
        for tool in [
            "cat",
            "chmod",
            "cp",
            "curl",
            "cut",
            "grep",
            "ldd",
            "mkdir",
            "mktemp",
            "mv",
            "readlink",
            "rm",
            "sh",
            "sha256sum",
            "uname",
        ] {
            machine.link(tool);
        }
        machine
    }

    fn link(&self, tool: &str) {
        let found = ["/usr/bin", "/bin"]
            .iter()
            .map(|dir| Path::new(dir).join(tool))
            .find(|path| path.exists())
            .unwrap_or_else(|| panic!("{tool} isn't installed"));
        symlink(found, self.tools.path().join(tool)).unwrap();
    }

    fn unlink(&self, tool: &str) {
        fs::remove_file(self.tools.path().join(tool)).unwrap();
    }

    /// Replaces a tool with a shell script.
    fn stub(&self, tool: &str, body: &str) {
        let path = self.tools.path().join(tool);
        let _ = fs::remove_file(&path);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn platform(&self, os: &str, arch: &str) {
        self.stub(
            "uname",
            &format!("case $1 in -s) echo {os} ;; -m) echo {arch} ;; esac"),
        );
    }

    fn program(&self) -> PathBuf {
        self.home.path().join(".local/bin/rakazo-stack")
    }

    fn home_entries(&self) -> Vec<String> {
        entries(self.home.path())
    }

    fn command(&self, program: &str, release: &Release) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("PATH", self.tools.path())
            .env("HOME", self.home.path())
            .env("TMPDIR", self.temp.path())
            .env("RAKAZO_STACK_TEST_DOWNLOAD_BASE", &release.base)
            .current_dir(self.temp.path());
        command
    }

    /// `curl … | sh -s -- <args>` with no controlling terminal.
    fn install(&self, release: &Release, script: &Path, args: &[&str]) -> Output {
        self.command("/usr/bin/setsid", release)
            .args(["-w", "sh", "-s", "--"])
            .args(args)
            .stdin(fs::File::open(script).unwrap())
            .output()
            .unwrap()
    }

    /// The same pipe, inside a new pseudo-terminal.
    fn install_in_terminal(&self, release: &Release, args: &str) -> Output {
        let pipe = format!("cat '{}' | sh -s -- {args}", release.script().display());
        let mut child = self
            .command("/usr/bin/script", release)
            .env("SHELL", "/bin/sh")
            .args(["-qec", &pipe, "/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // `script` ends the session early if its input closes first.
        let input = child.stdin.take();
        let out = child.wait_with_output().unwrap();
        drop(input);
        out
    }

    fn assert_installed(&self, out: &Output, release: &Release, name: &str) {
        assert!(out.status.success(), "{}", text(out));
        assert_eq!(
            fs::read(self.program()).unwrap(),
            fs::read(release.file(name)).unwrap()
        );
        let mode = fs::metadata(self.program()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            entries(self.program().parent().unwrap()),
            ["rakazo-stack"],
            "no staged file left behind"
        );
        assert_eq!(entries(self.temp.path()), Vec::<String>::new());
    }

    fn assert_nothing_installed(&self, out: &Output, message: &str) {
        assert!(!out.status.success(), "{}", text(out));
        assert!(text(out).contains(message), "{}", text(out));
        assert!(
            !self.home.path().join(".local").exists(),
            "{:?}",
            self.home_entries()
        );
        assert_eq!(entries(self.temp.path()), Vec::<String>::new());
    }
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn origin_lines(out: &Output) -> usize {
    text(out)
        .lines()
        .filter(|line| line.starts_with("build origin not checked; to check it yourself: "))
        .count()
}

#[test]
fn without_a_terminal_it_installs_and_prints_the_next_command() {
    let (release, machine) = (Release::new(), Machine::new());
    for profile in PROFILES {
        fs::write(machine.home.path().join(profile), "untouched\n").unwrap();
    }

    let out = machine.install(&release, &release.script(), &[]);

    machine.assert_installed(&out, &release, LINUX);
    let program = machine.program();
    let output = text(&out);
    assert!(
        output.contains(&format!("Next: run \"{}\" setup", program.display())),
        "{output}"
    );
    assert!(
        output.contains("\n  export PATH=\"$HOME/.local/bin:$PATH\"\n"),
        "{output}"
    );
    assert!(
        output.contains(&format!(
            "gh attestation verify \"{}\" --repo kamui/rakazo-stack",
            program.display()
        )),
        "{output}"
    );
    assert_eq!(origin_lines(&out), 1, "{output}");
    for profile in PROFILES {
        assert_eq!(
            fs::read_to_string(machine.home.path().join(profile)).unwrap(),
            "untouched\n"
        );
    }
    let mut expected: Vec<&str> = PROFILES.to_vec();
    expected.push(".local");
    expected.sort();
    assert_eq!(
        machine.home_entries(),
        expected,
        "setup didn't run and nothing else was written"
    );
}

#[test]
fn a_folder_already_on_path_gets_no_hint() {
    let (release, machine) = (Release::new(), Machine::new());
    let path = format!(
        "{}:{}",
        machine.tools.path().display(),
        machine.program().parent().unwrap().display()
    );

    let out = machine
        .command("/usr/bin/setsid", &release)
        .env("PATH", path)
        .args(["-w", "sh", "-s"])
        .stdin(fs::File::open(release.script()).unwrap())
        .output()
        .unwrap();

    machine.assert_installed(&out, &release, LINUX);
    assert!(!text(&out).contains("export PATH"), "{}", text(&out));
}

#[test]
fn a_newer_release_replaces_the_installed_program() {
    let (release, machine) = (Release::new(), Machine::new());
    fs::create_dir_all(machine.program().parent().unwrap()).unwrap();
    fs::write(machine.program(), "older").unwrap();

    let out = machine.install(&release, &release.script(), &["--no-setup"]);

    machine.assert_installed(&out, &release, LINUX);
}

#[test]
fn with_a_terminal_it_ends_in_setup_reading_the_terminal() {
    let (release, machine) = (Release::new(), Machine::new());

    let out = machine.install_in_terminal(&release, "");

    machine.assert_installed(&out, &release, LINUX);
    assert_eq!(
        fs::read_to_string(machine.home.path().join("program-ran")).unwrap(),
        "args=setup\nstdin=/dev/tty\n"
    );
}

#[test]
fn no_setup_only_installs_even_with_a_terminal() {
    let (release, machine) = (Release::new(), Machine::new());

    let out = machine.install_in_terminal(&release, "--no-setup");

    machine.assert_installed(&out, &release, LINUX);
    assert!(text(&out).contains("Next: run"), "{}", text(&out));
    assert_eq!(machine.home_entries(), [".local"]);
}

#[test]
fn a_tampered_download_installs_nothing() {
    let (release, machine) = (Release::new(), Machine::new());
    fs::write(release.file(LINUX), "#!/bin/sh\necho tampered\n").unwrap();

    let out = machine.install(&release, &release.script(), &[]);

    machine.assert_nothing_installed(&out, "doesn't match this install.sh");
}

#[test]
fn a_tampered_download_keeps_the_installed_program() {
    let (release, machine) = (Release::new(), Machine::new());
    fs::create_dir_all(machine.program().parent().unwrap()).unwrap();
    fs::write(machine.program(), "older").unwrap();
    fs::write(release.file(LINUX), "tampered").unwrap();

    let out = machine.install(&release, &release.script(), &[]);

    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(fs::read_to_string(machine.program()).unwrap(), "older");
}

#[test]
fn a_missing_download_installs_nothing() {
    let (release, machine) = (Release::new(), Machine::new());
    fs::remove_file(release.file(LINUX)).unwrap();

    let out = machine.install(&release, &release.script(), &[]);

    machine.assert_nothing_installed(&out, "couldn't download");
}

#[test]
fn a_script_with_an_unfilled_or_malformed_value_installs_nothing() {
    let release = Release::new();
    let baked = fs::read_to_string(release.script()).unwrap();
    let committed = fs::read_to_string(repo_file("install.sh")).unwrap();
    let value = |script: &str, name: &str| {
        script
            .lines()
            .find(|line| line.starts_with(&format!("{name}=")))
            .unwrap()
            .to_string()
    };
    let with = |name: &str, line: &str| baked.replace(&value(&baked, name), line);
    let scripts = [
        committed.clone(),
        with("release_tag", &value(&committed, "release_tag")),
        with(
            "sha256_linux_x86_64",
            &value(&committed, "sha256_linux_x86_64"),
        ),
        with("sha256_linux_x86_64", "sha256_linux_x86_64=''"),
        with("sha256_linux_x86_64", "sha256_linux_x86_64='abc123'"),
    ];

    for script in scripts {
        let machine = Machine::new();
        let path = release.files.path().join("candidate.sh");
        fs::write(&path, script).unwrap();

        let out = machine.install(&release, &path, &[]);

        machine.assert_nothing_installed(&out, "isn't from a release");
    }
    assert_eq!(release.requests.load(Ordering::SeqCst), 0);
}

#[test]
fn a_script_cut_off_part_way_runs_nothing() {
    let release = Release::new();
    let baked = fs::read_to_string(release.script()).unwrap();
    let lines: Vec<&str> = baked.lines().collect();
    assert_eq!(lines.last(), Some(&"}"));

    for kept in 1..lines.len() {
        let machine = Machine::new();
        let path = release.files.path().join("cut.sh");
        fs::write(&path, lines[..kept].join("\n")).unwrap();

        let out = machine.install(&release, &path, &[]);

        assert_eq!(machine.home_entries(), Vec::<String>::new(), "{kept} lines");
        assert!(!text(&out).contains("Downloading"), "{kept} lines");
    }
    assert_eq!(release.requests.load(Ordering::SeqCst), 0);
}

/// A signed-in `gh` that logs each call and exits `verify_status` from the check.
fn signed_in_gh(verify_status: u8) -> String {
    format!(
        r#"case "$1 $2 $3" in
'attestation verify --help' | 'auth status --hostname') exit 0 ;;
esac
[ ! -e "$HOME/.local" ] || exit 99
echo "$GH_HOST $*" >"$HOME/gh-verify"
exit {verify_status}"#
    )
}

#[test]
fn a_signed_in_gh_checks_the_download_before_it_is_installed() {
    let (release, machine) = (Release::new(), Machine::new());
    machine.stub("gh", &signed_in_gh(0));

    let out = machine.install(&release, &release.script(), &[]);

    machine.assert_installed(&out, &release, LINUX);
    assert_eq!(origin_lines(&out), 0, "{}", text(&out));
    let call = fs::read_to_string(machine.home.path().join("gh-verify")).unwrap();
    assert!(
        call.starts_with(&format!(
            "github.com attestation verify {}/",
            machine.temp.path().display()
        )) && call.ends_with("/rakazo-stack --repo kamui/rakazo-stack\n"),
        "{call}"
    );
}

#[test]
fn a_failed_build_origin_check_installs_nothing() {
    let (release, machine) = (Release::new(), Machine::new());
    machine.stub("gh", &signed_in_gh(1));

    let out = machine.install(&release, &release.script(), &[]);

    machine.assert_nothing_installed(&out, "gh couldn't verify");
}

#[test]
fn a_gh_that_cannot_check_gets_one_line_and_the_install_carries_on() {
    let not_signed_in = r#"case $1 in auth) exit 1 ;; esac
[ "$3" = --help ] || exit 99"#;
    let too_old = r#"case $1 in attestation) exit 1 ;; esac"#;

    for gh in [not_signed_in, too_old] {
        let (release, machine) = (Release::new(), Machine::new());
        machine.stub("gh", gh);

        let out = machine.install(&release, &release.script(), &[]);

        machine.assert_installed(&out, &release, LINUX);
        assert_eq!(origin_lines(&out), 1, "{}", text(&out));
    }
}

#[test]
fn other_platforms_are_refused() {
    for (os, arch, translated) in [
        ("Linux", "aarch64", None),
        ("Darwin", "x86_64", None),
        ("Darwin", "x86_64", Some("echo 0")),
        ("Darwin", "x86_64", Some("exit 1")),
        ("FreeBSD", "amd64", None),
    ] {
        let (release, machine) = (Release::new(), Machine::new());
        machine.platform(os, arch);
        if let Some(sysctl) = translated {
            machine.stub("sysctl", sysctl);
        }

        let out = machine.install(&release, &release.script(), &[]);

        machine.assert_nothing_installed(&out, &format!("{os} {arch} isn't supported"));
        assert_eq!(release.requests.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn apple_silicon_gets_the_mac_program_natively_and_under_rosetta() {
    for (arch, translated) in [("arm64", "echo 0"), ("x86_64", "echo 1")] {
        let (release, machine) = (Release::new(), Machine::new());
        machine.platform("Darwin", arch);
        machine.stub("sysctl", translated);
        // macOS before 15 has `shasum` and no `sha256sum`.
        machine.unlink("sha256sum");
        machine.link("shasum");

        let out = machine.install(&release, &release.script(), &[]);

        machine.assert_installed(&out, &release, MACOS);
    }
}

#[test]
fn musl_linux_is_refused() {
    let (release, machine) = (Release::new(), Machine::new());
    machine.stub("ldd", "echo 'musl libc (x86_64)' >&2; exit 1");

    let out = machine.install(&release, &release.script(), &[]);

    machine.assert_nothing_installed(
        &out,
        "Alpine/musl Linux isn't supported; use a glibc distro such as Ubuntu or Debian",
    );
}

#[test]
fn unknown_options_are_refused() {
    let (release, machine) = (Release::new(), Machine::new());

    let out = machine.install(&release, &release.script(), &["--nosetup"]);

    machine.assert_nothing_installed(&out, "unknown option --nosetup");
}

#[test]
#[ignore = "needs Docker; CI and the dev machine run it with --include-ignored"]
fn alpine_is_refused() {
    let release = Release::new();
    let out = common::docker()
        .args(["run", "--rm", "-i", "--name"])
        .arg(format!("rkst-install-{}", std::process::id()))
        .args(["alpine:3.24.4", "sh", "-c"])
        .arg(r#"sh -s -- --no-setup; echo "exit=$?"; ls -A "$HOME""#)
        .stdin(fs::File::open(release.script()).unwrap())
        .output()
        .unwrap();

    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "exit=1\n",
        "{}",
        text(&out)
    );
    assert!(
        text(&out).contains(
            "Stop: Alpine/musl Linux isn't supported; use a glibc distro such as Ubuntu or Debian"
        ),
        "{}",
        text(&out)
    );
}
