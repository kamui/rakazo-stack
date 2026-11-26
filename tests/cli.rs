use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Output};

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

#[test]
fn backup_is_not_implemented_yet() {
    let home = tempfile::tempdir().unwrap();
    let out = rakazo_stack(home.path(), &["backup"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not implemented yet"));
}

#[test]
fn a_held_lock_stops_a_second_command() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join("rakazo-stack");
    fs::create_dir_all(&folder).unwrap();
    let lock = File::create(folder.join(".lock")).unwrap();
    lock.try_lock().unwrap();
    for command in ["setup", "update", "backup", "restore"] {
        let out = rakazo_stack(home.path(), &[command]);
        assert!(!out.status.success());
        assert!(
            stderr(&out).contains("another rakazo-stack command is running"),
            "{command}: {}",
            stderr(&out)
        );
    }
}

#[test]
#[ignore = "needs Docker; CI and the dev machine run it with --include-ignored"]
fn setup_passes_preflight_and_changes_nothing_but_the_lock() {
    let home = tempfile::tempdir().unwrap();
    // The dev machine may already run Rakazo on 5173, so check a port nothing holds.
    let free_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let out = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
        .arg("setup")
        .env("HOME", home.path())
        .env("RAKAZO_STACK_TEST_WEB_PORT", free_port.to_string())
        .output()
        .expect("run rakazo-stack");
    let err = stderr(&out);
    assert!(err.contains("Preflight passed."), "{err}");
    assert!(err.contains("setup: not implemented yet"), "{err}");
    let mut entries: Vec<_> = walk(home.path());
    entries.sort();
    assert_eq!(entries, ["rakazo-stack", "rakazo-stack/.lock"]);
}

fn walk(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            found.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    found
}
