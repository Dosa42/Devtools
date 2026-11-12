mod archive;
pub mod cli;
mod files;
pub mod hashing;
mod manifest;
mod provenance;
mod signature;

use cli::{
    ArchiveCommand, Command, KeyCommand, ManifestCommand, ManifestFormat, ProvenanceCommand,
    SchemaKind,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn emit(value: &impl Serialize, compact: bool) -> Result<()> {
    println!(
        "{}",
        if compact {
            serde_json::to_string(value)?
        } else {
            serde_json::to_string_pretty(value)?
        }
    );
    Ok(())
}

fn default_root(file: &Path, root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| {
        file.parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_owned()
    })
}

fn sig_path(file: &Path, explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| {
        let mut name = file.as_os_str().to_owned();
        name.push(".sig");
        PathBuf::from(name)
    })
}

pub fn execute(cli: cli::Cli) -> Result<u8> {
    match cli.command {
        Command::Hash {
            path, algorithm, ..
        } => {
            let digest = match path {
                Some(path) => hashing::file(&path, algorithm)?,
                None => hashing::reader(std::io::stdin().lock(), algorithm)?,
            };
            emit(&digest, cli.json)?;
        }
        Command::Manifest { command } => match command {
            ManifestCommand::Create {
                root,
                output,
                algorithm,
                format,
                derived,
            } => {
                let output = files::output(&output.unwrap_or_else(|| {
                    root.join(match format {
                        ManifestFormat::Sha => "checksums.sha",
                        ManifestFormat::Json => "manifest.json",
                    })
                }))?;
                let manifest = manifest::capture(&root, &output, algorithm, &derived)?;
                let data = match format {
                    ManifestFormat::Sha => manifest::sha(&manifest),
                    ManifestFormat::Json => serde_json::to_string_pretty(&manifest)? + "\n",
                };
                fs::write(&output, data)?;
                emit(
                    &serde_json::json!({"output":output,"algorithm":algorithm,"entries":manifest.entries.len()}),
                    cli.json,
                )?;
            }
            ManifestCommand::Verify { file, root } => {
                let report = manifest::verify(&manifest::read(&file)?, &default_root(&file, root))?;
                let code = u8::from(!report.valid);
                emit(&report, cli.json)?;
                return Ok(code);
            }
            ManifestCommand::Compare { old, new } => emit(
                &manifest::compare(&manifest::read(&old)?, &manifest::read(&new)?),
                cli.json,
            )?,
        },
        Command::Key {
            command: KeyCommand::Generate {
                private, public, ..
            },
        } => {
            let fingerprint = signature::generate(&private, &public)?;
            emit(
                &serde_json::json!({"private":private,"public":public,"fingerprint_sha256":fingerprint}),
                cli.json,
            )?;
        }
        Command::Sign { file, key, output } => {
            let output = sig_path(&file, output);
            signature::sign(&file, &key, &output)?;
            emit(
                &serde_json::json!({"signature":output,"algorithm":"ed25519"}),
                cli.json,
            )?;
        }
        Command::VerifySignature {
            file,
            key,
            signature,
        } => {
            let valid = signature::verify(&file, &key, &sig_path(&file, signature))?;
            emit(&serde_json::json!({"valid":valid}), cli.json)?;
            return Ok(u8::from(!valid));
        }
        Command::Provenance { command } => match command {
            ProvenanceCommand::Create {
                root,
                output,
                run,
                command,
                tools,
                inputs,
                outputs,
                derived,
            } => {
                let record = provenance::create(
                    &root,
                    &output,
                    provenance::Options {
                        run,
                        command,
                        tools,
                        inputs,
                        outputs,
                        derived,
                    },
                )?;
                emit(
                    &serde_json::json!({"output":output,"files":record.snapshot.entries.len()}),
                    cli.json,
                )?;
            }
            ProvenanceCommand::Show { file } => emit(&provenance::read(&file)?, cli.json)?,
            ProvenanceCommand::Verify { file, root } => {
                let report =
                    provenance::verify(provenance::read(&file)?, &default_root(&file, root))?;
                let code = u8::from(report["valid"] != true);
                emit(&report, cli.json)?;
                return Ok(code);
            }
        },
        Command::Archive { command } => match command {
            ArchiveCommand::Encrypt {
                source,
                format,
                encrypt_names,
                level,
                output,
                password,
                ..
            } => emit(
                &archive::encrypt(
                    &source,
                    &output,
                    format,
                    encrypt_names,
                    level,
                    &archive::password(password)?,
                )?,
                cli.json,
            )?,
            ArchiveCommand::Decrypt {
                file,
                output,
                password,
            } => emit(
                &archive::read(&file, Some(&output), &archive::password(password)?)?,
                cli.json,
            )?,
            ArchiveCommand::Test { file, password } => emit(
                &archive::read(&file, None, &archive::password(password)?)?,
                cli.json,
            )?,
        },
        Command::Schema { kind } => {
            let schema = match kind {
                SchemaKind::Manifest => schemars::schema_for!(manifest::Manifest),
                SchemaKind::Key => schemars::schema_for!(signature::Key),
                SchemaKind::Signature => schemars::schema_for!(signature::Signature),
                SchemaKind::Provenance => schemars::schema_for!(provenance::Provenance),
            };
            emit(&schema, cli.json)?;
        }
    }
    Ok(0)
}
