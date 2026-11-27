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
