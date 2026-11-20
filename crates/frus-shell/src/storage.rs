// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Where the application's **preferences** are kept on each platform (milestone 776), and the
//! application's own folder on Android, which the secure store keeps its ciphertext in.
//!
//! - The desktop: `prefs.json` in the application's data folder ([`crate::cache_dir::desktop_data`]),
//!   named after the program as its cache folder is.
//! - Android: `frus-prefs.json` in the application's `files` folder, which the system removes
//!   with the application, and which "Clear storage" empties.
//! - The web: one `localStorage` entry per key, `frus.pref.<key>`, holding its JSON.
//!
//! A file is never written in place: the whole map goes to a temporary file beside it, which is
//! flushed to the disk and then renamed over the old one. A crash, a full disk or a power cut
//! leaves either the old file or the new one, never half of one.

#[cfg(not(web))]
use std::io::Write;
#[cfg(not(web))]
use std::path::{Path, PathBuf};

#[cfg(not(web))]
use frus_widgets::__serde_json as json;
use frus_widgets::{PrefsChange, PrefsError, PrefsMap, PrefsStorage};

/// Sets up where the preferences are kept, unless something already did. Called by the entry
/// points ([`crate::main!`], `run`, `run_web`, `run_android`) before the application is made,
/// so that `App::default()` may already read its preferences.
#[doc(hidden)]
pub fn __install_storage() {
    if frus_widgets::__prefs_storage_installed() {
        return;
    }
    match platform_storage() {
        Ok(storage) => frus_widgets::__install_prefs_storage(storage),
        Err(why) => log::warn!("preferences: {why}; they will not be kept"),
    }
}

#[cfg(any(desktop, ios))]
fn platform_storage() -> Result<Box<dyn PrefsStorage>, String> {
    let exe = std::env::current_exe().map_err(|err| format!("no program path ({err})"))?;
    let program = exe.file_stem().ok_or("the program has no name")?;
    let dir = crate::cache_dir::desktop_data(
        std::env::consts::OS,
        |name| std::env::var_os(name),
        program,
    )
    .ok_or("no data folder (APPDATA / HOME unset)")?;
    Ok(Box::new(FileStorage::new(dir.join("prefs.json"))))
}

#[cfg(android)]
fn platform_storage() -> Result<Box<dyn PrefsStorage>, String> {
    let files = android::files_dir().ok_or("the application's files folder is not known yet")?;
    Ok(Box::new(FileStorage::new(files.join("frus-prefs.json"))))
}

#[cfg(web)]
fn platform_storage() -> Result<Box<dyn PrefsStorage>, String> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or_else(|| "the page has no localStorage".to_owned())?;
    Ok(Box::new(LocalStorage))
}

/// What Android hands the application when it starts: its `files` folder and the Java VM.
#[cfg(android)]
pub(crate) mod android {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use winit::platform::android::activity::AndroidApp;

    static FILES: OnceLock<PathBuf> = OnceLock::new();
    static VM: OnceLock<usize> = OnceLock::new();

    /// Records what `app` says, once per process.
    pub(crate) fn record(app: &AndroidApp) {
        if let Some(files) = app.internal_data_path() {
            let _ = FILES.set(files);
        }
        let _ = VM.set(app.vm_as_ptr() as usize);
    }

    /// Records the application's `files` folder, handed over by the job service when no
    /// activity runs (milestone 828).
    pub(crate) fn record_files(files: PathBuf) {
        let _ = FILES.set(files);
    }

    /// Records the process's Java VM, handed over by the job service.
    pub(crate) fn record_vm(vm: &jni::JavaVM) {
        let _ = VM.set(vm.get_java_vm_pointer() as usize);
    }

    /// The application's `files` folder (`Context.getFilesDir`).
    pub(crate) fn files_dir() -> Option<PathBuf> {
        FILES.get().cloned()
    }

    /// The process's Java VM.
    #[cfg_attr(not(feature = "secure-store"), allow(dead_code))]
    pub(crate) fn vm() -> Option<jni::JavaVM> {
        let ptr = *VM.get()?;
        unsafe { jni::JavaVM::from_raw(ptr as *mut jni::sys::JavaVM) }.ok()
    }
}

/// Called by [`crate::main!`] on Android before the application is made: records the
/// application's folder, sets up its preferences and its logs.
#[cfg(android)]
#[doc(hidden)]
pub fn __android_started(app: &winit::platform::android::activity::AndroidApp) {
    android::record(app);
    __install_storage();
    // Before the application is made, so that what it logs while it is made reaches logcat.
    crate::logging::init::<()>();
}

/// Holds `file` locked against other processes until it is closed.
#[cfg(not(any(web, android)))]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    file.lock()
}

