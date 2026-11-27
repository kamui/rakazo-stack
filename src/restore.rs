//! `rakazo-stack restore` (spec §16): reads the whole Backup before the machine changes,
//! writes `settings.json` and `secrets.env` into a new Instance folder, restores both
//! databases and the volumes into fresh ones, then hands over to setup's no-questions run.

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::access::{AUTH_KEY_NAME, Access, AccessMode, AuthKey, SWITCH_WARNING};
use crate::answers::{AUTH_KEY_VAR, Prompt};
use crate::backup::{self, PASSPHRASE_VAR};
use crate::backup_file::{self, Part, Passphrase};
use crate::docker::Docker;
use crate::envfile::{self, EnvInputs};
use crate::http;
use crate::instance::InstanceFolder;
use crate::lemond;
use crate::memory;
use crate::overrides;
use crate::platform::Platform;
use crate::resolve::{self, HINDSIGHT_DB_TAG};
use crate::secrets::{HINDSIGHT_DB_PASSWORD, Secrets};
use crate::settings::{RakazoImages, RecordedRakazo, Settings};
use crate::setup::{self, Setup};
use crate::version::Version;

/// The answer to "Is the machine this Backup came from switched off for good?" when
/// there is no terminal: `yes` or `no`.
pub const OLD_MACHINE_OFF_VAR: &str = "RAKAZO_STACK_OLD_MACHINE_OFF";
const LOCK_FILE: &str = ".lock";
const WAIT_TIMEOUT_SECONDS: &str = "900";
/// As Rakazo's own row-level security migration creates it. Roles live outside a dump,
/// and the dump's policies name it; `migrate` gives it its login and grants on start.
const RUNTIME_ROLE_SQL: &str = "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'rakazo_app') THEN CREATE ROLE rakazo_app NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION; END IF; END $$;\n";

/// How `--replace` is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replace {
    /// No `--replace`: an Instance here stops the run.
    No,
    /// `--replace`: the Self-hoster types the Instance's name first.
    Typed,
    /// `update`'s rollback, inside its own lock, without the typed name (spec §14).
    Confirmed,
}

#[derive(Debug)]
pub struct RestoreError {
    pub message: String,
    /// From step 4 on, the half-restored Instance is left for inspection.
    pub touched: bool,
}

impl RestoreError {
    fn before(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            touched: false,
        }
    }
}

/// The steps that change the machine, named when one fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Folder = 4,
    Databases = 5,
    Volumes = 6,
    Setup = 7,
}

impl Step {
    fn describe(self) -> &'static str {
        match self {
            Self::Folder => "writing the Instance folder",
            Self::Databases => "restoring the databases",
            Self::Volumes => "unpacking the volumes",
            Self::Setup => "setup",
        }
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    format: u32,
    program_version: String,
    created: String,
    /// Absent from Backups written before restore existed.
    #[serde(default)]
    platform: Option<String>,
}

/// The Backup's three small parts, and which parts it holds.
struct Opened {
    manifest: Manifest,
    settings: Settings,
    secrets_text: Vec<u8>,
    secrets: Secrets,
    parts: Vec<Part>,
}

/// What the version rule changes in the Backup's recorded versions (spec §16).
#[derive(Debug, Default, PartialEq, Eq)]
struct VersionPlan {
    rakazo_to_latest: bool,
    hindsight_to_latest: bool,
    /// Lemonade's record is an image digest, or on macOS a tarball's SHA-256, so it
    /// doesn't carry over to an OS that runs Lemonade the other way.
    lemonade_afresh: bool,
}

/// What happens to the Backup's `tailscale-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    /// The Backup has none.
    Absent,
    /// Restored: the Instance comes back as the same device.
    Same,
    /// Left out: the Instance joins again as a new device.
    Rejoin,
}

pub struct Restore<'a> {
    pub platform: Platform,
    pub docker: &'a Docker,
    pub folder: &'a InstanceFolder,
    pub file: &'a Path,
    pub replace: Replace,
    /// From preflight, for setup's hand-over.
    pub nftables: bool,
}

