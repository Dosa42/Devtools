use crate::hashing::{self, Algorithm};
use crate::{Result, files, invalid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub path: String,
    pub hash: String,
    pub bytes: Option<u64>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub algorithm: Algorithm,
    pub entries: Vec<Record>,
}

pub fn capture(
    root: &Path,
    excluded: &Path,
    algorithm: Algorithm,
    derived: &[std::path::PathBuf],
) -> Result<Manifest> {
    if !root.is_dir() {
        return Err(invalid("Manifest root must be a directory"));
    }
    for path in derived {
        files::relative(
            path.to_str()
                .ok_or_else(|| invalid("Derived paths require UTF-8"))?,
        )?;
    }
    let mut entries = Vec::new();
    for entry in files::tree(root, false)? {
        if !entry.directory
            && entry.path != excluded
            && !derived
                .iter()
                .any(|path| std::path::Path::new(&entry.relative).starts_with(path))
        {
            let digest = hashing::file(&entry.path, algorithm)?;
            entries.push(Record {
                path: entry.relative,
                hash: digest.hash,
                bytes: Some(digest.bytes),
            });
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Manifest {
        schema: "devtool-proof.manifest/1".into(),
        algorithm,
        entries,
    })
}

pub fn validate(manifest: &Manifest) -> Result<()> {
    if manifest.schema != "devtool-proof.manifest/1" {
        return Err(invalid("Expected schema devtool-proof.manifest/1"));
    }
    let mut paths = BTreeSet::new();
    for entry in &manifest.entries {
        files::relative(&entry.path)?;
        hashing::valid(&entry.hash, manifest.algorithm)?;
        if !paths.insert(&entry.path) {
            return Err(invalid(format!("Duplicate manifest path: {}", entry.path)));
        }
    }
    Ok(())
}

pub fn sha(manifest: &Manifest) -> String {
    let mut text = String::new();
    for entry in &manifest.entries {
        let escaped = entry.path.contains(['\\', '\n', '\r']);
        let name = entry
            .path
            .replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        text.push_str(&format!(
            "{}{}  {}\n",
            if escaped { "\\" } else { "" },
            entry.hash,
            name
        ));
    }
    text
}

fn unescape(value: &str) -> Result<String> {
    let mut result = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            result.push(match characters.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('\\') => '\\',
                _ => return Err(invalid("Invalid checksum filename escape")),
            });
        } else {
            result.push(character);
        }
    }
    Ok(result)
}

pub fn read(path: &Path) -> Result<Manifest> {
    let text = fs::read_to_string(path)?;
    let manifest = if text.trim_start().starts_with('{') {
        serde_json::from_str(&text)?
    } else {
        let mut entries = Vec::new();
        let mut algorithm = None;
        for line in text.split_terminator('\n').filter(|line| !line.is_empty()) {
            let (escaped, line) = match line.strip_prefix('\\') {
                Some(line) => (true, line),
                None => (false, line),
            };
            let offset = line.find(' ').ok_or_else(|| {
                invalid("Expected HASH followed by two spaces and a relative path")
            })?;
            let hash = &line[..offset];
            let rest = &line[offset..];
            let name = rest
                .strip_prefix("  ")
                .or_else(|| rest.strip_prefix(" *"))
                .ok_or_else(|| invalid("Invalid checksum line separator"))?;
            let current = match hash.len() {
                64 => Algorithm::Sha256,
                128 => Algorithm::Sha512,
                _ => return Err(invalid("Expected SHA-256 or SHA-512")),
            };
            if algorithm.is_some_and(|previous| previous != current) {
                return Err(invalid("Mixed digest algorithms in a SHA manifest"));
            }
            algorithm = Some(current);
            entries.push(Record {
                path: if escaped {
                    unescape(name)?
                } else {
                    name.into()
                },
                hash: hash.to_ascii_lowercase(),
                bytes: None,
            });
        }
        Manifest {
            schema: "devtool-proof.manifest/1".into(),
            algorithm: algorithm.unwrap_or(Algorithm::Sha256),
            entries,
        }
    };
    validate(&manifest)?;
    Ok(manifest)
}

#[derive(Serialize)]
pub struct Verification {
    pub valid: bool,
    pub checked: usize,
    pub mismatches: Vec<String>,
    pub missing: Vec<String>,
}

pub fn verify(manifest: &Manifest, root: &Path) -> Result<Verification> {
    validate(manifest)?;
    let mut report = Verification {
        valid: true,
        checked: 0,
        mismatches: Vec::new(),
        missing: Vec::new(),
    };
    for entry in &manifest.entries {
        match hashing::file(&root.join(&entry.path), manifest.algorithm) {
            Ok(actual) => {
                report.checked += 1;
                if !actual.hash.eq_ignore_ascii_case(&entry.hash)
                    || entry.bytes.is_some_and(|bytes| bytes != actual.bytes)
                {
                    report.mismatches.push(entry.path.clone());
                }
            }
            Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                report.missing.push(entry.path.clone())
            }
            Err(error) => return Err(error),
        }
    }
    report.valid = report.mismatches.is_empty() && report.missing.is_empty();
    Ok(report)
}

pub fn compare(old: &Manifest, new: &Manifest) -> serde_json::Value {
    let old_paths: BTreeMap<_, _> = old
        .entries
        .iter()
        .map(|entry| (&entry.path, &entry.hash))
        .collect();
    let new_paths: BTreeMap<_, _> = new
        .entries
        .iter()
        .map(|entry| (&entry.path, &entry.hash))
        .collect();
    let added: Vec<_> = new_paths
        .keys()
        .filter(|path| !old_paths.contains_key(*path))
        .collect();
    let removed: Vec<_> = old_paths
        .keys()
        .filter(|path| !new_paths.contains_key(*path))
        .collect();
    let changed: Vec<_> = old_paths
        .iter()
        .filter(|(path, hash)| {
            new_paths.get(*path).is_some_and(|new_hash| {
                old.algorithm != new.algorithm || !hash.eq_ignore_ascii_case(new_hash)
            })
        })
        .map(|(path, _)| path)
        .collect();
    serde_json::json!({"added":added,"removed":removed,"changed":changed})
}
