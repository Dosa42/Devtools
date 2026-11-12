use crate::cli::{ArchiveFormat, PasswordArgs};
use crate::{Result, files, invalid};
use sevenz_rust2::{
    ArchiveEntry, ArchiveReader, ArchiveWriter, EncoderConfiguration, Password,
    encoder_options::{AesEncoderOptions, Lzma2Options},
};
use std::fs::{self, File};
use std::io::{self, BufRead, Read};
use std::path::Path;
use zeroize::Zeroizing;
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub fn password(args: PasswordArgs) -> Result<Zeroizing<String>> {
    let mut text = if args.password_stdin {
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        line
    } else if let Some(path) = args.password_file {
        fs::read_to_string(path)?
    } else {
        rpassword::prompt_password("Password: ")?
    };
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() {
        return Err(invalid("Password is empty"));
    }
    Ok(Zeroizing::new(text))
}

fn aes_options(password: &str) -> AesEncoderOptions {
    let mut options = AesEncoderOptions::new(Password::from(password));
    options.num_cycles_power = 19;
    options
}

pub fn encrypt(
    source: &Path,
    output: &Path,
    format: ArchiveFormat,
    encrypt_names: bool,
    level: u8,
    password: &str,
) -> Result<serde_json::Value> {
    if matches!(format, ArchiveFormat::Zip) && encrypt_names {
        return Err(invalid("--encrypt-names is a 7z option"));
    }
    let entries = files::tree(source, true)?;
    let output = files::output(output)?;
    let parent = output.parent().unwrap();
    let mut staging = tempfile::NamedTempFile::new_in(parent)?;
    match format {
        ArchiveFormat::Zip => {
            let mut writer = ZipWriter::new(staging.as_file_mut());
            for entry in &entries {
                if entry.directory {
                    writer
                        .add_directory(format!("{}/", entry.relative), SimpleFileOptions::default())
                        .map_err(archive_error)?;
                } else {
                    let options = SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Deflated)
                        .compression_level(Some(i64::from(level)))
                        .with_aes_encryption(AesMode::Aes256, password);
                    writer
                        .start_file(&entry.relative, options)
                        .map_err(archive_error)?;
                    io::copy(&mut File::open(&entry.path)?, &mut writer)?;
                }
            }
            writer.finish().map_err(archive_error)?;
        }
        ArchiveFormat::SevenZ => {
            let mut writer = ArchiveWriter::new(staging.as_file_mut()).map_err(archive_error)?;
            writer.set_encrypt_header(encrypt_names);
            for entry in &entries {
                writer.set_content_methods(vec![
                    aes_options(password).into(),
                    EncoderConfiguration::from(Lzma2Options::from_level(u32::from(level))),
                ]);
                let header = ArchiveEntry::from_path(&entry.path, entry.relative.clone());
                let source = if entry.directory {
                    None
                } else {
                    Some(File::open(&entry.path)?)
                };
                writer
                    .push_archive_entry(header, source)
                    .map_err(archive_error)?;
            }
            writer.set_content_methods(vec![
                aes_options(password).into(),
                EncoderConfiguration::from(Lzma2Options::from_level(u32::from(level))),
            ]);
            writer.finish()?;
        }
    }
    staging.as_file().sync_all()?;
    let bytes = staging.as_file().metadata()?.len();
    staging.persist(&output).map_err(|error| error.error)?;
    Ok(
        serde_json::json!({"output":output,"format":match format { ArchiveFormat::Zip => "zip", ArchiveFormat::SevenZ => "7z" },"cipher":"aes256","encrypt_names":encrypt_names,"entries":entries.len(),"bytes":bytes}),
    )
}

fn archive_error(error: impl std::fmt::Display) -> crate::Error {
    invalid(error.to_string())
}

fn detect(path: &Path) -> Result<ArchiveFormat> {
    let mut magic = [0; 6];
    File::open(path)?.read_exact(&mut magic)?;
    if magic == [0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c] {
        Ok(ArchiveFormat::SevenZ)
    } else if magic.starts_with(b"PK") {
        Ok(ArchiveFormat::Zip)
    } else {
        Err(invalid("Expected a ZIP or 7z archive"))
    }
}

pub fn read(file: &Path, output: Option<&Path>, password: &str) -> Result<serde_json::Value> {
    let format = detect(file)?;
    let mut entries = 0;
    let mut bytes = 0;
    match format {
        ArchiveFormat::Zip => {
            let mut archive = ZipArchive::new(File::open(file)?).map_err(archive_error)?;
            for index in 0..archive.len() {
                let mut entry = archive
                    .by_index_decrypt(index, password.as_bytes())
                    .map_err(archive_error)?;
                let relative = entry
                    .enclosed_name()
                    .ok_or_else(|| invalid("Invalid archive entry path"))?;
                if let Some(output) = output {
                    let path = output.join(relative);
                    if entry.is_dir() {
                        fs::create_dir_all(path)?;
                    } else {
                        fs::create_dir_all(path.parent().unwrap())?;
                        bytes += io::copy(&mut entry, &mut File::create(path)?)?;
                    }
                } else {
                    bytes += io::copy(&mut entry, &mut io::sink())?;
                }
                entries += 1;
            }
        }
        ArchiveFormat::SevenZ => {
            let mut archive = ArchiveReader::new(File::open(file)?, Password::from(password))
                .map_err(archive_error)?;
            archive
                .for_each_entries(|entry, reader| {
                    if let Some(output) = output {
                        files::relative(entry.name())
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        let path = output.join(entry.name());
                        if entry.is_directory() {
                            fs::create_dir_all(path)?;
                        } else {
                            fs::create_dir_all(path.parent().unwrap())?;
                            bytes += io::copy(reader, &mut File::create(path)?)?;
                        }
                    } else {
                        bytes += io::copy(reader, &mut io::sink())?;
                    }
                    entries += 1;
                    Ok(true)
                })
                .map_err(archive_error)?;
        }
    }
    Ok(
        serde_json::json!({"valid":true,"format":match format { ArchiveFormat::Zip => "zip", ArchiveFormat::SevenZ => "7z" },"entries":entries,"bytes":bytes,"output":output}),
    )
}
