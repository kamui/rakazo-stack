mod docker;
mod instance;
mod platform;
mod preflight;
mod version;

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

#[derive(Subcommand, Clone, Copy)]
enum Command {
    /// Set up the Instance, or change its settings.
    Setup,
    /// Move every Component to its newest release.
    Update,
    /// Write an encrypted Backup of the Instance.
    Backup,
    /// Restore the Instance from a Backup.
    Restore,
}

impl Command {
    fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Update => "update",
            Self::Backup => "backup",
            Self::Restore => "restore",
        }
    }

    fn preflight(self) -> Option<preflight::Command> {
        match self {
            Self::Setup => Some(preflight::Command::Setup),
            Self::Update => Some(preflight::Command::Update),
            Self::Restore => Some(preflight::Command::Restore),
            Self::Backup => None,
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

    if let Some(preflight_command) = command.preflight() {
        let facts = preflight::gather(Platform::detect(), preflight_command, &home, first_setup);
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

    eprintln!("rakazo-stack {}: not implemented yet", command.name());
    ExitCode::FAILURE
}
