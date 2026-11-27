//! The computer-egress sidecar (spec §12): its build folder in the Instance folder, and
//! setup's egress check (spec §7, step 11).

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::checklist::Checklist;
use crate::docker::Docker;
use crate::instance::InstanceFolder;
use crate::runlog::RunLog;

pub const SERVICE: &str = "computer-egress";
/// Rakazo's script, fetched from the recorded tag and built into the image unmodified.
pub const UPSTREAM_SCRIPT: &str = "infra/compose/restrict-computer-egress.sh";
pub const SCRIPT: &str = "computer-egress/restrict-computer-egress.sh";
const BUILD_FILES: [(&str, &[u8]); 2] = [
    (
        "computer-egress/Dockerfile",
        include_bytes!("../computer-egress/Dockerfile"),
    ),
    (
        "computer-egress/computer-egress.sh",
        include_bytes!("../computer-egress/computer-egress.sh"),
    ),
];

/// What Rakazo's supervisor logs once its probe shows the rules are in place.
const VERIFIED: &str = "restricted computer egress verified";
const PROBE_FAILED: &str = "computer egress probe failed";
const NOT_ENFORCED: &str = "SANDBOX_COMPUTER_EGRESS=restricted is not enforced";
const APPLIED: &str = "computer egress rules applied";
/// The wrapper's own status lines start with this; the script's errors don't.
const WRAPPER_STATUS: &str = "computer egress rules ";
/// The supervisor's probe gives up after 60 s, after it has checked the computer image.
const PROBE_WAIT: Duration = Duration::from_secs(90);

pub const NFTABLES_ITEM: &str = "Docker's firewall backend is nftables, so bot computers won't start. \
     Remove `firewall-backend` from /etc/docker/daemon.json, restart Docker, then run \
     `rakazo-stack setup` again.";

