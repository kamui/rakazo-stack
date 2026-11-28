//! `rakazo-stack update` (spec §14): moves every running Component to its newest release
//! with published images, or to its Hold, behind a Backup; and the update nudge that
//! `setup` and `backup` print (spec §7).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::access::AccessMode;
use crate::answers::{Prompt, Terminal};
use crate::backup::{self, Backup, PASSPHRASE_VAR, Taken};
use crate::backup_file::{self, Part, Passphrase};
use crate::computer_egress;
use crate::docker::Docker;
use crate::envfile::{self, Images};
use crate::github::{self, HINDSIGHT_REPO, RAKAZO_REPO};
use crate::holds::{self, Component, Holds};
use crate::http;
use crate::instance::InstanceFolder;
use crate::lemonade::{self, LEMONADE_IMAGE, LEMONADE_REPO};
use crate::lemond::{self, Lemond};
use crate::memory::Memory;
use crate::overrides;
use crate::platform::Platform;
use crate::preflight;
use crate::registry::{self, Ghcr};
use crate::resolve::{self, HINDSIGHT_DB_IMAGE, HINDSIGHT_DB_TAG, HINDSIGHT_FLOOR, ResolveError};
use crate::restore::{self, Database, Replace, Restore};
use crate::runlog::{self, Logged, utc_stamp};
use crate::say;
use crate::secrets::{LEMONADE_API_KEY, Secrets};
use crate::self_update;
use crate::settings::{
    Components, FailedUpdate, HindsightImages, RakazoImages, RecordedHindsight, RecordedImage,
    RecordedRakazo, Settings,
};
use crate::setup::{self, RAKAZO_COMPOSE_PATH, Setup};

/// The new versions' image lines, which Compose reads over `.env` for the fetch.
const UPDATE_ENV: &str = ".update.env";
/// When `setup` or `backup` last checked for updates, in Unix seconds (spec §7, §21).
pub const CHECKED_FILE: &str = ".update-check";
const NO_CHECK_VAR: &str = "RAKAZO_STACK_NO_UPDATE_CHECK";
const DAY: u64 = 86_400;
/// Left free after the pull, for what the running Instance writes meanwhile.
const DISK_HEADROOM: u64 = 2_000_000_000;
const TAILSCALE_IMAGE: &str = "tailscale/tailscale";
const WAIT_TIMEOUT_SECONDS: &str = "900";

#[derive(Debug)]
pub struct UpdateError(pub String);

impl<E: std::fmt::Display> From<E> for UpdateError {
    fn from(err: E) -> Self {
        Self(err.to_string())
    }
}

#[derive(Debug, Default)]
pub struct Options {
    pub yes: bool,
    pub no_backup: bool,
    /// `--hold <component>=<version>`, as typed.
    pub hold: Vec<String>,
    /// `--unhold <component>`, as typed.
    pub unhold: Vec<String>,
}

/// One Component's recorded version and images, as `settings.json` holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Rakazo(RecordedRakazo),
    Hindsight(RecordedHindsight),
    Lemonade(RecordedImage),
    Tailscale(RecordedImage),
}

impl Record {
    fn of(component: Component, components: &Components) -> Option<Self> {
        match component {
            Component::Rakazo => Some(Self::Rakazo(components.rakazo.clone())),
            Component::Hindsight => components.hindsight.clone().map(Self::Hindsight),
            Component::Lemonade => components.lemonade.clone().map(Self::Lemonade),
            Component::Tailscale => components.tailscale.clone().map(Self::Tailscale),
        }
    }

    fn version(&self) -> &str {
        match self {
            Self::Rakazo(r) => &r.version,
            Self::Hindsight(h) => &h.version,
            Self::Lemonade(i) | Self::Tailscale(i) => &i.version,
        }
    }

    fn put(self, components: &mut Components) {
        match self {
            Self::Rakazo(r) => components.rakazo = r,
            Self::Hindsight(h) => components.hindsight = Some(h),
            Self::Lemonade(i) => components.lemonade = Some(i),
            Self::Tailscale(i) => components.tailscale = Some(i),
        }
    }

    fn rakazo(release: resolve::RakazoRelease) -> Self {
        Self::Rakazo(RecordedRakazo {
            version: release.tag,
            images: RakazoImages {
                app: release.app_digest,
                computer: release.computer_digest,
            },
        })
    }

    fn hindsight(release: resolve::HindsightRelease) -> Self {
        Self::Hindsight(RecordedHindsight {
            version: release.version,
            images: HindsightImages {
                hindsight: release.digest,
                hindsight_db: release.db_digest,
            },
        })
    }

    fn lemonade(release: lemonade::LemonadeRelease) -> Self {
        Self::Lemonade(RecordedImage {
            version: release.tag,
            digest: release.digest,
        })
    }
}

/// A Component's newest release with published images.
#[derive(Debug, Clone)]
struct Latest {
    record: Record,
    /// Stable release tags, newest first, for the release-note links.
    tags: Vec<String>,
    /// Newer releases skipped because their images aren't published yet.
    skipped: Vec<String>,
}

/// What the Update does to one Component.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    component: Component,
    from: String,
    to: String,
    latest: String,
    /// The record changes: a new version, or the same one with new images.
    moves: bool,
    held: bool,
}

#[derive(Debug)]
struct Plan {
    after: Components,
    holds: Holds,
    lines: Vec<Line>,
    /// Holds this program's floor is above: those Components move to latest.
    released: Vec<(Component, String)>,
    skipped: Vec<String>,
    links: Vec<String>,
}

impl Plan {
    fn moves(&self) -> bool {
        self.lines.iter().any(|line| line.moves)
    }

    fn moves_component(&self, component: Component) -> bool {
        self.lines
            .iter()
            .any(|line| line.moves && line.component == component)
    }

    /// Everything but Lemonade has data a Backup holds (spec §14, step 7).
    fn needs_backup(&self) -> bool {
        self.lines
            .iter()
            .any(|line| line.moves && line.component != Component::Lemonade)
    }

    fn render(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            let name = line.component.name();
            if line.moves && line.from == line.to {
                text.push_str(&format!("{name} {}: newly published images\n", line.to));
            } else if line.moves {
                text.push_str(&format!("{name} {} → {}\n", line.from, line.to));
            }
            if line.held && line.to != line.latest {
                text.push_str(&format!(
                    "{name} is held at {}; the latest is {}\n",
                    line.to, line.latest
                ));
            }
        }
        for (component, version) in &self.released {
            text.push_str(&format!(
                "{name} was held at {version}, below the {} this rakazo-stack needs, so it moves to the latest release and its Hold is removed.\n",
                component.floor().unwrap_or_default(),
                name = component.name(),
            ));
        }
        for skipped in &self.skipped {
            text.push_str(skipped);
            text.push('\n');
        }
        if !self.moves() {
            text.push_str("Everything is up to date.\n");
        } else if !self.links.is_empty() {
            text.push_str(
                "Release notes (Rakazo's \"Self-host upgrade notes\" can ask for steps by hand):\n",
            );
            for link in &self.links {
                text.push_str(&format!("  {link}\n"));
            }
        }
        text
    }
}

fn repo(component: Component) -> &'static str {
    match component {
        Component::Rakazo => RAKAZO_REPO,
        Component::Hindsight => HINDSIGHT_REPO,
        Component::Lemonade => LEMONADE_REPO,
        Component::Tailscale => "tailscale/tailscale",
    }
}

