use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    Setup,
    /// Move every Component to its newest release.
    Update,
    /// Write an encrypted Backup of the Instance.
    Backup,
    /// Restore the Instance from a Backup.
    Restore,
}

fn main() -> ExitCode {
    let name = match Cli::parse().command {
        Command::Setup => "setup",
        Command::Update => "update",
        Command::Backup => "backup",
        Command::Restore => "restore",
    };
    eprintln!("rakazo-stack {name}: not implemented yet");
    ExitCode::FAILURE
}