pub fn write_build_folder(folder: &InstanceFolder, script: &[u8]) -> io::Result<()> {
    folder.write_build_file(SCRIPT, script)?;
    for (name, contents) in BUILD_FILES {
        folder.write_build_file(name, contents)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Probe {
    Verified,
    Failed(String),
    Pending,
}

/// The supervisor's verdict, from its log since it last started. Only success is cached
/// upstream, so a later `verified` overrides an earlier failure.
fn probe_in(log: &str) -> Probe {
    if log.contains(VERIFIED) {
        return Probe::Verified;
    }
    log.lines()
        .rev()
        .find(|line| line.contains(PROBE_FAILED))
        .map_or(Probe::Pending, |line| Probe::Failed(failure_reason(line)))
}

/// The error from the supervisor's JSON log line. Upstream's "not enforced" message ends
/// with advice that includes setting egress to `open`, so only its reason is kept.
fn failure_reason(line: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|entry| entry["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| line.trim().to_string());
    if message.starts_with(NOT_ENFORCED)
        && let Some((_, reason)) = message.rsplit_once(" (")
    {
        return reason.trim_end_matches(')').to_string();
    }
    message
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Sidecar {
    /// `docker inspect`'s state, such as `running` or `exited`; `None` with no container.
    status: Option<String>,
    log: String,
}

impl Sidecar {
    fn running(&self) -> bool {
        self.status.as_deref() == Some("running")
    }

    /// Whether its latest pass applied the rules. The wrapper reports only changes.
    fn applied(&self) -> bool {
        self.log
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .is_some_and(|line| line.starts_with(APPLIED))
    }

    /// The script's last message, which names what's wrong.
    fn last_error(&self) -> Option<&str> {
        self.log
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty() && !line.starts_with(WRAPPER_STATUS))
    }
}

fn cause(nftables: bool, sidecar: &Sidecar, probe: &Probe) -> String {
    if nftables {
        return "Docker's firewall backend is nftables, which has no DOCKER-USER chain".into();
    }
    if !sidecar.running() {
        return match &sidecar.status {
            Some(status) => format!("the computer-egress sidecar isn't running ({status})"),
            None => "the computer-egress sidecar doesn't exist".into(),
        };
    }
    if !sidecar.applied() {
        return match sidecar.last_error() {
            Some(error) => format!("the computer-egress sidecar can't apply its rules: {error}"),
            None => "the computer-egress sidecar hasn't applied its rules yet".into(),
        };
    }
    match probe {
        Probe::Failed(line) => format!("the supervisor's check failed: {line}"),
        _ => format!(
            "the supervisor didn't finish its check within {} s",
            PROBE_WAIT.as_secs()
        ),
    }
}

fn not_verified_item(cause: &str) -> String {
    format!(
        "Bot computers are off: restricted egress isn't verified, because {cause}. \
         Fix that, then run `rakazo-stack setup` again."
    )
}

/// Step 11: never a fallback to open egress, only a Checklist item when it isn't verified.
pub fn check(
    docker: &Docker,
    folder: &Path,
    nftables: bool,
    log: &RunLog,
    checklist: &mut Checklist,
) -> io::Result<()> {
    if nftables {
        checklist.add(NFTABLES_ITEM);
    }
    let compose = Compose { docker, folder };
    let mut probe = compose.wait_for_probe()?;
    if matches!(probe, Probe::Failed(_)) && compose.sidecar()?.applied() {
        // On a Docker restart both start at once, and the supervisor probes only at start
        // and before a computer starts, so a probe that lost the race is repeated here.
        log.say("Restarting the supervisor so it checks the computer-egress rules again…");
        compose.run(&["restart", "supervisor"])?;
        probe = compose.wait_for_probe()?;
    }
    if probe == Probe::Verified {
        log.say("Restricted computer egress is verified.");
    } else {
        checklist.add(not_verified_item(&cause(
            nftables,
            &compose.sidecar()?,
            &probe,
        )));
    }
    Ok(())
}

struct Compose<'a> {
    docker: &'a Docker,
    folder: &'a Path,
}

impl Compose<'_> {
    fn run(&self, args: &[&str]) -> io::Result<String> {
        let out = self.docker.compose(self.folder, args).output()?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "`docker compose {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn docker(&self, args: &[&str]) -> io::Result<String> {
        let out = self.docker.output(args)?;
        Ok(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .replace("\r\n", "\n"))
    }

    /// A service's container state and its log since it last started.
    fn current_run(&self, service: &str) -> io::Result<Option<(String, String)>> {
        let id = self.run(&["ps", "-a", "-q", service])?;
        let Some(id) = id.lines().next().map(str::trim).filter(|id| !id.is_empty()) else {
            return Ok(None);
        };
        let state = self.docker(&[
            "inspect",
            "--format",
            "{{.State.Status}} {{.State.StartedAt}}",
            id,
        ])?;
        let (status, started) = state.trim().split_once(' ').unwrap_or((state.trim(), ""));
        let log = self.docker(&["logs", "--since", started, id])?;
        Ok(Some((status.to_string(), log)))
    }

    fn sidecar(&self) -> io::Result<Sidecar> {
        Ok(match self.current_run(SERVICE)? {
            Some((status, log)) => Sidecar {
                status: Some(status),
                log,
            },
            None => Sidecar {
                status: None,
                log: String::new(),
            },
        })
    }

    fn wait_for_probe(&self) -> io::Result<Probe> {
        let deadline = Instant::now() + PROBE_WAIT;
        loop {
            let probe = self
                .current_run("supervisor")?
                .map_or(Probe::Pending, |(_, log)| probe_in(&log));
            if probe != Probe::Pending || Instant::now() >= deadline {
                return Ok(probe);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar(status: Option<&str>, log: &str) -> Sidecar {
        Sidecar {
            status: status.map(str::to_string),
            log: log.to_string(),
        }
    }

    /// As Rakazo v0.16.0's supervisor logs it, with the stack trace shortened.
    const PROBE_FAILED_LINE: &str = r#"{"timestamp":"2026-01-01T00:00:00.000Z","level":"error","message":"computer egress probe failed","service.name":"rakazo-sandbox-supervisor","error":{"name":"Error","message":"SANDBOX_COMPUTER_EGRESS=restricted is not enforced on this Docker host: run infra/compose/restrict-computer-egress.sh on the host, or set SANDBOX_COMPUTER_EGRESS=open (see docs/self-host-docker-computers.md) (a computer bridge reached the host)","stack":"Error: ..."}}"#;
    const VERIFIED_LINE: &str = r#"{"timestamp":"2026-01-01T00:00:10.000Z","level":"info","message":"restricted computer egress verified","service.name":"rakazo-sandbox-supervisor"}"#;

    #[test]
    fn a_later_verified_overrides_an_earlier_failure() {
        assert_eq!(
            probe_in(&format!("{PROBE_FAILED_LINE}\n{VERIFIED_LINE}\n")),
            Probe::Verified
        );
        assert_eq!(
            probe_in(&format!("supervisor listening\n{PROBE_FAILED_LINE}\n")),
            Probe::Failed("a computer bridge reached the host".into())
        );
        assert_eq!(probe_in("supervisor listening\n"), Probe::Pending);
    }

    #[test]
    fn keeps_other_failures_whole() {
        let line = r#"{"message":"computer egress probe failed","error":{"message":"connect ENOENT /var/run/docker.sock"}}"#;
        assert_eq!(failure_reason(line), "connect ENOENT /var/run/docker.sock");
        assert_eq!(
            failure_reason("computer egress probe failed: boom"),
            "computer egress probe failed: boom"
        );
    }

    #[test]
    fn reads_the_sidecars_latest_pass() {
        let applied = sidecar(
            Some("running"),
            "computer egress rules applied with iptables\n",
        );
        assert!(applied.running() && applied.applied());
        let failed = sidecar(
            Some("running"),
            "computer egress rules applied with iptables\n\
             DOCKER-USER chain not present — is Docker running with the iptables backend?\n\
             computer egress rules could not be applied; trying again in 60 s\n",
        );
        assert!(!failed.applied());
        assert_eq!(
            failed.last_error(),
            Some("DOCKER-USER chain not present — is Docker running with the iptables backend?")
        );
    }

    #[test]
    fn names_the_first_cause_that_applies() {
        let applied = sidecar(
            Some("running"),
            "computer egress rules applied with iptables\n",
        );
        let failed = Probe::Failed("a computer bridge reached the host".into());
        assert!(cause(true, &applied, &failed).contains("nftables"));
        assert_eq!(
            cause(false, &sidecar(Some("exited"), ""), &failed),
            "the computer-egress sidecar isn't running (exited)"
        );
        assert_eq!(
            cause(false, &sidecar(None, ""), &failed),
            "the computer-egress sidecar doesn't exist"
        );
        assert!(
            cause(
                false,
                &sidecar(Some("running"), "iptables not found\n"),
                &failed
            )
            .ends_with("can't apply its rules: iptables not found")
        );
        assert_eq!(
            cause(false, &applied, &failed),
            "the supervisor's check failed: a computer bridge reached the host"
        );
        assert!(cause(false, &applied, &Probe::Pending).contains("within 90 s"));
    }

    #[test]
    fn the_item_says_computers_are_off_and_never_offers_open_egress() {
        let item = not_verified_item("the computer-egress sidecar isn't running (exited)");
        assert!(item.starts_with("Bot computers are off: restricted egress isn't verified"));
        assert!(!item.contains("open") && !item.contains("auto"));
    }

    #[test]
    fn the_build_folder_holds_the_script_unmodified() {
        let home = tempfile::tempdir().unwrap();
        let folder = InstanceFolder::for_home(home.path());
        let _lock = folder.lock().unwrap();
        let script = b"#!/usr/bin/env bash\necho upstream\n";
        write_build_folder(&folder, script).unwrap();
        assert_eq!(folder.read(SCRIPT).unwrap().unwrap(), script);
        let dockerfile = folder.read("computer-egress/Dockerfile").unwrap().unwrap();
        let dockerfile = String::from_utf8(dockerfile).unwrap();
        assert!(dockerfile.contains("COPY restrict-computer-egress.sh "));
        assert!(dockerfile.contains("iptables-legacy"));
    }
}
