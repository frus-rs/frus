// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Where an application keeps what it can lose: the renderer's compiled pipelines (milestones
//! 685 and 697, #217). Plain functions of the environment, so every platform's answer is tested
//! on any of them.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The folder kept for the pipelines of `program` on a desktop `os` (`std::env::consts::OS`),
/// from the environment's variables `var`:
///
/// - Linux and the other Unixes: `$XDG_CACHE_HOME/frus/<program>`, or `~/.cache/frus/<program>`;
/// - macOS: `~/Library/Caches/frus/<program>`;
/// - Windows: `%LOCALAPPDATA%\frus\<program>`.
///
/// `None` when the variable it needs is unset or not a full path: then nothing is kept.
#[cfg_attr(not(desktop), allow(dead_code))]
pub(crate) fn desktop(
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
        "windows" => full("LOCALAPPDATA")?,
        "macos" => full("HOME")?.join("Library").join("Caches"),
        _ => full("XDG_CACHE_HOME").or_else(|| full("HOME").map(|home| home.join(".cache")))?,
    };
    (!program.is_empty()).then(|| base.join("frus").join(program))
}

/// The folder kept for what `program` must not lose — its preferences (milestone 776) — on a
/// desktop `os`, from the environment's variables `var`:
///
/// - Linux and the other Unixes: `$XDG_DATA_HOME/frus/<program>`, or
///   `~/.local/share/frus/<program>`;
/// - macOS (and iOS, inside the application's sandbox):
///   `~/Library/Application Support/frus/<program>`;
/// - Windows: `%APPDATA%\frus\<program>` (the roaming profile, which follows the person).
///
/// `None` when the variable it needs is unset or not a full path.
#[cfg_attr(not(any(desktop, ios)), allow(dead_code))]
pub(crate) fn desktop_data(
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
    (!program.is_empty()).then(|| base.join("frus").join(program))
}

/// Whether `path` is a full path on some platform: `C:\…` is one on Windows even when the tests
/// run elsewhere, and `/…` is one on Unix even when they run on Windows.
fn looks_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    text.starts_with('/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// The folder kept for the pipelines on Android, from the application's `files` folder
/// (`AndroidApp::internal_data_path`): its `cache` sibling — what `Context.getCacheDir` names —,
/// which the system may empty when it runs short and the user empties with "Clear cache". A
/// `files` folder laid out otherwise keeps them inside it.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn android(files: &Path) -> PathBuf {
    match files.parent() {
        Some(data) if files.file_name() == Some(OsStr::new("files")) => {
            data.join("cache").join("frus-pipelines")
        }
        _ => files.join("frus-pipelines"),
    }
}

/// The application's own cache folder (milestone 795): where what it can lose and fetch
/// again is kept — the image cache's files. Not created here.
///
/// - Android: the `cache` folder beside `files` (`Context.getCacheDir`), which the system may
///   empty when it runs short and the person empties with "Clear cache";
/// - the desktops and iOS: [`desktop`]'s folder for the running program;
/// - the Web: `None`, the browser keeping its own cache.
#[allow(dead_code)]
pub(crate) fn app_cache_dir() -> Option<PathBuf> {
    #[cfg(android)]
    {
        let files = crate::data_dir::android_files()?;
        Some(match files.parent() {
            Some(data) if files.file_name() == Some(OsStr::new("files")) => data.join("cache"),
            _ => files.join("cache"),
        })
    }
    #[cfg(any(desktop, ios))]
    {
        let exe = std::env::current_exe().ok()?;
        let os = if cfg!(ios) {
            "macos"
        } else {
            std::env::consts::OS
        };
        desktop(os, |name| std::env::var_os(name), exe.file_stem()?)
    }
    #[cfg(web)]
    {
        None
    }
}

