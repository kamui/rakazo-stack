//! `update`'s step 2 (spec §14): a newer rakazo-stack is installed by its own release's
//! `install.sh --no-setup`, then takes over this process and the lock it holds.

use std::cmp::Ordering;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::answers::{Prompt, Terminal};
use crate::github;
use crate::http::{self, HttpError};
use crate::instance::InstanceLock;
use crate::update;

const PROGRAM_REPO: &str = "kamui/rakazo-stack";
const DOWNLOAD_BASE: &str = "https://github.com/kamui/rakazo-stack/releases/download";
/// install.sh's test-only fake release (spec §19), which the program reads over http.
const TEST_DOWNLOAD_BASE_VAR: &str = "RAKAZO_STACK_TEST_DOWNLOAD_BASE";
/// The number of the lock's file descriptor, which the older program leaves open across
/// `exec` for the newer one (spec §14, §21).
pub const HANDOVER_VAR: &str = "RAKAZO_STACK_HANDOVER_LOCK_FD";
/// The release tag the build baked in; Cargo.toml alone can't tell `v0.1.0-rc.1` from
/// `v0.1.0-rc.2` (spec §18).
const BAKED_TAG: Option<&str> = option_env!("RAKAZO_STACK_RELEASE_TAG");

/// A release tag, `vX.Y.Z` or `vX.Y.Z-<pre-release>`, ordered as SemVer orders versions.
#[derive(Debug, Clone)]
pub struct Tag {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Tag {
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix('v')?;
        let (core, pre) = match rest.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (rest, None),
        };
        let core: Vec<u64> = core.split('.').map(number).collect::<Option<_>>()?;
        let pre: Vec<String> = match pre {
            None => Vec::new(),
            Some(pre) => pre.split('.').map(str::to_string).collect(),
        };
        let valid = |id: &String| {
            !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        };
        if !pre.iter().all(valid) {
            return None;
        }
        Some(Self {
            core: core.try_into().ok()?,
            pre,
        })
    }

    fn is_pre_release(&self) -> bool {
        !self.pre.is_empty()
    }
}

fn number(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.is_pre_release(), other.is_pre_release()) {
                (false, false) => Ordering::Equal,
                (false, true) => Ordering::Greater,
                (true, false) => Ordering::Less,
                (true, true) => {
                    let ids = self.pre.iter().zip(&other.pre);
                    ids.map(|(a, b)| match (number(a), number(b)) {
                        (Some(a), Some(b)) => a.cmp(&b),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => a.cmp(b),
                    })
                    .find(|ordering| ordering.is_ne())
                    .unwrap_or_else(|| self.pre.len().cmp(&other.pre.len()))
                }
            }
        })
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Tag {}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.core;
        write!(f, "v{major}.{minor}.{patch}")?;
        if self.is_pre_release() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

/// This program's release tag. A build without a baked tag, or with one that isn't of
/// Cargo.toml's version (CI's `v0.0.0-ci`), counts as the release `vX.Y.Z`.
pub fn own_tag() -> Tag {
    own_tag_from(BAKED_TAG, env!("CARGO_PKG_VERSION"))
}

fn own_tag_from(baked: Option<&str>, version: &str) -> Tag {
    let release = Tag::parse(&format!("v{version}")).expect("Cargo.toml's version is X.Y.Z");
    baked
        .and_then(Tag::parse)
        .filter(|tag| tag.core == release.core)
        .unwrap_or(release)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

/// The newest release `update` would move this program to. A program from a release
/// moves only to releases; one from a pre-release moves to newer pre-releases too.
fn newest_after(own: &Tag, releases_json: &str) -> Result<Option<Tag>, serde_json::Error> {
    let releases: Vec<Release> = serde_json::from_str(releases_json)?;
    Ok(releases
        .into_iter()
        .filter(|r| !r.draft && (own.is_pre_release() || !r.prerelease))
        .filter_map(|r| Tag::parse(&r.tag_name))
        .filter(|tag| own.is_pre_release() || !tag.is_pre_release())
        .filter(|tag| tag > own)
        .max())
}

fn test_download_base() -> Option<String> {
    std::env::var(TEST_DOWNLOAD_BASE_VAR)
        .ok()
        .filter(|base| !base.is_empty())
}

/// The newest rakazo-stack release, when it is newer than this program. GitHub's list of
/// releases, not its `latest`, which is missing while only pre-releases exist.
pub fn newer_program(agent: &ureq::Agent) -> Result<Option<Tag>, HttpError> {
    let body = match test_download_base() {
        Some(base) => http::get_bytes(agent, &format!("{base}/releases.json"))?,
        None => github::fetch_releases(agent, PROGRAM_REPO)?,
    };
    newest_after(&own_tag(), &String::from_utf8_lossy(&body))
        .map_err(|err| HttpError(format!("the list of rakazo-stack releases: {err}")))
}

/// Step 2. Returns when this program carries on with the Update; once a newer program is
/// installed it takes over this process, and this returns only if it can't start.
pub fn program_first(lock: &InstanceLock, home: &Path, yes: bool) -> Result<(), String> {
    let agent = http::agent();
    let newer = newer_program(&agent).map_err(|err| {
        format!("can't check for a newer rakazo-stack: {err}. Nothing was changed.")
    })?;
    let Some(newer) = newer else {
        return Ok(());
    };
    let own = own_tag();
    let offer = format!("rakazo-stack {newer} is available (you have {own}).");
    if yes {
        println!("{offer} Installing it.");
    } else {
        let mut terminal = Terminal::attached().map_err(|_| {
            "update asks before it changes anything, and there is no terminal. Run `rakazo-stack update --yes` to update without asking. Nothing was changed.".to_string()
        })?;
        if !update::confirmed(
            terminal
                .line(&format!("{offer} Install it? [Y/n] "))
                .as_deref(),
        ) {
            println!("Carrying on with rakazo-stack {own}.");
            return Ok(());
        }
    }
    install(&agent, &newer).map_err(|why| {
        format!(
            "rakazo-stack {newer} wasn't installed: {why}. rakazo-stack {own} is still installed, and nothing in the Instance was changed."
        )
    })?;
    hand_over(lock, &home.join(".local/bin/rakazo-stack"), &newer)
}

/// Runs the release's own `install.sh --no-setup`, so the SHA-256 and build origin checks
/// are those of a fresh install (spec §18).
fn install(agent: &ureq::Agent, tag: &Tag) -> Result<(), String> {
    let base = test_download_base().unwrap_or_else(|| DOWNLOAD_BASE.to_string());
    let script = http::get_bytes(agent, &format!("{base}/{tag}/install.sh"))
        .map_err(|err| err.to_string())?;
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let path = dir.path().join("install.sh");
    fs::write(&path, script).map_err(|err| err.to_string())?;
    let mut child = Command::new("sh")
        .arg(&path)
        .arg("--no-setup")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| format!("sh couldn't run install.sh: {err}"))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    // install.sh's last line names `setup` as the next step; here the Update is.
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if !line.starts_with("Next: ") {
            println!("{line}");
        }
    }
    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("install.sh stopped".into())
    }
}

