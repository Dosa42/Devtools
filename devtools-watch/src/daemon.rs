use crate::config::{self, Config, Paths, Target};
use crate::state::{self, State};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Process {
    pub pid: u32,
    pub start_ticks: u64,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn identity(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = text.rsplit_once(") ")?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    if fields.first() == Some(&"Z") {
        return None;
    }
    fields.get(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: libc::c_int,
        flavor: libc::c_int,
        arg: u64,
        buffer: *mut libc::c_void,
        size: libc::c_int,
    ) -> libc::c_int;
}

#[cfg(target_os = "macos")]
fn identity(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if read != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    if info.pbi_status == libc::SZOMB {
        return None;
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(info.pbi_start_tvusec)
}

pub fn running(paths: &Paths) -> Option<Process> {
    let record: Process =
        serde_json::from_slice(&fs::read(paths.runtime.join("process.json")).ok()?).ok()?;
    (identity(record.pid) == Some(record.start_ticks)).then_some(record)
}

fn lock(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?)
}

pub fn on(paths: &Paths) -> Result<serde_json::Value> {
    fs::create_dir_all(&paths.runtime)?;
    let control = lock(&paths.runtime.join("control.lock"))?;
    control.lock()?;
    if let Some(process) = running(paths) {
        return Ok(serde_json::json!({"ok":true,"running":true,"pid":process.pid}));
    }
    let config = config::load(paths)?;
    config::save(paths, &config)?;
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .open(paths.runtime.join("worker.log"))?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--on")
        .env("DEVTOOLS_WATCH_DAEMON", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(process) = running(paths) {
            return Ok(serde_json::json!({"ok":true,"running":true,"pid":process.pid}));
        }
        if child.try_wait()?.is_some() {
            return Err(Error::Invalid("daemon_start_failed"));
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(Error::Invalid("daemon_start_timeout"));
        }
        sleep(Duration::from_millis(20));
    }
}

pub fn off(paths: &Paths) -> Result<serde_json::Value> {
    fs::create_dir_all(&paths.runtime)?;
    let control = lock(&paths.runtime.join("control.lock"))?;
    control.lock()?;
    if let Some(process) = running(paths) {
        let result = unsafe { libc::kill(process.pid as libc::pid_t, libc::SIGTERM) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while running(paths).is_some() {
            if Instant::now() >= deadline {
                return Err(Error::Invalid("daemon_stop_timeout"));
            }
            sleep(Duration::from_millis(20));
        }
    }
    let owner = lock(&paths.runtime.join("worker.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if owner.try_lock().is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(Error::Invalid("daemon_stop_timeout"));
        }
        sleep(Duration::from_millis(20));
    }
    Ok(serde_json::json!({"ok":true,"running":false}))
}

extern "C" fn signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

fn root_name(name: &str) -> bool {
    let Some((stamp, run)) = name.split_once("_run-nr.") else {
        return false;
    };
    stamp.len() == 16
        && stamp.as_bytes()[8] == b'T'
        && stamp.as_bytes()[15] == b'Z'
        && stamp
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 8 || index == 15 || byte.is_ascii_digit())
        && run.len() >= 3
        && run.bytes().all(|byte| byte.is_ascii_digit())
}

fn targets(config: &Config) -> BTreeMap<PathBuf, Target> {
    let mut result = BTreeMap::new();
    for parent in &config.parents {
        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && root_name(&entry.file_name().to_string_lossy())
                {
                    result.insert(entry.path(), Target::new(entry.path()));
                }
            }
        }
    }
    for target in &config.targets {
        result.insert(target.root.clone(), target.clone());
    }
    result
}

fn stamp(path: &Path, ignored: &Path, hash: &mut impl Hasher) {
    path.hash(hash);
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            metadata.len().hash(hash);
            metadata.modified().ok().hash(hash);
            if metadata.is_dir()
                && let Ok(entries) = fs::read_dir(path)
            {
                let mut paths: Vec<_> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path != ignored)
                    .collect();
                paths.sort();
                for path in paths {
                    stamp(&path, ignored, hash);
                }
            }
        }
        Err(_) => 0u8.hash(hash),
    }
}

