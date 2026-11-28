use std::fs::{self, File};
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

/// `uninstall` without a terminal, as a script runs it.
fn uninstall(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
        .arg("uninstall")
        .args(args)
        .env("HOME", home)
        .env_remove("RAKAZO_STACK_TEST_PROJECT")
        .stdin(Stdio::null())
        .output()
        .expect("run rakazo-stack")
}

#[test]
fn uninstall_stops_before_anything_changes_without_an_instance_or_its_name() {
    let home = tempfile::tempdir().unwrap();
    let out = uninstall(home.path(), &["--confirm", "rakazo-stack"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("there's no Instance in ~/rakazo-stack"),
        "{}",
        stderr(&out)
    );
    assert!(!home.path().join("rakazo-stack").exists());

    let folder = home.path().join("rakazo-stack");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("settings.json"), "{}").unwrap();
    let out = uninstall(home.path(), &[]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("pass `--confirm rakazo-stack`"),
        "{}",
        stderr(&out)
    );
    for wrong in ["rakazo", "Rakazo-Stack", ""] {
        let out = uninstall(home.path(), &["--confirm", wrong, "--delete-images"]);
        assert!(!out.status.success());
        assert!(
            stderr(&out).contains("the name didn't match, so nothing was changed."),
            "{wrong:?}: {}",
            stderr(&out)
        );
    }
    assert_eq!(
        fs::read_to_string(folder.join("settings.json")).unwrap(),
        "{}"
    );

    let kept = folder.join("kept");
    fs::create_dir_all(&kept).unwrap();
    let inside = kept.join("rakazo-stack-backup-20261127T120101Z.tar.age");
    fs::write(&inside, "backup").unwrap();
    let out = uninstall(home.path(), &["--confirm", "rakazo-stack"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("these Backups are inside ~/rakazo-stack"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("~/rakazo-stack/kept/rakazo-stack-backup-20261127T120101Z.tar.age"),
        "{}",
        stderr(&out)
    );
    assert_eq!(fs::read_to_string(&inside).unwrap(), "backup");

    let lock = File::create(folder.join(".lock")).unwrap();
    lock.try_lock().unwrap();
    let out = uninstall(home.path(), &["--confirm", "rakazo-stack"]);
    assert!(
        stderr(&out).contains("another rakazo-stack command is running"),
        "{}",
        stderr(&out)
    );
    assert!(folder.join("settings.json").exists());
}
