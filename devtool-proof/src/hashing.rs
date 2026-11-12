use crate::{Result, invalid};
use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn size(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }
}

#[derive(Serialize)]
pub struct DigestReport {
    pub algorithm: Algorithm,
    pub hash: String,
    pub bytes: u64,
}

pub fn reader(mut source: impl Read, algorithm: Algorithm) -> Result<DigestReport> {
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut bytes = 0;
    let mut buffer = [0; 65536];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        match algorithm {
            Algorithm::Sha256 => sha256.update(&buffer[..count]),
            Algorithm::Sha512 => sha512.update(&buffer[..count]),
        }
        bytes += count as u64;
    }
    let hash = match algorithm {
        Algorithm::Sha256 => hex::encode(sha256.finalize()),
        Algorithm::Sha512 => hex::encode(sha512.finalize()),
    };
    Ok(DigestReport {
        algorithm,
        hash,
        bytes,
    })
}

pub fn file(path: &Path, algorithm: Algorithm) -> Result<DigestReport> {
    reader(File::open(path)?, algorithm)
}

pub fn valid(hash: &str, algorithm: Algorithm) -> Result<()> {
    if hash.len() != algorithm.size() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Invalid SHA digest"));
    }
    Ok(())
}