/// Holds `file` locked against other processes until it is closed. Android's standard library
/// does not lock files (`lock()` answers "not supported"), yet a background job and the
/// window can both write the preferences; its C library has `flock`, which does.
#[cfg(android)]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    const LOCK_EX: i32 = 2;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    loop {
        // SAFETY: the descriptor is the open file's, alive for the whole call.
        if unsafe { flock(file.as_raw_fd(), LOCK_EX) } == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// Writes `bytes` to `path` whole or not at all: to a temporary file beside it, flushed to the
/// disk, then renamed over it. The folder is made if it is missing.
#[cfg(not(web))]
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{}.tmp", std::process::id()));
    let temporary = path.with_file_name(name);
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// The preferences in one JSON file.
///
/// Another process may keep the same file: a background job run by the system scheduler on
/// the desktop, while the window is open (milestone 828). A change is made under a lock on
/// `<file>.lock` that every process takes, after reading the file again if another process
/// wrote it since this one last did; a read looks at the file's stamp at most every
/// [`RECHECK`], and reads it again when it changed. On Android the job and the window share
/// one process, and the store, so neither costs anything there beyond the look.
#[cfg(not(web))]
pub(crate) struct FileStorage {
    path: PathBuf,
    seen: std::sync::Mutex<Seen>,
}

/// What a [`FileStorage`] knows of its file.
#[cfg(not(web))]
#[derive(Default)]
struct Seen {
    /// The file as this process last read or wrote it.
    stamp: Option<Stamp>,
    /// When a read last looked at the file; `None` makes the next look happen.
    looked: Option<std::time::Instant>,
}

/// How often a read looks at the file for another process's change.
#[cfg(not(web))]
pub(crate) const RECHECK: std::time::Duration = std::time::Duration::from_millis(250);

/// A file's version as seen from outside: every write renames a new file over the old one,
/// so its time, its length and (on Unix) its inode change with it.
#[cfg(not(web))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
    inode: u64,
}

#[cfg(not(web))]
fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let inode = 0;
    Some(Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
        inode,
    })
}

#[cfg(not(web))]
impl FileStorage {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            seen: std::sync::Mutex::new(Seen::default()),
        }
    }

    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Remembers the file as it is now: this process just read or wrote it.
    fn note_stamp(&self) {
        let stamp = stamp_of(&self.path);
        self.seen().stamp = stamp;
    }
}

#[cfg(not(web))]
impl PrefsStorage for FileStorage {
    fn load(&self) -> Result<PrefsMap, PrefsError> {
        // Noted before reading: a write that lands in between is seen at the next look.
        self.note_stamp();
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(PrefsMap::new()),
            Err(err) => {
                return Err(PrefsError::Storage(format!(
                    "{} could not be read: {err}",
                    self.path.display()
                )))
            }
        };
        match json::from_slice::<PrefsMap>(&bytes) {
            Ok(map) => Ok(map),
            Err(err) => {
                // Not ours to throw away: kept aside, where the next write cannot reach it.
                let aside = self.path.with_extension("json.bad");
                let _ = std::fs::rename(&self.path, &aside);
                Err(PrefsError::Storage(format!(
                    "{} is not a preferences file ({err}); moved to {}",
                    self.path.display(),
                    aside.display()
                )))
            }
        }
    }

    fn save(&self, all: &PrefsMap, _: PrefsChange<'_>) -> Result<(), PrefsError> {
        let bytes = json::to_vec_pretty(all).map_err(|err| PrefsError::Encode(err.to_string()))?;
        write_atomically(&self.path, &bytes).map_err(|err| {
            PrefsError::Storage(format!(
                "{} could not be written: {err}",
                self.path.display()
            ))
        })?;
        self.note_stamp();
        Ok(())
    }

    fn lock(&self) -> Result<Option<Box<dyn std::any::Any>>, PrefsError> {
        let mut name = self.path.file_name().unwrap_or_default().to_owned();
        name.push(".lock");
        let path = self.path.with_file_name(name);
        let refused = |err: std::io::Error| {
            PrefsError::Storage(format!("{} could not be locked: {err}", path.display()))
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(refused)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(refused)?;
        lock_exclusive(&file).map_err(refused)?;
        // The change about to be made looks at the file now, not at the last read's look.
        self.seen().looked = None;
        Ok(Some(Box::new(file)))
    }

    fn changed_elsewhere(&self) -> bool {
        let mut seen = self.seen();
        let now = std::time::Instant::now();
        if seen
            .looked
            .is_some_and(|at| now.duration_since(at) < RECHECK)
        {
            return false;
        }
        seen.looked = Some(now);
        stamp_of(&self.path) != seen.stamp
    }
}

/// The preferences in the page's `localStorage`, an entry per key.
#[cfg(web)]
struct LocalStorage;

#[cfg(web)]
const PREFIX: &str = "frus.pref.";

#[cfg(web)]
impl LocalStorage {
    fn storage() -> Result<web_sys::Storage, PrefsError> {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or(PrefsError::Unavailable("the page has no localStorage"))
    }
}