fn fingerprint(target: &Target) -> u64 {
    let mut hash = std::hash::DefaultHasher::new();
    stamp(&target.root, &target.root.join("state.txt"), &mut hash);
    for path in [
        &target.checksums,
        &target.checksum_signature,
        &target.provenance,
        &target.provenance_signature,
    ] {
        stamp(
            &target.path(path),
            &target.root.join("state.txt"),
            &mut hash,
        );
    }
    if let Some(path) = &target.public_key {
        stamp(
            &target.path(path),
            &target.root.join("state.txt"),
            &mut hash,
        );
    }
    hash.finish()
}

struct Cached {
    fingerprint: u64,
    verified: Instant,
    state: State,
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(&bytes)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn worker(paths: &Paths) -> Result<()> {
    if unsafe { libc::setsid() } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    fs::create_dir_all(&paths.runtime)?;
    let owner = lock(&paths.runtime.join("worker.lock"))?;
    owner
        .try_lock()
        .map_err(|_| Error::Invalid("daemon_already_running"))?;
    unsafe {
        libc::signal(libc::SIGTERM, signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, signal as *const () as libc::sighandler_t);
    }
    let process = Process {
        pid: std::process::id(),
        start_ticks: identity(std::process::id()).ok_or(Error::Invalid("process_identity"))?,
    };
    write_json(&paths.runtime.join("process.json"), &process)?;
    let result = observe(paths);
    let _ = fs::remove_file(paths.runtime.join("process.json"));
    result
}

fn observe(paths: &Paths) -> Result<()> {
    let mut config = config::load(paths)?;
    let mut encoded = serde_json::to_vec(&config)?;
    let mut cache = BTreeMap::<PathBuf, Cached>::new();
    while !STOP.load(Ordering::Relaxed) {
        match config::load(paths) {
            Ok(next) => {
                let bytes = serde_json::to_vec(&next)?;
                if bytes != encoded {
                    cache.clear();
                    config = next;
                    encoded = bytes;
                }
            }
            Err(_) => {
                write_json(
                    &paths.runtime.join("config-error.json"),
                    &serde_json::json!({"error":"config_invalid"}),
                )?;
            }
        }
        let targets = targets(&config);
        cache.retain(|root, _| targets.contains_key(root));
        let mut reports = Vec::new();
        for (root, target) in targets {
            if STOP.load(Ordering::Relaxed) {
                break;
            }
            if !state::canonical(&root) {
                cache.remove(&root);
                continue;
            }
            let fingerprint = fingerprint(&target);
            let refresh = cache.get(&root).is_none_or(|cached| {
                cached.fingerprint != fingerprint
                    || cached.verified.elapsed() >= Duration::from_millis(config.verify_interval_ms)
            });
            if refresh {
                match state::evaluate(&config, &target, &paths.runtime) {
                    Ok(state) => {
                        cache.insert(
                            root.clone(),
                            Cached {
                                fingerprint,
                                verified: Instant::now(),
                                state,
                            },
                        );
                    }
                    Err(_) => {
                        reports.push(serde_json::json!({"root":root,"error":"verification_io"}));
                        continue;
                    }
                }
            }
            if let Some(cached) = cache.get(&root) {
                match state::write(&root, &cached.state) {
                    Ok(()) => reports.push(serde_json::json!({"root":root,"state":cached.state})),
                    Err(_) => reports.push(serde_json::json!({"root":root,"error":"state_write"})),
                }
            }
        }
        write_json(&paths.runtime.join("last-scan.json"), &reports)?;
        let interval = Duration::from_millis(config.poll_ms);
        let deadline = Instant::now() + interval;
        while !STOP.load(Ordering::Relaxed) && Instant::now() < deadline {
            sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            );
        }
    }
    Ok(())
}
