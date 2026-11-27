use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Deserialize;

use crate::compose_env;
use crate::platform::Platform;
use crate::version::Version;

/// The Docker CLI every command goes through. On WSL it is Windows' `docker.exe`
/// pinned to Docker Desktop's Linux engine (spec §7).
#[derive(Debug, Clone)]
pub struct Docker {
    program: PathBuf,
    context: Option<&'static str>,
}

/// Docker Desktop's Linux engine. Pinning it means a context switched on the Windows
/// side can't send the Instance to another engine.
const WSL_CONTEXT: &str = "desktop-linux";

const WSL_DOCKER_EXE_FALLBACKS: [&str; 2] = [
    "/Docker/host/bin/docker.exe",
    "/mnt/c/Program Files/Docker/Docker/resources/bin/docker.exe",
];

impl Docker {
    /// Finds the CLI for this platform. On WSL that is `docker.exe` on `PATH`, then
    /// the two places Docker Desktop installs it; `None` when none exists.
    pub fn find(platform: Platform) -> Option<Self> {
        match platform {
            Platform::Linux | Platform::MacOs => Some(Self {
                program: PathBuf::from("docker"),
                context: None,
            }),
            Platform::Wsl => find_on_path("docker.exe")
                .or_else(|| {
                    WSL_DOCKER_EXE_FALLBACKS
                        .iter()
                        .map(PathBuf::from)
                        .find(|path| path.is_file())
                })
                .map(|program| Self {
                    program,
                    context: Some(WSL_CONTEXT),
                }),
        }
    }

    /// A command with the context flag already set. Environment variables don't reach
    /// `docker.exe`, so callers pass everything as arguments, files or stdin. The
    /// program's own `RAKAZO_STACK_*` variables, which can hold the Owner password and
    /// pasted keys, are kept out of Docker's environment.
    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.program);
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"RAKAZO_STACK_") {
                command.env_remove(name);
            }
        }
        if let Some(context) = self.context {
            command.args(["--context", context]);
        }
        command.args(args);
        command
    }

    /// `docker compose` run from the Instance folder, without the shell variables that
    /// would take the place of `.env`'s values.
    pub fn compose<I, S>(&self, folder: &Path, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command(["compose"]);
        command.args(args).current_dir(folder);
        let environment = std::env::vars_os().filter_map(|(name, _)| name.into_string().ok());
        for name in compose_env::shadowing(folder, environment) {
            command.env_remove(name);
        }
        command
    }

    pub fn output<I, S>(&self, args: I) -> std::io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command(args).output()
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_file(candidate))
}

fn is_file(path: &Path) -> bool {
    path.metadata().map(|m| m.is_file()).unwrap_or(false)
}

/// The server half of `docker version --format '{{json .}}'`.
pub fn server_api_version(version_json: &str) -> Option<Version> {
    #[derive(Deserialize)]
    struct Output {
        #[serde(rename = "Server")]
        server: Option<Server>,
    }
    #[derive(Deserialize)]
    struct Server {
        #[serde(rename = "ApiVersion")]
        api_version: String,
    }
    let output: Output = serde_json::from_str(version_json).ok()?;
    Version::parse(&output.server?.api_version)
}

/// The fields of `docker info --format '{{json .}}'` that preflight reads.
#[derive(Debug, Clone, Deserialize)]
pub struct Info {
    #[serde(rename = "OSType")]
    pub os_type: String,
    #[serde(rename = "OperatingSystem")]
    pub operating_system: String,
    #[serde(rename = "MemTotal")]
    pub mem_total: u64,
    #[serde(rename = "DockerRootDir")]
    pub docker_root_dir: String,
    #[serde(rename = "FirewallBackend")]
    pub firewall_backend: Option<FirewallBackend>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirewallBackend {
    #[serde(rename = "Driver")]
    pub driver: String,
}

impl Info {
    pub fn parse(info_json: &str) -> Option<Self> {
        serde_json::from_str(info_json).ok()
    }

    pub fn is_docker_desktop(&self) -> bool {
        self.operating_system == "Docker Desktop"
    }

    pub fn firewall_is_nftables(&self) -> bool {
        self.firewall_backend
            .as_ref()
            .is_some_and(|backend| backend.driver == "nftables")
    }
}

/// One line of `docker ps --format '{{json .}}'`.
#[derive(Debug, Deserialize)]
pub struct ContainerSummary {
    #[serde(rename = "Labels", default)]
    pub labels: String,
    #[serde(rename = "Ports", default)]
    pub ports: String,
}

impl ContainerSummary {
    pub fn parse_lines(ps_output: &str) -> Vec<Self> {
        ps_output
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub fn publishes_port(&self, port: u16) -> bool {
        let needle = format!(":{port}->");
        self.ports
            .split(", ")
            .any(|mapping| mapping.contains(&needle))
    }

    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels
            .split(',')
            .filter_map(|pair| pair.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_server_api_version() {
        let json =
            r#"{"Client":{"ApiVersion":"1.56"},"Server":{"Version":"26.1.0","ApiVersion":"1.44"}}"#;
        assert_eq!(server_api_version(json), Version::parse("1.44"));
        let no_server = r#"{"Client":{"ApiVersion":"1.56"},"Server":null}"#;
        assert_eq!(server_api_version(no_server), None);
    }

    #[test]
    fn reads_info() {
        let json = r#"{"OSType":"linux","OperatingSystem":"Ubuntu 24.04.1 LTS","MemTotal":16586530816,
            "DockerRootDir":"/var/lib/docker","FirewallBackend":{"Driver":"nftables"},"NCPU":4}"#;
        let info = Info::parse(json).unwrap();
        assert!(info.firewall_is_nftables());
        assert!(!info.is_docker_desktop());
        let older = r#"{"OSType":"linux","OperatingSystem":"Docker Desktop","MemTotal":1,"DockerRootDir":"/var/lib/docker"}"#;
        let info = Info::parse(older).unwrap();
        assert!(!info.firewall_is_nftables());
        assert!(info.is_docker_desktop());
    }

    #[test]
    fn reads_published_ports_and_labels() {
        let ps = concat!(
            r#"{"Labels":"com.docker.compose.project=rakazo-stack,com.docker.compose.service=web","Ports":"127.0.0.1:5173->5173/tcp"}"#,
            "\n",
            r#"{"Labels":"","Ports":"0.0.0.0:8080->80/tcp, [::]:8080->80/tcp"}"#,
            "\n"
        );
        let containers = ContainerSummary::parse_lines(ps);
        assert_eq!(containers.len(), 2);
        assert!(containers[0].publishes_port(5173));
        assert_eq!(
            containers[0].label("com.docker.compose.service"),
            Some("web")
        );
        assert!(!containers[1].publishes_port(5173));
        assert!(containers[1].publishes_port(8080));
        assert!(!containers[1].publishes_port(80));
    }

    #[test]
    fn pins_the_desktop_context_on_wsl_only() {
        let docker = Docker {
            program: PathBuf::from("docker.exe"),
            context: Some(WSL_CONTEXT),
        };
        let command = docker.command(["info"]);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["--context", "desktop-linux", "info"]);
        let linux = Docker::find(Platform::Linux).unwrap();
        let args: Vec<_> = linux
            .command(["info"])
            .get_args()
            .map(ToOwned::to_owned)
            .collect();
        assert_eq!(args, ["info"]);
    }
}
