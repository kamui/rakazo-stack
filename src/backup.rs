//! `rakazo-stack backup` (spec §15): stops Rakazo's writers, dumps both databases inside
//! their containers, archives the volumes in throwaway containers, starts Rakazo again,
//! then writes one encrypted Backup file and checks it.

use std::fs::{self, DirBuilder};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::answers::Prompt;
use crate::backup_file::{self, EXTENSION, Part, Passphrase, Spool};
use crate::docker::Docker;
use crate::instance::InstanceFolder;
use crate::memory;
use crate::resolve::HINDSIGHT_DB_TAG;
use crate::runlog::utc_stamp;
use crate::settings::{Components, Settings};

pub const PASSPHRASE_VAR: &str = "RAKAZO_STACK_BACKUP_PASSPHRASE";
pub const BACKUPS_FOLDER: &str = "rakazo-stack-backups";

/// Rakazo's services that write to its database or `appdata`. `web` and Postgres keep
/// running.
const WRITERS: [&str; 3] = ["api", "worker", "supervisor"];
const HINDSIGHT_DB: &str = "hindsight-db";
/// Bot computers carry this label and mount the Instance's `appdata` volume.
const COMPUTER_LABEL: &str = "label=rakazo.managed=true";
/// `appdata` folders Rakazo rebuilds itself.
const APPDATA_EXCLUDED: [&str; 2] = ["staging", "mcp-packages"];
/// Holds `hindsight-db`'s image while memory is off, when `.env` doesn't.
const BACKUP_ENV: &str = ".backup.env";
const WAIT_TIMEOUT_SECONDS: &str = "900";
const FORMAT: u32 = 1;

#[derive(Debug)]
pub struct BackupError(pub String);

impl<E: std::fmt::Display> From<E> for BackupError {
    fn from(err: E) -> Self {
        Self(err.to_string())
    }
}

/// The passphrase from [`PASSPHRASE_VAR`], or typed twice on the terminal. Without
/// either, the run stops before anything changes.
pub fn passphrase(
    env: impl Fn(&str) -> Option<String>,
    terminal: Option<&mut dyn Prompt>,
) -> Result<Passphrase, BackupError> {
    if let Some(passphrase) = env(PASSPHRASE_VAR).and_then(Passphrase::new) {
        return Ok(passphrase);
    }
    let Some(prompt) = terminal else {
        return Err(BackupError(format!(
            "no Backup passphrase. Set {PASSPHRASE_VAR}, or run `rakazo-stack backup` on a terminal."
        )));
    };
    let first = prompt
        .secret("Backup passphrase (a lost passphrase makes the Backup unusable): ")
        .and_then(Passphrase::new)
        .ok_or_else(|| BackupError("no Backup passphrase was given.".into()))?;
    let again = prompt.secret("The same passphrase again: ");
    if again.and_then(Passphrase::new).as_ref() != Some(&first) {
        return Err(BackupError("the two passphrases don't match.".into()));
    }
    Ok(first)
}

/// The folder Backups go to: `~/rakazo-stack-backups` at mode 700, or `--output`.
fn output_folder(home: &Path, chosen: Option<&Path>) -> io::Result<PathBuf> {
    let folder = chosen.map_or_else(|| home.join(BACKUPS_FOLDER), Path::to_path_buf);
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)?;
    if chosen.is_none() {
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700))?;
    }
    Ok(folder)
}

pub fn file_name(stamp: &str) -> String {
    format!("rakazo-stack-backup-{stamp}.{EXTENSION}")
}

#[derive(Serialize)]
struct Manifest<'a> {
    format: u32,
    program_version: &'static str,
    created: &'a str,
    components: &'a Components,
}

/// What the Backup stopped, to start again afterwards.
struct Stopped {
    services: Vec<String>,
    computers: Vec<String>,
}

/// The images that read each part back in the self-check.
struct Images {
    postgres: String,
    hindsight_db: Option<String>,
}

pub struct Backup<'a> {
    pub docker: &'a Docker,
    pub folder: &'a InstanceFolder,
    pub home: &'a Path,
    /// `--output`.
    pub output: Option<&'a Path>,
    /// `update` keeps Rakazo's writers and bot computers stopped (spec §14).
    pub keep_rakazo_stopped: bool,
}

