//! `rakazo-stack logs` (spec §17): one file for the Self-hoster to attach to an issue. It
//! holds the newest run log of each command, `lemond.log`, the Instance's containers with
//! the last lines of each service's log, and Docker's version and uptime, all through the
//! run logs' replacements.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::docker::Docker;
use crate::instance::InstanceFolder;
use crate::lemond;
use crate::overrides;
use crate::platform::Platform;
use crate::redact::Redactions;
use crate::runlog::{self, KEPT_PER_COMMAND, now, utc_stamp, utc_time};

pub const FILE_PREFIX: &str = "rakazo-stack-logs-";
const FILE_SUFFIX: &str = ".txt";
const RUN_LOG_COMMANDS: [&str; 4] = ["setup", "update", "backup", "restore"];
const SERVICE_LINES: &str = "200";
const LEMOND_LINES: usize = 500;

pub const READ_FIRST: &str = "Secrets and personal values in it are replaced, but a log line can still hold something no rule knows about, so read the file before you post it.";

/// Writes the file and returns its path, with a note when Docker didn't answer.
pub fn run(
    platform: Platform,
    folder: &InstanceFolder,
) -> Result<(PathBuf, Option<String>), String> {
    if !folder.has_settings() {
        return Err("there is no Instance here: ~/rakazo-stack has no settings.json.".into());
    }
    let mut report = Report::default();
    report.line(&format!("rakazo-stack logs, written {}", utc_time(now())));
    report.line(READ_FIRST);
    report.section("rakazo-stack");
    for line in runlog::header(platform) {
        report.line(&line);
    }
    let docker_missing = report.docker(platform, folder.path());
    let dir = folder.file(runlog::DIR);
    for command in RUN_LOG_COMMANDS {
        report.run_log(&dir, command);
    }
    if platform == Platform::MacOs {
        report.lemond(&dir);
    }

    let name = format!("{FILE_PREFIX}{}{FILE_SUFFIX}", utc_stamp(now()));
    let path = dir.join(&name);
    let redacted = Redactions::for_folder(folder.path()).apply(&report.text);
    runlog::create_private(&dir, &name)
        .and_then(|mut file| file.write_all(redacted.as_bytes()))
        .map_err(|err| format!("can't write {}: {err}", path.display()))?;
    runlog::prune(&dir, FILE_PREFIX, FILE_SUFFIX, KEPT_PER_COMMAND);
    Ok((path, docker_missing))
}

#[derive(Default)]
struct Report {
    text: String,
}

impl Report {
    fn line(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
    }

    fn section(&mut self, title: &str) {
        self.text.push_str(&format!("\n===== {title} =====\n"));
    }

    fn output(&mut self, out: &std::io::Result<Output>) {
        match out {
            Ok(out) => {
                for stream in [&out.stdout, &out.stderr] {
                    let text = String::from_utf8_lossy(stream).replace("\r\n", "\n");
                    self.text.push_str(&text);
                    if !text.is_empty() && !text.ends_with('\n') {
                        self.text.push('\n');
                    }
                }
                if !out.status.success() {
                    self.line(&format!("({})", out.status));
                }
            }
            Err(err) => self.line(&format!("(didn't run: {err})")),
        }
    }

    /// Docker's sections, or why they're missing.
    fn docker(&mut self, platform: Platform, folder: &Path) -> Option<String> {
        self.section("docker version");
        let Some(docker) = Docker::find(platform) else {
            return Some(self.containers_missing("no Docker CLI was found"));
        };
        let version = docker.command(["version"]).output();
        self.output(&version);
        match &version {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                let why = last_line(&out.stderr).unwrap_or_else(|| out.status.to_string());
                return Some(self.containers_missing(&format!("Docker isn't answering ({why})")));
            }
            Err(err) => {
                return Some(self.containers_missing(&format!("`docker` didn't run ({err})")));
            }
        }
        self.line(&format!(
            "Docker's VM: {}",
            vm_uptime(platform, &docker, folder)
        ));