/// Each release's notes after `from`, up to and including `to`.
fn release_links(component: Component, tags: &[String], from: &str, to: &str) -> Vec<String> {
    let url = |tag: &str| format!("https://github.com/{}/releases/tag/{tag}", repo(component));
    if tags.is_empty() {
        return vec![url(to)];
    }
    let mut links: Vec<String> = tags
        .iter()
        .filter(|tag| holds::below(from, tag) && !holds::below(to, tag))
        .map(|tag| url(tag))
        .collect();
    links.reverse();
    links
}

/// Picks each running Component's target: its Hold, or the latest release (spec §4).
/// `exact` has the records of Holds at versions other than the recorded one.
fn plan(
    before: &Components,
    holds: &Holds,
    latest: Vec<(Component, Latest)>,
    exact: &BTreeMap<Component, Record>,
) -> Plan {
    let mut plan = Plan {
        after: before.clone(),
        holds: holds.clone(),
        lines: Vec::new(),
        released: Vec::new(),
        skipped: Vec::new(),
        links: Vec::new(),
    };
    for (component, latest) in latest {
        let recorded = Record::of(component, before);
        let hold = holds.get(&component);
        let target = match hold {
            Some(version) if component.below_floor(version) => {
                plan.released.push((component, version.clone()));
                plan.holds.remove(&component);
                None
            }
            Some(version) if recorded.as_ref().is_some_and(|r| r.version() == version) => {
                recorded.clone()
            }
            Some(_) => exact.get(&component).cloned(),
            None => None,
        };
        let held = target.is_some();
        let target = target.unwrap_or_else(|| latest.record.clone());
        let from = recorded
            .as_ref()
            .map_or("none", Record::version)
            .to_string();
        let line = Line {
            component,
            from: from.clone(),
            to: target.version().to_string(),
            latest: latest.record.version().to_string(),
            moves: recorded.as_ref() != Some(&target),
            held,
        };
        if line.moves && line.from != line.to {
            plan.links
                .extend(release_links(component, &latest.tags, &from, &line.to));
        }
        if !held {
            plan.skipped.extend(latest.skipped);
        }
        target.put(&mut plan.after);
        plan.lines.push(line);
    }
    plan
}

/// The Components the Instance runs, as setup decides (spec §4, §10, §11).
fn running(settings: &Settings, secrets: &Secrets) -> Vec<Component> {
    let mut running = vec![Component::Rakazo];
    if settings.components.lemonade.is_some() {
        running.push(Component::Lemonade);
    }
    let key_saved = settings
        .model_provider
        .and_then(|choice| choice.0)
        .is_some_and(|provider| secrets.get(provider.key_name()).is_some());
    if settings.components.hindsight.is_some()
        && key_saved
        && settings.memory.unwrap_or_default() == Memory::On
    {
        running.push(Component::Hindsight);
    }
    if settings.access == AccessMode::Tailscale && settings.components.tailscale.is_some() {
        running.push(Component::Tailscale);
    }
    running
}

/// Release lookups on GitHub and the registries, as setup makes them.
struct Lookup<'a> {
    agent: &'a ureq::Agent,
    /// macOS: Lemonade is the embeddable tarball, not an image.
    native: bool,
}

impl Lookup<'_> {
    fn tags(&self, component: Component) -> Result<Vec<String>, String> {
        github::fetch_stable_tags(self.agent, repo(component))
            .map_err(|err| format!("Couldn't check {}'s releases: {err}", component.name()))
    }

    fn ghcr_digest(&self, image: &str, tag: &str) -> Result<Option<String>, String> {
        Ghcr::new(self.agent)
            .digest(image, tag)
            .map_err(|err| err.to_string())
    }

    fn hindsight_db_digest(&self) -> Result<Option<String>, String> {
        registry::docker_hub_digest(self.agent, HINDSIGHT_DB_IMAGE, HINDSIGHT_DB_TAG)
            .map_err(|err| err.to_string())
    }

    fn lemond_releases(&self) -> Result<String, String> {
        github::fetch_releases(self.agent, LEMONADE_REPO)
            .map(|json| String::from_utf8_lossy(&json).into_owned())
            .map_err(|err| format!("Couldn't check Lemonade's releases: {err}"))
    }

    fn latest(&self, component: Component) -> Result<Latest, String> {
        let mut skipped = Vec::new();
        let mut say = |line: &str| skipped.push(line.to_string());
        let (record, tags) = match component {
            Component::Rakazo => {
                let tags = self.tags(component)?;
                let resolution =
                    resolve::choose_rakazo(&tags, |image, tag| self.ghcr_digest(image, tag));
                let mut release =
                    setup::report(resolution, "Rakazo", &resolve::rakazo_floor(), &mut say)
                        .map_err(|err| err.0)?;
                if let Some((tag, _)) = overrides::next_rakazo() {
                    release.tag = tag;
                }
                (Record::rakazo(release), tags)
            }
            Component::Hindsight => {
                let tags = self.tags(component)?;
                let resolution = resolve::choose_hindsight(
                    &tags,
                    |image, tag| self.ghcr_digest(image, tag),
                    || self.hindsight_db_digest(),
                );
                let release = setup::report(resolution, "Hindsight", HINDSIGHT_FLOOR, &mut say)
                    .map_err(|err| err.0)?;
                (Record::hindsight(release), tags)
            }
            Component::Lemonade if self.native => {
                let json = self.lemond_releases()?;
                let resolution = lemond::choose_tarball(&json, None)
                    .map_err(|err| format!("Couldn't pick a Lemonade release: {err}"))?;
                skipped.extend(
                    resolution
                        .skipped
                        .iter()
                        .map(|t| lemond::describe_skipped(t)),
                );
                let tags = github::stable_tags(&json).map_err(|err| err.to_string())?;
                (Record::lemonade(resolution.release), tags)
            }
            Component::Lemonade => {
                let tags = self.tags(component)?;
                let resolution =
                    lemonade::choose_lemonade(&tags, |tag| self.ghcr_digest(LEMONADE_IMAGE, tag))
                        .map_err(|err| format!("Couldn't pick a Lemonade release: {err}"))?;
                skipped.extend(
                    resolution
                        .skipped
                        .iter()
                        .map(|t| lemonade::describe_skipped(t)),
                );
                (Record::lemonade(resolution.release), tags)
            }
            Component::Tailscale => {
                let image = registry::tailscale_stable(self.agent)
                    .map_err(|err| format!("Couldn't check Tailscale's stable image: {err}"))?;
                (Record::Tailscale(image), Vec::new())
            }
        };
        Ok(Latest {
            record,
            tags,
            skipped,
        })
    }

    /// The record of one held release, which must be stable and have its images.
    fn exact(
        &self,
        component: Component,
        version: &str,
        tags: &[String],
    ) -> Result<Record, String> {
        let name = component.name();
        let tag = match component {
            Component::Hindsight => format!("v{version}"),
            _ => version.to_string(),
        };
        if component != Component::Tailscale && !tags.contains(&tag) {
            return Err(format!(
                "{name} {version} isn't one of its recent stable releases, so it can't be held."
            ));
        }
        let unpublished =
            || format!("{name} {version}'s images aren't published yet, so it can't be held.");
        let resolved = |result: Result<Record, ResolveError>| match result {
            Ok(record) => Ok(record),
            Err(ResolveError::Lookup(err)) => Err(err),
            Err(ResolveError::NothingAtFloor { .. }) => Err(unpublished()),
        };
        let only = [tag.clone()];
        match component {
            Component::Rakazo => resolved(
                resolve::choose_rakazo(&only, |image, tag| self.ghcr_digest(image, tag))
                    .map(|r| Record::rakazo(r.release)),
            ),
            Component::Hindsight => resolved(
                resolve::choose_hindsight(
                    &only,
                    |image, tag| self.ghcr_digest(image, tag),
                    || self.hindsight_db_digest(),
                )
                .map(|r| Record::hindsight(r.release)),
            ),
            Component::Lemonade if self.native => {
                lemond::choose_tarball(&self.lemond_releases()?, Some(&tag))
                    .map(|r| Record::lemonade(r.release))
                    .map_err(|_| unpublished())
            }
            Component::Lemonade => match self.ghcr_digest(LEMONADE_IMAGE, &tag)? {
                Some(digest) => Ok(Record::Lemonade(RecordedImage {
                    version: tag,
                    digest,
                })),
                None => Err(unpublished()),
            },
            Component::Tailscale => {
                match registry::docker_hub_digest(self.agent, TAILSCALE_IMAGE, version)
                    .map_err(|err| err.to_string())?
                {
                    Some(digest) => Ok(Record::Tailscale(RecordedImage {
                        version: version.to_string(),
                        digest,
                    })),
                    None => Err(unpublished()),
                }
            }
        }
    }

    /// Resolves every running Component and plans the Update.
    fn plan(
        &self,
        settings: &Settings,
        running: &[Component],
        holds: &Holds,
    ) -> Result<Plan, String> {
        let mut latest = Vec::new();
        let mut exact = BTreeMap::new();
        for &component in running {
            let found = self.latest(component)?;
            if let Some(version) = holds.get(&component).filter(|version| {
                !component.below_floor(version)
                    && component.recorded(&settings.components) != Some(version.as_str())
            }) {
                exact.insert(component, self.exact(component, version, &found.tags)?);
            }
            latest.push((component, found));
        }
        Ok(plan(&settings.components, holds, latest, &exact))
    }
}

