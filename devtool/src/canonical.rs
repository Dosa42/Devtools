use crate::error::{Error, Result, io};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const TEMPLATE_JSON: &str = include_str!("../templates/canonical-tree.json");
pub const RUNNER_SH: &str = include_str!("../templates/canonical-tree.sh");
pub const DIRECTORIES: &[&str] = &["docs", "data", "logs", "output"];

pub struct EmbeddedFile {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

pub const FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        path: "document.pdf",
        bytes: include_bytes!("../templates/files/document.pdf"),
    },
    EmbeddedFile {
        path: "README.md",
        bytes: include_bytes!("../templates/files/README.md"),
    },
    EmbeddedFile {
        path: "manifest.json",
        bytes: include_bytes!("../templates/files/manifest.json"),
    },
    EmbeddedFile {
        path: "state.txt",
        bytes: include_bytes!("../templates/files/state.txt"),
    },
    EmbeddedFile {
        path: "checksums.sha",
        bytes: include_bytes!("../templates/files/checksums.sha"),
    },
    EmbeddedFile {
        path: "docs/README.md",
        bytes: include_bytes!("../templates/files/docs/README.md"),
    },
    EmbeddedFile {
        path: "docs/outline.txt",
        bytes: include_bytes!("../templates/files/docs/outline.txt"),
    },
    EmbeddedFile {
        path: "docs/metadata.json",
        bytes: include_bytes!("../templates/files/docs/metadata.json"),
    },
    EmbeddedFile {
        path: "data/README.md",
        bytes: include_bytes!("../templates/files/data/README.md"),
    },
    EmbeddedFile {
        path: "data/records.json",
        bytes: include_bytes!("../templates/files/data/records.json"),
    },
    EmbeddedFile {
        path: "data/input.txt",
        bytes: include_bytes!("../templates/files/data/input.txt"),
    },
    EmbeddedFile {
        path: "logs/README.md",
        bytes: include_bytes!("../templates/files/logs/README.md"),
    },
    EmbeddedFile {
        path: "logs/run.log",
        bytes: include_bytes!("../templates/files/logs/run.log"),
    },
    EmbeddedFile {
        path: "logs/events.jsonl",
        bytes: include_bytes!("../templates/files/logs/events.jsonl"),
    },
    EmbeddedFile {
        path: "output/result.txt",
        bytes: include_bytes!("../templates/files/output/result.txt"),
    },
    EmbeddedFile {
        path: "output/summary.md",
        bytes: include_bytes!("../templates/files/output/summary.md"),
    },
    EmbeddedFile {
        path: "output/index.json",
        bytes: include_bytes!("../templates/files/output/index.json"),
    },
];

#[derive(Debug, Serialize)]
pub struct TreeReport {
    pub path: PathBuf,
    pub template: &'static str,
    pub template_version: u32,
    pub runner: &'static str,
    pub directories: Vec<&'static str>,
    pub files: Vec<&'static str>,
}

pub fn apply(path: &Path) -> Result<TreeReport> {
    let path = std::env::current_dir()
        .map_err(|error| io("Read current directory", Path::new("."), error))?
        .join(path);
    let workspace = tempfile::tempdir()
        .map_err(|error| io("Create template workspace", Path::new("temp"), error))?;
    let payload = workspace.path().join("files");
    fs::create_dir(&payload).map_err(|error| io("Prepare template files", &payload, error))?;
    for directory in DIRECTORIES {
        let directory = payload.join(directory);
        fs::create_dir(&directory)
            .map_err(|error| io("Prepare template directory", &directory, error))?;
    }
    for file in FILES {
        let target = payload.join(file.path);
        fs::write(&target, file.bytes)
            .map_err(|error| io("Prepare embedded file", &target, error))?;
    }
    let runner = workspace.path().join("canonical-tree.sh");
    fs::write(&runner, RUNNER_SH)
        .map_err(|error| io("Prepare embedded shell runner", &runner, error))?;
    let output = Command::new("sh")
        .arg(&runner)
        .arg(&path)
        .arg(&payload)
        .output()
        .map_err(|error| io("Execute embedded shell runner", &runner, error))?;
    match output.status.success() {
        true => (),
        false => {
            return Err(Error::Message(format!(
                "Canonical tree shell runner failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    let report = TreeReport {
        path: fs::canonicalize(&path).map_err(|error| io("Read template target", &path, error))?,
        template: "canonical-tree",
        template_version: 2,
        runner: "embedded-shell",
        directories: DIRECTORIES.to_vec(),
        files: FILES.iter().map(|file| file.path).collect(),
    };
    workspace
        .close()
        .map_err(|error| io("Clean template workspace", Path::new("temp"), error))?;
    Ok(report)
}