impl Restore<'_> {
    /// Steps 2–7. The caller holds the lock and has run preflight (step 1).
    pub fn run(
        &self,
        env: &dyn Fn(&str) -> Option<String>,
        mut terminal: Option<&mut (dyn Prompt + '_)>,
    ) -> Result<(), RestoreError> {
        let passphrase = passphrase(env, terminal.as_deref_mut()).map_err(RestoreError::before)?;
        self.run_with(&passphrase, env, terminal)
    }

    /// [`Self::run`] with the passphrase already given, as `update`'s rollback has it.
    pub fn run_with(
        &self,
        passphrase: &Passphrase,
        env: &dyn Fn(&str) -> Option<String>,
        mut terminal: Option<&mut (dyn Prompt + '_)>,
    ) -> Result<(), RestoreError> {
        println!("Reading the Backup…");
        let mut opened = open(self.file, passphrase).map_err(RestoreError::before)?;
        let plan = version_rule(
            &opened.manifest,
            &opened.settings,
            &opened.parts,
            &opened.secrets,
            self.platform,
        )
        .map_err(|err| RestoreError::before(format!("This Backup can't be restored: {err}")))?;

        let project = overrides::compose_project();
        let present = self.instance_here(&project).map_err(RestoreError::before)?;
        let (replacing, replaced) = match (present.is_empty(), self.replace) {
            (true, _) => (false, None),
            (false, Replace::No) => {
                return Err(RestoreError::before(format!(
                    "an Instance is already here ({}). To wipe it and restore this Backup in its place, run `rakazo-stack restore --replace {}`; back it up first if you may want it again.",
                    present.join(", "),
                    self.file.display()
                )));
            }
            (false, replace) => {
                self.refuse_a_backup_inside_the_folder()?;
                if replace == Replace::Typed {
                    confirm_name(terminal.as_deref_mut(), &project, &present)
                        .map_err(RestoreError::before)?;
                }
                (true, self.recorded_here())
            }
        };
        let device = self
            .device(&mut opened, replaced.as_ref(), env, terminal)
            .map_err(RestoreError::before)?;

        let agent = http::agent();
        let say = |line: &str| println!("{line}");
        for note in plan.notes(&opened.settings) {
            say(&note);
        }
        plan.apply(&mut opened.settings, &agent, &say)
            .map_err(RestoreError::before)?;

        let mut step = Step::Folder;
        self.change(
            &mut step, &opened, passphrase, &agent, &project, replacing, device,
        )
        .map_err(|err| self.failed(step, &err))?;
        println!(
            "Restored the Backup made {} by rakazo-stack v{}.",
            opened.manifest.created, opened.manifest.program_version
        );
        Ok(())
    }

    /// Steps 4–7, which change the machine.
    #[allow(clippy::too_many_arguments)]
    fn change(
        &self,
        step: &mut Step,
        opened: &Opened,
        passphrase: &Passphrase,
        agent: &ureq::Agent,
        project: &str,
        replacing: bool,
        device: Device,
    ) -> Result<(), String> {
        if replacing {
            println!("Wiping the Instance here…");
            self.wipe(project)?;
        }
        self.folder
            .write("settings.json", opened.settings.to_json().as_bytes())
            .and_then(|()| self.folder.write("secrets.env", &opened.secrets_text))
            .and_then(|()| self.folder.reapply_modes())
            .map_err(|err| err.to_string())?;

        *step = Step::Databases;
        println!("Starting Rakazo's database on a fresh volume…");
        setup::write_stack_files(
            agent,
            self.folder,
            &opened.settings.components.rakazo.version,
        )
        .map_err(|err| err.0)?;
        let env = provisional_env(project, &opened.settings, &opened.secrets, self.platform);
        self.folder
            .write(".env", env.as_bytes())
            .map_err(|err| err.to_string())?;
        self.compose(&[
            "up",
            "-d",
            "--no-deps",
            "--wait",
            "--wait-timeout",
            WAIT_TIMEOUT_SECONDS,
            Database::Rakazo.service(),
        ])?;
        if opened.parts.contains(&Part::HindsightDump) {
            println!("Starting Hindsight's database on a fresh volume…");
            let mut args = Database::Hindsight.prefix().to_vec();
            args.extend([
                "up",
                "-d",
                "--no-deps",
                "--wait",
                "--wait-timeout",
                WAIT_TIMEOUT_SECONDS,
                Database::Hindsight.service(),
            ]);
            self.compose(&args)?;
        }
        let image = self.image_of(Database::Rakazo.service())?;

        let restored = backup_file::check(self.file, passphrase, |part, data| match part {
            Part::RakazoDump | Part::HindsightDump => {
                let database = match part {
                    Part::RakazoDump => Database::Rakazo,
                    _ => Database::Hindsight,
                };
                println!("Restoring {}…", database.label());
                restore_database(self.docker, self.folder.path(), database, data)
            }
            Part::Appdata => {
                *step = Step::Volumes;
                println!("Unpacking appdata…");
                self.unpack(project, "appdata", &image, data)
            }
            Part::TailscaleState if device == Device::Same => {
                *step = Step::Volumes;
                self.unpack(project, "tailscale-state", &image, data)
            }
            _ => Ok(()),
        });
        restored?;

        *step = Step::Setup;
        println!("Handing over to setup…");
        Setup {
            platform: self.platform,
            docker: self.docker,
            folder: self.folder,
            no_questions: true,
            nftables: self.nftables,
            switch: None,
        }
        .run()
        .map_err(|err| err.0)
    }

    fn failed(&self, step: Step, err: &str) -> RestoreError {
        RestoreError {
            message: format!(
                "restore failed at step {} ({}): {err}\nThe half-restored Instance is left as it is, for inspection. Once the cause is fixed, run `rakazo-stack restore --replace {}` again; the Backup file is unchanged.",
                step as u8,
                step.describe(),
                self.file.display()
            ),
            touched: true,
        }
    }

    /// What makes up an Instance here: files in the Instance folder besides the lock,
    /// and its Docker volumes.
    fn instance_here(&self, project: &str) -> Result<Vec<String>, String> {
        let mut present = Vec::new();
        if folder_holds_an_instance(self.folder.path()).map_err(|err| err.to_string())? {
            present.push("~/rakazo-stack".to_string());
        }
        present.extend(self.volumes(project)?);
        Ok(present)
    }

    fn volumes(&self, project: &str) -> Result<Vec<String>, String> {
        let listing = self.docker_ok(&[
            "volume",
            "ls",
            "--format",
            "{{.Name}}\t{{.Label \"com.docker.compose.project\"}}",
        ])?;
        Ok(instance_volumes(&listing, project))
    }

    /// `settings.json` of the Instance being replaced, if it can still be read.
    fn recorded_here(&self) -> Option<Settings> {
        let bytes = self.folder.read("settings.json").ok().flatten()?;
        Settings::parse(&String::from_utf8_lossy(&bytes)).ok()
    }

    /// `--replace` empties the Instance folder, so a Backup inside it would be lost.
    fn refuse_a_backup_inside_the_folder(&self) -> Result<(), RestoreError> {
        let file = fs::canonicalize(self.file).unwrap_or_else(|_| self.file.to_path_buf());
        let folder = fs::canonicalize(self.folder.path())
            .unwrap_or_else(|_| self.folder.path().to_path_buf());
        if file.starts_with(&folder) {
            return Err(RestoreError::before(format!(
                "{} is inside the Instance folder, which --replace empties. Move it out first.",
                self.file.display()
            )));
        }
        Ok(())
    }

    /// Step 3's Tailscale question, and what follows from the answer.
    fn device(
        &self,
        opened: &mut Opened,
        replaced: Option<&Settings>,
        env: &dyn Fn(&str) -> Option<String>,
        terminal: Option<&mut (dyn Prompt + '_)>,
    ) -> Result<Device, String> {
        if !opened.parts.contains(&Part::TailscaleState) {
            return Ok(Device::Absent);
        }
        if same_device(replaced, &opened.settings) {
            return Ok(Device::Same);
        }
        let mut terminal = terminal;
        if old_machine_off(env, terminal.as_deref_mut())? {
            return Ok(Device::Same);
        }
        if !rejoin(&mut opened.settings) {
            return Ok(Device::Rejoin);
        }
        let key = new_auth_key(env, terminal)?;
        let mut secrets = opened.secrets.clone();
        secrets.set_pasted(AUTH_KEY_NAME, Some(key.pasted()));
        opened.secrets_text = secrets.to_text().into_bytes();
        opened.secrets = secrets;
        println!(
            "The Instance joins the tailnet again as a new device, so Tailscale gives it a new name and its Instance URL changes. {SWITCH_WARNING}"
        );
        Ok(Device::Rejoin)
    }

    /// The Instance's containers, its bot computers, its volumes and the Instance
    /// folder's files. The lock stays, because this run holds it.
    fn wipe(&self, project: &str) -> Result<(), String> {
        if self.platform == Platform::MacOs {
            // lemond runs from the Instance folder, so it stops before the folder goes.
            let uid = rustix::process::getuid().as_raw();
            let _ = Command::new("launchctl")
                .args(["bootout", &lemond::service(uid, &lemond::label(project))])
                .output();
        }
        let in_project = format!("label=com.docker.compose.project={project}");
        let appdata = format!("volume={}", backup::volume(project, "appdata"));
        let mut containers =
            self.docker_ok(&["ps", "-a", "-q", "--no-trunc", "--filter", &in_project])?;
        containers.push_str(&self.docker_ok(&[
            "ps",
            "-a",
            "-q",
            "--no-trunc",
            "--filter",
            "label=rakazo.managed=true",
            "--filter",
            &appdata,
        ])?);
        let mut ids: Vec<&str> = containers.split_whitespace().collect();
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            let mut args = vec!["rm", "--force", "--volumes"];
            args.extend(ids);
            self.docker_ok(&args)?;
        }
        let volumes = self.volumes(project)?;
        if !volumes.is_empty() {
            let mut args = vec!["volume", "rm"];
            args.extend(volumes.iter().map(String::as_str));
            self.docker_ok(&args)?;
        }
        for entry in fs::read_dir(self.folder.path()).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            if entry.file_name() == LOCK_FILE {
                continue;
            }
            let path = entry.path();
            let removed = if entry.file_type().map_err(|err| err.to_string())?.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            removed.map_err(|err| format!("can't remove {}: {err}", path.display()))?;
        }
        Ok(())
    }

    /// Unpacks an archive into a fresh volume, labelled as Compose labels its own so that
    /// Compose takes it as the Instance's, in a throwaway container on no network.
    fn unpack(
        &self,
        project: &str,
        name: &str,
        image: &str,
        archive: &mut dyn Read,
    ) -> Result<(), String> {
        let volume = backup::volume(project, name);
        self.docker_ok(&[
            "volume",
            "create",
            "--label",
            &format!("com.docker.compose.project={project}"),
            "--label",
            &format!("com.docker.compose.volume={name}"),
            &volume,
        ])?;
        let args = [
            "run".to_string(),
            "--rm".into(),
            "-i".into(),
            "--network".into(),
            "none".into(),
            "--name".into(),
            format!("{project}-restore-{name}"),
            "-v".into(),
            format!("{volume}:/data"),
            "--entrypoint".into(),
            "tar".into(),
            image.into(),
            "--extract".into(),
            "--file=-".into(),
            "--numeric-owner".into(),
            "--directory=/data".into(),
        ];
        backup::stream_in(self.docker.command(&args), archive)
            .map_err(|err| format!("unpacking {name} failed: {err}"))
    }

    fn image_of(&self, service: &str) -> Result<String, String> {
        let id = self.compose(&["ps", "-q", service])?;
        let id = id.lines().next().unwrap_or_default().trim().to_string();
        if id.is_empty() {
            return Err(format!("{service} didn't start."));
        }
        Ok(self
            .docker_ok(&["inspect", "--format", "{{.Image}}", &id])?
            .trim()
            .to_string())
    }

    fn compose(&self, args: &[&str]) -> Result<String, String> {
        let out = self
            .docker
            .compose(self.folder.path(), args)
            .output()
            .map_err(|err| err.to_string())?;
        backup::checked("docker compose", args, out).map_err(|err| err.0)
    }

    fn docker_ok(&self, args: &[&str]) -> Result<String, String> {
        let out = self.docker.output(args).map_err(|err| err.to_string())?;
        backup::checked("docker", args, out).map_err(|err| err.0)
    }
}