/// Where milestone 685 kept them on Android, before they moved to the cache folder: removed at
/// the next start, since nothing reads it any more.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn android_before_697(files: &Path) -> PathBuf {
    files.join("frus-pipelines")
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

    const APP: &str = "counter";

    #[test]
    fn linux_uses_xdg_cache_home_then_home() {
        let both = env(&[("XDG_CACHE_HOME", "/xdg"), ("HOME", "/home/me")]);
        assert_eq!(
            desktop("linux", both, OsStr::new(APP)),
            Some(PathBuf::from("/xdg").join("frus").join(APP))
        );
        let home = env(&[("HOME", "/home/me")]);
        assert_eq!(
            desktop("linux", home, OsStr::new(APP)),
            Some(
                PathBuf::from("/home/me")
                    .join(".cache")
                    .join("frus")
                    .join(APP)
            )
        );
        // A relative XDG_CACHE_HOME is to be ignored (the XDG rules).
        let relative = env(&[("XDG_CACHE_HOME", "cache"), ("HOME", "/home/me")]);
        assert_eq!(
            desktop("freebsd", relative, OsStr::new(APP)),
            Some(
                PathBuf::from("/home/me")
                    .join(".cache")
                    .join("frus")
                    .join(APP)
            )
        );
    }

    #[test]
    fn macos_uses_library_caches() {
        let home = env(&[("HOME", "/Users/me"), ("XDG_CACHE_HOME", "/xdg")]);
        assert_eq!(
            desktop("macos", home, OsStr::new(APP)),
            Some(
                PathBuf::from("/Users/me")
                    .join("Library")
                    .join("Caches")
                    .join("frus")
                    .join(APP)
            )
        );
    }

    #[test]
    fn windows_uses_local_app_data() {
        let local = env(&[
            ("LOCALAPPDATA", r"C:\Users\me\AppData\Local"),
            ("HOME", "/home/me"),
        ]);
        assert_eq!(
            desktop("windows", local, OsStr::new(APP)),
            Some(
                PathBuf::from(r"C:\Users\me\AppData\Local")
                    .join("frus")
                    .join(APP)
            )
        );
    }

    /// Without the variable the platform needs, or without a program's name, nothing is kept —
    /// never a folder relative to wherever the program was started.
    #[test]
    fn nothing_is_kept_without_a_full_path() {
        assert_eq!(desktop("linux", env(&[]), OsStr::new(APP)), None);
        assert_eq!(desktop("macos", env(&[]), OsStr::new(APP)), None);
        assert_eq!(
            desktop("windows", env(&[("HOME", "/h")]), OsStr::new(APP)),
            None
        );
        assert_eq!(
            desktop("windows", env(&[("LOCALAPPDATA", "")]), OsStr::new(APP)),
            None
        );
        assert_eq!(
            desktop("linux", env(&[("HOME", "home")]), OsStr::new(APP)),
            None
        );
        assert_eq!(
            desktop("linux", env(&[("HOME", "/home/me")]), OsStr::new("")),
            None
        );
    }

    /// The data folder: kept apart from the cache, which the system or the person may empty.
    #[test]
    fn data_lives_where_each_desktop_keeps_it() {
        let linux = env(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/home/me")]);
        assert_eq!(
            desktop_data("linux", linux, OsStr::new(APP)),
            Some(PathBuf::from("/xdg").join("frus").join(APP))
        );
        let home = env(&[("HOME", "/home/me"), ("XDG_DATA_HOME", "rel")]);
        assert_eq!(
            desktop_data("linux", home, OsStr::new(APP)),
            Some(
                PathBuf::from("/home/me")
                    .join(".local")
                    .join("share")
                    .join("frus")
                    .join(APP)
            )
        );
        let mac = env(&[("HOME", "/Users/me")]);
        assert_eq!(
            desktop_data("macos", mac, OsStr::new(APP)),
            Some(
                PathBuf::from("/Users/me")
                    .join("Library")
                    .join("Application Support")
                    .join("frus")
                    .join(APP)
            )
        );
        let windows = env(&[
            ("APPDATA", r"C:\Users\me\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\me\AppData\Local"),
        ]);
        assert_eq!(
            desktop_data("windows", windows, OsStr::new(APP)),
            Some(
                PathBuf::from(r"C:\Users\me\AppData\Roaming")
                    .join("frus")
                    .join(APP)
            )
        );
        assert_eq!(desktop_data("windows", env(&[]), OsStr::new(APP)), None);
        assert_eq!(
            desktop_data("linux", env(&[("HOME", "/h")]), OsStr::new("")),
            None
        );
    }

    /// Android: the `cache` folder beside `files`, and the folder used before inside `files`.
    #[test]
    fn android_keeps_them_in_the_cache_folder() {
        let files = Path::new("/data/user/0/com.example.counter/files");
        assert_eq!(
            android(files),
            Path::new("/data/user/0/com.example.counter")
                .join("cache")
                .join("frus-pipelines")
        );
        assert_ne!(android(files), android_before_697(files));
        let odd = Path::new("/somewhere/else");
        assert_eq!(android(odd), odd.join("frus-pipelines"));
    }
}
