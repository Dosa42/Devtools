use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "Directory containing saved profile JSON files"
    )]
    pub profiles_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Create, inspect and configure saved profiles")]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    #[command(about = "Add, inspect and remove ordered automation steps")]
    Step {
        #[command(subcommand)]
        command: StepCommand,
    },
    #[command(about = "Execute a profile in order; return a failed step's exit status")]
    Run {
        name: String,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        repeat: Option<u64>,
        #[arg(long)]
        interval_ms: Option<u64>,
    },
    #[command(about = "Configure and start saved timers")]
    Timer {
        #[command(subcommand)]
        command: TimerCommand,
    },
    #[command(about = "Configure and start filesystem watchers")]
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
    #[command(about = "Create a fake Linux directory tree and three automation profiles")]
    Lab {
        #[command(subcommand)]
        command: LabCommand,
    },
    #[command(about = "Manage mode-matcher-action rules in our own rule language")]
    Rule {
        #[command(subcommand)]
        command: crate::rules::RuleCommand,
    },
    #[command(about = "Execute a command with before, after and error rule events")]
    Exec {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    #[command(about = "Print the profile JSON schema")]
    Schema,
}

#[derive(Args)]
pub struct Settings {
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub repeat: Option<u64>,
    #[arg(long)]
    pub interval_ms: Option<u64>,
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,
    #[arg(long, value_name = "KEY")]
    pub unset_env: Vec<String>,
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    Create {
        name: String,
        #[command(flatten)]
        settings: Settings,
    },
    Set {
        name: String,
        #[command(flatten)]
        settings: Settings,
    },
    List,
    Show {
        name: String,
    },
    Export {
        name: String,
        #[arg(short, long)]
        output: PathBuf,
    },
    Import {
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum StepCommand {
    Add {
        profile: String,
        #[arg(long)]
        name: String,
        #[arg(long, conflicts_with = "command", required_unless_present = "command")]
        program: Option<String>,
        #[arg(long = "arg", allow_hyphen_values = true, requires = "program")]
        args: Vec<String>,
        #[arg(
            long,
            conflicts_with = "program",
            help = "Bash text executed with bash -c"
        )]
        command: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
        #[arg(long, default_value_t = 0)]
        delay_ms: u64,
    },
    List {
        profile: String,
    },
    Remove {
        profile: String,
        #[arg(long, value_parser = clap::value_parser!(usize))]
        index: usize,
    },
    Clear {
        profile: String,
    },
}

#[derive(Subcommand)]
pub enum TimerCommand {
    Set {
        profile: String,
        #[arg(long, default_value_t = 0)]
        after_ms: u64,
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u64).range(1..))]
        interval_ms: u64,
        #[arg(
            long,
            default_value_t = 1,
            help = "Number of activations; 0 runs until interrupted"
        )]
        runs: u64,
    },
    Start {
        profile: String,
        #[arg(long)]
        after_ms: Option<u64>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        interval_ms: Option<u64>,
        #[arg(long)]
        runs: Option<u64>,
    },
    Show {
        profile: String,
    },
    Clear {
        profile: String,
    },
}

#[derive(Subcommand)]
pub enum WatchCommand {
    Set {
        profile: String,
        #[arg(long = "path", required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        recursive: bool,
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(1..))]
        poll_ms: u64,
        #[arg(long, default_value_t = 300)]
        debounce_ms: u64,
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        initial: bool,
        #[arg(
            long,
            default_value_t = 0,
            help = "Number of activations; 0 watches until interrupted"
        )]
        runs: u64,
    },
    Start {
        profile: String,
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Set)]
        recursive: Option<bool>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        poll_ms: Option<u64>,
        #[arg(long)]
        debounce_ms: Option<u64>,
        #[arg(long, action = clap::ArgAction::Set)]
        initial: Option<bool>,
        #[arg(long)]
        runs: Option<u64>,
    },
    Show {
        profile: String,
    },
    Clear {
        profile: String,
    },
}

#[derive(Subcommand)]
pub enum LabCommand {
    Init {
        #[arg(long)]
        path: PathBuf,
    },
}