impl Backup<'_> {
    /// Writes the Backup and returns its path. On any failure no Backup file is left.
    pub fn run(&self, passphrase: &Passphrase) -> Result<PathBuf, BackupError> {
        let no_instance =
            || BackupError("there is no Instance here. Run `rakazo-stack setup`.".into());
        let settings_json = self.folder.read("settings.json")?.ok_or_else(no_instance)?;
        let secrets_env = self.folder.read("secrets.env")?.ok_or_else(no_instance)?;
        let env = self.folder.read(".env")?.ok_or_else(no_instance)?;
        let settings = Settings::parse(&String::from_utf8_lossy(&settings_json))
            .map_err(|err| BackupError(format!("settings.json can't be read: {err}")))?;
        let project = project_name(&String::from_utf8_lossy(&env)).ok_or_else(|| {
            BackupError(".env names no Compose project. Run `rakazo-stack setup`.".into())
        })?;

        let output = output_folder(self.home, self.output)
            .map_err(|err| BackupError(format!("can't create the Backup folder: {err}")))?;
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let stamp = utc_stamp(seconds);
        let path = output.join(file_name(&stamp));
        if path.exists() {
            return Err(BackupError(format!("{} already exists.", path.display())));
        }
        let mut spool = Spool::new_in(&output)?;

        let stopped = self.running(&project)?;
        println!("Stopping Rakazo for the Backup…");
        let stopped_at = Instant::now();
        let taken = self
            .stop(&stopped)
            .and_then(|()| self.take_parts(&project, &settings, &mut spool));
        let restarted = if self.keep_rakazo_stopped {
            Ok(())
        } else {
            let restarted = self.start(&stopped);
            println!(
                "Rakazo was stopped for {} s.",
                stopped_at.elapsed().as_secs()
            );
            restarted
        };
        let images = taken?;
        restarted?;

        let manifest = Manifest {
            format: FORMAT,
            program_version: env!("CARGO_PKG_VERSION"),
            created: &stamp,
            components: &settings.components,
        };
        let manifest = serde_json::to_vec_pretty(&manifest)?;
        let small: [(Part, &[u8]); 3] = [
            (Part::Manifest, &manifest),
            (Part::Settings, &settings_json),
            (Part::Secrets, &secrets_env),
        ];
        let partial = output.join(format!(".{}.partial", file_name(&stamp)));
        let checked = backup_file::write(&partial, passphrase, &small, &spool, seconds)
            .map_err(|err| format!("writing it failed: {err}"))
            .and_then(|()| {
                println!("Checking the Backup…");
                self.check(&partial, passphrase, &small, &spool, &project, &images)
            })
            .and_then(|()| fs::rename(&partial, &path).map_err(|err| err.to_string()));
        if let Err(err) = checked {
            let _ = fs::remove_file(&partial);
            return Err(BackupError(format!("the Backup failed its check: {err}")));
        }
        let size = fs::metadata(&path).map_or(0, |m| m.len());
        println!("Backup written: {} ({})", path.display(), megabytes(size));
        Ok(path)
    }

    /// Rakazo's running writers and this Instance's running bot computers.
    fn running(&self, project: &str) -> Result<Stopped, BackupError> {
        let running = self.compose(&["ps", "--status", "running", "--services"])?;
        let services = WRITERS
            .iter()
            .filter(|writer| running.lines().any(|line| line.trim() == **writer))
            .map(|writer| writer.to_string())
            .collect();
        let appdata = format!("volume={}", volume(project, "appdata"));
        let computers = self.docker_ok(&[
            "ps",
            "-q",
            "--no-trunc",
            "--filter",
            COMPUTER_LABEL,
            "--filter",
            &appdata,
        ])?;
        Ok(Stopped {
            services,
            computers: computers.split_whitespace().map(str::to_string).collect(),
        })
    }

    fn stop(&self, stopped: &Stopped) -> Result<(), BackupError> {
        if !stopped.services.is_empty() {
            let mut args = vec!["stop"];
            args.extend(stopped.services.iter().map(String::as_str));
            self.compose(&args)?;
        }
        if !stopped.computers.is_empty() {
            let mut args = vec!["stop"];
            args.extend(stopped.computers.iter().map(String::as_str));
            self.docker_ok(&args)?;
        }
        Ok(())
    }

    fn start(&self, stopped: &Stopped) -> Result<(), BackupError> {
        println!("Starting Rakazo again…");
        if !stopped.services.is_empty() {
            let mut args = vec![
                "up",
                "-d",
                "--no-recreate",
                "--wait",
                "--wait-timeout",
                WAIT_TIMEOUT_SECONDS,
            ];
            args.extend(stopped.services.iter().map(String::as_str));
            self.compose(&args)?;
        }
        if !stopped.computers.is_empty() {
            let mut args = vec!["start"];
            args.extend(stopped.computers.iter().map(String::as_str));
            self.docker_ok(&args)?;
        }
        Ok(())
    }

    /// Dumping first means every row has its file in the archive.
    fn take_parts(
        &self,
        project: &str,
        settings: &Settings,
        spool: &mut Spool,
    ) -> Result<Images, BackupError> {
        println!("Dumping Rakazo's database…");
        let postgres = self.image_of(&[], "postgres")?;
        self.dump(spool, Part::RakazoDump, &[], "postgres", "rakazo")?;
        let hindsight_db = if self.volume_exists(&volume(project, "hindsight-pgdata")) {
            println!("Dumping Hindsight's database…");
            Some(self.dump_hindsight(settings, spool)?)
        } else {
            None
        };
        println!("Archiving appdata…");
        self.archive(spool, Part::Appdata, project, "appdata", &postgres)?;
        if self.volume_exists(&volume(project, "tailscale-state")) {
            self.archive(
                spool,
                Part::TailscaleState,
                project,
                "tailscale-state",
                &postgres,
            )?;
        }
        Ok(Images {
            postgres,
            hindsight_db,
        })
    }

    /// With memory off, `hindsight-db` starts alone for its dump and is removed again,
    /// as setup leaves it.
    fn dump_hindsight(
        &self,
        settings: &Settings,
        spool: &mut Spool,
    ) -> Result<String, BackupError> {
        let running = self.compose(&["ps", "--status", "running", "--services"])?;
        if running.lines().any(|line| line.trim() == HINDSIGHT_DB) {
            let image = self.image_of(&[], HINDSIGHT_DB)?;
            self.dump(spool, Part::HindsightDump, &[], HINDSIGHT_DB, "hindsight")?;
            return Ok(image);
        }
        let release = settings.hindsight_release().ok_or_else(|| {
            BackupError(
                "hindsight-pgdata exists, but settings.json records no Hindsight version.".into(),
            )
        })?;
        self.folder.write(
            BACKUP_ENV,
            format!(
                "HINDSIGHT_DB_IMAGE_TAG={HINDSIGHT_DB_TAG}@{}\n",
                release.db_digest
            )
            .as_bytes(),
        )?;
        let alone = [
            "--env-file",
            ".env",
            "--env-file",
            BACKUP_ENV,
            "--profile",
            memory::PROFILE,
        ];
        let dumped = self
            .compose(
                &[
                    &alone[..],
                    &[
                        "up",
                        "-d",
                        "--no-deps",
                        "--wait",
                        "--wait-timeout",
                        WAIT_TIMEOUT_SECONDS,
                        HINDSIGHT_DB,
                    ],
                ]
                .concat(),
            )
            .and_then(|_| self.image_of(&alone, HINDSIGHT_DB))
            .and_then(|image| {
                self.dump(
                    spool,
                    Part::HindsightDump,
                    &alone,
                    HINDSIGHT_DB,
                    "hindsight",
                )
                .map(|()| image)
            });
        let removed =
            self.compose(&[&alone[..], &["rm", "--stop", "--force", HINDSIGHT_DB]].concat());
        let _ = fs::remove_file(self.folder.file(BACKUP_ENV));
        let image = dumped?;
        removed?;
        Ok(image)
    }

    /// `pg_dump` inside the service's container. `-T` keeps a terminal from corrupting it.
    fn dump(
        &self,
        spool: &mut Spool,
        part: Part,
        prefix: &[&str],
        service: &str,
        database: &str,
    ) -> Result<(), BackupError> {
        let mut args = prefix.to_vec();
        args.extend([
            "exec",
            "-T",
            service,
            "pg_dump",
            "-Fc",
            "--no-owner",
            "--no-privileges",
            "-U",
            database,
            "-d",
            database,
        ]);
        let command = self.docker.compose(self.folder.path(), &args);
        spool
            .add(part, |out| {
                stream_out(command, out)
                    .map_err(|err| format!("pg_dump in {service} failed: {err}"))
            })
            .map_err(BackupError)
    }

    /// GNU tar in a throwaway container from Rakazo's own Postgres image, which the
    /// Instance already has.
    fn archive(
        &self,
        spool: &mut Spool,
        part: Part,
        project: &str,
        name: &str,
        image: &str,
    ) -> Result<(), BackupError> {
        let mut args = vec![
            "run".to_string(),
            "--rm".into(),
            "--network".into(),
            "none".into(),
            "--name".into(),
            format!("{project}-backup-{name}"),
            "-v".into(),
            format!("{}:/data:ro", volume(project, name)),
            "--entrypoint".into(),
            "tar".into(),
            image.into(),
            "--create".into(),
            "--file=-".into(),
            "--numeric-owner".into(),
            "--directory=/data".into(),
        ];
        if part == Part::Appdata {
            args.push("--anchored".into());
            args.extend(
                APPDATA_EXCLUDED
                    .iter()
                    .map(|dir| format!("--exclude=./{dir}")),
            );
        }
        args.push(".".into());
        let command = self.docker.command(&args);
        spool
            .add(part, |out| {
                stream_out(command, out).map_err(|err| format!("archiving {name} failed: {err}"))
            })
            .map_err(BackupError)
    }

    /// Spec §15 step 6: every part is read back by the tool that will restore it.
    fn check(
        &self,
        path: &Path,
        passphrase: &Passphrase,
        small: &[(Part, &[u8])],
        spool: &Spool,
        project: &str,
        images: &Images,
    ) -> Result<(), String> {
        let read_back = |part: Part, data: &mut dyn Read| -> Result<(), String> {
            if let Some((_, expected)) = small.iter().find(|(p, _)| *p == part) {
                let mut bytes = Vec::new();
                data.read_to_end(&mut bytes)
                    .map_err(|err| err.to_string())?;
                return if bytes == *expected {
                    Ok(())
                } else {
                    Err("it differs from what was written".into())
                };
            }
            let (image, tool): (&str, &[&str]) = match part {
                Part::RakazoDump => (&images.postgres, &["pg_restore", "--list"]),
                Part::HindsightDump => (
                    images
                        .hindsight_db
                        .as_deref()
                        .ok_or("no Hindsight dump was taken")?,
                    &["pg_restore", "--list"],
                ),
                _ => (&images.postgres, &["tar", "--list", "--file=-"]),
            };
            let name = format!("{project}-check-{}", part.name().replace('.', "-"));
            let args = [
                "run",
                "--rm",
                "-i",
                "--network",
                "none",
                "--name",
                &name,
                "--entrypoint",
                tool[0],
                image,
            ];
            let command = self.docker.command(args.iter().chain(&tool[1..]));
            stream_in(command, data).map_err(|err| format!("`{}` failed: {err}", tool.join(" ")))
        };
        let seen = backup_file::check(path, passphrase, read_back)?;
        let written: Vec<Part> = small
            .iter()
            .map(|(part, _)| *part)
            .chain(
                [
                    Part::RakazoDump,
                    Part::HindsightDump,
                    Part::Appdata,
                    Part::TailscaleState,
                ]
                .into_iter()
                .filter(|part| spool.has(*part)),
            )
            .collect();
        match written.iter().find(|part| !seen.contains(part)) {
            Some(missing) => Err(format!("it has no {}", missing.name())),
            None => Ok(()),
        }
    }

    fn image_of(&self, prefix: &[&str], service: &str) -> Result<String, BackupError> {
        let id = self.compose(&[prefix, &["ps", "-a", "-q", service]].concat())?;
        let id = id.lines().next().unwrap_or_default().trim();
        if id.is_empty() {
            return Err(BackupError(format!(
                "the Instance has no {service} container."
            )));
        }
        Ok(self
            .docker_ok(&["inspect", "--format", "{{.Image}}", id])?
            .trim()
            .to_string())
    }

    fn volume_exists(&self, name: &str) -> bool {
        self.docker
            .command(["volume", "inspect", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn compose(&self, args: &[&str]) -> Result<String, BackupError> {
        let out = self.docker.compose(self.folder.path(), args).output()?;
        checked("docker compose", args, out)
    }

    fn docker_ok(&self, args: &[&str]) -> Result<String, BackupError> {
        let out = self.docker.output(args)?;
        checked("docker", args, out)
    }
}

fn checked(program: &str, args: &[&str], out: std::process::Output) -> Result<String, BackupError> {
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let reason =
        last_line(&String::from_utf8_lossy(&out.stderr)).unwrap_or_else(|| out.status.to_string());
    Err(BackupError(format!(
        "`{program} {}` failed: {reason}",
        args.join(" ")
    )))
}

fn volume(project: &str, name: &str) -> String {
    format!("{project}_{name}")
}

fn project_name(env: &str) -> Option<String> {
    env.lines()
        .find_map(|line| line.trim().strip_prefix("COMPOSE_PROJECT_NAME="))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

/// Runs `command` with its standard output copied into `out`.
fn stream_out(mut command: Command, out: &mut dyn Write) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let errors = collect_stderr(&mut child);
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let copied = io::copy(&mut stdout, out).map(drop);
    drop(stdout);
    let status = child.wait().map_err(|err| err.to_string())?;
    finish(status, errors, copied)
}

/// Runs `command` with `input` on its standard input.
fn stream_in(mut command: Command, input: &mut dyn Read) -> Result<(), String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let errors = collect_stderr(&mut child);
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let copied = io::copy(input, &mut stdin).map(drop);
    drop(stdin);
    let status = child.wait().map_err(|err| err.to_string())?;
    finish(status, errors, copied)
}

/// Reads standard error on its own thread, so a chatty child can't block on a full pipe.
fn collect_stderr(child: &mut std::process::Child) -> thread::JoinHandle<String> {
    let mut stderr = child.stderr.take().expect("stderr is piped");
    thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    })
}

