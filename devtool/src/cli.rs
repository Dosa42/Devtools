use crate::config::{ArchiveFormat, ColorMode};
use clap::{Args, CommandFactory, Parser, Subcommand, error::ErrorKind};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "devtool",
    version,
    about = "Create files, directories and compressed archives",
    arg_required_else_help = true,
    group(clap::ArgGroup::new("tree_action").args(["directory", "canonical_tree"]))
)]
pub struct Cli {
    #[arg(
        short = 'D',
        long = "directory",
        conflicts_with = "canonical_tree",
        help = "Create a timestamped main directory with the next persistent run number"
    )]
    pub directory: bool,
    #[arg(
        long,
        help = "Run the embedded canonical JSON tree and shell runner; defaults to the latest main directory"
    )]
    pub canonical_tree: bool,
    #[arg(
        long,
        requires = "tree_action",
        value_name = "PATH",
        help = "Parent for -D, or target for --canonical-tree; default depends on the action"
    )]
    pub path: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Use this TOML configuration file"
    )]
    pub config: Option<PathBuf>,
    #[arg(long, global = true, help = "Print machine-readable JSON output")]
    pub json: bool,
    #[arg(long, global = true, value_enum, help = "Set output color behavior")]
    pub color: Option<ColorMode>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    pub fn validate(&self) -> std::result::Result<(), clap::Error> {
        if (self.directory || self.canonical_tree) && self.command.is_some() {
            return Err(Self::command().error(
                ErrorKind::ArgumentConflict,
                "-D and --canonical-tree cannot be combined with a subcommand",
            ));
        }
        if !self.directory && !self.canonical_tree && self.command.is_none() {
            return Err(Self::command().error(
                ErrorKind::MissingSubcommand,
                "Choose -D, --canonical-tree or a subcommand; use --help for details",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Create one or more directories")]
    Mkdir(Mkdir),
    #[command(about = "Create files and write their contents")]
    File {
        #[command(subcommand)]
        command: FileCommand,
    },
    #[command(about = "Compress a file or directory into 7z or tar.gz")]
    Compress(Compress),
    #[command(about = "View, edit and validate settings")]
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    #[command(about = "Export the CLI command structure as JSON")]
    Schema {
        #[arg(short, long, help = "Write JSON to this file instead of stdout")]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
pub struct Parents {
    #[arg(
        long,
        conflicts_with = "no_parents",
        help = "Create missing parent directories"
    )]
    pub parents: bool,
    #[arg(long, help = "Require parent directories to exist")]
    pub no_parents: bool,
}

impl Parents {
    pub fn resolve(&self, configured: bool) -> bool {
        if self.parents {
            true
        } else if self.no_parents {
            false
        } else {
            configured
        }
    }
}

#[derive(Debug, Args)]
pub struct Mkdir {
    #[arg(required = true, num_args = 1.., help = "Directories to create")]
    pub paths: Vec<PathBuf>,
    #[command(flatten)]
    pub parents: Parents,
}

#[derive(Debug, Subcommand)]
pub enum FileCommand {
    #[command(about = "Create a file, optionally writing text or stdin")]
    Create(CreateFile),
}

#[derive(Debug, Args)]
pub struct CreateFile {
    #[arg(help = "File to create")]
    pub path: PathBuf,
    #[arg(long, conflicts_with = "stdin", help = "Write this UTF-8 text")]
    pub content: Option<String>,
    #[arg(long, help = "Copy bytes from standard input")]
    pub stdin: bool,
    #[arg(
        long,
        conflicts_with = "append",
        help = "Replace existing file contents"
    )]
    pub overwrite: bool,
    #[arg(long, help = "Append contents if the file already exists")]
    pub append: bool,
    #[command(flatten)]
    pub parents: Parents,
}

#[derive(Debug, Args)]
pub struct Compress {
    #[arg(help = "Source file or directory")]
    pub source: PathBuf,
    #[arg(short, long, help = "Archive path; defaults to SOURCE.FORMAT")]
    pub output: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        help = "Archive format; defaults to the configured format"
    )]
    pub format: Option<ArchiveFormat>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=9), help = "Compression level: 0 (fastest) to 9 (strongest)")]
    pub level: Option<u8>,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    #[command(about = "Open the interactive configuration menu")]
    Menu,
    #[command(about = "Show stored settings, including defaults")]
    Show {
        #[arg(long, help = "Include environment/CLI overrides and value sources")]
        effective: bool,
    },
    #[command(about = "Read an effective setting by dotted key")]
    Get { key: String },
    #[command(about = "Persist one setting, for example compression.level 3")]
    Set { key: String, value: String },
    #[command(about = "Create a default configuration file")]
    Init {
        #[arg(long, help = "Replace the configuration file with defaults")]
        force: bool,
    },
    #[command(about = "Print the selected configuration path")]
    Path,
    #[command(about = "Validate the selected configuration file")]
    Validate,
    #[command(about = "Export the settings JSON Schema")]
    Schema {
        #[arg(short, long, help = "Write JSON to this file instead of stdout")]
        output: Option<PathBuf>,
    },
}
