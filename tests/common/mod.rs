use std::fs;
use std::path::Path;
use std::process::Command;

pub fn is_wsl() -> bool {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .is_ok_and(|release| release.to_lowercase().contains("microsoft"))
}

/// The same Docker CLI the program drives: Docker Desktop's `docker.exe` on WSL.
pub fn docker() -> Command {
    if !is_wsl() {
        return Command::new("docker");
    }
    let program = [
        "/Docker/host/bin/docker.exe",
        "/mnt/c/Program Files/Docker/Docker/resources/bin/docker.exe",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists())
    .unwrap_or("docker.exe");
    let mut command = Command::new(program);
    command.args(["--context", "desktop-linux"]);
    command
}
