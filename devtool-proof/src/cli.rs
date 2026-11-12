use crate::hashing::Algorithm;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[arg(long, global = true, help = "Compact JSON reports")]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Calculate a streaming SHA-256 or SHA-512 digest")]
    Hash {
        #[arg(required_unless_present = "stdin", conflicts_with = "stdin")]
        path: Option<PathBuf>,
        #[arg(long)]
        stdin: bool,
        #[arg(long, value_enum, default_value = "sha256")]
        algorithm: Algorithm,
    },
    #[command(about = "Create, verify and compare checksum manifests")]
    Manifest {
        #[command(subcommand)]
        command: ManifestCommand,
    },
    #[command(about = "Generate an Ed25519 key pair")]
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    #[command(about = "Sign a file digest with an Ed25519 private key")]
    Sign {
        file: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    #[command(about = "Verify a detached signature against a chosen public key")]
    VerifySignature {
        file: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        signature: Option<PathBuf>,
    },
    #[command(about = "Record, inspect and verify file provenance")]
    Provenance {
        #[command(subcommand)]
        command: ProvenanceCommand,
    },
    #[command(about = "Create, extract and test password-encrypted ZIP and 7z archives")]
    Archive {
        #[command(subcommand)]
        command: ArchiveCommand,
    },
    #[command(about = "Print a canonical document JSON schema")]
    Schema {
        #[arg(long, value_enum)]
        kind: SchemaKind,
    },
}

#[derive(Subcommand)]
pub enum ManifestCommand {
    #[command(about = "Write a sorted manifest with explicitly declared derived outputs")]
    Create {
        root: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "sha256")]
        algorithm: Algorithm,
        #[arg(long, value_enum, default_value = "sha")]
        format: ManifestFormat,
        #[arg(
            long = "derived",
            help = "Derived output path relative to the source root"
        )]
        derived: Vec<PathBuf>,
    },
    #[command(about = "Check every listed file; return 1 for mismatches or missing files")]
    Verify {
        file: PathBuf,
        #[arg(long)]
        root: Option<PathBuf>,
    },
    #[command(about = "Compare two manifests and report added, removed and changed paths")]
    Compare { old: PathBuf, new: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ManifestFormat {
    Sha,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum KeyType {
    Ed25519,
}

#[derive(Subcommand)]
pub enum KeyCommand {
    Generate {
        #[arg(long = "type", value_enum, default_value = "ed25519")]
        kind: KeyType,
        #[arg(long)]
        private: PathBuf,
        #[arg(long)]
        public: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum ProvenanceCommand {
    #[command(about = "Capture hashes and explicitly declared command/run/tool metadata")]
    Create {
        root: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long = "tool", value_name = "NAME=VERSION")]
        tools: Vec<String>,
        #[arg(long = "input")]
        inputs: Vec<PathBuf>,
        #[arg(long = "artifact")]
        outputs: Vec<PathBuf>,
        #[arg(
            long = "derived",
            help = "Derived output path relative to the source root"
        )]
        derived: Vec<PathBuf>,
    },
    Show {
        file: PathBuf,
    },
    Verify {
        file: PathBuf,
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ArchiveFormat {
    Zip,
    #[value(name = "7z")]
    SevenZ,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Cipher {
    Aes256,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct PasswordArgs {
    #[arg(long)]
    pub password_prompt: bool,
    #[arg(long)]
    pub password_stdin: bool,
    #[arg(long)]
    pub password_file: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum ArchiveCommand {
    #[command(about = "Write a new AES-256 archive from a directory or file")]
    Encrypt {
        source: PathBuf,
        #[arg(long, value_enum)]
        format: ArchiveFormat,
        #[arg(long, value_enum, default_value = "aes256")]
        cipher: Cipher,
        #[arg(long, help = "Encrypt 7z headers including names")]
        encrypt_names: bool,
        #[arg(long, default_value_t = 6, value_parser = clap::value_parser!(u8).range(0..=9))]
        level: u8,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        password: PasswordArgs,
    },
    #[command(about = "Decrypt and extract ZIP or 7z, detected from the archive header")]
    Decrypt {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        password: PasswordArgs,
    },
    #[command(about = "Read every archive entry to completion without extracting files")]
    Test {
        file: PathBuf,
        #[command(flatten)]
        password: PasswordArgs,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum SchemaKind {
    Manifest,
    Key,
    Signature,
    Provenance,
}