/// `exec`s the newer program with the same arguments. The lock's descriptor stays open
/// across it, so the lock is never free between the two programs.
fn hand_over(lock: &InstanceLock, program: &Path, tag: &Tag) -> Result<(), String> {
    let cannot_start = |err: std::io::Error| {
        format!(
            "rakazo-stack {tag} is installed at {}, but it couldn't take over: {err}. Nothing in the Instance was changed; run `rakazo-stack update` again.",
            program.display()
        )
    };
    let fd = lock.keep_across_exec().map_err(cannot_start)?;
    println!("Handing over to rakazo-stack {tag}.");
    let _ = std::io::stdout().flush();
    let err = Command::new(program)
        .args(std::env::args_os().skip(1))
        .env(HANDOVER_VAR, fd.to_string())
        .exec();
    Err(cannot_start(err))
}

#[cfg(test)]
mod tests {
    use super::{Tag, newest_after, own_tag_from};

    fn tag(text: &str) -> Tag {
        Tag::parse(text).unwrap_or_else(|| panic!("{text}"))
    }

    #[test]
    fn orders_tags_as_semver_does() {
        let ordered = [
            "v0.1.0-alpha",
            "v0.1.0-alpha.1",
            "v0.1.0-alpha.beta",
            "v0.1.0-rc.1",
            "v0.1.0-rc.2",
            "v0.1.0-rc.10",
            "v0.1.0",
            "v0.1.1-rc.1",
            "v0.10.0",
        ];
        for pair in ordered.windows(2) {
            assert!(tag(pair[0]) < tag(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(tag("v0.1.0-rc.2").to_string(), "v0.1.0-rc.2");
        for bad in [
            "0.1.0",
            "v0.1",
            "v0.1.0.1",
            "v0.1.0-",
            "v0.1.0-rc..1",
            "v0.x.0",
        ] {
            assert!(Tag::parse(bad).is_none(), "{bad}");
        }
    }

    fn releases(tags: &[(&str, bool)]) -> String {
        let entries: Vec<String> = tags
            .iter()
            .map(|(tag, draft)| {
                format!(
                    r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":{}}}"#,
                    tag.contains('-')
                )
            })
            .collect();
        format!("[{}]", entries.join(","))
    }

    fn newest(own: &str, tags: &[(&str, bool)]) -> Option<String> {
        newest_after(&tag(own), &releases(tags))
            .unwrap()
            .map(|t| t.to_string())
    }

    #[test]
    fn a_release_moves_only_to_newer_releases() {
        let listed = [
            ("v0.3.0", true),
            ("v0.2.1-rc.1", false),
            ("v0.2.0", false),
            ("v0.1.0", false),
            ("android", false),
        ];
        assert_eq!(newest("v0.1.0", &listed), Some("v0.2.0".into()));
        assert_eq!(newest("v0.2.0", &listed), None);
        assert_eq!(newest("v0.1.0", &[("v0.1.0-rc.1", false)]), None);
    }

    #[test]
    fn a_pre_release_moves_to_newer_pre_releases_and_releases() {
        let candidates = [("v0.1.0-rc.2", false), ("v0.1.0-rc.1", false)];
        assert_eq!(
            newest("v0.1.0-rc.1", &candidates),
            Some("v0.1.0-rc.2".into())
        );
        assert_eq!(newest("v0.1.0-rc.2", &candidates), None);
        let released = [("v0.1.0", false), ("v0.1.0-rc.2", false)];
        assert_eq!(newest("v0.1.0-rc.1", &released), Some("v0.1.0".into()));
        assert_eq!(newest("v0.1.0-rc.1", &[("v0.1.0-rc.3", true)]), None);
    }

    #[test]
    fn the_baked_tag_counts_only_when_it_is_of_this_version() {
        let own = |baked| own_tag_from(baked, "0.1.0").to_string();
        assert_eq!(own(Some("v0.1.0-rc.2")), "v0.1.0-rc.2");
        assert_eq!(own(Some("v0.1.0")), "v0.1.0");
        assert_eq!(own(Some("v0.0.0-ci")), "v0.1.0");
        assert_eq!(own(Some("garbage")), "v0.1.0");
        assert_eq!(own(None), "v0.1.0");
    }
}
