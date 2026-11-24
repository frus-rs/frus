// SPDX-License-Identifier: MIT OR Apache-2.0
//! Which window system a Linux desktop application talks to (milestone 843).
//!
//! Under WSLg (Linux GUI applications on Windows) the compositor is a weston 9 that crashes,
//! with a segmentation fault, a few seconds after a Wayland window with client-side decorations
//! appears: every window on it closes, and the application ends with a broken pipe. It does not
//! fail that way with an X11 window, which its own window manager frames. So under WSLg, when an
//! X server is there, the window goes through X11. `FRUS_WAYLAND=1` keeps Wayland.

use std::ffi::OsStr;

/// Whether this process should open its window through X11 rather than Wayland.
#[cfg(target_os = "linux")]
pub(crate) fn x11_preferred() -> bool {
    let version = std::fs::read_to_string("/proc/version").unwrap_or_default();
    let wslg = std::path::Path::new("/mnt/wslg").is_dir();
    let chosen = x11_under_wslg(
        &version,
        wslg,
        std::env::var_os("DISPLAY").as_deref(),
        std::env::var_os("FRUS_WAYLAND").as_deref(),
    );
    if chosen {
        log::info!(
            "WSLg: the window goes through X11, as its compositor ends Wayland windows \
             (FRUS_WAYLAND=1 keeps Wayland)"
        );
    }
    chosen
}

/// The decision, from what the system says: the kernel's version line, whether WSLg's folder is
/// there, `DISPLAY` and `FRUS_WAYLAND`.
pub(crate) fn x11_under_wslg(
    proc_version: &str,
    wslg: bool,
    display: Option<&OsStr>,
    wayland_wanted: Option<&OsStr>,
) -> bool {
    let wsl = proc_version.to_ascii_lowercase().contains("microsoft");
    let x_server = display.is_some_and(|d| !d.is_empty());
    let keep_wayland = wayland_wanted.is_some_and(|v| v == "1");
    wsl && wslg && x_server && !keep_wayland
}

#[cfg(test)]
mod tests {
    use super::*;

    const WSL: &str = "Linux version 6.6.87.2-microsoft-standard-WSL2 (root@...) #1 SMP";
    const LINUX: &str = "Linux version 6.8.0-49-generic (buildd@lcy02-amd64-103) #49-Ubuntu SMP";

    #[test]
    fn wslg_with_an_x_server_takes_x11() {
        assert!(x11_under_wslg(WSL, true, Some(OsStr::new(":0")), None));
    }

    #[test]
    fn everywhere_else_winit_chooses() {
        // A Linux machine of its own.
        assert!(!x11_under_wslg(LINUX, false, Some(OsStr::new(":0")), None));
        // WSL without its graphical part, or without an X server.
        assert!(!x11_under_wslg(WSL, false, Some(OsStr::new(":0")), None));
        assert!(!x11_under_wslg(WSL, true, None, None));
        assert!(!x11_under_wslg(WSL, true, Some(OsStr::new("")), None));
    }

    #[test]
    fn frus_wayland_keeps_wayland() {
        assert!(!x11_under_wslg(
            WSL,
            true,
            Some(OsStr::new(":0")),
            Some(OsStr::new("1"))
        ));
        assert!(x11_under_wslg(
            WSL,
            true,
            Some(OsStr::new(":0")),
            Some(OsStr::new("0"))
        ));
    }
}
