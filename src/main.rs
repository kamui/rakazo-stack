mod access;
mod answers;
mod backup;
mod backup_file;
mod checklist;
mod compose_env;
mod composio;
mod computer_egress;
mod database_settings;
mod docker;
mod envfile;
mod github;
mod holds;
mod http;
mod instance;
mod interrupt;
mod lemonade;
mod lemond;
mod logs;
mod memory;
mod model_provider;
mod overrides;
mod platform;
mod preflight;
mod rakazo;
mod redact;
mod registry;
mod resolve;
mod restore;
mod runlog;
mod secrets;
mod self_update;
mod settings;
mod setup;
mod tailscale;
mod update;
mod version;
mod voice;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use instance::{InstanceFolder, LockError};
use platform::Platform;
use preflight::Level;

/// Installs and runs a self-hosted Rakazo Instance.
#[derive(Parser)]
#[command(name = "rakazo-stack", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Set up the Instance, or change its settings.
    Setup {
        /// Ask nothing: take answers from environment variables, then settings.json.
        #[arg(long)]
        no_questions: bool,
    },
    /// Move every Component to its newest release, behind a Backup.
    Update {
        /// Ask nothing; the Backup passphrase comes from RAKAZO_STACK_BACKUP_PASSPHRASE.
        #[arg(long)]
        yes: bool,
        /// Skip the Backup taken before the switch.
        #[arg(long)]
        no_backup: bool,
        /// Keep a Component at a version (rakazo, hindsight, lemonade or tailscale).
        #[arg(long, value_name = "COMPONENT=VERSION")]
        hold: Vec<String>,
        /// Release a Component's Hold.
        #[arg(long, value_name = "COMPONENT")]
        unhold: Vec<String>,
    },
    /// Write an encrypted Backup of the Instance.
    Backup {
        /// The folder to write the Backup to, instead of ~/rakazo-stack-backups.
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
    /// Restore the Instance from a Backup.
    Restore {
        /// The Backup file.
        file: PathBuf,
        /// Wipe the Instance here first, once you type its name.
        #[arg(long)]
        replace: bool,
    },
    /// Write one file of logs, with secrets replaced, to attach to a bug report.
    Logs,
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Setup { .. } => "setup",
            Self::Update { .. } => "update",
            Self::Backup { .. } => "backup",
            Self::Restore { .. } => "restore",
            Self::Logs => "logs",
        }
    }

    fn preflight(&self) -> Option<preflight::Command> {
        match self {
            Self::Setup { .. } => Some(preflight::Command::Setup),
            Self::Update { .. } => Some(preflight::Command::Update),
            Self::Restore { .. } => Some(preflight::Command::Restore),
            Self::Backup { .. } | Self::Logs => None,
        }
    }
}

