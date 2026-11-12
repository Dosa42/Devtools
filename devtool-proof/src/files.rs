use crate::{Result, invalid};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct Entry {
    pub path: PathBuf,
    pub relative: String,
    pub directory: bool,
}

pub fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || !Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(invalid(format!("Expected a relative entry path: {path}")));
    }
    Ok(())
}

pub fn output(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(fs::canonicalize(parent)?.join(
        path.file_name()
            .ok_or_else(|| invalid("Output requires a filename"))?,
    ))
}

fn visit(
    path: &Path,
    name: String,
    parents: &mut BTreeSet<PathBuf>,
    entries: &mut Vec<Entry>,
) -> Result<()> {
    let metadata = fs::metadata(path)?;
    let directory = metadata.is_dir();
    if !directory && !metadata.is_file() {
        return Err(invalid(format!(
            "Unsupported filesystem entry: {}",
            path.display()
        )));
    }
    entries.push(Entry {
        path: path.to_owned(),
        relative: name.clone(),
        directory,
    });
    if directory {
        let identity = fs::canonicalize(path)?;
        if !parents.insert(identity.clone()) {
            return Err(invalid(format!("Directory cycle at {}", path.display())));
        }
        let mut children = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            let component = entry
                .file_name()
                .into_string()
                .map_err(|_| invalid("Entry names require UTF-8"))?;
            let child_name = if name.is_empty() {
                component
            } else {
                format!("{name}/{component}")
            };
            visit(&entry.path(), child_name, parents, entries)?;
        }
        parents.remove(&identity);
    }
    Ok(())
}

pub fn tree(path: &Path, include_root: bool) -> Result<Vec<Entry>> {
    let path = fs::canonicalize(path)?;
    let root = if include_root {
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("Source requires a named root"))?
            .to_owned()
    } else {
        String::new()
    };
    let mut entries = Vec::new();
    visit(&path, root, &mut BTreeSet::new(), &mut entries)?;
    Ok(entries)
}