/// The major version in a Postgres image's tag: `postgres:16@sha256:…` is 16 and
/// `pgvector/pgvector:pg18` is 18.
fn postgres_major(image: &str) -> Option<u32> {
    let name = image.split('@').next()?;
    let (_, tag) = name
        .rsplit_once('/')
        .unwrap_or(("", name))
        .1
        .split_once(':')?;
    let digits: String = tag
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// `name:tag@digest` without its tag, which Docker ignores once a digest is given.
fn by_digest(image: &str) -> String {
    match image.split_once('@') {
        Some((name, digest)) => format!("{}@{digest}", repository(name)),
        None => image.to_string(),
    }
}

fn repository(image: &str) -> &str {
    let name = image.split('@').next().unwrap_or(image);
    match name.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => repo,
        _ => name,
    }
}

/// `"Update? [Y/n]"`: Enter is yes.
pub fn confirmed(answer: Option<&str>) -> bool {
    answer.is_some_and(|a| matches!(a.trim(), "" | "y" | "Y" | "yes" | "Yes"))
}

/// A database whose cluster is on another Postgres major version than its new image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PostgresMove {
    database: Database,
    from: u32,
    to: u32,
}

impl PostgresMove {
    fn describe(&self) -> String {
        let owner = match self.database {
            Database::Rakazo => "Rakazo",
            Database::Hindsight => "Hindsight",
        };
        format!(
            "{owner}'s Postgres {} → {}: a Backup is required",
            self.from, self.to
        )
    }
}

/// What a rolled-back Update moved: each Component whose record the switch changed.
fn moved(before: &Components, after: &Components) -> BTreeMap<Component, String> {
    [
        Component::Rakazo,
        Component::Hindsight,
        Component::Lemonade,
        Component::Tailscale,
    ]
    .into_iter()
    .filter_map(|component| {
        let after = Record::of(component, after)?;
        (Record::of(component, before).as_ref() != Some(&after))
            .then(|| (component, after.version().to_string()))
    })
    .collect()
}

/// "The last update to Rakazo v0.17.0 failed on 2026-11-27 and was rolled back."
fn describe_failed(failed: &FailedUpdate) -> String {
    let versions: Vec<String> = failed
        .to
        .iter()
        .map(|(component, version)| format!("{} {version}", component.name()))
        .collect();
    let versions = match versions.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => versions.join(""),
    };
    format!(
        "The last update to {versions} failed on {} and was rolled back.",
        failed.on
    )
}

/// `YYYYMMDDTHHMMSSZ` as `2026-11-27 12:01 UTC`.
fn readable_time(stamp: &str) -> String {
    match (stamp.get(0..4), stamp.get(4..6), stamp.get(6..8)) {
        (Some(year), Some(month), Some(day)) => {
            let time = match (stamp.get(9..11), stamp.get(11..13)) {
                (Some(hour), Some(minute)) => format!(" {hour}:{minute} UTC"),
                _ => String::new(),
            };
            format!("{year}-{month}-{day}{time}")
        }
        _ => stamp.to_string(),
    }
}

fn today() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = utc_stamp(now);
    format!("{}-{}-{}", &stamp[0..4], &stamp[4..6], &stamp[6..8])
}

/// What the Update wrote before the switch, so a failure or a No can take it back.
struct Staged {
    /// The new tag's compose file, when this run wrote it.
    compose_file: Option<String>,
    /// The new tag's egress script, written for step 6's build, and the one it replaces.
    script: Option<(Vec<u8>, Option<Vec<u8>>)>,
}

pub struct Update<'a> {
    pub platform: Platform,
    pub docker: &'a Docker,
    pub folder: &'a InstanceFolder,
    pub home: &'a Path,
    pub nftables: bool,
    pub options: Options,
}