fn main() -> ExitCode {
    let command = Cli::parse().command;
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("rakazo-stack: HOME isn't set");
        return ExitCode::FAILURE;
    };
    let folder = InstanceFolder::for_home(&home);
    let platform = Platform::detect();
    // It only reads, so it runs beside another command, such as a setup that hangs.
    if let Command::Logs = command {
        return run_logs(platform, &folder);
    }
    runlog::begin(command.name(), platform);
    let folder_existed = folder.path().exists();
    let first_setup = !folder.has_settings();
    let handed_over = matches!(command, Command::Update { .. })
        .then(|| std::env::var(self_update::HANDOVER_VAR).ok())
        .flatten();
    let lock = match handed_over.as_deref() {
        Some(fd) => folder.adopt_lock(fd),
        None => folder.lock(),
    };
    let lock = match lock {
        Ok(lock) => lock,
        Err(LockError::Held) => {
            eprintln!("Stop: another rakazo-stack command is running.");
            return ExitCode::FAILURE;
        }
        Err(LockError::HandOver) => {
            eprintln!(
                "Stop: the older rakazo-stack didn't hand over the Instance's lock. Run `rakazo-stack update` again."
            );
            return ExitCode::FAILURE;
        }
        Err(LockError::Io(err)) => {
            eprintln!(
                "rakazo-stack: can't lock {}: {err}",
                folder.path().display()
            );
            return ExitCode::FAILURE;
        }
    };

    if let Command::Update { yes, .. } = &command
        && handed_over.is_none()
        && !first_setup
        && let Err(err) = self_update::program_first(&lock, &home, *yes)
    {
        runlog::warn(&format!("Stop: {err}"));
        return ExitCode::FAILURE;
    }

    // After update's step 2, which stops with the Instance as it was. Setup's log starts
    // after its key checks (spec §7), and a new Instance's restore's once it has written
    // the Instance (spec §16).
    if !first_setup
        && !matches!(command, Command::Setup { .. })
        && let Err(err) = runlog::keep(folder.path())
    {
        runlog::warn(&format!("Stop: can't write the run log: {err}"));
        return ExitCode::FAILURE;
    }

    let mut nftables = false;
    if let Some(preflight_command) = command.preflight() {
        let recorded_rakazo = folder
            .read("settings.json")
            .ok()
            .flatten()
            .and_then(|bytes| settings::Settings::parse(&String::from_utf8_lossy(&bytes)).ok())
            .map(|s| s.components.rakazo.version);
        let facts = preflight::gather(
            platform,
            preflight_command,
            &home,
            first_setup,
            recorded_rakazo,
        );
        nftables = facts
            .info
            .as_ref()
            .is_some_and(|i| i.firewall_is_nftables());
        let findings = preflight::evaluate(&facts);
        for finding in &findings {
            let label = match finding.level {
                Level::Stop => "Stop",
                Level::Warn => "Warning",
            };
            runlog::warn(&format!("{label}: {}", finding.message));
        }
        if findings.iter().any(|f| f.level == Level::Stop) {
            if matches!(command, Command::Restore { .. }) && !folder_existed {
                restore::remove_new_folder(&folder);
            }
            return ExitCode::FAILURE;
        }
        runlog::warn("Preflight passed.");
    }

    if let Command::Backup { output } = &command {
        return run_backup(platform, &home, &folder, output.as_deref());
    }

    if let Command::Restore { file, replace } = &command {
        let replace = if *replace {
            restore::Replace::Typed
        } else {
            restore::Replace::No
        };
        return run_restore(platform, &folder, file, replace, nftables, folder_existed);
    }

    if let Command::Update {
        yes,
        no_backup,
        hold,
        unhold,
    } = command
    {
        let docker = docker::Docker::find(platform).expect("preflight found Docker");
        let update = update::Update {
            platform,
            docker: &docker,
            folder: &folder,
            home: &home,
            nftables,
            options: update::Options {
                yes,
                no_backup,
                hold,
                unhold,
            },
        };
        return match update.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                runlog::warn(&format!("Stop: {}", err.0));
                ExitCode::FAILURE
            }
        };
    }

    if let Command::Setup { no_questions } = command {
        let docker = docker::Docker::find(platform).expect("preflight found Docker");
        let setup = setup::Setup {
            platform,
            docker: &docker,
            folder: &folder,
            no_questions,
            nftables,
            switch: None,
        };
        return match setup.run() {
            Ok(()) => {
                update::nudge(&folder, platform);
                ExitCode::SUCCESS
            }
            Err(err) => {
                runlog::warn(&format!("Stop: {}", err.0));
                ExitCode::FAILURE
            }
        };
    }

    eprintln!("rakazo-stack {}: not implemented yet", command.name());
    ExitCode::FAILURE
}

fn run_logs(platform: Platform, folder: &InstanceFolder) -> ExitCode {
    match logs::run(platform, folder) {
        Ok((path, docker_missing)) => {
            if let Some(why) = docker_missing {
                eprintln!("{why}");
            }
            println!("Wrote {}.", path.display());
            println!("{}", logs::READ_FIRST);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Stop: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_restore(
    platform: Platform,
    folder: &InstanceFolder,
    file: &std::path::Path,
    replace: restore::Replace,
    nftables: bool,
    folder_existed: bool,
) -> ExitCode {
    let docker = docker::Docker::find(platform).expect("preflight found Docker");
    let mut terminal = answers::Terminal::attached().ok();
    let restore = restore::Restore {
        platform,
        docker: &docker,
        folder,
        file,
        replace,
        nftables,
    };
    let env = |name: &str| std::env::var(name).ok();
    match restore.run(
        &env,
        terminal.as_mut().map(|t| t as &mut dyn answers::Prompt),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if !err.touched && !folder_existed {
                restore::remove_new_folder(folder);
            }
            runlog::warn(&format!("Stop: {}", err.message));
            ExitCode::FAILURE
        }
    }
}

fn run_backup(
    platform: Platform,
    home: &std::path::Path,
    folder: &InstanceFolder,
    output: Option<&std::path::Path>,
) -> ExitCode {
    let stop = |message: String| {
        runlog::warn(&format!("Stop: {message}"));
        ExitCode::FAILURE
    };
    let mut terminal = answers::Terminal::attached().ok();
    let passphrase = match backup::passphrase(
        |name| std::env::var(name).ok(),
        terminal.as_mut().map(|t| t as &mut dyn answers::Prompt),
    ) {
        Ok(passphrase) => passphrase,
        Err(err) => return stop(err.0),
    };
    let Some(docker) = docker::Docker::find(platform) else {
        return stop("no Docker CLI was found.".into());
    };
    let backup = backup::Backup {
        docker: &docker,
        folder,
        home,
        output,
        keep_rakazo_stopped: false,
    };
    match backup.run(&passphrase) {
        Ok(_) => {
            update::nudge(folder, platform);
            ExitCode::SUCCESS
        }
        Err(err) => stop(err.0),
    }
}
