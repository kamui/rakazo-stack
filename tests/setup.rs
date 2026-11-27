//! `setup` against real Docker: starts Rakazo on localhost and tears it down afterwards.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn is_wsl() -> bool {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .is_ok_and(|release| release.to_lowercase().contains("microsoft"))
}

/// The same Docker CLI the program drives: Docker Desktop's `docker.exe` on WSL.
fn docker() -> Command {
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

/// One test Instance: its own HOME, Compose project and web port, removed on drop with
/// its volumes and networks.
struct TestInstance {
    home: tempfile::TempDir,
    project: String,
    web_port: u16,
}

impl TestInstance {
    fn new() -> Self {
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            home: tempfile::tempdir().unwrap(),
            project: format!("rkst-test-{}", std::process::id()),
            web_port,
        }
    }

    fn folder(&self) -> PathBuf {
        self.home.path().join("rakazo-stack")
    }

    fn setup(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rakazo-stack"))
            .arg("setup")
            .env("HOME", self.home.path())
            .env("RAKAZO_STACK_TEST_PROJECT", &self.project)
            .env("RAKAZO_STACK_TEST_WEB_PORT", self.web_port.to_string())
            .output()
            .expect("run rakazo-stack")
    }

    fn containers(&self, format: &str) -> Vec<String> {
        let filter = format!("label=com.docker.compose.project={}", self.project);
        let out = docker()
            .args(["ps", "-a", "--filter", &filter, "--format", format])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }
}

impl Drop for TestInstance {
    fn drop(&mut self) {
        let mut down = docker();
        down.args([
            "compose",
            "-p",
            &self.project,
            "down",
            "-v",
            "--remove-orphans",
        ]);
        if self.folder().exists() {
            down.current_dir(self.folder());
        }
        let _ = down.output();
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn secret_values(folder: &Path) -> Vec<String> {
    fs::read_to_string(folder.join("secrets.env"))
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(_, value)| value.to_string())
        .collect()
}

#[test]
#[ignore = "needs Docker and pulls Rakazo's images; CI and the dev machine run it with --include-ignored"]
fn setup_starts_rakazo_on_localhost_and_a_second_run_changes_nothing() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    let origin = format!("http://127.0.0.1:{}", instance.web_port);

    let first = instance.setup();
    let first_text = text(&first);
    assert!(first.status.success(), "{first_text}");
    assert!(
        first_text.contains(&format!("is running at {origin}")),
        "{first_text}"
    );
    let agent = ureq::Agent::new_with_defaults();
    assert!(agent.get(&origin).call().unwrap().status().is_success());

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    let rakazo = &settings["components"]["rakazo"];
    let tag = rakazo["version"].as_str().unwrap();
    assert!(tag.starts_with('v'), "{settings}");
    for image in ["app", "computer"] {
        assert!(
            rakazo["images"][image]
                .as_str()
                .unwrap()
                .starts_with("sha256:"),
            "{settings}"
        );
    }
    assert_eq!(settings["program_version"], env!("CARGO_PKG_VERSION"));

    let upstream = agent
        .get(format!(
            "https://raw.githubusercontent.com/elie222/rakazo/{tag}/infra/compose/docker-compose.images.yml"
        ))
        .call()
        .unwrap()
        .into_body()
        .read_to_vec()
        .unwrap();
    assert_eq!(
        fs::read(folder.join(format!("rakazo.{tag}.images.yml"))).unwrap(),
        upstream,
        "Rakazo's compose file must be written byte for byte"
    );

    let published: Vec<String> = instance
        .containers("{{.Label \"com.docker.compose.service\"}} {{.Ports}}")
        .iter()
        .filter_map(|line| line.split_once(' '))
        .flat_map(|(service, ports)| {
            ports
                .split(", ")
                .filter(|port| port.contains("->"))
                .map(move |port| format!("{service} {port}"))
        })
        .collect();
    assert_eq!(
        published,
        [format!("web 127.0.0.1:{}->5173/tcp", instance.web_port)],
        "only the web port is published"
    );

    let ps = docker()
        .args(["compose", "ps", "--format", "{{.Service}}"])
        .current_dir(&folder)
        .output()
        .unwrap();
    assert!(ps.status.success(), "{}", text(&ps));
    assert!(
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .any(|s| s == "web"),
        "`docker compose ps` from the Instance folder sees the project: {}",
        text(&ps)
    );

    if is_wsl() && Path::new("/usr/bin/docker").exists() {
        let wsl_up = Command::new("/usr/bin/docker")
            .args([
                "compose",
                "-p",
                &instance.project,
                "up",
                "-d",
                "--no-deps",
                "--force-recreate",
                "worker",
            ])
            .current_dir(&folder)
            .output()
            .unwrap();
        assert!(wsl_up.status.success(), "{}", text(&wsl_up));
    }

    let secrets_before = fs::read(folder.join("secrets.env")).unwrap();
    let settings_before = fs::read(folder.join("settings.json")).unwrap();
    let second = instance.setup();
    let second_text = text(&second);
    assert!(second.status.success(), "{second_text}");
    assert_eq!(
        fs::read(folder.join("secrets.env")).unwrap(),
        secrets_before
    );
    assert_eq!(
        fs::read(folder.join("settings.json")).unwrap(),
        settings_before
    );

    let made_in_wsl: Vec<String> = instance
        .containers("{{.Label \"com.docker.compose.project.working_dir\"}}")
        .into_iter()
        .filter(|dir| is_wsl() && dir.starts_with('/'))
        .collect();
    assert!(made_in_wsl.is_empty(), "{made_in_wsl:?}");

    for secret in secret_values(&folder) {
        assert!(!first_text.contains(&secret) && !second_text.contains(&secret));
    }
}