/// The Postgres a dump goes back into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Database {
    Rakazo,
    Hindsight,
}

impl Database {
    pub fn service(self) -> &'static str {
        match self {
            Self::Rakazo => "postgres",
            Self::Hindsight => "hindsight-db",
        }
    }

    /// The owner role and the database, which share a name.
    fn name(self) -> &'static str {
        match self {
            Self::Rakazo => "rakazo",
            Self::Hindsight => "hindsight",
        }
    }

    /// Its data volume's name in the compose files.
    pub fn volume(self) -> &'static str {
        match self {
            Self::Rakazo => "pgdata",
            Self::Hindsight => "hindsight-pgdata",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rakazo => "Rakazo's database",
            Self::Hindsight => "Hindsight's database",
        }
    }

    /// `hindsight-db` is in a profile that `.env` may leave off.
    pub fn prefix(self) -> &'static [&'static str] {
        match self {
            Self::Rakazo => &[],
            Self::Hindsight => &["--profile", memory::PROFILE],
        }
    }

    fn exec<'a>(self, command: &[&'a str]) -> Vec<&'a str> {
        let mut args = self.prefix().to_vec();
        args.extend(["exec", "-T", self.service()]);
        args.extend(command);
        args
    }
}

/// Step 5, which `update`'s Postgres major move reuses (spec §14): once the service's
/// fresh database answers over TCP, creates the `rakazo_app` role Rakazo's dump expects,
/// then restores the `pg_dump -Fc` dump in one transaction. The dump arrives on standard
/// input, so nothing secret reaches an argument list.
pub fn restore_database(
    docker: &Docker,
    folder: &Path,
    database: Database,
    dump: &mut dyn Read,
) -> Result<(), String> {
    let name = database.name();
    let service = database.service();
    // The image's first start runs a server on the socket only, then restarts it; TCP
    // answers once that has finished.
    let ready = database.exec(&[
        "pg_isready",
        "-q",
        "-h",
        "127.0.0.1",
        "-U",
        name,
        "-d",
        name,
    ]);
    let mut answered = false;
    for _ in 0..60 {
        let status = docker.compose(folder, &ready).output();
        if status.is_ok_and(|out| out.status.success()) {
            answered = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if !answered {
        return Err(format!("{service} didn't answer within 60 s."));
    }
    if database == Database::Rakazo {
        let psql = database.exec(&[
            "psql",
            "-X",
            "-q",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            name,
            "-d",
            name,
        ]);
        backup::stream_in(
            docker.compose(folder, &psql),
            &mut RUNTIME_ROLE_SQL.as_bytes(),
        )
        .map_err(|err| format!("creating the rakazo_app role failed: {err}"))?;
    }
    let restore = database.exec(&[
        "pg_restore",
        "--no-owner",
        "--no-privileges",
        "--single-transaction",
        "--exit-on-error",
        "-U",
        name,
        "-d",
        name,
    ]);
    backup::stream_in(docker.compose(folder, &restore), dump)
        .map_err(|err| format!("pg_restore in {service} failed: {err}"))
}

/// The passphrase from [`PASSPHRASE_VAR`], or typed once on the terminal.
fn passphrase(
    env: &dyn Fn(&str) -> Option<String>,
    terminal: Option<&mut (dyn Prompt + '_)>,
) -> Result<Passphrase, String> {
    if let Some(passphrase) = env(PASSPHRASE_VAR).and_then(Passphrase::new) {
        return Ok(passphrase);
    }
    let Some(prompt) = terminal else {
        return Err(format!(
            "no Backup passphrase. Set {PASSPHRASE_VAR}, or run `rakazo-stack restore` on a terminal."
        ));
    };
    prompt
        .secret("Backup passphrase: ")
        .and_then(Passphrase::new)
        .ok_or_else(|| "no Backup passphrase was given.".to_string())
}

/// Step 2: decrypts every part, which `age` authenticates chunk by chunk, and keeps the
/// three small ones. The dumps and archives were read back by their own tools when the
/// Backup was written, and `age` proves they haven't changed since.
fn open(path: &Path, passphrase: &Passphrase) -> Result<Opened, String> {
    let mut small: Vec<(Part, Vec<u8>)> = Vec::new();
    let parts = backup_file::check(path, passphrase, |part, data| {
        if matches!(part, Part::Manifest | Part::Settings | Part::Secrets) {
            let mut bytes = Vec::new();
            data.read_to_end(&mut bytes)
                .map_err(|err| err.to_string())?;
            small.push((part, bytes));
        }
        Ok(())
    })
    .map_err(|err| format!("{} can't be restored: {err}", path.display()))?;
    let mut take = |wanted: Part| -> Vec<u8> {
        let at = small
            .iter()
            .position(|(part, _)| *part == wanted)
            .expect("the check requires every small part");
        small.swap_remove(at).1
    };
    let manifest = serde_json::from_slice(&take(Part::Manifest))
        .map_err(|err| format!("its manifest.json can't be read: {err}"))?;
    let settings = Settings::parse(&String::from_utf8_lossy(&take(Part::Settings)))
        .map_err(|err| format!("its settings.json can't be read: {err}"))?;
    let secrets_text = take(Part::Secrets);
    let secrets = Secrets::parse(&String::from_utf8_lossy(&secrets_text));
    Ok(Opened {
        manifest,
        settings,
        secrets_text,
        secrets,
        parts,
    })
}

/// The version rule (spec §16): the Backup's recorded versions come back as they are,
/// apart from a Component below this program's floor, which goes to its latest version.
fn version_rule(
    manifest: &Manifest,
    settings: &Settings,
    parts: &[Part],
    secrets: &Secrets,
    here: Platform,
) -> Result<VersionPlan, String> {
    let ours = env!("CARGO_PKG_VERSION");
    if manifest.format != backup::FORMAT {
        return Err(format!(
            "its format is {}, which rakazo-stack v{ours} doesn't read.",
            manifest.format
        ));
    }
    let made_by = Version::parse(&manifest.program_version)
        .ok_or("its manifest names no program version.")?;
    if made_by > Version::parse(ours).expect("the program's own version") {
        return Err(format!(
            "it was made by rakazo-stack v{}, newer than this v{ours}. Run `rakazo-stack update` to get the newer program, then restore again.",
            manifest.program_version
        ));
    }
    if let Some(missing) = secrets.missing_rakazo() {
        return Err(format!("its secrets.env has no {missing}."));
    }
    let hindsight = settings.components.hindsight.as_ref();
    if parts.contains(&Part::HindsightDump) {
        if hindsight.is_none() {
            return Err(
                "it holds hindsight.dump, but its settings.json records no Hindsight version."
                    .into(),
            );
        }
        if secrets.get(HINDSIGHT_DB_PASSWORD).is_none() {
            return Err(format!(
                "it holds hindsight.dump, but its secrets.env has no {HINDSIGHT_DB_PASSWORD}."
            ));
        }
    }
    let native = |platform: Platform| platform == Platform::MacOs;
    Ok(VersionPlan {
        rakazo_to_latest: resolve::below_floor(&settings.components.rakazo.version),
        hindsight_to_latest: hindsight.is_some_and(|h| resolve::hindsight_below_floor(&h.version)),
        lemonade_afresh: settings.components.lemonade.is_some()
            && manifest
                .platform
                .as_deref()
                .and_then(Platform::from_id)
                .is_some_and(|from| native(from) != native(here)),
    })
}

impl VersionPlan {
    fn notes(&self, settings: &Settings) -> Vec<String> {
        let mut notes = Vec::new();
        if self.rakazo_to_latest {
            notes.push(format!(
                "This Backup runs Rakazo {}, below the {} this rakazo-stack drives, so it is restored onto the latest Rakazo, which migrates its data forward.",
                settings.components.rakazo.version,
                resolve::rakazo_floor()
            ));
        }
        if let Some(hindsight) = settings.components.hindsight.as_ref()
            && self.hindsight_to_latest
        {
            notes.push(format!(
                "This Backup runs Hindsight {}, below the {} this rakazo-stack drives, so it is restored onto the latest Hindsight, which migrates its data forward.",
                hindsight.version,
                resolve::HINDSIGHT_FLOOR
            ));
        }
        if self.lemonade_afresh {
            notes.push(
                "This Backup comes from an OS that runs Lemonade the other way, so setup sets Lemonade up afresh for this one."
                    .into(),
            );
        }
        notes
    }

    /// Looks up what goes to its latest version. Nothing is written yet.
    fn apply(
        &self,
        settings: &mut Settings,
        agent: &ureq::Agent,
        say: &dyn Fn(&str),
    ) -> Result<(), String> {
        if self.rakazo_to_latest {
            let release = setup::resolve_rakazo(agent, say).map_err(|err| err.0)?;
            settings.components.rakazo = RecordedRakazo {
                version: release.tag,
                images: RakazoImages {
                    app: release.app_digest,
                    computer: release.computer_digest,
                },
            };
        }
        if self.hindsight_to_latest {
            let release = setup::resolve_hindsight(agent, say).map_err(|err| err.0)?;
            settings.record_hindsight(&release);
        }
        if self.lemonade_afresh {
            settings.components.lemonade = None;
        }
        Ok(())
    }
}

/// `.env` for step 5 only: enough for Compose to start both databases. Setup rewrites it.
fn provisional_env(
    project: &str,
    settings: &Settings,
    secrets: &Secrets,
    here: Platform,
) -> String {
    let web_port = overrides::web_port();
    let lemonade = settings
        .lemonade_release()
        .filter(|_| here != Platform::MacOs);
    let mut env = envfile::render(&EnvInputs {
        project,
        rakazo: &settings.rakazo_release(),
        lemonade: lemonade.as_ref(),
        secrets,
        web_port,
        chat: None,
        addresses: &Access::Localhost.addresses(None, web_port),
        tailscale: None,
        memory: None,
    });
    if let Some(hindsight) = settings.hindsight_release() {
        env.push_str(&format!(
            "HINDSIGHT_DB_IMAGE_TAG={HINDSIGHT_DB_TAG}@{}\n",
            hindsight.db_digest
        ));
        for (name, value) in secrets.hindsight() {
            env.push_str(&format!("{name}={value}\n"));
        }
    }
    env
}

/// Anything in the Instance folder but the lock, which every command creates.
fn folder_holds_an_instance(folder: &Path) -> io::Result<bool> {
    match fs::read_dir(folder) {
        Ok(entries) => {
            for entry in entries {
                if entry?.file_name() != LOCK_FILE {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// `<project>_*` volumes that no other Compose project claims, from lines of
/// `name<TAB>project label`.
fn instance_volumes(listing: &str, project: &str) -> Vec<String> {
    let prefix = format!("{project}_");
    listing
        .lines()
        .filter_map(|line| {
            let (name, owner) = line.split_once('\t').unwrap_or((line, ""));
            let (name, owner) = (name.trim(), owner.trim());
            (name.starts_with(&prefix) && (owner.is_empty() || owner == project))
                .then(|| name.to_string())
        })
        .collect()
}

/// `--replace`'s confirmation: the Instance's name, typed on a terminal.
fn confirm_name(
    terminal: Option<&mut (dyn Prompt + '_)>,
    project: &str,
    present: &[String],
) -> Result<(), String> {
    let Some(prompt) = terminal else {
        return Err(
            "--replace asks you to type the Instance's name, which needs a terminal.".into(),
        );
    };
    println!(
        "--replace wipes the Instance here: {}. Restore doesn't back it up first; run `rakazo-stack backup` before this if you may want it again.",
        present.join(", ")
    );
    let typed = prompt.line(&format!(
        "Type the Instance's name, {project}, to wipe it: "
    ));
    if typed.as_deref().map(str::trim) == Some(project) {
        Ok(())
    } else {
        Err("the name didn't match, so nothing was changed.".into())
    }
}

/// `--replace` of the device the Backup came from: the Instance being replaced is the
/// old copy, so its state can come back without a "Duplicate node key".
fn same_device(replaced: Option<&Settings>, backup: &Settings) -> bool {
    replaced
        .and_then(|settings| settings.tailscale_dns_name.as_deref())
        .is_some_and(|name| backup.tailscale_dns_name.as_deref() == Some(name))
}

fn old_machine_off(
    env: &dyn Fn(&str) -> Option<String>,
    terminal: Option<&mut (dyn Prompt + '_)>,
) -> Result<bool, String> {
    if let Some(answer) = env(OLD_MACHINE_OFF_VAR).filter(|value| !value.is_empty()) {
        return yes_or_no(&answer)
            .ok_or_else(|| format!("{OLD_MACHINE_OFF_VAR} is `yes` or `no`."));
    }
    let Some(prompt) = terminal else {
        return Err(format!(
            "this Backup holds the Instance's Tailscale device. Set {OLD_MACHINE_OFF_VAR} to `yes` if the machine it came from is switched off for good, or `no` to join as a new device."
        ));
    };
    println!(
        "This Backup holds the Instance's Tailscale device. If the machine it came from still runs, both copies would share one device (\"Duplicate node key\"), so restore can join as a new device instead."
    );
    let answer = prompt.line("Is the machine this Backup came from switched off for good? [y/N] ");
    Ok(answer.as_deref().and_then(yes_or_no).unwrap_or(false))
}

fn yes_or_no(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    }
}

/// Leaves the Backup's device out: the Instance hasn't joined a tailnet yet. Returns
/// whether it joins one now, which needs a new auth key.
fn rejoin(settings: &mut Settings) -> bool {
    settings.tailscale_dns_name = None;
    settings.access == AccessMode::Tailscale
}

fn new_auth_key(
    env: &dyn Fn(&str) -> Option<String>,
    terminal: Option<&mut (dyn Prompt + '_)>,
) -> Result<AuthKey, String> {
    let typed = match env(AUTH_KEY_VAR).filter(|value| !value.is_empty()) {
        Some(key) => key,
        None => terminal
            .and_then(|prompt| {
                prompt.secret("Tailscale auth key for the new device (not an ephemeral one): ")
            })
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| {
                format!("joining as a new device needs a Tailscale auth key. Set {AUTH_KEY_VAR}, or run this on a terminal.")
            })?,
    };
    AuthKey::parse(&typed).map_err(|err| err.to_string())
}

/// After a stop before step 4, the folder this run created to hold its lock goes again,
/// so a refused restore leaves the machine as it was.
pub fn remove_new_folder(folder: &InstanceFolder) {
    let _ = fs::remove_dir_all(folder.path());
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::backup_file::Spool;
    use crate::resolve::{HindsightRelease, RakazoRelease};
    use crate::settings::RecordedImage;

    struct Typed(VecDeque<&'static str>);

    impl Prompt for Typed {
        fn line(&mut self, _question: &str) -> Option<String> {
            self.0.pop_front().map(str::to_string)
        }

        fn secret(&mut self, _question: &str) -> Option<String> {
            self.0.pop_front().map(str::to_string)
        }
    }

    fn typed(lines: &[&'static str]) -> Typed {
        Typed(lines.iter().copied().collect())
    }

    fn no_env(_name: &str) -> Option<String> {
        None
    }

    fn settings(rakazo: &str) -> Settings {
        Settings::new(&RakazoRelease {
            tag: rakazo.into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        })
    }

    fn with_hindsight(mut settings: Settings, version: &str) -> Settings {
        settings.record_hindsight(&HindsightRelease {
            version: version.into(),
            digest: "sha256:hhh".into(),
            db_digest: "sha256:ddd".into(),
        });
        settings
    }

    fn secrets() -> Secrets {
        let mut secrets = Secrets::default();
        secrets.fill_missing(|n| "ab".repeat(n));
        secrets.fill_missing_hindsight(|n| "cd".repeat(n));
        secrets
    }

    fn manifest(program: &str, platform: Option<&str>) -> Manifest {
        Manifest {
            format: backup::FORMAT,
            program_version: program.into(),
            created: "20261127T120101Z".into(),
            platform: platform.map(str::to_string),
        }
    }

    const ALL_PARTS: [Part; 5] = [
        Part::Manifest,
        Part::Settings,
        Part::Secrets,
        Part::RakazoDump,
        Part::Appdata,
    ];

    #[test]
    fn a_backup_at_the_floor_comes_back_on_its_own_versions() {
        let ours = env!("CARGO_PKG_VERSION");
        let plan = version_rule(
            &manifest(ours, Some("linux")),
            &with_hindsight(settings("v0.16.0"), "1.3.0"),
            &ALL_PARTS,
            &secrets(),
            Platform::Wsl,
        )
        .unwrap();
        assert_eq!(plan, VersionPlan::default());
        assert!(plan.notes(&settings("v0.16.0")).is_empty());
        let older_program = version_rule(
            &manifest("0.0.9", None),
            &settings("v0.17.2"),
            &ALL_PARTS,
            &secrets(),
            Platform::Linux,
        );
        assert_eq!(older_program, Ok(VersionPlan::default()));
    }

    #[test]
    fn a_backup_from_a_newer_program_or_format_is_refused() {
        let err = version_rule(
            &manifest("99.0.0", Some("linux")),
            &settings("v0.16.0"),
            &ALL_PARTS,
            &secrets(),
            Platform::Linux,
        )
        .unwrap_err();
        assert!(
            err.starts_with("it was made by rakazo-stack v99.0.0, newer than this v")
                && err.contains("Run `rakazo-stack update`"),
            "{err}"
        );
        let mut later_format = manifest(env!("CARGO_PKG_VERSION"), None);
        later_format.format = 2;
        let err = version_rule(
            &later_format,
            &settings("v0.16.0"),
            &ALL_PARTS,
            &secrets(),
            Platform::Linux,
        )
        .unwrap_err();
        assert!(err.starts_with("its format is 2"), "{err}");
    }

    #[test]
    fn components_below_the_floor_go_to_latest() {
        let old = with_hindsight(settings("v0.15.0"), "1.2.0");
        let plan = version_rule(
            &manifest(env!("CARGO_PKG_VERSION"), None),
            &old,
            &ALL_PARTS,
            &secrets(),
            Platform::Linux,
        )
        .unwrap();
        assert!(
            plan.rakazo_to_latest && plan.hindsight_to_latest,
            "{plan:?}"
        );
        let notes = plan.notes(&old);
        assert!(notes[0].starts_with("This Backup runs Rakazo v0.15.0, below the v0.16.0"));
        assert!(notes[1].starts_with("This Backup runs Hindsight 1.2.0, below the 1.3.0"));
    }

    #[test]
    fn lemonade_is_set_up_afresh_only_across_the_native_line() {
        let mut recorded = settings("v0.16.0");
        recorded.components.lemonade = Some(RecordedImage {
            version: "v2026.48.3".into(),
            digest: "sha256:lll".into(),
        });
        let afresh = |from: Option<&str>, here: Platform| {
            version_rule(
                &manifest(env!("CARGO_PKG_VERSION"), from),
                &recorded,
                &ALL_PARTS,
                &secrets(),
                here,
            )
            .unwrap()
            .lemonade_afresh
        };
        assert!(afresh(Some("macos"), Platform::Linux));
        assert!(afresh(Some("wsl"), Platform::MacOs));
        assert!(!afresh(Some("linux"), Platform::Wsl));
        assert!(!afresh(Some("macos"), Platform::MacOs));
        assert!(!afresh(None, Platform::MacOs));

        let mut dropped = recorded.clone();
        let plan = VersionPlan {
            lemonade_afresh: true,
            ..VersionPlan::default()
        };
        plan.apply(&mut dropped, &http::agent(), &|_| {}).unwrap();
        assert_eq!(dropped.components.lemonade, None);
        assert_eq!(dropped.components.rakazo, recorded.components.rakazo);
    }

    #[test]
    fn a_backup_missing_what_its_data_needs_is_refused() {
        let mut parts = ALL_PARTS.to_vec();
        parts.push(Part::HindsightDump);
        let err = version_rule(
            &manifest(env!("CARGO_PKG_VERSION"), None),
            &settings("v0.16.0"),
            &parts,
            &secrets(),
            Platform::Linux,
        )
        .unwrap_err();
        assert!(err.contains("records no Hindsight version"), "{err}");

        let partial = Secrets::parse("POSTGRES_PASSWORD=ab\n");
        let err = version_rule(
            &manifest(env!("CARGO_PKG_VERSION"), None),
            &settings("v0.16.0"),
            &ALL_PARTS,
            &partial,
            Platform::Linux,
        )
        .unwrap_err();
        assert_eq!(err, "its secrets.env has no POSTGRES_APP_PASSWORD.");
    }

    #[test]
    fn the_instances_volumes_are_its_projects_and_unclaimed_ones() {
        let listing = "rakazo-stack_appdata\trakazo-stack\nrakazo-stack_pgdata\t\nrakazo-stack_test_pgdata\trakazo-stack_test\nother_appdata\tother\nrakazo-stackx_data\t\n";
        assert_eq!(
            instance_volumes(listing, "rakazo-stack"),
            ["rakazo-stack_appdata", "rakazo-stack_pgdata"]
        );
        assert!(instance_volumes("", "rakazo-stack").is_empty());
    }

    #[test]
    fn a_folder_holding_only_the_lock_is_no_instance() {
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join("rakazo-stack");
        assert!(!folder_holds_an_instance(&folder).unwrap());
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join(LOCK_FILE), b"").unwrap();
        assert!(!folder_holds_an_instance(&folder).unwrap());
        fs::create_dir(folder.join("logs")).unwrap();
        assert!(folder_holds_an_instance(&folder).unwrap());
    }

    #[test]
    fn replace_needs_the_name_typed_on_a_terminal() {
        let present = ["~/rakazo-stack".to_string()];
        let err = confirm_name(None, "rakazo-stack", &present).unwrap_err();
        assert!(err.contains("needs a terminal"), "{err}");
        let mut wrong = typed(&["rakazo"]);
        assert_eq!(
            confirm_name(Some(&mut wrong), "rakazo-stack", &present),
            Err("the name didn't match, so nothing was changed.".into())
        );
        let mut closed = typed(&[]);
        assert!(confirm_name(Some(&mut closed), "rakazo-stack", &present).is_err());
        let mut right = typed(&[" rakazo-stack "]);
        assert_eq!(
            confirm_name(Some(&mut right), "rakazo-stack", &present),
            Ok(())
        );
    }

    #[test]
    fn replacing_the_same_device_keeps_it_without_asking() {
        let mut backup = settings("v0.16.0");
        backup.tailscale_dns_name = Some("rakazo.example.ts.net".into());
        let mut here = backup.clone();
        assert!(same_device(Some(&here), &backup));
        here.tailscale_dns_name = Some("rakazo-1.example.ts.net".into());
        assert!(!same_device(Some(&here), &backup));
        here.tailscale_dns_name = None;
        assert!(!same_device(Some(&here), &backup));
        assert!(!same_device(None, &backup));
        backup.tailscale_dns_name = None;
        assert!(!same_device(Some(&here), &backup));
    }

    #[test]
    fn the_old_machine_question_comes_from_the_variable_or_the_terminal() {
        let answer = |value: &'static str| {
            move |name: &str| (name == OLD_MACHINE_OFF_VAR).then(|| value.to_string())
        };
        assert_eq!(old_machine_off(&answer("yes"), None), Ok(true));
        assert_eq!(old_machine_off(&answer("No"), None), Ok(false));
        assert!(old_machine_off(&answer("maybe"), None).is_err());
        let err = old_machine_off(&no_env, None).unwrap_err();
        assert!(err.contains(OLD_MACHINE_OFF_VAR), "{err}");
        assert_eq!(old_machine_off(&no_env, Some(&mut typed(&["y"]))), Ok(true));
        assert_eq!(old_machine_off(&no_env, Some(&mut typed(&[""]))), Ok(false));
    }

    #[test]
    fn rejoining_forgets_the_device_and_needs_a_key_only_on_tailscale() {
        let mut joined = settings("v0.16.0");
        joined.access = AccessMode::Tailscale;
        joined.tailscale_device_name = Some("rakazo".into());
        joined.tailscale_dns_name = Some("rakazo.example.ts.net".into());
        assert!(rejoin(&mut joined));
        assert_eq!(joined.tailscale_dns_name, None);
        assert_eq!(joined.tailscale_device_name.as_deref(), Some("rakazo"));
        let mut localhost = settings("v0.16.0");
        localhost.tailscale_dns_name = Some("rakazo.example.ts.net".into());
        assert!(!rejoin(&mut localhost));
        assert_eq!(localhost.tailscale_dns_name, None);
    }

    #[test]
    fn the_new_auth_key_is_checked_before_anything_changes() {
        let key = |value: &'static str| {
            move |name: &str| (name == AUTH_KEY_VAR).then(|| value.to_string())
        };
        assert!(new_auth_key(&key("tskey-auth-kPLACEHOLDER-0000"), None).is_ok());
        assert!(new_auth_key(&key("tskey-client-kPLACEHOLDER-0000"), None).is_err());
        let err = new_auth_key(&no_env, None).unwrap_err();
        assert!(err.contains(AUTH_KEY_VAR), "{err}");
        assert!(new_auth_key(&no_env, Some(&mut typed(&["tskey-auth-kTYPED-1"]))).is_ok());
    }

    #[test]
    fn the_passphrase_comes_from_the_variable_then_is_typed_once() {
        let set = |name: &str| (name == PASSPHRASE_VAR).then(|| "from the environment".to_string());
        let mut terminal = typed(&["typed"]);
        assert_eq!(
            passphrase(&set, Some(&mut terminal)).unwrap(),
            Passphrase::new("from the environment".into()).unwrap()
        );
        assert_eq!(
            passphrase(&no_env, Some(&mut terminal)).unwrap(),
            Passphrase::new("typed".into()).unwrap()
        );
        let err = passphrase(&no_env, None).unwrap_err();
        assert!(err.contains(PASSPHRASE_VAR), "{err}");
    }

    #[test]
    fn the_provisional_env_starts_both_databases_and_nothing_else() {
        let recorded = with_hindsight(settings("v0.16.0"), "1.3.0");
        let env = provisional_env("rkst-unit", &recorded, &secrets(), Platform::MacOs);
        for line in [
            "COMPOSE_PROJECT_NAME=rkst-unit",
            "COMPOSE_PROFILES=",
            "RAKAZO_IMAGE_TAG=v0.16.0@sha256:aaa",
            "HINDSIGHT_DB_IMAGE_TAG=pg18@sha256:ddd",
        ] {
            assert!(env.lines().any(|l| l == line), "{line}: {env}");
        }
        for name in ["POSTGRES_PASSWORD=", "HINDSIGHT_DB_PASSWORD="] {
            assert!(env.lines().any(|l| l.starts_with(name)), "{name}");
        }
        assert!(!env.contains("PI_DEFAULT") && !env.contains("HINDSIGHT_API_LLM"));
        let without = provisional_env(
            "rkst-unit",
            &settings("v0.16.0"),
            &secrets(),
            Platform::Linux,
        );
        assert!(!without.contains("HINDSIGHT_DB_IMAGE_TAG"));
    }

    fn write_backup(dir: &Path, passphrase: &Passphrase) -> std::path::PathBuf {
        let mut spool = Spool::new_in(dir).unwrap();
        for part in [Part::RakazoDump, Part::Appdata, Part::TailscaleState] {
            spool
                .add(part, |out| {
                    out.write_all(&[7; 50_000]).map_err(|e| e.to_string())
                })
                .unwrap();
        }
        let manifest = format!(
            r#"{{"format":1,"program_version":"{}","created":"20261127T120101Z","platform":"linux","components":{{}}}}"#,
            env!("CARGO_PKG_VERSION")
        );
        let settings = settings("v0.16.0").to_json();
        let secrets = secrets().to_text();
        let small: [(Part, &[u8]); 3] = [
            (Part::Manifest, manifest.as_bytes()),
            (Part::Settings, settings.as_bytes()),
            (Part::Secrets, secrets.as_bytes()),
        ];
        let path = dir.join("rakazo-stack-backup-20261127T120101Z.tar.age");
        backup_file::write(&path, passphrase, &small, &spool, 0).unwrap();
        path
    }

    #[test]
    fn opens_a_backup_and_refuses_a_wrong_passphrase_or_a_damaged_file() {
        let dir = tempfile::tempdir().unwrap();
        let right = Passphrase::new("placeholder-backup-passphrase".into()).unwrap();
        let path = write_backup(dir.path(), &right);
        let before = fs::read(&path).unwrap();

        let opened = open(&path, &right).unwrap();
        assert_eq!(opened.manifest.platform.as_deref(), Some("linux"));
        assert_eq!(opened.settings, settings("v0.16.0"));
        assert_eq!(opened.secrets_text, secrets().to_text().into_bytes());
        assert!(opened.parts.contains(&Part::TailscaleState));
        assert!(!opened.parts.contains(&Part::HindsightDump));

        let wrong = Passphrase::new("not the passphrase".into()).unwrap();
        let err = open(&path, &wrong).err().unwrap();
        assert!(err.contains("the passphrase doesn't open it"), "{err}");

        let damaged = dir.path().join("damaged.tar.age");
        let mut bytes = before.clone();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 1;
        fs::write(&damaged, &bytes).unwrap();
        assert!(open(&damaged, &right).is_err());
        let cut = dir.path().join("cut.tar.age");
        fs::write(&cut, &before[..before.len() - 10]).unwrap();
        assert!(open(&cut, &right).is_err());

        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "the Backup file is never modified"
        );
    }
}
