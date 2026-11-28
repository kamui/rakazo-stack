use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Runs the program with a throwaway HOME, so tests never touch a real Instance folder.
fn rakazo_stack(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run rakazo-stack")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn version_prints_the_program_version() {
    let home = tempfile::tempdir().unwrap();
    let out = rakazo_stack(home.path(), &["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("rakazo-stack {}", env!("CARGO_PKG_VERSION"))
    );
}

/// `backup` without a terminal, as cron runs it.
fn backup(home: &Path, passphrase: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"));
    command
        .arg("backup")
        .env("HOME", home)
        .env_remove("RAKAZO_STACK_BACKUP_PASSPHRASE")
        .stdin(Stdio::null());
    if let Some(passphrase) = passphrase {
        command.env("RAKAZO_STACK_BACKUP_PASSPHRASE", passphrase);
    }
    command.output().expect("run rakazo-stack")
}

#[test]
fn backup_without_a_passphrase_or_an_instance_stops_before_it_starts() {
    let home = tempfile::tempdir().unwrap();
    let out = backup(home.path(), None);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("Stop: no Backup passphrase. Set RAKAZO_STACK_BACKUP_PASSPHRASE"),
        "{}",
        stderr(&out)
    );
    let out = backup(home.path(), Some("placeholder-passphrase"));
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("there is no Instance here"),
        "{}",
        stderr(&out)
    );
    assert!(!home.path().join("rakazo-stack-backups").exists());
}

#[test]
fn a_held_lock_stops_a_second_command() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join("rakazo-stack");
    fs::create_dir_all(&folder).unwrap();
    let lock = File::create(folder.join(".lock")).unwrap();
    lock.try_lock().unwrap();
    for command in [
        &["setup"][..],
        &["update"],
        &["backup"],
        &["restore", "backup.tar.age"],
    ] {
        let out = rakazo_stack(home.path(), command);
        assert!(!out.status.success());
        assert!(
            stderr(&out).contains("another rakazo-stack command is running"),
            "{command:?}: {}",
            stderr(&out)
        );
    }
}

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const PASTED_KEY: &str = "sk-or-v1-PLACEHOLDER0000000000000000";
const OWNER: &str = "owner@example.com";
const PASSPHRASE: &str = "placeholder-passphrase";

/// An Instance folder as setup leaves it, as far as `backup` and `logs` read it before
/// they reach Docker, with a run log from an earlier setup.
fn instance(home: &Path) -> std::path::PathBuf {
    let folder = home.join("rakazo-stack");
    fs::create_dir_all(folder.join("logs")).unwrap();
    fs::write(
        folder.join("settings.json"),
        format!(
            r#"{{"schema":1,"program_version":"0.1.0","owner_email":"{OWNER}","access":"bring-your-own","bring_your_own_url":"https://chat.example.com","components":{{"rakazo":{{"version":"v0.16.0","images":{{"app":"sha256:a","computer":"sha256:c"}}}}}}}}"#
        ),
    )
    .unwrap();
    fs::write(
        folder.join("secrets.env"),
        format!("POSTGRES_PASSWORD={SECRET}\nOPENROUTER_API_KEY={PASTED_KEY}\n"),
    )
    .unwrap();
    fs::write(folder.join(".env"), "COMPOSE_PROJECT_NAME=rkst-cli-test\n").unwrap();
    fs::write(
        folder.join("logs/setup-20261126T120101Z.log"),
        format!(
            "2026-11-26T12:01:01Z Restricted computer egress is verified.\n2026-11-26T12:01:02Z key {PASTED_KEY} for {OWNER} at https://chat.example.com in {}\n",
            home.display()
        ),
    )
    .unwrap();
    folder
}

/// A `docker` that answers as the CLI does while Docker is stopped.
fn stopped_docker(dir: &Path) -> String {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let docker = bin.join("docker");
    fs::write(
        &docker,
        "#!/bin/sh\necho 'Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:/usr/bin:/bin", bin.display())
}

fn no_private_values(text: &str, home: &Path) {
    for value in [SECRET, PASTED_KEY, OWNER, PASSPHRASE, "chat.example.com"] {
        assert!(!text.contains(value), "{value} in:\n{text}");
    }
    assert!(!text.contains(&home.display().to_string()), "{text}");
}

fn timed(line: &str) -> bool {
    line.split_once(' ').is_some_and(|(stamp, _)| {
        stamp.len() == 20 && stamp.as_bytes()[10] == b'T' && stamp.ends_with('Z')
    })
}

/// On WSL the program finds Docker Desktop's `docker.exe` whatever `PATH` holds.
fn on_wsl() -> bool {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .is_ok_and(|release| release.to_lowercase().contains("microsoft"))
}

#[test]
fn a_failed_run_logs_its_header_the_failed_command_and_the_stop_without_secrets() {
    if on_wsl() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let folder = instance(home.path());
    let out = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
        .arg("backup")
        .env("HOME", home.path())
        .env("PATH", stopped_docker(home.path()))
        .env("RAKAZO_STACK_BACKUP_PASSPHRASE", PASSPHRASE)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());

    let log = fs::read_dir(folder.join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("backup-")
        })
        .expect("backup wrote a run log");
    assert_eq!(
        fs::metadata(&log).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.lines().all(timed), "{text}");
    let lines: Vec<&str> = text.lines().map(|l| l.split_once(' ').unwrap().1).collect();
    assert_eq!(lines[0], "rakazo-stack backup");
    assert!(lines[1].starts_with("Program: rakazo-stack v"), "{text}");
    assert!(lines[2].starts_with("OS: "), "{text}");
    assert!(lines[3].starts_with("Docker: "), "{text}");
    assert!(lines[4].starts_with("Started: "), "{text}");
    let failed = lines
        .iter()
        .position(|line| line.starts_with("$ docker compose"))
        .expect("the command is logged");
    assert_eq!(
        lines[failed + 1],
        "  err: Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"
    );
    assert!(
        lines[failed + 2].starts_with("  exit status: 1 after "),
        "{text}"
    );
    assert!(lines.last().unwrap().starts_with("Stop: "), "{text}");
    no_private_values(&text, home.path());
}

#[test]
fn logs_with_docker_stopped_says_what_is_missing_and_keeps_the_run_logs() {
    if on_wsl() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let out = rakazo_stack(home.path(), &["logs"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("there is no Instance here"),
        "{}",
        stderr(&out)
    );

    let folder = instance(home.path());
    let out = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
        .arg("logs")
        .env("HOME", home.path())
        .env("PATH", stopped_docker(home.path()))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("The container logs are missing, because Docker isn't answering (Cannot connect to the Docker daemon"),
        "{}",
        stderr(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("read the file before you post it"),
        "{stdout}"
    );
    let report = fs::read_dir(folder.join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|e| e == "txt"))
        .expect("logs wrote its file");
    assert!(
        stdout.contains(&format!("Wrote {}.", report.display())),
        "{stdout}"
    );
    let text = fs::read_to_string(&report).unwrap();
    for part in [
        "Program: rakazo-stack v",
        "The container logs are missing, because Docker isn't answering",
        "Run log setup-20261126T120101Z.log",
        "Restricted computer egress is verified.",
        "key <OPENROUTER_API_KEY> for <Owner email> at <Instance URL> in ~",
        "No update run log.",
    ] {
        assert!(text.contains(part), "{part} missing from:\n{text}");
    }
    no_private_values(&text, home.path());
}
