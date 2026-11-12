use crate::{
    config::ExistingFile,
    error::{Error, Result, io},
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, macros::format_description};

#[derive(Debug, Serialize)]
pub struct RootDirectory {
    pub path: PathBuf,
    pub name: String,
    pub timestamp: String,
    pub run: u64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectoryCounter {
    schema_version: u32,
    last_run: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_root: Option<PathBuf>,
}

fn state_directory() -> Result<PathBuf> {
    let base = env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or_else(|| {
            Error::Message(
                "HOME or XDG_STATE_HOME must be set to store the directory run counter".into(),
            )
        })?;
    Ok(base.join("devtool"))
}

pub fn create_root(path: &Path) -> Result<RootDirectory> {
    let parent =
        fs::canonicalize(path).map_err(|error| io("Read main directory parent", path, error))?;
    let state = state_directory()?;
    fs::create_dir_all(&state)
        .map_err(|error| io("Create run counter directory", &state, error))?;
    let lock_path = state.join("directory-counter.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| io("Open run counter lock", &lock_path, error))?;
    lock.lock()
        .map_err(|error| io("Lock run counter", &lock_path, error))?;
    let counter_path = state.join("directory-counter.json");
    let counter = match fs::read(&counter_path) {
        Ok(bytes) => {
            let counter: DirectoryCounter = serde_json::from_slice(&bytes).map_err(|error| {
                Error::Message(format!(
                    "Invalid directory run counter '{}': {error}",
                    counter_path.display()
                ))
            })?;
            if counter.schema_version != 1 {
                return Err(Error::Message(
                    "Unsupported directory run counter schema_version".into(),
                ));
            }
            counter
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DirectoryCounter {
            schema_version: 1,
            last_run: 0,
            last_root: None,
        },
        Err(error) => return Err(io("Read run counter", &counter_path, error)),
    };
    let run = counter
        .last_run
        .checked_add(1)
        .ok_or_else(|| Error::Message("Directory run counter exhausted".into()))?;
    let timestamp = OffsetDateTime::now_utc()
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .map_err(|error| Error::Message(format!("Format directory timestamp: {error}")))?;
    let name = format!("{timestamp}_run-nr.{run:03}");
    let path = parent.join(&name);
    fs::create_dir(&path).map_err(|error| io("Create main directory", &path, error))?;
    let next = DirectoryCounter {
        schema_version: 1,
        last_run: run,
        last_root: Some(path.clone()),
    };
    save_counter(&state, &counter_path, &next)?;
    Ok(RootDirectory {
        path,
        name,
        timestamp,
        run,
    })
}

fn save_counter(state: &Path, counter_path: &Path, counter: &DirectoryCounter) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(state)
        .map_err(|error| io("Create run counter", counter_path, error))?;
    temporary
        .write_all(&serde_json::to_vec(counter)?)
        .map_err(|error| io("Write run counter", counter_path, error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io("Flush run counter", counter_path, error))?;
    temporary
        .persist(counter_path)
        .map_err(|error| io("Save run counter", counter_path, error.error))?;
    Ok(())
}

pub fn latest_root() -> Result<PathBuf> {
    let state = state_directory()?;
    let counter_path = state.join("directory-counter.json");
    let missing = || {
        Error::Message(
            "No latest main directory recorded; run devtool -D first or use --path".into(),
        )
    };
    if !counter_path
        .try_exists()
        .map_err(|error| io("Read latest main directory", &counter_path, error))?
    {
        return Err(missing());
    }
    let lock_path = state.join("directory-counter.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| io("Open run counter lock", &lock_path, error))?;
    lock.lock_shared()
        .map_err(|error| io("Lock run counter", &lock_path, error))?;
    let bytes = fs::read(&counter_path)
        .map_err(|error| io("Read latest main directory", &counter_path, error))?;
    let counter: DirectoryCounter = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Message(format!("Invalid directory run counter: {error}")))?;
    if counter.schema_version != 1 {
        return Err(Error::Message(
            "Unsupported directory run counter schema_version".into(),
        ));
    }
    let root = counter.last_root.ok_or_else(missing)?;
    Ok(root)
}

pub fn parent(path: &Path, create: bool) -> Result<()> {
    if create && let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| io("Create parent directory", parent, error))?;
    }
    Ok(())
}

pub fn mkdir(path: &Path, parents: bool) -> Result<()> {
    let result = if parents {
        fs::create_dir_all(path)
    } else {
        fs::create_dir(path)
    };
    result.map_err(|error| io("Create directory", path, error))
}

pub fn create(path: &Path, input: &mut dyn Read, parents: bool, mode: ExistingFile) -> Result<u64> {
    parent(path, parents)?;
    let mut options = OpenOptions::new();
    options.write(true);
    match mode {
        ExistingFile::Error => {
            options.create_new(true);
        }
        ExistingFile::Overwrite => {
            options.create(true).truncate(true);
        }
        ExistingFile::Append => {
            options.create(true).append(true);
        }
    }
    let mut file = options
        .open(path)
        .map_err(|error| io("Create file", path, error))?;
    let count = std::io::copy(input, &mut file).map_err(|error| io("Write file", path, error))?;
    file.flush()
        .map_err(|error| io("Flush file", path, error))?;
    Ok(count)
}

pub fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    create(path, &mut bytes.as_slice(), true, ExistingFile::Overwrite)?;
    Ok(())
}
