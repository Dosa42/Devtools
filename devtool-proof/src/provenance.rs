use crate::hashing::{self, Algorithm};
use crate::manifest::{self, Manifest, Record};
use crate::{Result, files, invalid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub schema: String,
    pub created_unix_ms: u64,
    pub run: Option<String>,
    pub declared_command: Option<String>,
    pub tools: BTreeMap<String, String>,
    pub snapshot: Manifest,
    pub inputs: Vec<Record>,
    pub outputs: Vec<Record>,
}

pub struct Options {
    pub run: Option<String>,
    pub command: Option<String>,
    pub tools: Vec<String>,
    pub inputs: Vec<PathBuf>,
    pub outputs: Vec<PathBuf>,
    pub derived: Vec<PathBuf>,
}

fn records(root: &Path, paths: Vec<PathBuf>) -> Result<Vec<Record>> {
    let mut records = Vec::new();
    for path in paths {
        let name = path
            .to_str()
            .ok_or_else(|| invalid("Artifact paths require UTF-8"))?;
        files::relative(name)?;
        let digest = hashing::file(&root.join(&path), Algorithm::Sha256)?;
        records.push(Record {
            path: name.into(),
            hash: digest.hash,
            bytes: Some(digest.bytes),
        });
    }
    records.sort_by(|left, right| left.path.cmp(&right.path));
    manifest::validate(&Manifest {
        schema: "devtool-proof.manifest/1".into(),
        algorithm: Algorithm::Sha256,
        entries: records.clone(),
    })?;
    Ok(records)
}

pub fn create(root: &Path, output: &Path, options: Options) -> Result<Provenance> {
    let mut tools = BTreeMap::from([("devtool-proof".into(), env!("CARGO_PKG_VERSION").into())]);
    for tool in options.tools {
        let (name, version) = tool
            .split_once('=')
            .ok_or_else(|| invalid("--tool expects NAME=VERSION"))?;
        if name.is_empty() || version.is_empty() || name == "devtool-proof" {
            return Err(invalid(
                "Tool requires a name/version; devtool-proof records its own version",
            ));
        }
        tools.insert(name.into(), version.into());
    }
    let provenance = Provenance {
        schema: "devtool-proof.provenance/1".into(),
        created_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| invalid(error.to_string()))?
            .as_millis()
            .try_into()
            .map_err(|_| invalid("Timestamp overflow"))?,
        run: options.run,
        declared_command: options.command,
        tools,
        snapshot: manifest::capture(
            root,
            &files::output(output)?,
            Algorithm::Sha256,
            &options.derived,
        )?,
        inputs: records(root, options.inputs)?,
        outputs: records(root, options.outputs)?,
    };
    fs::write(output, serde_json::to_vec_pretty(&provenance)?)?;
    Ok(provenance)
}

pub fn read(file: &Path) -> Result<Provenance> {
    let provenance: Provenance = serde_json::from_slice(&fs::read(file)?)?;
    if provenance.schema != "devtool-proof.provenance/1" {
        return Err(invalid("Expected schema devtool-proof.provenance/1"));
    }
    manifest::validate(&provenance.snapshot)?;
    for entries in [&provenance.inputs, &provenance.outputs] {
        manifest::validate(&Manifest {
            schema: "devtool-proof.manifest/1".into(),
            algorithm: Algorithm::Sha256,
            entries: entries.clone(),
        })?;
    }
    Ok(provenance)
}

pub fn verify(provenance: Provenance, root: &Path) -> Result<serde_json::Value> {
    let snapshot = manifest::verify(&provenance.snapshot, root)?;
    let inputs = manifest::verify(
        &Manifest {
            schema: "devtool-proof.manifest/1".into(),
            algorithm: Algorithm::Sha256,
            entries: provenance.inputs,
        },
        root,
    )?;
    let outputs = manifest::verify(
        &Manifest {
            schema: "devtool-proof.manifest/1".into(),
            algorithm: Algorithm::Sha256,
            entries: provenance.outputs,
        },
        root,
    )?;
    Ok(
        serde_json::json!({"valid":snapshot.valid&&inputs.valid&&outputs.valid,"snapshot":snapshot,"inputs":inputs,"outputs":outputs}),
    )
}
