//! Preflight (spec §3): checks that run before anything on the machine changes.
//! `gather` reads the machine; `evaluate` turns what it found into stops and warnings.

use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use crate::docker::{ContainerSummary, Docker, Info, server_api_version};
use crate::overrides;
use crate::platform::{self, Platform};
use crate::resolve;
use crate::version::Version;

const MIN_API: &str = "1.45";
const MIN_COMPOSE: &str = "2.24.4";
const MIN_CPU_THREADS: usize = 4;
/// "16 GB" machines report a little less than 16 GiB once firmware takes its share.
const MIN_HOST_RAM: u64 = 15 * GIB;
const MIN_FREE_DISK: u64 = 35_000_000_000;
/// A 16 GB Windows host gives Docker Desktop's VM about 7.8 GiB, and a busy Instance
/// needs 7.1–7.6 GiB (#46).
const MIN_WSL_VM_MEMORY: u64 = 7 * GIB + GIB / 2;
const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Stop,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub message: String,
}

impl Finding {
    fn stop(message: impl Into<String>) -> Self {
        Self {
            level: Level::Stop,
            message: message.into(),
        }
    }

    fn warn(message: impl Into<String>) -> Self {
        Self {
            level: Level::Warn,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Setup,
    Update,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerState {
    /// No usable CLI: on WSL, no `docker.exe` was found.
    Missing,
    /// The CLI runs but the engine can't be reached.
    Unreachable { permission_denied: bool },
    Reachable {
        api: Option<Version>,
        compose: Option<Version>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortState {
    Free,
    HeldByInstance,
    Taken,
}

/// What `gather` found. `None` means the fact couldn't be read, which never stops a run
/// on its own.
#[derive(Debug, Clone)]
pub struct Facts {
    pub platform: Platform,
    pub command: Command,
    pub home: String,
    pub first_setup: bool,
    /// The Rakazo version `settings.json` records, if any.
    pub recorded_rakazo: Option<String>,
    pub wsl_interop: bool,
    pub docker: DockerState,
    pub info: Option<Info>,
    pub windows_edition: Option<String>,
    pub cpu_threads: Option<usize>,
    pub host_ram: Option<u64>,
    pub free_disk: Option<u64>,
    pub web_port_number: u16,
    pub web_port: Option<PortState>,
    pub github_reachable: Option<bool>,
}

pub fn evaluate(facts: &Facts) -> Vec<Finding> {
    let mut findings = Vec::new();
    let wsl = facts.platform == Platform::Wsl;

    if wsl && facts.home.starts_with("/mnt/") {
        findings.push(Finding::stop(format!(
            "Your home folder ({}) is on a Windows drive. Run rakazo-stack from your Linux home inside WSL.",
            facts.home
        )));
    }
    if wsl && !facts.wsl_interop {
        findings.push(Finding::stop(
            "WSL can't run Windows programs, so rakazo-stack can't reach Docker Desktop. \
             Turn interop on: remove `enabled = false` under [interop] in /etc/wsl.conf, \
             then run `wsl --shutdown` in Windows and open WSL again.",
        ));
    }

    match &facts.docker {
        DockerState::Missing => findings.push(Finding::stop(
            "Docker Desktop's docker.exe wasn't found. Install Docker Desktop for Windows \
             (https://docs.docker.com/desktop/setup/install/windows-install/) and start it.",
        )),
        DockerState::Unreachable {
            permission_denied: true,
        } => findings.push(Finding::stop(
            "Your user can't run `docker` without sudo. Add it to the docker group with \
             `sudo usermod -aG docker $USER`, then log out and back in \
             (https://docs.docker.com/engine/install/linux-postinstall/).",
        )),
        DockerState::Unreachable { .. } => findings.push(Finding::stop(match facts.platform {
            Platform::Linux => {
                "Docker isn't running. Start it with `sudo systemctl start docker`, then run this again."
            }
            Platform::Wsl => {
                "Docker Desktop isn't running, or it's in Windows containers mode. Start Docker \
                 Desktop and choose \"Switch to Linux containers\" if it offers that."
            }
            Platform::MacOs => "Docker Desktop isn't running. Start it, then run this again.",
        })),
        DockerState::Reachable { api, compose } => {
            findings.extend(check_versions(facts.platform, api.as_ref(), compose.as_ref()));
            if facts.platform == Platform::Linux {
                findings.push(Finding::warn(
                    "Your user runs `docker` through the docker group, which gives it full control of this computer.",
                ));
            }
        }
    }

    if let Some(info) = &facts.info {
        findings.extend(check_info(facts.platform, info));
    }
    if wsl
        && facts
            .windows_edition
            .as_deref()
            .is_some_and(is_windows_home)
    {
        findings.push(Finding::warn(
            "This is Windows Home. Docker no longer lists Home as supported, but Docker Desktop's WSL 2 backend runs on it.",
        ));
    }
    findings.extend(check_resources(facts));

    if facts.web_port == Some(PortState::Taken) {
        findings.push(Finding::stop(format!(
            "Port {} on 127.0.0.1, where Rakazo's web app listens, is already in use. Stop whatever uses it, then run this again.",
            facts.web_port_number
        )));
    }
    if facts.command == Command::Setup
        && let Some(tag) = facts
            .recorded_rakazo
            .as_deref()
            .filter(|t| resolve::below_floor(t))
    {
        findings.push(Finding::stop(format!(
            "This Instance runs Rakazo {tag}, below the {} this rakazo-stack needs. Run `rakazo-stack update`.",
            resolve::RAKAZO_FLOOR
        )));
    }
    if needs_github(facts.command, facts.first_setup) && facts.github_reachable == Some(false) {
        findings.push(Finding::stop(
            "GitHub can't be reached, and rakazo-stack needs it to find the newest releases. Check your internet connection.",
        ));
    }
    findings
}

/// Re-runs of `setup` keep their recorded versions, so only a first `setup` and
/// `update` look anything up on GitHub.
fn needs_github(command: Command, first_setup: bool) -> bool {
    match command {
        Command::Setup => first_setup,
        Command::Update => true,
        Command::Restore => false,
    }
}

fn check_versions(
    platform: Platform,
    api: Option<&Version>,
    compose: Option<&Version>,
) -> Option<Finding> {
    let min_api = Version::parse(MIN_API).expect("valid version");
    let min_compose = Version::parse(MIN_COMPOSE).expect("valid version");
    let api_ok = api.is_some_and(|v| *v >= min_api);
    let compose_ok = compose.is_some_and(|v| *v >= min_compose);
    if api_ok && compose_ok {
        return None;
    }
    let show = |v: Option<&Version>| v.map_or("none found".to_string(), Version::to_string);
    let how = match platform {
        Platform::Linux => {
            "Install Docker Engine and its Compose plugin from Docker's own repository: https://docs.docker.com/engine/install/"
        }
        Platform::Wsl | Platform::MacOs => "Update Docker Desktop to its newest version.",
    };
    Some(Finding::stop(format!(
        "rakazo-stack needs Docker Engine API {MIN_API} or later and Docker Compose {MIN_COMPOSE} or later. \
         This machine has API {} and Compose {}. {how}",
        show(api),
        show(compose)
    )))
}

fn check_info(platform: Platform, info: &Info) -> Vec<Finding> {
    let mut findings = Vec::new();
    match platform {
        Platform::Linux if info.is_docker_desktop() => findings.push(Finding::stop(
            "Docker Desktop for Linux is the active engine, and rakazo-stack doesn't support it. \
             Install Docker Engine from https://docs.docker.com/engine/install/ and switch to it \
             with `docker context use default`.",
        )),
        Platform::Wsl if info.os_type != "linux" => findings.push(Finding::stop(
            "Docker Desktop is in Windows containers mode. Choose \"Switch to Linux containers\" in Docker Desktop, then run this again.",
        )),
        _ => {}
    }
    if info.firewall_is_nftables() {
        findings.push(Finding::warn(
            "Docker's firewall backend is nftables, so bot computers won't start. Remove \
             `firewall-backend` from /etc/docker/daemon.json and restart Docker.",
        ));
    }
    if platform == Platform::Wsl && info.mem_total < MIN_WSL_VM_MEMORY {
        findings.push(Finding::warn(format!(
            "Docker Desktop's VM has {} of memory; an Instance with a busy bot computer needs about 7.6 GiB. \
             Raise `memory` in %UserProfile%\\.wslconfig, or add RAM.",
            gib(info.mem_total)
        )));
    }
    findings
}

fn check_resources(facts: &Facts) -> Vec<Finding> {
    let mut low = Vec::new();
    if let Some(threads) = facts.cpu_threads.filter(|t| *t < MIN_CPU_THREADS) {
        low.push(format!(
            "{threads} processor threads (minimum {MIN_CPU_THREADS})"
        ));
    }
    if let Some(ram) = facts.host_ram.filter(|r| *r < MIN_HOST_RAM) {
        low.push(format!("{} of RAM (minimum 16 GB)", gib(ram)));
    }
    if let Some(disk) = facts.free_disk.filter(|d| *d < MIN_FREE_DISK) {
        low.push(format!(
            "{:.1} GB free where Docker stores its data (minimum 35 GB)",
            disk as f64 / 1e9
        ));
    }
    if low.is_empty() {
        return Vec::new();
    }
    vec![Finding::warn(format!(
        "This computer is below rakazo-stack's minimums: {}. It may run slowly or run out of space.",
        low.join(", ")
    ))]
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

fn is_windows_home(edition: &str) -> bool {
    edition.starts_with("Core")
}

pub fn gather(
    platform: Platform,
    command: Command,
    home: &Path,
    first_setup: bool,
    recorded_rakazo: Option<String>,
) -> Facts {
    let docker = Docker::find(platform);
    let docker_state = docker.as_ref().map_or(DockerState::Missing, probe_docker);
    let reachable = matches!(docker_state, DockerState::Reachable { .. });
    let info = docker
        .as_ref()
        .filter(|_| reachable)
        .and_then(|d| run(d, &["info", "--format", "{{json .}}"]))
        .and_then(|out| Info::parse(&out));
    let wsl = platform == Platform::Wsl;
    let web_port_number = overrides::web_port();
    Facts {
        platform,
        command,
        home: home.display().to_string(),
        first_setup,
        recorded_rakazo,
        wsl_interop: !wsl || platform::wsl_interop_enabled(),
        free_disk: info.as_ref().and_then(|i| free_disk(platform, i, home)),
        web_port: docker
            .as_ref()
            .filter(|_| reachable)
            .map(|d| web_port_state(platform, d, web_port_number)),
        web_port_number,
        info,
        docker: docker_state,
        windows_edition: wsl.then(windows_edition).flatten(),
        cpu_threads: std::thread::available_parallelism().ok().map(Into::into),
        host_ram: host_ram(platform),
        github_reachable: needs_github(command, first_setup).then(github_reachable),
    }
}

fn probe_docker(docker: &Docker) -> DockerState {
    let Ok(out) = docker.output(["version", "--format", "{{json .}}"]) else {
        return DockerState::Unreachable {
            permission_denied: false,
        };
    };
    let api = server_api_version(&String::from_utf8_lossy(&out.stdout));
    if api.is_none() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        return DockerState::Unreachable {
            permission_denied: stderr.contains("permission denied"),
        };
    }
    let compose = run(docker, &["compose", "version", "--short"]).and_then(|v| Version::parse(&v));
    DockerState::Reachable { api, compose }
}

fn run(docker: &Docker, args: &[&str]) -> Option<String> {
    let out = docker.output(args).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Measured as preflight measures it, for setup's second check before the voice models.
pub fn docker_free_disk(platform: Platform, docker: &Docker, home: &Path) -> Option<u64> {
    let info =
        run(docker, &["info", "--format", "{{json .}}"]).and_then(|out| Info::parse(&out))?;
    free_disk(platform, &info, home)
}

/// Docker Engine stores data under its root dir. Docker Desktop keeps it in a disk
/// image on the host, so the host disk holding that image stands in for it.
fn free_disk(platform: Platform, info: &Info, home: &Path) -> Option<u64> {
    let path = match platform {
        Platform::Linux if !info.is_docker_desktop() => Path::new(&info.docker_root_dir),
        Platform::Wsl => Path::new("/mnt/c"),
        _ => home,
    };
    let stat = rustix::fs::statvfs(path).ok()?;
    Some(stat.f_bavail * stat.f_frsize)
}

fn host_ram(platform: Platform) -> Option<u64> {
    match platform {
        Platform::Linux => {
            let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
            let kib: u64 = meminfo
                .lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))?
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .ok()?;
            Some(kib * 1024)
        }
        // WSL's own memory is only a share of the host's, so ask Windows.
        Platform::Wsl => command_output(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ],
        )?
        .trim()
        .parse()
        .ok(),
        Platform::MacOs => command_output("sysctl", &["-n", "hw.memsize"])?
            .trim()
            .parse()
            .ok(),
    }
}

fn windows_edition() -> Option<String> {
    let out = command_output(
        "reg.exe",
        &[
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "/v",
            "EditionID",
        ],
    )?;
    out.lines()
        .find_map(|line| line.trim().strip_prefix("EditionID"))
        .and_then(|rest| rest.split_whitespace().nth(1))
        .map(str::to_string)
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn web_port_state(platform: Platform, docker: &Docker, port: u16) -> PortState {
    let containers = run(docker, &["ps", "--format", "{{json .}}"])
        .map(|out| ContainerSummary::parse_lines(&out))
        .unwrap_or_default();
    let holder = containers.iter().find(|c| c.publishes_port(port));
    if let Some(container) = holder {
        let ours = container.label("com.docker.compose.project")
            == Some(overrides::compose_project().as_str())
            && container.label("com.docker.compose.service") == Some("web");
        return if ours {
            PortState::HeldByInstance
        } else {
            PortState::Taken
        };
    }
    // On WSL a Windows program listening on the port is invisible from Linux.
    let windows_listener = platform == Platform::Wsl && windows_port_listening(port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let linux_listener = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok();
    if windows_listener || linux_listener {
        PortState::Taken
    } else {
        PortState::Free
    }
}

fn windows_port_listening(port: u16) -> bool {
    let Some(out) = command_output("netstat.exe", &["-an", "-p", "TCP"]) else {
        return false;
    };
    netstat_shows_listener(&out, port)
}

fn netstat_shows_listener(netstat: &str, port: u16) -> bool {
    let suffix = format!(":{port}");
    netstat.lines().any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        matches!(fields.as_slice(), [_, local, _, state, ..]
            if local.ends_with(&suffix) && state.eq_ignore_ascii_case("LISTENING"))
    })
}

fn github_reachable() -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into();
    // A second try, because one slow DNS answer on WSL has timed the first out.
    (0..2).any(|_| agent.get("https://api.github.com/").call().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_facts(platform: Platform) -> Facts {
        Facts {
            platform,
            command: Command::Setup,
            home: "/home/user".into(),
            first_setup: true,
            recorded_rakazo: None,
            wsl_interop: true,
            docker: DockerState::Reachable {
                api: Version::parse("1.56"),
                compose: Version::parse("5.7.1"),
            },
            info: Info::parse(
                r#"{"OSType":"linux","OperatingSystem":"Ubuntu 24.04 LTS","MemTotal":17000000000,
                    "DockerRootDir":"/var/lib/docker","FirewallBackend":{"Driver":"iptables"}}"#,
            ),
            windows_edition: Some("Professional".into()),
            cpu_threads: Some(8),
            host_ram: Some(32 * GIB),
            free_disk: Some(100_000_000_000),
            web_port_number: 5173,
            web_port: Some(PortState::Free),
            github_reachable: Some(true),
        }
    }

    fn stops(facts: &Facts) -> Vec<String> {
        evaluate(facts)
            .into_iter()
            .filter(|f| f.level == Level::Stop)
            .map(|f| f.message)
            .collect()
    }

    fn warns(facts: &Facts) -> Vec<String> {
        evaluate(facts)
            .into_iter()
            .filter(|f| f.level == Level::Warn)
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn a_healthy_machine_passes() {
        for platform in [Platform::Linux, Platform::Wsl, Platform::MacOs] {
            assert!(stops(&good_facts(platform)).is_empty(), "{platform:?}");
        }
        assert_eq!(
            warns(&good_facts(Platform::Linux)).len(),
            1,
            "docker group warning"
        );
        assert!(warns(&good_facts(Platform::Wsl)).is_empty());
    }

    #[test]
    fn api_1_44_stops() {
        let mut facts = good_facts(Platform::Linux);
        facts.docker = DockerState::Reachable {
            api: Version::parse("1.44"),
            compose: Version::parse("2.24.4"),
        };
        let stops = stops(&facts);
        assert_eq!(stops.len(), 1);
        assert!(stops[0].contains("API 1.44"), "{}", stops[0]);
        assert!(stops[0].contains("docs.docker.com/engine/install"));
    }

    #[test]
    fn compose_2_24_3_stops_and_missing_compose_stops() {
        let mut facts = good_facts(Platform::MacOs);
        facts.docker = DockerState::Reachable {
            api: Version::parse("1.45"),
            compose: Version::parse("2.24.3"),
        };
        assert!(stops(&facts)[0].contains("Compose 2.24.3"));
        facts.docker = DockerState::Reachable {
            api: Version::parse("1.45"),
            compose: None,
        };
        assert!(stops(&facts)[0].contains("Compose none found"));
    }

    #[test]
    fn docker_desktop_for_linux_stops() {
        let mut facts = good_facts(Platform::Linux);
        facts.info = Info::parse(
            r#"{"OSType":"linux","OperatingSystem":"Docker Desktop","MemTotal":8000000000,"DockerRootDir":"/var/lib/docker"}"#,
        );
        assert!(stops(&facts)[0].contains("Docker Desktop for Linux"));
        let mut wsl = good_facts(Platform::Wsl);
        wsl.info = facts.info.clone();
        assert!(stops(&wsl).is_empty(), "Docker Desktop is expected on WSL");
    }

    #[test]
    fn nftables_warns() {
        let mut facts = good_facts(Platform::Linux);
        facts.info = Info::parse(
            r#"{"OSType":"linux","OperatingSystem":"Debian","MemTotal":17000000000,"DockerRootDir":"/var/lib/docker","FirewallBackend":{"Driver":"nftables"}}"#,
        );
        assert!(stops(&facts).is_empty());
        assert!(warns(&facts).iter().any(|w| w.contains("nftables")));
    }

    #[test]
    fn windows_containers_mode_stops() {
        let mut facts = good_facts(Platform::Wsl);
        facts.info = Info::parse(
            r#"{"OSType":"windows","OperatingSystem":"Windows 11 Pro","MemTotal":17000000000,"DockerRootDir":"C:\\ProgramData\\docker"}"#,
        );
        assert!(stops(&facts)[0].contains("Windows containers mode"));
    }

    #[test]
    fn a_mnt_home_stops_on_wsl_only() {
        let mut facts = good_facts(Platform::Wsl);
        facts.home = "/mnt/c/Users/someone".into();
        assert!(stops(&facts)[0].contains("Windows drive"));
        let mut linux = good_facts(Platform::Linux);
        linux.home = "/mnt/data/home".into();
        assert!(stops(&linux).is_empty());
    }

    #[test]
    fn wsl_interop_and_docker_exe_stop() {
        let mut facts = good_facts(Platform::Wsl);
        facts.wsl_interop = false;
        facts.docker = DockerState::Missing;
        let stops = stops(&facts);
        assert!(stops.iter().any(|s| s.contains("interop")));
        assert!(stops.iter().any(|s| s.contains("docker.exe")));
    }

    #[test]
    fn docker_without_sudo_stops_with_instructions() {
        let mut facts = good_facts(Platform::Linux);
        facts.docker = DockerState::Unreachable {
            permission_denied: true,
        };
        facts.info = None;
        let stops = stops(&facts);
        assert_eq!(stops.len(), 1);
        assert!(stops[0].contains("usermod -aG docker"));
        assert!(!warns(&facts).iter().any(|w| w.contains("docker group")));
    }

    #[test]
    fn windows_home_warns() {
        let mut facts = good_facts(Platform::Wsl);
        facts.windows_edition = Some("CoreSingleLanguage".into());
        assert!(warns(&facts).iter().any(|w| w.contains("Windows Home")));
    }

    #[test]
    fn low_resources_warn_once() {
        let mut facts = good_facts(Platform::MacOs);
        facts.cpu_threads = Some(2);
        facts.host_ram = Some(8 * GIB);
        facts.free_disk = Some(20_000_000_000);
        let warns = warns(&facts);
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("2 processor threads"));
        assert!(warns[0].contains("8.0 GiB of RAM"));
        assert!(warns[0].contains("20.0 GB free"));
        assert!(stops(&facts).is_empty());
    }

    #[test]
    fn a_16_gb_machine_does_not_warn_about_ram() {
        let mut facts = good_facts(Platform::Linux);
        facts.host_ram = Some(16_197_784 * 1024);
        assert!(!warns(&facts).iter().any(|w| w.contains("RAM")));
    }

    #[test]
    fn low_wsl_vm_memory_warns() {
        let mut facts = good_facts(Platform::Wsl);
        facts.info = Info::parse(
            r#"{"OSType":"linux","OperatingSystem":"Docker Desktop","MemTotal":4000000000,"DockerRootDir":"/var/lib/docker"}"#,
        );
        assert!(warns(&facts).iter().any(|w| w.contains(".wslconfig")));
    }

    #[test]
    fn a_taken_web_port_stops_but_the_instances_own_does_not() {
        let mut facts = good_facts(Platform::Linux);
        facts.web_port = Some(PortState::Taken);
        assert!(stops(&facts)[0].contains("5173"));
        facts.web_port = Some(PortState::HeldByInstance);
        assert!(stops(&facts).is_empty());
    }

    #[test]
    fn github_is_needed_on_a_first_setup_and_update_only() {
        let mut facts = good_facts(Platform::Linux);
        facts.github_reachable = Some(false);
        assert!(stops(&facts)[0].contains("GitHub"));
        facts.command = Command::Update;
        facts.first_setup = false;
        assert!(!stops(&facts).is_empty());
        facts.command = Command::Setup;
        assert!(stops(&facts).is_empty());
        facts.command = Command::Restore;
        assert!(stops(&facts).is_empty());
    }

    #[test]
    fn recorded_versions_below_the_floor_stop_setup_only() {
        let mut facts = good_facts(Platform::Linux);
        facts.first_setup = false;
        facts.recorded_rakazo = Some("v0.15.0".into());
        assert!(stops(&facts)[0].contains("Run `rakazo-stack update`"));
        facts.command = Command::Update;
        assert!(stops(&facts).is_empty());
        facts.command = Command::Setup;
        facts.recorded_rakazo = Some("v0.16.0".into());
        assert!(stops(&facts).is_empty());
    }

    #[test]
    fn reads_windows_netstat() {
        let netstat = "\r\nActive Connections\r\n\r\n  Proto  Local Address          Foreign Address        State\r\n  TCP    127.0.0.1:5173         0.0.0.0:0              LISTENING\r\n  TCP    10.0.0.5:51733         10.0.0.9:443           ESTABLISHED\r\n";
        assert!(netstat_shows_listener(netstat, 5173));
        assert!(!netstat_shows_listener(netstat, 5174));
        assert!(!netstat_shows_listener(
            "  TCP    10.0.0.5:5173   10.0.0.9:443   ESTABLISHED",
            5173
        ));
    }
}
