use crate::config::{Config, Target};
use crate::daemon::STOP;
use crate::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::thread::sleep;
use std::time::Duration;

pub const TEMPLATE: &str = include_str!("../templates/state.txt");
pub const TREE: &str = include_str!("../templates/canonical-tree.json");

#[derive(Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct State {
    pub canonical_tree: bool,
    pub sha_present: bool,
    pub sha_scope: bool,
    pub sha_checksum: bool,
    pub sha_signature: bool,
    pub provenance_present: bool,
    pub provenance_scope: bool,
    pub provenance: bool,
    pub provenance_signature: bool,
}

impl State {
    pub fn text(&self) -> String {
        format!(
            "schema_version: 1\ncanonical_tree: {}\nsha_present: {}\nsha_scope: {}\nsha_checksum: {}\nsha_signature: {}\nprovenance_present: {}\nprovenance_scope: {}\nprovenance: {}\nprovenance_signature: {}\n",
            self.canonical_tree,
            self.sha_present,
            self.sha_scope,
            self.sha_checksum,
            self.sha_signature,
            self.provenance_present,
            self.provenance_scope,
            self.provenance,
            self.provenance_signature
        )
    }
}

pub fn canonical(root: &Path) -> bool {
    let marker = fs::read(root.join("manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    marker.is_some_and(|value| {
        value["schema_version"] == 1
            && value["template"] == "canonical-tree"
            && value["template_version"] == 2
            && value["derived_outputs"] == serde_json::json!(["state.txt"])
    })
}

fn complete(root: &Path) -> bool {
    let tree: Value = serde_json::from_str(TREE).unwrap();
    tree["directories"]
        .as_array()
        .unwrap()
        .iter()
        .all(|directory| root.join(directory.as_str().unwrap()).is_dir())
        && tree["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| root.join(entry["path"].as_str().unwrap()).is_file())
}

fn state_path(value: &str) -> bool {
    let path: PathBuf = Path::new(value)
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir))
        .collect();
    path == Path::new("state.txt")
}

fn checksum_scope(file: &Path) -> bool {
    let Ok(text) = fs::read_to_string(file) else {
        return false;
    };
    if text.trim_start().starts_with('{') {
        return serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| value["entries"].as_array().cloned())
            .is_some_and(|entries| {
                !entries.is_empty()
                    && entries
                        .iter()
                        .all(|entry| entry["path"].as_str().is_some_and(|path| !state_path(path)))
            });
    }
    let mut count = 0;
    for line in text.split_terminator('\n').filter(|line| !line.is_empty()) {
        let line = line.strip_prefix('\\').unwrap_or(line);
        let Some(offset) = line.find(' ') else {
            return false;
        };
        let Some(name) = line[offset..]
            .strip_prefix("  ")
            .or_else(|| line[offset..].strip_prefix(" *"))
        else {
            return false;
        };
        if state_path(name) {
            return false;
        }
        count += 1;
    }
    count > 0
}

fn provenance_scope(file: &Path) -> bool {
    let Some(document) = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    let Some(snapshot) = document["snapshot"]["entries"].as_array() else {
        return false;
    };
    if snapshot.is_empty() {
        return false;
    }
    for list in [
        snapshot,
        document["inputs"].as_array().unwrap_or(&Vec::new()),
        document["outputs"].as_array().unwrap_or(&Vec::new()),
    ] {
        if list
            .iter()
            .any(|entry| entry["path"].as_str().is_none_or(state_path))
        {
            return false;
        }
    }
    true
}

fn verify(
    config: &Config,
    target: &Target,
    runtime: &Path,
    args: &[&std::ffi::OsStr],
) -> Result<bool> {
    let mut output = tempfile::tempfile_in(runtime)?;
    let child = Command::new(&config.proof_executable)
        .arg("--json")
        .args(args)
        .current_dir(&target.root)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Ok(false);
    };
    let status = loop {
        if STOP.load(Ordering::Relaxed) {
            child.kill()?;
            child.wait()?;
            return Ok(false);
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => sleep(Duration::from_millis(20)),
        }
    };
    if !status.success() {
        return Ok(false);
    }
    output.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    output.read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice::<Value>(&bytes)
        .ok()
        .is_some_and(|value| value["valid"] == true))
}

pub fn evaluate(config: &Config, target: &Target, runtime: &Path) -> Result<State> {
    let checksum = target.path(&target.checksums);
    let record = target.path(&target.provenance);
    let mut state = State {
        canonical_tree: complete(&target.root),
        sha_present: fs::metadata(&checksum).is_ok_and(|metadata| metadata.is_file()),
        provenance_present: record.is_file(),
        ..Default::default()
    };
    state.sha_scope = state.sha_present && checksum_scope(&checksum);
    state.provenance_scope = state.provenance_present && provenance_scope(&record);
    if state.sha_scope {
        state.sha_checksum = verify(
            config,
            target,
            runtime,
            &[
                "manifest".as_ref(),
                "verify".as_ref(),
                checksum.as_os_str(),
                "--root".as_ref(),
                target.root.as_os_str(),
            ],
        )?;
    }
    if state.provenance_scope {
        state.provenance = verify(
            config,
            target,
            runtime,
            &[
                "provenance".as_ref(),
                "verify".as_ref(),
                record.as_os_str(),
                "--root".as_ref(),
                target.root.as_os_str(),
            ],
        )?;
    }
    if let Some(public) = &target.public_key {
        let public = target.path(public);
        let checksum_signature = target.path(&target.checksum_signature);
        let provenance_signature = target.path(&target.provenance_signature);
        if state.sha_present && checksum_signature.is_file() {
            state.sha_signature = verify(
                config,
                target,
                runtime,
                &[
                    "verify-signature".as_ref(),
                    checksum.as_os_str(),
                    "--key".as_ref(),
                    public.as_os_str(),
                    "--signature".as_ref(),
                    checksum_signature.as_os_str(),
                ],
            )?;
        }
        if state.provenance_present && provenance_signature.is_file() {
            state.provenance_signature = verify(
                config,
                target,
                runtime,
                &[
                    "verify-signature".as_ref(),
                    record.as_os_str(),
                    "--key".as_ref(),
                    public.as_os_str(),
                    "--signature".as_ref(),
                    provenance_signature.as_os_str(),
                ],
            )?;
        }
    }
    Ok(state)
}

pub fn write(root: &Path, state: &State) -> Result<()> {
    if STOP.load(Ordering::Relaxed) {
        return Ok(());
    }
    let path = root.join("state.txt");
    let text = state.text();
    match fs::read(&path) {
        Ok(bytes) if bytes == text.as_bytes() => Ok(()),
        _ => fs::write(path, text).map_err(Error::from),
    }
}
