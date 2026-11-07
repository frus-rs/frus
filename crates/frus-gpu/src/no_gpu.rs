// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Why no GPU could be reached**, said so that someone can act on it (milestone 769).
//!
//! frus draws through Vulkan or OpenGL ES 3.0 and later. A device that offers neither — in
//! practice the Android emulator running an Android 9 (or older) system image with its
//! default graphics, which gives the guest OpenGL ES 2.0 only and no Vulkan — used to end
//! with the graphics library's own words, "Failed to create surface for any enabled
//! backend", which name neither the cause nor the remedy. The failure now says what the
//! device offers, what frus needs, and, on the emulator, the one flag that changes it.

/// What the device says of its GPU, as far as the cause of a failure goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GpuSurvey {
    /// The OpenGL ES version the system reports (`ro.opengles.version`), as major and
    /// minor; `None` where it says nothing.
    pub gles: Option<(u32, u32)>,
    /// Why Vulkan was not used — no driver, or only the emulator's — or `None` when it was
    /// tried.
    pub vulkan_unusable: Option<&'static str>,
    /// Whether this is the Android emulator.
    pub emulator: bool,
}

/// `ro.opengles.version`, a number whose upper 16 bits are the major version and lower 16
/// bits the minor: `131072` is 2.0, `196610` is 3.2.
pub(crate) fn gles_version(property: u32) -> (u32, u32) {
    (property >> 16, property & 0xffff)
}

/// What to say in front of the graphics library's error, when the survey explains it:
/// `None` when the device offers what frus needs, and the cause is elsewhere.
pub(crate) fn explain(survey: &GpuSurvey) -> Option<String> {
    let (major, minor) = survey.gles?;
    if major >= 3 || survey.vulkan_unusable.is_none() {
        return None;
    }
    let vulkan = survey.vulkan_unusable.unwrap_or_default();
    let mut text = format!(
        "this device offers OpenGL ES {major}.{minor} only ({vulkan}), and frus draws with \
         Vulkan or OpenGL ES 3.0 and later"
    );
    if survey.emulator {
        text.push_str(
            ". The Android emulator gives an Android 9 (or older) system image OpenGL ES 2.0 \
             unless it is started with `-feature GLESDynamicVersion` (`frus run` and `frus \
             emulators --launch` start it so): restart the emulator with that flag",
        );
    } else {
        text.push_str(": its GPU is older than frus supports");
    }
    Some(text)
}

/// The survey of this device: Android's own system properties.
#[cfg(target_os = "android")]
pub(crate) fn survey(vulkan_unusable: Option<&'static str>) -> GpuSurvey {
    let gles = property("ro.opengles.version")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&v| v > 0)
        .map(gles_version);
    let emulator = property("ro.kernel.qemu").is_some_and(|v| v.trim() == "1")
        || property("ro.boot.qemu").is_some_and(|v| v.trim() == "1");
    GpuSurvey {
        gles,
        vulkan_unusable,
        emulator,
    }
}

/// A system property, through the C library every Android process has.
#[cfg(target_os = "android")]
fn property(name: &str) -> Option<String> {
    use std::ffi::{c_char, c_int, CString};
    extern "C" {
        fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
    }
    // PROP_VALUE_MAX: a value is at most 92 bytes, its terminating zero included.
    let mut value = [0u8; 92];
    let name = CString::new(name).ok()?;
    // SAFETY: `name` is a C string and `value` is PROP_VALUE_MAX bytes long, as the call
    // requires.
    let len = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr().cast()) };
    let len = usize::try_from(len).ok().filter(|&l| l > 0)?;
    Some(String::from_utf8_lossy(&value[..len.min(value.len())]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_property_is_read_as_major_and_minor() {
        assert_eq!(gles_version(131072), (2, 0));
        assert_eq!(gles_version(196608), (3, 0));
        assert_eq!(gles_version(196610), (3, 2));
    }

    /// The Android 9 emulator with its default graphics: OpenGL ES 2.0 and no Vulkan. The
    /// failure names what the device offers, what frus needs, and the emulator's flag.
    #[test]
    fn the_emulator_with_opengl_es_2_is_told_the_flag_that_changes_it() {
        let text = explain(&GpuSurvey {
            gles: Some((2, 0)),
            vulkan_unusable: Some("no Vulkan driver on this device"),
            emulator: true,
        })
        .expect("explained");
        assert!(text.contains("OpenGL ES 2.0 only"), "{text}");
        assert!(text.contains("no Vulkan driver"), "{text}");
        assert!(text.contains("OpenGL ES 3.0"), "{text}");
        assert!(text.contains("-feature GLESDynamicVersion"), "{text}");
    }

    #[test]
    fn a_device_with_opengl_es_2_is_told_its_gpu_is_too_old() {
        let text = explain(&GpuSurvey {
            gles: Some((2, 0)),
            vulkan_unusable: Some("no Vulkan driver on this device"),
            emulator: false,
        })
        .expect("explained");
        assert!(text.contains("older than frus supports"), "{text}");
        assert!(!text.contains("emulator"), "{text}");
    }

    /// A device that offers what frus needs failed for another reason, which the survey
    /// cannot tell: nothing is added to the graphics library's own words.
    #[test]
    fn nothing_is_said_when_the_device_offers_what_frus_needs() {
        let es3 = GpuSurvey {
            gles: Some((3, 2)),
            vulkan_unusable: Some("no Vulkan driver on this device"),
            emulator: true,
        };
        assert_eq!(explain(&es3), None);
        let vulkan_tried = GpuSurvey {
            gles: Some((2, 0)),
            vulkan_unusable: None,
            emulator: false,
        };
        assert_eq!(explain(&vulkan_tried), None);
        assert_eq!(explain(&GpuSurvey::default()), None);
    }
}
