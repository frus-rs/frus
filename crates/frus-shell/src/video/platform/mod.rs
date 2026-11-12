// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Each platform's player, behind [`open`].

use std::sync::Arc;

use super::controller::{DataSource, Player, Sink, VideoError};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "android")]
pub(crate) use android::remember;

/// Opens `source` with the platform's player, which reports to `sink`.
pub(crate) fn open(source: &DataSource, sink: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
    #[cfg(target_os = "android")]
    {
        android::open(source, sink)
    }
    #[cfg(windows)]
    {
        windows::open(source, sink)
    }
    #[cfg(target_arch = "wasm32")]
    {
        web::open(source, sink)
    }
    #[cfg(not(any(target_os = "android", windows, target_arch = "wasm32")))]
    {
        let _ = (source, sink);
        Err(VideoError::Unsupported(
            "frus plays video on Android, Windows and the Web for now",
        ))
    }
}

/// What a file's first bytes say it is, for the file name a player reads it under.
#[cfg_attr(not(any(target_os = "android", windows)), allow(dead_code))]
pub(crate) fn extension(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        if &bytes[8..10] == b"qt" {
            "mov"
        } else {
            "mp4"
        }
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        "webm"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"AVI " {
        "avi"
    } else if bytes.starts_with(b"OggS") {
        "ogg"
    } else if bytes.starts_with(&[0x47]) && bytes.len() > 188 && bytes[188] == 0x47 {
        "ts"
    } else {
        "mp4"
    }
}

/// Where an asset's bytes are read from by a player that reads files: written once into the
/// application's cache folder, under a name made of their length and a hash of them, and
/// reused while the same bytes are there.
#[cfg(any(target_os = "android", windows))]
pub(crate) fn asset_file(bytes: &'static [u8]) -> Result<std::path::PathBuf, VideoError> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let dir = crate::cache_dir::app_cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("video");
    let path = dir.join(format!(
        "{:016x}-{}.{}",
        hasher.finish(),
        bytes.len(),
        extension(bytes)
    ));
    let fresh = std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64);
    if !fresh {
        std::fs::create_dir_all(&dir)
            .and_then(|()| {
                // Written aside and renamed, so a player never reads half a file.
                let partial = path.with_extension("part");
                std::fs::write(&partial, bytes)?;
                std::fs::rename(&partial, &path)
            })
            .map_err(|e| VideoError::Source(format!("{}: {e}", path.display())))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_follow_the_first_bytes() {
        let mut mp4 = vec![0, 0, 0, 0x20];
        mp4.extend_from_slice(b"ftypisom");
        assert_eq!(extension(&mp4), "mp4");
        let mut mov = vec![0, 0, 0, 0x14];
        mov.extend_from_slice(b"ftypqt  ");
        assert_eq!(extension(&mov), "mov");
        assert_eq!(extension(&[0x1A, 0x45, 0xDF, 0xA3, 1]), "webm");
        assert_eq!(extension(b"OggS...."), "ogg");
        assert_eq!(extension(b""), "mp4");
    }

    #[cfg(any(target_os = "android", windows))]
    #[test]
    fn an_asset_is_written_once_and_reused() {
        static CLIP: &[u8] = b"\0\0\0\x20ftypisom-a-test-clip-of-no-length";
        let first = asset_file(CLIP).expect("written");
        assert_eq!(std::fs::read(&first).expect("read"), CLIP);
        assert!(first.extension().is_some_and(|e| e == "mp4"));
        let again = asset_file(CLIP).expect("again");
        assert_eq!(first, again);
    }
}
