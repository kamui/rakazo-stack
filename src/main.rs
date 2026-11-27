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
mod http;
mod instance;
mod lemonade;
mod lemond;
mod memory;
mod model_provider;
mod overrides;
mod platform;
mod preflight;
mod rakazo;
mod registry;
mod resolve;
mod runlog;
mod secrets;
mod settings;
mod setup;
mod tailscale;
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
    /// Move every Component to its newest release.
    Update,
    /// Write an encrypted Backup of the Instance.
    Backup {
        /// The folder to write the Backup to, instead of ~/rakazo-stack-backups.
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
    /// Restore the Instance from a Backup.
    Restore,
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Setup { .. } => "setup",
            Self::Update => "update",
            Self::Backup { .. } => "backup",
            Self::Restore => "restore",
        }
    }

    fn preflight(&self) -> Option<preflight::Command> {
        match self {
            Self::Setup { .. } => Some(preflight::Command::Setup),
            Self::Update => Some(preflight::Command::Update),
            Self::Restore => Some(preflight::Command::Restore),
            Self::Backup { .. } => None,
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
    let first_setup = !folder.has_settings();
    let _lock = match folder.lock() {
        Ok(lock) => lock,
        Err(LockError::Held) => {
            eprintln!("Stop: another rakazo-stack command is running.");
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

    let platform = Platform::detect();
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
            eprintln!("{label}: {}", finding.message);
        }
        if findings.iter().any(|f| f.level == Level::Stop) {
            return ExitCode::FAILURE;
        }
        eprintln!("Preflight passed.");
    }

    if let Command::Backup { output } = &command {
        return run_backup(platform, &home, &folder, output.as_deref());
    }

    if let Command::Setup { no_questions } = command {
        let docker = docker::Docker::find(platform).expect("preflight found Docker");
        let setup = setup::Setup {
            platform,
            docker: &docker,
            folder: &folder,
            no_questions,
            nftables,
        };
        return match setup.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("Stop: {}", err.0);
                ExitCode::FAILURE
            }
        };
    }

    eprintln!("rakazo-stack {}: not implemented yet", command.name());
    ExitCode::FAILURE
}

fn run_backup(
    platform: Platform,
    home: &std::path::Path,
    folder: &InstanceFolder,
    output: Option<&std::path::Path>,
) -> ExitCode {
    let stop = |message: String| {
        eprintln!("Stop: {message}");
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
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => stop(err.0),
    }
}
