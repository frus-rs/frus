// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Where an application keeps the files it must not lose: [`app_data_dir`] (milestone 779).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
#[cfg(android)]
use std::sync::OnceLock;

/// The application's private `files` folder on Android, handed over by `android_main`.
#[cfg(android)]
static ANDROID_FILES: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the application's `files` folder (`AndroidApp::internal_data_path`), once.
#[cfg(android)]
pub(crate) fn set_android_files(files: PathBuf) {
    let _ = ANDROID_FILES.set(files);
}

/// The folder where the application keeps its own data — what it saves and must find again at
/// the next start: documents, a database, settings. The folder is created if it is missing.
///
/// - Android: the application's private `files` folder (what `Context.getFilesDir` names),
///   which only the application can read and write. The system removes it with the
///   application, and "Clear storage" empties it.
/// - Windows: `%APPDATA%\<program>`.
/// - macOS and iOS: `~/Library/Application Support/<program>`.
/// - Linux and the other Unixes: `$XDG_DATA_HOME/<program>`, or `~/.local/share/<program>`.
///
/// `<program>` is the name of the running executable, without its extension.
///
/// `None` on the Web, which has no file system (keep data in the page's storage there), and
/// wherever the location cannot be known or the folder cannot be created.
///
/// ```no_run
/// let path = frus_shell::app_data_dir()
///     .expect("this platform keeps files")
///     .join("notes.txt");
/// std::fs::write(&path, "remember me").unwrap();
/// ```
pub fn app_data_dir() -> Option<PathBuf> {
    let dir = located()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(android)]
fn located() -> Option<PathBuf> {
    ANDROID_FILES.get().cloned()
}

#[cfg(any(desktop, ios))]
fn located() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    located_in(
        std::env::consts::OS,
        |name| std::env::var_os(name),
        exe.file_stem()?,
    )
}

#[cfg(web)]
fn located() -> Option<PathBuf> {
    None
}

/// The data folder of `program` on an `os` (`std::env::consts::OS`) whose environment's
/// variables are `var`. `None` when the variable it needs is unset or not a full path.
#[cfg_attr(not(any(desktop, ios)), allow(dead_code))]
pub(crate) fn located_in(
    os: &str,
    var: impl Fn(&str) -> Option<OsString>,
    program: &OsStr,
) -> Option<PathBuf> {
    let full = |name: &str| {
        var(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() || looks_absolute(path))
    };
    let base = match os {
        "windows" => full("APPDATA")?,
        "macos" | "ios" => full("HOME")?.join("Library").join("Application Support"),
        _ => full("XDG_DATA_HOME")
            .or_else(|| full("HOME").map(|home| home.join(".local").join("share")))?,
    };
    (!program.is_empty()).then(|| base.join(program))
}

/// Whether `path` is a full path on some platform, whichever one the tests run on.
fn looks_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    text.starts_with('/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    const APP: &str = "notes";

    #[test]
    fn each_desktop_has_its_own_data_folder() {
        assert_eq!(
            located_in(
                "windows",
                env(&[("APPDATA", r"C:\Users\me\AppData\Roaming")]),
                OsStr::new(APP)
            ),
            Some(PathBuf::from(r"C:\Users\me\AppData\Roaming").join(APP))
        );
        let mac = PathBuf::from("/Users/me")
            .join("Library")
            .join("Application Support")
            .join(APP);
        for os in ["macos", "ios"] {
            assert_eq!(
                located_in(os, env(&[("HOME", "/Users/me")]), OsStr::new(APP)),
                Some(mac.clone())
            );
        }
        assert_eq!(
            located_in(
                "linux",
                env(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/home/me")]),
                OsStr::new(APP)
            ),
            Some(PathBuf::from("/xdg").join(APP))
        );
        assert_eq!(
            located_in("linux", env(&[("HOME", "/home/me")]), OsStr::new(APP)),
            Some(
                PathBuf::from("/home/me")
                    .join(".local")
                    .join("share")
                    .join(APP)
            )
        );
    }

    #[test]
    fn no_full_path_no_folder() {
        assert_eq!(located_in("windows", env(&[]), OsStr::new(APP)), None);
        assert_eq!(
            located_in("linux", env(&[("XDG_DATA_HOME", "data")]), OsStr::new(APP)),
            None
        );
        assert_eq!(
            located_in("linux", env(&[("HOME", "/home/me")]), OsStr::new("")),
            None
        );
    }

    /// Not the temporary folder: on Android that is `/data/local/tmp`, which an application
    /// cannot write, and the demo's Save failed there without a word (milestone 779).
    #[cfg(any(desktop, ios))]
    #[test]
    fn the_folder_is_made_and_written_to() {
        let dir = app_data_dir().expect("a desktop keeps files");
        assert!(dir.is_dir());
        assert_ne!(dir, std::env::temp_dir());
        let probe = dir.join("frus-data-dir-probe.txt");
        std::fs::write(&probe, "x").unwrap();
        std::fs::remove_file(&probe).unwrap();
        // The test's own folder, named after its executable: not left behind.
        let _ = std::fs::remove_dir(&dir);
    }
}
