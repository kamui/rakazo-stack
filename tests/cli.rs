use std::process::Command;

fn rakazo_stack(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
        .args(args)
        .output()
        .expect("run rakazo-stack")
}

#[test]
fn version_prints_the_program_version() {
    let out = rakazo_stack(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("rakazo-stack {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unbuilt_commands_fail_and_say_so() {
    for command in ["setup", "update", "backup", "restore"] {
        let out = rakazo_stack(&[command]);
        assert!(!out.status.success(), "{command} should fail");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("not implemented yet"),
            "{command} should say it is not implemented"
        );
    }
}