#[cfg(web)]
impl PrefsStorage for LocalStorage {
    fn load(&self) -> Result<PrefsMap, PrefsError> {
        let storage = Self::storage()?;
        let mut map = PrefsMap::new();
        let count = storage.length().unwrap_or(0);
        for index in 0..count {
            let Some(name) = storage.key(index).ok().flatten() else {
                continue;
            };
            let Some(key) = name.strip_prefix(PREFIX) else {
                continue;
            };
            if let Some(value) = storage
                .get_item(&name)
                .ok()
                .flatten()
                .and_then(|text| frus_widgets::__serde_json::from_str(&text).ok())
            {
                map.insert(key.to_owned(), value);
            }
        }
        Ok(map)
    }

    fn save(&self, all: &PrefsMap, change: PrefsChange<'_>) -> Result<(), PrefsError> {
        let storage = Self::storage()?;
        let refused = |err: wasm_bindgen::JsValue| {
            PrefsError::Storage(format!("localStorage refused it: {err:?}"))
        };
        match change {
            PrefsChange::Set(key, value) => storage
                .set_item(&format!("{PREFIX}{key}"), &value.to_string())
                .map_err(refused),
            PrefsChange::Remove(key) => storage
                .remove_item(&format!("{PREFIX}{key}"))
                .map_err(refused),
            PrefsChange::Clear => {
                let count = storage.length().unwrap_or(0);
                let ours: Vec<String> = (0..count)
                    .filter_map(|index| storage.key(index).ok().flatten())
                    .filter(|name| {
                        name.starts_with(PREFIX) && !all.contains_key(&name[PREFIX.len()..])
                    })
                    .collect();
                for name in ours {
                    storage.remove_item(&name).map_err(refused)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(all(test, not(web)))]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("frus-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The file round-trips, in a folder made for it, and no temporary file is left behind.
    #[test]
    fn a_file_storage_keeps_the_map() {
        let dir = scratch("round-trip");
        let path = dir.join("nested").join("prefs.json");
        let storage = FileStorage::new(path.clone());
        assert!(storage.load().unwrap().is_empty(), "no file yet: empty");
        let mut map = PrefsMap::new();
        map.insert("theme".into(), json::json!("dark"));
        map.insert("count".into(), json::json!(3));
        storage
            .save(&map, PrefsChange::Set("count", &json::json!(3)))
            .unwrap();
        assert_eq!(FileStorage::new(path.clone()).load().unwrap(), map);
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("prefs.json")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Two processes over one file (milestone 828): each sees the other's write, at once for
    /// a change (under the lock) and within a read's recheck; its own writes are not news.
    #[test]
    fn a_write_from_another_process_is_seen() {
        let dir = scratch("shared");
        let path = dir.join("prefs.json");
        let window = FileStorage::new(path.clone());
        let job = FileStorage::new(path.clone());
        let mut map = PrefsMap::new();
        map.insert("theme".into(), json::json!("dark"));
        window.save(&map, PrefsChange::Clear).unwrap();
        assert!(!window.changed_elsewhere(), "its own write");
        job.load().unwrap();
        map.insert("runs".into(), json::json!(1));
        // Another length: seen whatever the clock's resolution.
        job.save(&map, PrefsChange::Clear).unwrap();
        let held = window.lock().unwrap();
        assert!(held.is_some(), "a file is locked");
        assert!(
            window.changed_elsewhere(),
            "seen under the lock, without waiting"
        );
        drop(held);
        assert_eq!(window.load().unwrap(), map);
        assert!(!window.changed_elsewhere());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The lock keeps a second holder waiting until the first lets go.
    #[test]
    fn the_lock_is_exclusive() {
        let dir = scratch("exclusive");
        let path = dir.join("prefs.json");
        let one = FileStorage::new(path.clone());
        let held = one.lock().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let two = FileStorage::new(path);
            let _held = two.lock().unwrap();
            sender.send(()).unwrap();
        });
        assert!(
            receiver
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "taken while held"
        );
        drop(held);
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("taken once let go");
        waiter.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A file that is not ours is moved aside rather than overwritten by the next write.
    #[test]
    fn a_broken_file_is_moved_aside() {
        let dir = scratch("broken");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let storage = FileStorage::new(path.clone());
        assert!(matches!(storage.load(), Err(PrefsError::Storage(_))));
        assert_eq!(
            std::fs::read(dir.join("prefs.json.bad")).unwrap(),
            b"{ not json"
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The rename replaces an existing file, and a write that fails leaves the old one whole.
    #[test]
    fn an_atomic_write_replaces_or_leaves_whole() {
        let dir = scratch("atomic");
        let path = dir.join("f.json");
        write_atomically(&path, b"one").unwrap();
        write_atomically(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // A folder where the file should be: the rename fails, and nothing is left behind.
        let blocked = dir.join("blocked");
        std::fs::create_dir_all(blocked.join("inside")).unwrap();
        assert!(write_atomically(&blocked, b"x").is_err());
        assert!(blocked.join("inside").exists());
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|name| !name.ends_with(".tmp")),
            "{names:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