fn finish(
    status: ExitStatus,
    errors: thread::JoinHandle<String>,
    copied: io::Result<()>,
) -> Result<(), String> {
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        return Err(last_line(&errors).unwrap_or_else(|| format!("exited with {status}")));
    }
    copied.map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Typed(VecDeque<&'static str>);

    impl Prompt for Typed {
        fn line(&mut self, _question: &str) -> Option<String> {
            unreachable!("backup asks only for secrets")
        }

        fn secret(&mut self, _question: &str) -> Option<String> {
            self.0.pop_front().map(str::to_string)
        }
    }

    fn env(value: Option<&'static str>) -> impl Fn(&str) -> Option<String> {
        move |name| {
            assert_eq!(name, PASSPHRASE_VAR);
            value.map(str::to_string)
        }
    }

    #[test]
    fn the_passphrase_comes_from_the_variable_before_the_terminal() {
        let mut terminal = Typed(VecDeque::from(["typed", "typed"]));
        let from_env = passphrase(env(Some("from the environment")), Some(&mut terminal)).unwrap();
        assert_eq!(
            from_env,
            Passphrase::new("from the environment".into()).unwrap()
        );
        assert_eq!(terminal.0.len(), 2);

        let typed = passphrase(env(Some("")), Some(&mut terminal)).unwrap();
        assert_eq!(typed, Passphrase::new("typed".into()).unwrap());
    }

    #[test]
    fn no_passphrase_or_two_different_ones_stop_the_run() {
        let err = passphrase(env(None), None).unwrap_err();
        assert!(err.0.contains(PASSPHRASE_VAR), "{}", err.0);
        let mut differ = Typed(VecDeque::from(["one", "two"]));
        assert_eq!(
            passphrase(env(None), Some(&mut differ)).unwrap_err().0,
            "the two passphrases don't match."
        );
        let mut empty = Typed(VecDeque::from([""]));
        assert_eq!(
            passphrase(env(None), Some(&mut empty)).unwrap_err().0,
            "no Backup passphrase was given."
        );
    }

    #[test]
    fn names_the_file_after_its_utc_time() {
        assert_eq!(
            file_name("20261127T120101Z"),
            "rakazo-stack-backup-20261127T120101Z.tar.age"
        );
    }

    #[test]
    fn reads_the_project_from_env() {
        let env = "# Generated\nCOMPOSE_PROJECT_NAME=rkst-test-1\nCOMPOSE_FILE=a:b\n";
        assert_eq!(project_name(env).as_deref(), Some("rkst-test-1"));
        assert_eq!(project_name("COMPOSE_PROJECT_NAME=\n"), None);
    }

    #[test]
    fn the_default_folder_is_private_and_a_chosen_one_keeps_its_mode() {
        let home = tempfile::tempdir().unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let default = output_folder(home.path(), None).unwrap();
        assert_eq!(default, home.path().join(BACKUPS_FOLDER));
        assert_eq!(mode(&default), 0o700);

        let chosen = home.path().join("usb");
        fs::create_dir(&chosen).unwrap();
        fs::set_permissions(&chosen, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(output_folder(home.path(), Some(&chosen)).unwrap(), chosen);
        assert_eq!(mode(&chosen), 0o755);
        let new = home.path().join("new/backups");
        output_folder(home.path(), Some(&new)).unwrap();
        assert_eq!(mode(&new), 0o700);
    }
}
