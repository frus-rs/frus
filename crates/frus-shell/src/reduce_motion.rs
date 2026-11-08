// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Whether the user asked for less motion**, read from the system on the desktops, iOS
//! and the Web (milestone 778). Android reads it with its other settings, in
//! `android_settings`.
//!
//! - Windows: *Show animations in Windows* (`SPI_GETCLIENTAREAANIMATION`).
//! - macOS: *Reduce motion* (`NSWorkspace.accessibilityDisplayShouldReduceMotion`).
//! - Linux: GTK's `gtk-enable-animations`, from its `settings.ini`, else GNOME's
//!   `enable-animations` through `gsettings`. Read once a process.
//! - iOS: *Reduce Motion* (`UIAccessibilityIsReduceMotionEnabled`).
//! - The Web: the `prefers-reduced-motion` media query.
//!
//! `None` where the answer could not be read: the setting then stays off.

/// Whether the system asks for reduced motion, if it could be read.
pub(crate) fn system_reduce_motion() -> Option<bool> {
    imp::read()
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };

    pub(super) fn read() -> Option<bool> {
        let mut enabled: i32 = 1;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL through the pointer, which
        // points at a live, aligned i32 for the length of the call.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            )
        };
        (ok != 0).then_some(enabled == 0)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    pub(super) fn read() -> Option<bool> {
        let class = AnyClass::get(c"NSWorkspace")?;
        // SAFETY: `sharedWorkspace` is a class method returning the shared instance (or
        // nil), and `accessibilityDisplayShouldReduceMotion` an instance property returning
        // a BOOL, available since macOS 10.12.
        unsafe {
            let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
            if workspace.is_null() {
                return None;
            }
            let reduce: Bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
            Some(reduce.as_bool())
        }
    }
}

#[cfg(target_os = "ios")]
mod imp {
    #[link(name = "UIKit", kind = "framework")]
    extern "C" {
        fn UIAccessibilityIsReduceMotionEnabled() -> bool;
    }

    pub(super) fn read() -> Option<bool> {
        // SAFETY: a UIKit function with no argument, callable from any thread.
        Some(unsafe { UIAccessibilityIsReduceMotionEnabled() })
    }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32"
    ))
))]
mod imp {
    use std::sync::OnceLock;

    pub(super) fn read() -> Option<bool> {
        static READ: OnceLock<Option<bool>> = OnceLock::new();
        *READ.get_or_init(|| from_settings_ini().or_else(from_gsettings))
    }

    /// `gtk-enable-animations` from the user's GTK 4, then GTK 3, settings file.
    fn from_settings_ini() -> Option<bool> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
            })?;
        ["gtk-4.0", "gtk-3.0"].iter().find_map(|dir| {
            let text = std::fs::read_to_string(config.join(dir).join("settings.ini")).ok()?;
            super::enable_animations_in_ini(&text).map(|enabled| !enabled)
        })
    }

    /// GNOME's `enable-animations`, which its settings daemon hands GTK.
    fn from_gsettings() -> Option<bool> {
        let out = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        match String::from_utf8_lossy(&out.stdout).trim() {
            "true" => Some(false),
            "false" => Some(true),
            _ => None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    pub(super) fn read() -> Option<bool> {
        let list = web_sys::window()?
            .match_media("(prefers-reduced-motion: reduce)")
            .ok()??;
        Some(list.matches())
    }
}

#[cfg(target_os = "android")]
mod imp {
    /// Android reads it with its other settings (`android_settings`).
    pub(super) fn read() -> Option<bool> {
        None
    }
}

/// The value of `gtk-enable-animations` in a GTK `settings.ini`, if it says.
#[cfg_attr(
    not(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "android",
            target_arch = "wasm32"
        ))
    )),
    allow(dead_code)
)]
fn enable_animations_in_ini(text: &str) -> Option<bool> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "gtk-enable-animations" {
            return None;
        }
        match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gtk_setting_is_read_from_its_file() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\ngtk-enable-animations = false\n";
        assert_eq!(enable_animations_in_ini(ini), Some(false));
        assert_eq!(
            enable_animations_in_ini("[Settings]\ngtk-enable-animations=1"),
            Some(true)
        );
        assert_eq!(enable_animations_in_ini("[Settings]\n"), None);
    }

    /// The system can be asked without failing; what it answers is the machine's.
    #[test]
    fn the_system_can_be_asked() {
        eprintln!("system reduce motion: {:?}", system_reduce_motion());
    }
}
