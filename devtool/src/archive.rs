use crate::{
    config::ArchiveFormat,
    error::{Error, Result, io},
};
use flate2::{Compression, write::GzEncoder};
use serde::Serialize;
use sevenz_rust2::{
    ArchiveEntry, ArchiveWriter, EncoderConfiguration, encoder_options::Lzma2Options,
};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct ArchiveReport {
    pub path: PathBuf,
    pub format: ArchiveFormat,
    pub level: u8,
    pub entries: usize,
    pub bytes: u64,
}

struct Entry {
    path: PathBuf,
    name: PathBuf,
    directory: bool,
}

fn collect(path: &Path, name: &Path, entries: &mut Vec<Entry>) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io("Read archive source", path, error))?;
    let directory = metadata.is_dir();
    entries.push(Entry {
        path: path.to_owned(),
        name: name.to_owned(),
        directory,
    });
    if directory {
        let mut children = fs::read_dir(path)
            .map_err(|error| io("Read directory", path, error))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| io("Read directory entry", path, error))?;
        children.sort_by_key(|child| child.file_name());
        for child in children {
            collect(&child.path(), &name.join(child.file_name()), entries)?;
        }
    }
    Ok(())
}

pub fn compress(
    source: &Path,
    output: &Path,
    format: ArchiveFormat,
    level: u8,
) -> Result<ArchiveReport> {
    let source =
        fs::canonicalize(source).map_err(|error| io("Read archive source", source, error))?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent =
        fs::canonicalize(parent).map_err(|error| io("Read output directory", parent, error))?;
    let output = parent.join(
        output
            .file_name()
            .ok_or_else(|| Error::Archive("Archive output requires a filename".into()))?,
    );
    let name = source
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("root"));
    let mut entries = Vec::new();
    collect(&source, Path::new(name), &mut entries)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&parent)
        .map_err(|error| io("Create archive", &output, error))?;
    match format {
        ArchiveFormat::TarGz => {
            let encoder =
                GzEncoder::new(temporary.as_file_mut(), Compression::new(u32::from(level)));
            let mut builder = tar::Builder::new(encoder);
            builder.follow_symlinks(false);
            for entry in &entries {
                builder
                    .append_path_with_name(&entry.path, &entry.name)
                    .map_err(|error| io("Archive entry", &entry.path, error))?;
            }
            builder
                .into_inner()
                .map_err(|error| io("Finish tar archive", &output, error))?
                .finish()
                .map_err(|error| io("Finish gzip stream", &output, error))?;
        }
        ArchiveFormat::SevenZ => {
            let mut writer = ArchiveWriter::new(temporary.as_file_mut())
                .map_err(|error| Error::Archive(error.to_string()))?;
            writer.set_content_methods(vec![EncoderConfiguration::from(Lzma2Options::from_level(
                u32::from(level),
            ))]);
            for entry in &entries {
                let name = entry
                    .name
                    .components()
                    .map(|part| {
                        part.as_os_str().to_str().ok_or_else(|| {
                            Error::Archive("7z entry path must contain UTF-8".into())
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join("/");
                let header = ArchiveEntry::from_path(&entry.path, name);
                let reader: Option<Box<dyn Read>> = if entry.directory {
                    None
                } else {
                    Some(Box::new(File::open(&entry.path).map_err(|error| {
                        io("Read archive entry", &entry.path, error)
                    })?))
                };
                writer
                    .push_archive_entry(header, reader)
                    .map_err(|error| Error::Archive(error.to_string()))?;
            }
            writer
                .finish()
                .map_err(|error| io("Finish 7z archive", &output, error))?;
        }
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io("Flush archive", &output, error))?;
    let bytes = temporary
        .as_file()
        .metadata()
        .map_err(|error| io("Inspect archive", &output, error))?
        .len();
    temporary
        .persist(&output)
        .map_err(|error| io("Save archive", &output, error.error))?;
    Ok(ArchiveReport {
        path: output,
        format,
        level,
        entries: entries.len(),
        bytes,
    })
}