impl Update<'_> {
    pub fn run(&self) -> Result<(), UpdateError> {
        let no_instance =
            || UpdateError("there is no Instance here. Run `rakazo-stack setup`.".into());
        let settings_text = self.folder.read("settings.json")?.ok_or_else(no_instance)?;
        let settings = Settings::parse(&String::from_utf8_lossy(&settings_text))
            .map_err(|err| UpdateError(format!("settings.json can't be read: {err}")))?;
        let secrets = Secrets::parse(&String::from_utf8_lossy(
            &self.folder.read("secrets.env")?.unwrap_or_default(),
        ));
        let add = self
            .options
            .hold
            .iter()
            .map(|text| holds::parse_hold(text))
            .collect::<Result<Vec<_>, _>>()
            .map_err(UpdateError)?;
        let remove = self
            .options
            .unhold
            .iter()
            .map(|text| holds::parse_component(text))
            .collect::<Result<Vec<_>, _>>()
            .map_err(UpdateError)?;
        let mut holds = settings.holds.clone();
        holds::apply(&mut holds, &add, &remove, &settings.components).map_err(UpdateError)?;

        // Step 4.
        say!("Checking for newer releases…");
        let agent = http::agent();
        let native = self.platform == Platform::MacOs;
        let lookup = Lookup {
            agent: &agent,
            native,
        };
        let plan = lookup
            .plan(&settings, &running(&settings, &secrets), &holds)
            .map_err(|err| UpdateError(format!("{err} Nothing was changed.")))?;
        if let Some(failed) = &settings.failed_update {
            say!("{}", describe_failed(failed));
        }
        say!("{}", plan.render().trim_end());
        if !plan.moves() {
            if plan.holds != settings.holds {
                let mut kept = settings.clone();
                kept.holds = plan.holds;
                self.folder
                    .write("settings.json", kept.to_json().as_bytes())?;
            }
            return Ok(());
        }
        if self.options.yes
            && let Some((component, _)) = plan.released.first()
        {
            return Err(UpdateError(format!(
                "with --yes, update never drops a Hold. Run `rakazo-stack update` without --yes to move {} to the latest release, or release the Hold with `--unhold {}`. Nothing was changed.",
                component.name(),
                component.id()
            )));
        }
        let mut next = settings.clone();
        next.components = plan.after.clone();
        next.holds = plan.holds.clone();
        next.failed_update = None;

        let staged = self.stage(&agent, &next, &plan)?;
        let checked = self.check_postgres(&plan).and_then(|moves| {
            for line in &moves {
                say!("{}", line.describe());
            }
            if !moves.is_empty() && self.options.no_backup {
                return Err(UpdateError(
                    "--no-backup is refused when Postgres moves: the move restores the Backup's dump into a fresh database. Run it without --no-backup. Nothing was changed.".into(),
                ));
            }
            self.check_disk().map(|()| moves)
        });
        let moves = match checked {
            Ok(moves) => moves,
            Err(err) => {
                self.discard(&staged);
                return Err(err);
            }
        };

        // Step 5.
        let mut terminal = Terminal::attached().ok();
        if !self.options.yes {
            let Some(prompt) = terminal.as_mut() else {
                self.discard(&staged);
                return Err(UpdateError(
                    "update asks before it changes anything, and there is no terminal. Run `rakazo-stack update --yes` to update without asking. Nothing was changed.".into(),
                ));
            };
            if !confirmed(prompt.line("Update? [Y/n] ").as_deref()) {
                self.discard(&staged);
                say!("Nothing was changed.");
                return Ok(());
            }
        }
        let backup = plan.needs_backup() && !self.options.no_backup;
        let passphrase = if backup {
            let prompt = (!self.options.yes)
                .then_some(terminal.as_mut())
                .flatten()
                .map(|t| t as &mut dyn Prompt);
            match backup::passphrase(|name| std::env::var(name).ok(), prompt) {
                Ok(passphrase) => Some(passphrase),
                Err(err) => {
                    self.discard(&staged);
                    return Err(UpdateError(if self.options.yes {
                        format!(
                            "with --yes, update takes its Backup's passphrase from {PASSPHRASE_VAR}, which isn't set. Nothing was changed."
                        )
                    } else {
                        format!("{} Nothing was changed.", err.0)
                    }));
                }
            }
        } else {
            if plan.needs_backup() {
                say!("Skipping the Backup (--no-backup).");
            }
            None
        };

        // Steps 6 and 7.
        let taken = self
            .fetch(&agent, &next, &plan, &secrets, &staged)
            .and_then(|()| match &passphrase {
                Some(passphrase) => self.backup().run(passphrase).map(Some).map_err(|err| {
                    UpdateError(format!("the Backup before the Update failed: {}", err.0))
                }),
                None => Ok(None),
            });
        let taken = match taken {
            Ok(taken) => taken,
            Err(err) => {
                self.discard(&staged);
                return Err(UpdateError(format!(
                    "{} Nothing was recorded, and the Instance runs the versions it ran before.",
                    err.0
                )));
            }
        };
        let backup = taken.as_ref().zip(passphrase.as_ref());
        let project = overrides::compose_project();
        let images_before = self.image_ids(&project)?;

        // Step 8.
        if !moves.is_empty() {
            let (taken, passphrase) =
                backup.expect("a Postgres move is refused without its Backup");
            if let Err(err) = self.move_postgres(&next, &moves, taken, passphrase) {
                let _ = fs::remove_file(self.folder.file(UPDATE_ENV));
                return Err(self.failed(
                    "the Postgres major move",
                    &err.0,
                    &settings,
                    &staged,
                    backup,
                    terminal.as_mut(),
                ));
            }
        }
        let _ = fs::remove_file(self.folder.file(UPDATE_ENV));

        // Steps 9–11.
        let switch = Setup {
            platform: self.platform,
            docker: self.docker,
            folder: self.folder,
            no_questions: true,
            nftables: self.nftables,
            switch: Some(&next),
        };
        if let Err(err) = switch.run() {
            return Err(self.failed(
                "the switch",
                &err.0,
                &settings,
                &staged,
                backup,
                terminal.as_mut(),
            ));
        }

        // Step 12.
        self.remove_images(&images_before, &self.image_ids(&project)?);
        if native
            && plan.moves_component(Component::Lemonade)
            && let Some(old) = &settings.components.lemonade
            && next.components.lemonade.as_ref() != Some(old)
        {
            let layout = lemond::Layout::new(self.folder.path(), &project);
            let _ = fs::remove_dir_all(layout.version_dir(&old.version));
        }
        say!("Update finished.");
        Ok(())
    }

    fn backup(&self) -> Backup<'_> {
        Backup {
            docker: self.docker,
            folder: self.folder,
            home: self.home,
            output: None,
            keep_rakazo_stopped: true,
        }
    }

    /// A failure from step 8 on. Before the new versions are recorded the old ones start
    /// again. After, the data belongs to the new versions: that is a failed Update, which
    /// the Backup taken before it can roll back (spec §14, "Failures").
    fn failed(
        &self,
        step: &str,
        reason: &str,
        settings: &Settings,
        staged: &Staged,
        backup: Option<(&Taken, &Passphrase)>,
        terminal: Option<&mut Terminal>,
    ) -> UpdateError {
        let recorded = self
            .folder
            .read("settings.json")
            .ok()
            .flatten()
            .and_then(|text| Settings::parse(&String::from_utf8_lossy(&text)).ok());
        let Some(recorded) = recorded.filter(|recorded| recorded.components != settings.components)
        else {
            self.discard(staged);
            let restarted = backup.map_or(Ok(()), |(taken, _)| self.backup().start_again(taken));
            return UpdateError(match restarted {
                Ok(()) => format!(
                    "{reason} Nothing was recorded, and the Instance runs the versions it ran before."
                ),
                Err(err) => format!(
                    "{reason} Nothing was recorded, but Rakazo didn't start again ({}). Run `rakazo-stack setup` to start it.",
                    err.0
                ),
            });
        };
        let reason = format!("the Update failed at {step}: {reason}");
        let stay = "The new versions are recorded and stay in place; re-running `rakazo-stack setup` retries starting them.";
        let Some((taken, passphrase)) = backup else {
            return UpdateError(format!(
                "{reason} {stay} No Backup was taken before it, so the data may already belong to the new versions and the old ones aren't brought back: fix forward, or restore an older Backup."
            ));
        };
        let time = readable_time(&taken.created);
        let command = format!("rakazo-stack restore --replace {}", taken.path.display());
        let prompt = terminal.filter(|_| !self.options.yes);
        let Some(prompt) = prompt else {
            return UpdateError(format!(
                "{reason} {stay} To roll back to the previous versions from the Backup taken at {time}, run:\n  {command}"
            ));
        };
        say!("{reason}");
        let question = format!(
            "Update failed at {step}. Roll back to the previous versions from the Backup taken at {time}? [Y/n] "
        );
        if !confirmed(prompt.line(&question).as_deref()) {
            return UpdateError(format!(
                "{reason} {stay} To roll back later, run:\n  {command}"
            ));
        }
        let to = moved(&settings.components, &recorded.components);
        match self.roll_back(taken, passphrase, to, prompt) {
            Ok(()) => UpdateError(format!(
                "the Update failed at {step} and was rolled back: the Instance runs the versions it ran before, with its data from the Backup taken at {time}."
            )),
            Err(err) => UpdateError(format!(
                "the Update failed at {step}, and so did the rollback: {err}"
            )),
        }
    }

    /// `restore --replace` on the Backup taken before the Update, inside update's lock and
    /// without the typed name, then the record of the failed Update. No Hold is added.
    fn roll_back(
        &self,
        taken: &Taken,
        passphrase: &Passphrase,
        to: BTreeMap<Component, String>,
        terminal: &mut Terminal,
    ) -> Result<(), String> {
        say!("Rolling back from {}…", taken.path.display());
        let restore = Restore {
            platform: self.platform,
            docker: self.docker,
            folder: self.folder,
            file: &taken.path,
            replace: Replace::Confirmed,
            nftables: self.nftables,
        };
        let env = |name: &str| std::env::var(name).ok();
        restore
            .run_with(passphrase, &env, Some(terminal as &mut dyn Prompt))
            .map_err(|err| err.message)?;
        let text = self
            .folder
            .read("settings.json")
            .map_err(|err| err.to_string())?
            .ok_or("the restored Instance has no settings.json.")?;
        let mut restored = Settings::parse(&String::from_utf8_lossy(&text))
            .map_err(|err| format!("the restored settings.json can't be read: {err}"))?;
        restored.failed_update = Some(FailedUpdate { to, on: today() });
        self.folder
            .write("settings.json", restored.to_json().as_bytes())
            .map_err(|err| err.to_string())
    }

    /// Step 4's files: the new tag's compose file, and the new image lines that Compose
    /// reads over `.env`. Neither changes what runs.
    fn stage(
        &self,
        agent: &ureq::Agent,
        next: &Settings,
        plan: &Plan,
    ) -> Result<Staged, UpdateError> {
        let mut staged = Staged {
            compose_file: None,
            script: None,
        };
        if plan.moves_component(Component::Rakazo) {
            let tag = &next.components.rakazo.version;
            let file = envfile::rakazo_compose_file(tag);
            let (compose, script) = match overrides::next_rakazo() {
                // An unpublished release has nothing on GitHub; it keeps the current script.
                Some((next_tag, path)) if next_tag == *tag => (
                    fs::read(path)?,
                    self.folder
                        .read(computer_egress::SCRIPT)?
                        .ok_or_else(|| UpdateError("the egress script is missing.".into()))?,
                ),
                _ => (
                    github::fetch_file(agent, RAKAZO_REPO, tag, RAKAZO_COMPOSE_PATH)?,
                    github::fetch_file(agent, RAKAZO_REPO, tag, computer_egress::UPSTREAM_SCRIPT)?,
                ),
            };
            if self.folder.read(&file)?.is_none() {
                self.folder.write(&file, &compose)?;
                staged.compose_file = Some(file);
            }
            staged.script = Some((script, self.folder.read(computer_egress::SCRIPT)?));
        }
        let release = next.rakazo_release();
        let lemonade = next
            .lemonade_release()
            .filter(|_| self.platform != Platform::MacOs);
        let hindsight = next.hindsight_release();
        let lines = envfile::image_lines(&Images {
            rakazo: &release,
            lemonade: lemonade.as_ref(),
            hindsight: hindsight.as_ref(),
            tailscale: next.components.tailscale.as_ref(),
        });
        self.folder
            .write(UPDATE_ENV, format!("{}\n", lines.join("\n")).as_bytes())?;
        Ok(staged)
    }

    /// Takes back what [`Self::stage`] and [`Self::fetch`] wrote.
    fn discard(&self, staged: &Staged) {
        let _ = fs::remove_file(self.folder.file(UPDATE_ENV));
        if let Some(file) = &staged.compose_file {
            let _ = fs::remove_file(self.folder.file(file));
        }
        if let Some((_, Some(old))) = &staged.script {
            let _ = computer_egress::write_build_folder(self.folder, old);
        }
    }

    /// Compose with the new versions' images over `.env`.
    fn compose_new(&self, args: &[&str]) -> Result<String, UpdateError> {
        let mut all = vec!["--env-file", ".env", "--env-file", UPDATE_ENV];
        all.extend(args);
        self.compose(&all)
    }

    fn compose(&self, args: &[&str]) -> Result<String, UpdateError> {
        let out = self
            .docker
            .compose(self.folder.path(), args)
            .logged_output()?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(UpdateError(format!(
                "`docker compose {}` failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    fn docker_ok(&self, args: &[&str]) -> Result<String, UpdateError> {
        let out = self.docker.output(args)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(UpdateError(format!(
                "`docker {}` failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    /// The cluster's major version, from `PG_VERSION` in the running service's data.
    fn cluster_major(&self, service: &str) -> Result<u32, UpdateError> {
        let out = self.compose(&[
            "exec",
            "-T",
            service,
            "sh",
            "-c",
            "cat \"$PGDATA/PG_VERSION\"",
        ])?;
        out.trim().parse().map_err(|_| {
            UpdateError(format!(
                "{service}'s PG_VERSION reads `{}`, not a version.",
                out.trim()
            ))
        })
    }

    /// Step 4's Postgres check: each moving database's cluster major (`PG_VERSION`) against
    /// its new image's.
    fn check_postgres(&self, plan: &Plan) -> Result<Vec<PostgresMove>, UpdateError> {
        let mut moves = Vec::new();
        if plan.moves_component(Component::Rakazo) {
            let model: serde_json::Value =
                serde_json::from_str(&self.compose_new(&["config", "--format", "json"])?)?;
            let image = model["services"]["postgres"]["image"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let to = postgres_major(&image).ok_or_else(|| {
                UpdateError(format!(
                    "Couldn't tell which Postgres major version `{image}` is. Nothing was changed."
                ))
            })?;
            let from = self.cluster_major("postgres")?;
            if from != to {
                moves.push(PostgresMove {
                    database: Database::Rakazo,
                    from,
                    to,
                });
            }
        }
        if plan.moves_component(Component::Hindsight) {
            let to = postgres_major(&format!("{HINDSIGHT_DB_IMAGE}:{HINDSIGHT_DB_TAG}"))
                .expect("the program's own tag has a major");
            let from = self.cluster_major("hindsight-db")?;
            if from != to {
                moves.push(PostgresMove {
                    database: Database::Hindsight,
                    from,
                    to,
                });
            }
        }
        Ok(moves)
    }

    /// Step 8. The new versions are recorded first: once an old volume goes, the old
    /// versions have no data to run on, and a missed move fails loudly instead of starting
    /// on an empty database. Each moving database then starts under its new image on a
    /// fresh volume and takes the Backup's dump, through restore's database step.
    fn move_postgres(
        &self,
        next: &Settings,
        moves: &[PostgresMove],
        taken: &Taken,
        passphrase: &Passphrase,
    ) -> Result<(), UpdateError> {
        let project = overrides::compose_project();
        self.folder
            .write("settings.json", next.to_json().as_bytes())?;
        for line in moves {
            let database = line.database;
            say!(
                "Moving {} from Postgres {} to {}…",
                database.label(),
                line.from,
                line.to
            );
            let mut remove = database.prefix().to_vec();
            remove.extend(["rm", "--stop", "--force", database.service()]);
            if database == Database::Hindsight {
                remove.push("hindsight");
            }
            self.compose(&remove)?;
            self.docker_ok(&["volume", "rm", &backup::volume(&project, database.volume())])?;
            let mut up = database.prefix().to_vec();
            up.extend([
                "up",
                "-d",
                "--no-deps",
                "--wait",
                "--wait-timeout",
                WAIT_TIMEOUT_SECONDS,
                database.service(),
            ]);
            self.compose_new(&up)?;
        }
        let mut restored = Vec::new();
        backup_file::check(&taken.path, passphrase, |part, dump| {
            let database = match part {
                Part::RakazoDump => Database::Rakazo,
                Part::HindsightDump => Database::Hindsight,
                _ => return Ok(()),
            };
            if !moves.iter().any(|line| line.database == database) {
                return Ok(());
            }
            say!("Restoring {}…", database.label());
            restored.push(database);
            restore::restore_database(self.docker, self.folder.path(), database, dump)
        })
        .map_err(UpdateError)?;
        match moves.iter().find(|line| !restored.contains(&line.database)) {
            Some(line) => Err(UpdateError(format!(
                "the Backup holds no dump of {}.",
                line.database.label()
            ))),
            None => Ok(()),
        }
    }

    /// Step 3's free-disk check, sized to the new images not on the machine yet. Each is
    /// taken to be as big as the image of the same repository the Instance runs now.
    fn check_disk(&self) -> Result<(), UpdateError> {
        let running = self.compose(&["config", "--images"])?;
        let wanted = self.compose_new(&["config", "--images"])?;
        let sizes = self.running_image_sizes()?;
        let mut needed = 0;
        for image in wanted.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if running.lines().any(|line| line.trim() == image) || self.has_image(image) {
                continue;
            }
            needed += sizes.get(repository(image)).copied().unwrap_or(0);
        }
        let Some(home) = self.folder.path().parent() else {
            return Ok(());
        };
        let Some(free) = preflight::docker_free_disk(self.platform, self.docker, home) else {
            return Ok(());
        };
        if free < needed + DISK_HEADROOM {
            return Err(UpdateError(format!(
                "the new images need about {:.1} GB, and {:.1} GB is free where Docker keeps its data. Free some space, then run this again. Nothing was changed.",
                (needed + DISK_HEADROOM) as f64 / 1e9,
                free as f64 / 1e9
            )));
        }
        Ok(())
    }

    fn has_image(&self, image: &str) -> bool {
        self.docker_ok(&["image", "inspect", "--format", "{{.Id}}", &by_digest(image)])
            .is_ok()
    }

    /// The size of each image the Instance's containers run, by repository.
    fn running_image_sizes(&self) -> Result<BTreeMap<String, u64>, UpdateError> {
        let project = overrides::compose_project();
        let mut sizes = BTreeMap::new();
        for id in self.container_ids(&project)? {
            let line =
                self.docker_ok(&["inspect", "--format", "{{.Config.Image}} {{.Image}}", &id])?;
            let Some((image, image_id)) = line.trim().split_once(' ') else {
                continue;
            };
            let size = self
                .docker_ok(&["image", "inspect", "--format", "{{.Size}}", image_id])
                .ok()
                .and_then(|size| size.trim().parse().ok())
                .unwrap_or(0);
            sizes.insert(repository(image).to_string(), size);
        }
        Ok(sizes)
    }

    fn container_ids(&self, project: &str) -> Result<Vec<String>, UpdateError> {
        let filter = format!("label=com.docker.compose.project={project}");
        Ok(self
            .docker_ok(&["ps", "-a", "-q", "--no-trunc", "--filter", &filter])?
            .split_whitespace()
            .map(str::to_string)
            .collect())
    }

    /// The images the Instance's containers run, by ID.
    fn image_ids(&self, project: &str) -> Result<BTreeSet<String>, UpdateError> {
        let mut ids = BTreeSet::new();
        for container in self.container_ids(project)? {
            let id = self.docker_ok(&["inspect", "--format", "{{.Image}}", &container])?;
            ids.insert(id.trim().to_string());
        }
        Ok(ids)
    }

    /// Step 6, while the Instance runs.
    fn fetch(
        &self,
        agent: &ureq::Agent,
        next: &Settings,
        plan: &Plan,
        secrets: &Secrets,
        staged: &Staged,
    ) -> Result<(), UpdateError> {
        if let Some((script, _)) = &staged.script {
            computer_egress::write_build_folder(self.folder, script)?;
        }
        say!("Pulling the new images while the Instance runs…");
        self.compose_new(&["pull", "--quiet", "--ignore-buildable"])?;
        say!("Building the Lemonade relay and the computer-egress sidecar…");
        self.compose_new(&[
            "build",
            "--quiet",
            "lemonade-relay",
            computer_egress::SERVICE,
        ])?;
        if self.platform == Platform::MacOs
            && plan.moves_component(Component::Lemonade)
            && let Some(release) = next.lemonade_release()
        {
            let layout = lemond::Layout::new(self.folder.path(), &overrides::compose_project());
            let key = secrets.get(LEMONADE_API_KEY.0).unwrap_or_default();
            Lemond::new(layout, release, key)
                .install(agent, &runlog::say)
                .map_err(UpdateError)?;
        }
        Ok(())
    }

    /// Removes the images the switch replaced. The program pulls by digest, so an image
    /// that also has a `name:tag` is someone else's too and stays. Docker's containerd
    /// store lists a digest pull among `RepoTags` as `name@sha256:…`.
    fn remove_images(&self, before: &BTreeSet<String>, after: &BTreeSet<String>) {
        let mut removed = 0;
        for id in before.difference(after) {
            let Some(names) = self
                .docker_ok(&["image", "inspect", "--format", "{{json .RepoTags}}", id])
                .ok()
                .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
                .filter(|names| names.iter().all(|name| name.contains('@')))
            else {
                continue;
            };
            let refs = if names.is_empty() {
                vec![id.clone()]
            } else {
                names
            };
            let gone = refs
                .iter()
                .try_for_each(|name| self.docker_ok(&["image", "rm", name]).map(|_| ()));
            match gone {
                Ok(()) => removed += 1,
                Err(err) => say!("Couldn't remove a replaced image: {}", err.0),
            }
        }
        if removed > 0 {
            say!(
                "Removed {removed} image{} of replaced versions.",
                if removed == 1 { "" } else { "s" }
            );
        }
    }
}

/// Whether the nudge checks now: at most once a day (spec §7).
fn due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|last| last > now || now - last >= DAY)
}

/// The update nudge, at the end of `setup` and `backup`: at most once a day, one line
/// when the program or an un-held Component has a newer version, silent on any failure.
pub fn nudge(folder: &InstanceFolder, platform: Platform) {
    if std::env::var(NO_CHECK_VAR).as_deref() == Ok("1") {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let last = folder
        .read(CHECKED_FILE)
        .ok()
        .flatten()
        .and_then(|text| String::from_utf8_lossy(&text).trim().parse().ok());
    if !due(last, now)
        || folder
            .write(CHECKED_FILE, format!("{now}\n").as_bytes())
            .is_err()
    {
        return;
    }
    if updates_available(folder, platform) {
        say!("Updates available: run `rakazo-stack update`");
    }
}

fn updates_available(folder: &InstanceFolder, platform: Platform) -> bool {
    let agent = http::agent();
    if self_update::newer_program(&agent).is_ok_and(|newer| newer.is_some()) {
        return true;
    }
    let Some(settings) = folder
        .read("settings.json")
        .ok()
        .flatten()
        .and_then(|text| Settings::parse(&String::from_utf8_lossy(&text)).ok())
    else {
        return false;
    };
    let secrets = Secrets::parse(&String::from_utf8_lossy(
        &folder
            .read("secrets.env")
            .ok()
            .flatten()
            .unwrap_or_default(),
    ));
    let lookup = Lookup {
        agent: &agent,
        native: platform == Platform::MacOs,
    };
    lookup
        .plan(&settings, &running(&settings, &secrets), &settings.holds)
        .is_ok_and(|plan| plan.moves())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_provider::{Choice, Provider};

    fn rakazo(version: &str, app: &str) -> Record {
        Record::Rakazo(RecordedRakazo {
            version: version.into(),
            images: RakazoImages {
                app: app.into(),
                computer: format!("{app}-computer"),
            },
        })
    }

    fn image(component: Component, version: &str) -> Record {
        let image = RecordedImage {
            version: version.into(),
            digest: format!("sha256:{version}"),
        };
        match component {
            Component::Lemonade => Record::Lemonade(image),
            _ => Record::Tailscale(image),
        }
    }

    fn components(rakazo_version: &str, lemonade: &str) -> Components {
        let mut components = Settings::new(&resolve::RakazoRelease {
            tag: rakazo_version.into(),
            app_digest: "sha256:old".into(),
            computer_digest: "sha256:old-computer".into(),
        })
        .components;
        image(Component::Lemonade, lemonade).put(&mut components);
        components
    }

    const SKIPPED: &str = "Skipped Rakazo v0.18.0: its computer image isn't published yet.";

    fn latest(record: Record, tags: &[&str]) -> Latest {
        let skipped = match record {
            Record::Rakazo(_) => vec![SKIPPED.to_string()],
            _ => Vec::new(),
        };
        Latest {
            record,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            skipped,
        }
    }

    const RAKAZO_TAGS: [&str; 4] = ["v0.17.1", "v0.17.0", "v0.16.0", "v0.15.0"];

    #[test]
    fn moves_each_component_to_latest_and_links_every_release_on_the_way() {
        let before = components("v0.16.0", "v2026.48.3");
        let plan = plan(
            &before,
            &Holds::new(),
            vec![
                (
                    Component::Rakazo,
                    latest(rakazo("v0.17.1", "sha256:new"), &RAKAZO_TAGS),
                ),
                (
                    Component::Lemonade,
                    latest(image(Component::Lemonade, "v2026.48.3"), &["v2026.48.3"]),
                ),
            ],
            &BTreeMap::new(),
        );
        assert!(plan.moves() && plan.needs_backup());
        assert_eq!(plan.after.rakazo.version, "v0.17.1");
        assert_eq!(plan.after.lemonade, before.lemonade);
        assert_eq!(
            plan.render(),
            "Rakazo v0.16.0 → v0.17.1\n\
             Skipped Rakazo v0.18.0: its computer image isn't published yet.\n\
             Release notes (Rakazo's \"Self-host upgrade notes\" can ask for steps by hand):\n  \
             https://github.com/elie222/rakazo/releases/tag/v0.17.0\n  \
             https://github.com/elie222/rakazo/releases/tag/v0.17.1\n"
        );
    }

    #[test]
    fn nothing_newer_is_up_to_date() {
        let before = components("v0.16.0", "v2026.48.3");
        let mut found = latest(
            Record::of(Component::Rakazo, &before).unwrap(),
            &RAKAZO_TAGS,
        );
        found.skipped.clear();
        let plan = plan(
            &before,
            &Holds::new(),
            vec![(Component::Rakazo, found)],
            &BTreeMap::new(),
        );
        assert!(!plan.moves());
        assert_eq!(plan.after, before);
        assert_eq!(plan.render(), "Everything is up to date.\n");
    }

    #[test]
    fn a_hold_keeps_a_component_and_a_lemonade_only_update_takes_no_backup() {
        let before = components("v0.16.0", "v2026.47.1");
        let holds = Holds::from([(Component::Rakazo, "v0.16.0".to_string())]);
        let plan = plan(
            &before,
            &holds,
            vec![
                (
                    Component::Rakazo,
                    latest(rakazo("v0.17.1", "sha256:new"), &RAKAZO_TAGS),
                ),
                (
                    Component::Lemonade,
                    latest(
                        image(Component::Lemonade, "v2026.48.3"),
                        &["v2026.48.3", "v2026.47.1"],
                    ),
                ),
            ],
            &BTreeMap::new(),
        );
        assert_eq!(plan.after.rakazo, before.rakazo);
        assert_eq!(plan.holds, holds);
        assert!(plan.moves() && !plan.needs_backup());
        let text = plan.render();
        assert!(
            text.starts_with(
                "Rakazo is held at v0.16.0; the latest is v0.17.1\nLemonade v2026.47.1 → v2026.48.3\n"
            ),
            "{text}"
        );
        assert!(
            !text.contains("v0.18.0"),
            "a held Component's skips don't matter"
        );
        assert!(
            text.ends_with("  https://github.com/lemonade-sdk/lemonade/releases/tag/v2026.48.3\n")
        );
    }

    #[test]
    fn a_hold_ahead_of_the_recorded_version_moves_there() {
        let before = components("v0.16.0", "v2026.48.3");
        let holds = Holds::from([(Component::Rakazo, "v0.17.0".to_string())]);
        let exact = BTreeMap::from([(Component::Rakazo, rakazo("v0.17.0", "sha256:held"))]);
        let plan = plan(
            &before,
            &holds,
            vec![(
                Component::Rakazo,
                latest(rakazo("v0.17.1", "sha256:new"), &RAKAZO_TAGS),
            )],
            &exact,
        );
        assert_eq!(plan.after.rakazo.images.app, "sha256:held");
        let text = plan.render();
        assert!(
            text.starts_with(
                "Rakazo v0.16.0 → v0.17.0\nRakazo is held at v0.17.0; the latest is v0.17.1\n"
            ),
            "{text}"
        );
        assert!(!text.contains("v0.17.1\n  "), "{text}");
    }

    #[test]
    fn a_floor_above_a_hold_moves_to_latest_and_says_so() {
        let before = components("v0.15.0", "v2026.48.3");
        let holds = Holds::from([(Component::Rakazo, "v0.15.0".to_string())]);
        let plan = plan(
            &before,
            &holds,
            vec![(
                Component::Rakazo,
                latest(rakazo("v0.16.0", "sha256:new"), &RAKAZO_TAGS),
            )],
            &BTreeMap::new(),
        );
        assert_eq!(plan.after.rakazo.version, "v0.16.0");
        assert!(plan.holds.is_empty());
        assert_eq!(plan.released, [(Component::Rakazo, "v0.15.0".to_string())]);
        assert!(plan.render().contains(
            "Rakazo was held at v0.15.0, below the v0.16.0 this rakazo-stack needs, so it moves to the latest release and its Hold is removed."
        ));
    }

    #[test]
    fn republished_images_of_the_same_version_move_too() {
        let before = components("v0.16.0", "v2026.48.3");
        let plan = plan(
            &before,
            &Holds::new(),
            vec![(
                Component::Rakazo,
                latest(rakazo("v0.16.0", "sha256:rebuilt"), &RAKAZO_TAGS),
            )],
            &BTreeMap::new(),
        );
        assert!(plan.moves() && plan.links.is_empty());
        assert!(
            plan.render()
                .starts_with("Rakazo v0.16.0: newly published images\n")
        );
    }

    #[test]
    fn tailscale_links_its_release_and_hindsight_its_v_tags() {
        assert_eq!(
            release_links(Component::Tailscale, &[], "v1.104.7", "v1.106.0"),
            ["https://github.com/tailscale/tailscale/releases/tag/v1.106.0"]
        );
        let tags: Vec<String> = ["v1.4.0", "v1.3.1", "v1.3.0"].map(String::from).to_vec();
        assert_eq!(
            release_links(Component::Hindsight, &tags, "1.3.0", "1.3.1"),
            ["https://github.com/vectorize-io/hindsight/releases/tag/v1.3.1"]
        );
    }

    #[test]
    fn runs_hindsight_only_with_memory_on_and_a_key_and_tailscale_only_when_chosen() {
        let mut settings = Settings::new(&resolve::RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        let mut secrets = Secrets::default();
        assert_eq!(running(&settings, &secrets), [Component::Rakazo]);
        image(Component::Lemonade, "v2026.48.3").put(&mut settings.components);
        settings.components.hindsight = Some(RecordedHindsight {
            version: "1.3.0".into(),
            images: HindsightImages {
                hindsight: "sha256:h".into(),
                hindsight_db: "sha256:d".into(),
            },
        });
        image(Component::Tailscale, "v1.104.7").put(&mut settings.components);
        settings.model_provider = Some(Choice(Some(Provider::OpenRouter)));
        assert_eq!(
            running(&settings, &secrets),
            [Component::Rakazo, Component::Lemonade]
        );
        secrets = Secrets::parse("OPENROUTER_API_KEY=placeholder\n");
        settings.access = AccessMode::Tailscale;
        assert_eq!(
            running(&settings, &secrets),
            [
                Component::Rakazo,
                Component::Lemonade,
                Component::Hindsight,
                Component::Tailscale
            ]
        );
        settings.memory = Some(Memory::Off);
        assert!(!running(&settings, &secrets).contains(&Component::Hindsight));
    }

    #[test]
    fn reads_postgres_majors_from_image_tags() {
        assert_eq!(
            postgres_major(
                "postgres:16@sha256:e17e86066e5ef83e0952a9347f5c792b7ece00972e2aa787a6986f471b3dd3d5"
            ),
            Some(16)
        );
        assert_eq!(postgres_major("pgvector/pgvector:pg18"), Some(18));
        assert_eq!(
            postgres_major("docker.io/library/postgres:18.1-trixie"),
            Some(18)
        );
        assert_eq!(postgres_major("postgres@sha256:abc"), None);
        assert_eq!(postgres_major("registry.example:5000/postgres"), None);
    }

    #[test]
    fn a_postgres_move_says_a_backup_is_required() {
        let line = PostgresMove {
            database: Database::Rakazo,
            from: 16,
            to: 18,
        };
        assert_eq!(
            line.describe(),
            "Rakazo's Postgres 16 → 18: a Backup is required"
        );
    }

    #[test]
    fn records_what_a_rolled_back_update_moved_and_shows_it_next_time() {
        let before = components("v0.16.0", "v2026.48.3");
        let mut after = before.clone();
        rakazo("v0.17.0", "sha256:new").put(&mut after);
        image(Component::Tailscale, "v1.106.0").put(&mut after);
        let to = moved(&before, &after);
        assert_eq!(
            to,
            BTreeMap::from([
                (Component::Rakazo, "v0.17.0".to_string()),
                (Component::Tailscale, "v1.106.0".to_string()),
            ])
        );
        let failed = FailedUpdate {
            to,
            on: "2026-11-27".into(),
        };
        let mut settings = Settings::new(&resolve::RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        settings.failed_update = Some(failed.clone());
        let json = settings.to_json();
        assert!(
            json.contains(
                "\"failed_update\": {\n    \"to\": {\n      \"rakazo\": \"v0.17.0\",\n      \"tailscale\": \"v1.106.0\"\n    },\n    \"on\": \"2026-11-27\"\n  }"
            ),
            "{json}"
        );
        assert_eq!(Settings::parse(&json).unwrap(), settings);
        assert_eq!(
            describe_failed(&failed),
            "The last update to Rakazo v0.17.0 and Tailscale v1.106.0 failed on 2026-11-27 and was rolled back."
        );
        let one = FailedUpdate {
            to: BTreeMap::from([(Component::Rakazo, "v0.5.2".to_string())]),
            on: "2026-11-27".into(),
        };
        assert_eq!(
            describe_failed(&one),
            "The last update to Rakazo v0.5.2 failed on 2026-11-27 and was rolled back."
        );
    }

    #[test]
    fn backup_times_read_as_utc() {
        assert_eq!(readable_time("20261127T120101Z"), "2026-11-27 12:01 UTC");
        assert_eq!(readable_time("odd"), "odd");
        assert_eq!(today().len(), "2026-11-27".len());
    }

    #[test]
    fn names_images_without_their_tags() {
        assert_eq!(
            by_digest("ghcr.io/elie222/rakazo/app:v0.16.0@sha256:abc"),
            "ghcr.io/elie222/rakazo/app@sha256:abc"
        );
        assert_eq!(by_digest("busybox:1"), "busybox:1");
        assert_eq!(
            repository("registry.example:5000/postgres:16"),
            "registry.example:5000/postgres"
        );
        assert_eq!(
            repository("registry.example:5000/postgres"),
            "registry.example:5000/postgres"
        );
    }

    #[test]
    fn enter_confirms() {
        for yes in ["", "y", "Yes", " y "] {
            assert!(confirmed(Some(yes)), "{yes:?}");
        }
        for no in ["n", "no", "nope"] {
            assert!(!confirmed(Some(no)), "{no:?}");
        }
        assert!(!confirmed(None), "end of input");
    }

    #[test]
    fn the_nudge_checks_at_most_once_a_day() {
        assert!(due(None, 1_000_000));
        assert!(!due(Some(1_000_000), 1_000_000 + DAY - 1));
        assert!(due(Some(1_000_000), 1_000_000 + DAY));
        assert!(due(Some(2_000_000), 1_000_000), "a clock set back");
    }
}