        let compose = |args: &[&str]| docker.compose(folder, args).stdin(Stdio::null()).output();
        self.section("docker compose ps -a");
        self.output(&compose(&["ps", "-a"]));
        let services = compose(&["ps", "-a", "--format", "{{.Service}}"])
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        let mut services: Vec<&str> = services
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        services.sort_unstable();
        services.dedup();
        for service in services {
            self.section(&format!(
                "docker compose logs {service}, last {SERVICE_LINES} lines"
            ));
            self.output(&compose(&[
                "logs",
                "--no-color",
                "--timestamps",
                "--tail",
                SERVICE_LINES,
                service,
            ]));
        }
        None
    }

    fn containers_missing(&mut self, why: &str) -> String {
        let message = format!(
            "The container logs are missing, because {why}. Start Docker, then run `rakazo-stack logs` again to add them."
        );
        self.section("Container logs");
        self.line(&message);
        message
    }

    fn run_log(&mut self, dir: &Path, command: &str) {
        let prefix = format!("{command}-");
        let newest = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with(&prefix) && name.ends_with(".log"))
            .max();
        match newest {
            Some(name) => {
                self.section(&format!("Run log {name}"));
                match fs::read(dir.join(&name)) {
                    Ok(bytes) => self.text.push_str(&String::from_utf8_lossy(&bytes)),
                    Err(err) => self.line(&format!("(can't read it: {err})")),
                }
            }
            None => {
                self.section(&format!("Run log of {command}"));
                self.line(&format!("No {command} run log."));
            }
        }
    }

    /// macOS: the end of `lemond.log`, and the Background Task Management entry of the
    /// LaunchAgent, which tells whether macOS lets it run.
    fn lemond(&mut self, dir: &Path) {
        self.section(&format!("lemond.log, last {LEMOND_LINES} lines"));
        match fs::read(dir.join("lemond.log")) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let lines: Vec<&str> = text.lines().collect();
                for line in &lines[lines.len().saturating_sub(LEMOND_LINES)..] {
                    self.line(line);
                }
            }
            Err(err) => self.line(&format!("(can't read it: {err})")),
        }
        let label = lemond::label(&overrides::compose_project());
        self.section(&format!("sfltool dumpbtm, the entry for {label}"));
        match Command::new("sfltool")
            .arg("dumpbtm")
            .stdin(Stdio::null())
            .output()
        {
            Ok(out) if out.status.success() => {
                let entries = btm_entries(&String::from_utf8_lossy(&out.stdout), &label);
                if entries.is_empty() {
                    self.line("No entry names it.");
                }
                for entry in entries {
                    self.text.push_str(&entry);
                }
            }
            out => self.output(&out),
        }
    }
}

fn last_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// How long Docker's kernel has run, from `/proc/uptime`: this machine's on Linux, and
/// otherwise Docker Desktop's VM, read in Rakazo's `postgres` container.
fn vm_uptime(platform: Platform, docker: &Docker, folder: &Path) -> String {
    let uptime = match platform {
        Platform::Linux => fs::read_to_string("/proc/uptime").map_err(|err| err.to_string()),
        Platform::Wsl | Platform::MacOs => docker
            .compose(folder, ["exec", "-T", "postgres", "cat", "/proc/uptime"])
            .stdin(Stdio::null())
            .output()
            .map_err(|err| err.to_string())
            .and_then(|out| {
                if out.status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
                } else {
                    Err(last_line(&out.stderr).unwrap_or_else(|| out.status.to_string()))
                }
            }),
    };
    match uptime.map(|text| seconds_up(&text)) {
        Ok(Some(seconds)) => format!(
            "up {}, since {}",
            duration(seconds),
            utc_time(now().saturating_sub(seconds))
        ),
        Ok(None) => "uptime unknown: /proc/uptime gave no number".into(),
        Err(why) => format!("uptime unknown: {why}"),
    }
}

fn seconds_up(proc_uptime: &str) -> Option<u64> {
    let first = proc_uptime.split_whitespace().next()?;
    first.split('.').next()?.parse().ok()
}

fn duration(seconds: u64) -> String {
    let (days, hours, minutes) = (
        seconds / 86_400,
        seconds % 86_400 / 3_600,
        seconds % 3_600 / 60,
    );
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, _) => format!("{hours} h {minutes} min"),
        _ => format!("{days} d {hours} h {minutes} min"),
    }
}

/// The entries of `sfltool dumpbtm` that name `label`. Each starts with a ` #<n>:` line.
fn btm_entries(dump: &str, label: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in dump.lines() {
        let starts_entry = line
            .trim_start()
            .strip_prefix('#')
            .and_then(|rest| rest.strip_suffix(':'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if starts_entry {
            entries.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    entries.push(current);
    entries.retain(|entry| entry.contains(label));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lemonds_entry_in_sfltools_dump() {
        let dump = "\
========================
 Records for UID 501 : 00000000-0000-0000-0000-000000000000
========================

 ServiceManagement migrated: true

 Items:

 #1:
                 UUID: 11111111-1111-1111-1111-111111111111
                 Name: Docker
           Identifier: com.docker.helper
          Disposition: [enabled, allowed, visible, notified] (0xb)

 #2:
                 UUID: 22222222-2222-2222-2222-222222222222
                 Name: lemond
                 Type: legacy agent (0x10010)
          Disposition: [enabled, allowed, notified] (0xb)
           Identifier: 16.com.github.kamui.rakazo-stack.lemond
                  URL: file:///Users/example/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist

 #10:
                 Name: other
";
        let entries = btm_entries(dump, "com.github.kamui.rakazo-stack.lemond");
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert!(entries[0].starts_with(" #2:\n"), "{}", entries[0]);
        assert!(entries[0].contains("[enabled, allowed, notified]"));
        assert!(!entries[0].contains("#10"));
        assert!(btm_entries(dump, "com.github.kamui.rkst-test.lemond").is_empty());
    }

    #[test]
    fn reads_uptime() {
        assert_eq!(seconds_up("11234.56 44000.12\n"), Some(11_234));
        assert_eq!(seconds_up(""), None);
        assert_eq!(duration(11_234), "3 h 7 min");
        assert_eq!(duration(59), "0 min");
        assert_eq!(duration(90_061), "1 d 1 h 1 min");
    }
}
