// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Giving memory back when the system is short of it** (milestone 840).
//!
//! [`trim_caches`] is the one call: the platform layer makes it when the system says memory is
//! low or the application leaves the screen, and an application may make it too. What the
//! process holds that it can rebuild goes, in two places:
//!
//! - the **decoded images** nobody shows, here and now, from whatever thread asks: the image
//!   stores are behind locks;
//! - the **GPU's** textures, which belong to the renderer on the interface's thread: the
//!   request is left for it ([`take_gpu_trim`]) and a frame is asked for, so the shell takes
//!   it on the loop's next turn. A request made while nothing renders (the window gone, in the
//!   background on Android) has nothing to trim there, and is simply taken.

use std::sync::atomic::{AtomicU8, Ordering};

use frus_core::TrimLevel;

/// The GPU trim asked for and not yet taken: 0 for none, else the level's code. Several
/// requests before the loop turns are one, at the highest level asked.
static GPU_TRIM: AtomicU8 = AtomicU8::new(0);

fn code(level: TrimLevel) -> u8 {
    match level {
        TrimLevel::Moderate => 1,
        TrimLevel::Critical => 2,
        TrimLevel::Hidden => 3,
    }
}

fn level(code: u8) -> Option<TrimLevel> {
    match code {
        1 => Some(TrimLevel::Moderate),
        2 => Some(TrimLevel::Critical),
        3 => Some(TrimLevel::Hidden),
        _ => None,
    }
}

/// **Gives back what the application can rebuild**, as much as `level` says (see
/// [`TrimLevel`]).
///
/// The decoded images nobody is showing go at once, at every level. From
/// [`TrimLevel::Critical`] up, the renderer also lets go of GPU memory, on the loop's next
/// turn: the glyph atlas shrinks back to what the next frame draws, and at
/// [`TrimLevel::Hidden`] every texture the next frame can make again goes too.
///
/// frus calls this itself: on Android's `onTrimMemory` and low-memory callbacks, on iOS's
/// memory warning, and when a window or a browser tab is hidden. An application can call it on
/// leaving a screen full of photographs, or from its own signal on a desktop:
///
/// ```
/// use frus_widgets::{trim_caches, TrimLevel};
///
/// trim_caches(TrimLevel::Moderate);
/// ```
///
/// It may be called from any thread, and costs nothing when there is nothing to give back.
pub fn trim_caches(level: TrimLevel) {
    let before = frus_core::decoded_image_bytes();
    frus_core::trim_image_caches();
    let after = frus_core::decoded_image_bytes();
    log::info!(
        "memory trimmed ({level:?}): decoded images from {} to {} KB",
        before / 1024,
        after / 1024
    );
    if level >= TrimLevel::Critical {
        GPU_TRIM.fetch_max(code(level), Ordering::AcqRel);
        crate::local_task::request_frame();
    }
}

/// The GPU trim [`trim_caches`] asked for since this was last called, at the highest level
/// asked; forgets it. The platform layer reads it once a turn of its loop and hands it to its
/// renderer.
#[doc(hidden)]
pub fn take_gpu_trim() -> Option<TrimLevel> {
    level(GPU_TRIM.swap(0, Ordering::AcqRel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_core::{DecodeSize, ImageData};

    /// Bytes of this test's own, so that its pictures are keys no other test uses.
    static BYTES: [u8; 4] = [8, 4, 0, 0];

    fn decode(_: &[u8]) -> Result<ImageData, String> {
        Ok(ImageData::from_rgba(64, 64, vec![255; 64 * 64 * 4]))
    }

    /// **Every level gives back the decoded pictures nobody shows**, and keeps the ones on
    /// screen; only the levels that reach the GPU leave it a request, taken once, at the
    /// highest level asked.
    #[test]
    fn trimming_gives_back_unshown_pictures_and_asks_the_gpu_from_critical_up() {
        let size = DecodeSize::new(Some(64), None);
        let shown = frus_core::cached_at(&BYTES, size, decode).unwrap();
        let unshown = frus_core::cached_at(&BYTES, DecodeSize::FULL, decode)
            .unwrap()
            .id();

        trim_caches(TrimLevel::Moderate);
        assert!(
            std::sync::Arc::ptr_eq(&shown, &frus_core::cached_at(&BYTES, size, decode).unwrap()),
            "the picture held is kept"
        );
        assert_ne!(
            frus_core::cached_at(&BYTES, DecodeSize::FULL, decode)
                .unwrap()
                .id(),
            unshown,
            "the one nobody held was given back, and is decoded again"
        );
        // Another test's request, if one ran meanwhile, is taken here too: this one checks
        // only what its own calls leave.
        let _ = take_gpu_trim();
        trim_caches(TrimLevel::Moderate);
        assert_eq!(
            take_gpu_trim(),
            None,
            "a moderate trim leaves the GPU alone"
        );

        trim_caches(TrimLevel::Hidden);
        trim_caches(TrimLevel::Critical);
        assert_eq!(
            take_gpu_trim(),
            Some(TrimLevel::Hidden),
            "two requests before the loop turns are one, at the higher level"
        );
        assert_eq!(take_gpu_trim(), None, "and it is taken once");
    }
}
