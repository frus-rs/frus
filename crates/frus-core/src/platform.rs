// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Which platform the application is running on**, and which platform's conventions it
//! should follow (milestone 614).
//!
//! Two questions, kept apart:
//!
//! - [`TargetPlatform`] is the **design language** a widget adapts to: how a list scrolls
//!   past its end, where a bar puts its title, how a page arrives. It is read from the
//!   theme (`Theme::platform`), which starts at [`default_target_platform`], so an
//!   application can show iOS behaviour on an Android phone, or a test can try both.
//! - [`Platform`] answers questions about the **machine** itself: the operating system
//!   the binary was built for, how many processors it has. It never changes at run time
//!   and no theme overrides it.
//!
//! On the web the two part company: the binary is built for `wasm32` and runs in a browser
//! on some operating system. [`IS_WEB`] says so, and [`default_target_platform`] is the
//! browser's operating system, which the web shell reads from the navigator at start-up.

use std::sync::atomic::{AtomicU8, Ordering};

/// The platforms whose conventions a widget can follow.
///
/// Every platform a frus application can run on has its own value, rather than borrowing
/// another's: a platform folded into another at the start can never be given behaviour
/// of its own without breaking every application that relied on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetPlatform {
    /// Android.
    Android,
    /// Fuchsia.
    Fuchsia,
    /// iOS and iPadOS.
    Ios,
    /// Linux and the other free Unix desktops.
    Linux,
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
}

impl TargetPlatform {
    /// Every platform, in declaration order.
    pub const ALL: [TargetPlatform; 6] = [
        TargetPlatform::Android,
        TargetPlatform::Fuchsia,
        TargetPlatform::Ios,
        TargetPlatform::Linux,
        TargetPlatform::MacOs,
        TargetPlatform::Windows,
    ];

    /// The platform this binary was **built for**. A web build, which is built for no
    /// operating system, reports Android here: what the browser runs on is
    /// [`default_target_platform`]'s business.
    pub const fn of_build() -> Self {
        if cfg!(target_os = "ios") {
            TargetPlatform::Ios
        } else if cfg!(target_os = "macos") {
            TargetPlatform::MacOs
        } else if cfg!(target_os = "windows") {
            TargetPlatform::Windows
        } else if cfg!(target_os = "fuchsia") {
            TargetPlatform::Fuchsia
        } else if cfg!(any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        )) {
            TargetPlatform::Linux
        } else {
            TargetPlatform::Android
        }
    }

    /// The platform a browser runs on, from what its navigator says: `navigator.platform`,
    /// `navigator.userAgent` and `navigator.maxTouchPoints`.
    ///
    /// An iPad asks for desktop pages and says it is a Mac; it is told apart by its touch
    /// screen. Android's `platform` is a Linux, so the user agent is asked first, and an
    /// iPhone's or an Android phone's user agent wins over a `platform` that says otherwise (a
    /// desktop browser emulating a phone keeps its own `platform`). A browser
    /// that keeps its `platform` empty (some do, for privacy) is read from its user agent
    /// alone. Anything unrecognised is Android.
    pub fn from_browser(platform: &str, user_agent: &str, max_touch_points: i32) -> Self {
        let lower = platform.to_ascii_lowercase();
        if platform.starts_with("Mac") {
            if max_touch_points > 2 {
                TargetPlatform::Ios
            } else {
                TargetPlatform::MacOs
            }
        } else if lower.contains("iphone") || lower.contains("ipad") || lower.contains("ipod") {
            TargetPlatform::Ios
        } else if user_agent.contains("Android") {
            TargetPlatform::Android
        } else if user_agent.contains("iPhone")
            || user_agent.contains("iPad")
            || user_agent.contains("iPod")
        {
            TargetPlatform::Ios
        } else if platform.starts_with("Linux") || lower.contains("bsd") || lower.contains("x11") {
            TargetPlatform::Linux
        } else if platform.starts_with("Win") {
            TargetPlatform::Windows
        } else if user_agent.contains("Macintosh") || user_agent.contains("Mac OS X") {
            if max_touch_points > 2 {
                TargetPlatform::Ios
            } else {
                TargetPlatform::MacOs
            }
        } else if user_agent.contains("Windows") {
            TargetPlatform::Windows
        } else if user_agent.contains("X11")
            || user_agent.contains("Linux")
            || user_agent.contains("CrOS")
            || user_agent.contains("BSD")
        {
            TargetPlatform::Linux
        } else {
            TargetPlatform::Android
        }
    }

    /// iOS or macOS: the platforms that share Apple's conventions.
    pub const fn is_apple(self) -> bool {
        matches!(self, TargetPlatform::Ios | TargetPlatform::MacOs)
    }

    /// Android, Fuchsia or iOS: the platforms driven by touch first.
    pub const fn is_mobile(self) -> bool {
        matches!(
            self,
            TargetPlatform::Android | TargetPlatform::Fuchsia | TargetPlatform::Ios
        )
    }

    /// Linux, macOS or Windows: the platforms driven by a mouse and a keyboard first.
    pub const fn is_desktop(self) -> bool {
        !self.is_mobile()
    }

    /// The platform's name, as people write it: `"Android"`, `"iOS"`, `"macOS"`…
    pub const fn name(self) -> &'static str {
        match self {
            TargetPlatform::Android => "Android",
            TargetPlatform::Fuchsia => "Fuchsia",
            TargetPlatform::Ios => "iOS",
            TargetPlatform::Linux => "Linux",
            TargetPlatform::MacOs => "macOS",
            TargetPlatform::Windows => "Windows",
        }
    }

    const fn code(self) -> u8 {
        match self {
            TargetPlatform::Android => 1,
            TargetPlatform::Fuchsia => 2,
            TargetPlatform::Ios => 3,
            TargetPlatform::Linux => 4,
            TargetPlatform::MacOs => 5,
            TargetPlatform::Windows => 6,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(TargetPlatform::Android),
            2 => Some(TargetPlatform::Fuchsia),
            3 => Some(TargetPlatform::Ios),
            4 => Some(TargetPlatform::Linux),
            5 => Some(TargetPlatform::MacOs),
            6 => Some(TargetPlatform::Windows),
            _ => None,
        }
    }
}

impl std::fmt::Display for TargetPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether this binary runs in a **browser**. A constant: a build is for the web or not.
pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

/// The override set by [`set_debug_default_target_platform_override`]; 0 for none.
static OVERRIDE: AtomicU8 = AtomicU8::new(0);
/// What the shell found the platform to be at start-up (the browser's operating system);
/// 0 until then, and on every platform where the build says it all.
static DETECTED: AtomicU8 = AtomicU8::new(0);

/// The platform whose conventions an application follows **unless its theme says
/// otherwise**: the debug override if one is set, else the browser's operating system on
/// the web, else the platform the binary was built for.
///
/// Widgets do not read this directly: they read `Theme::platform`, which starts here, so
/// that a theme can change it for a subtree or for the whole application.
pub fn default_target_platform() -> TargetPlatform {
    debug_default_target_platform_override()
        .or_else(detected_target_platform)
        .unwrap_or(TargetPlatform::of_build())
}

/// The platform whose **keyboard** the application is typed on: the one a shortcut's
/// primary modifier follows, Cmd on an Apple keyboard and Ctrl on every other
/// (milestone 852).
///
/// It is the machine's, read the way [`default_target_platform`] reads it (the browser's
/// operating system on the web, the build's elsewhere), but the **debug override does not
/// change it**: a window standing in for an iPhone on a Windows machine is still typed on a
/// Windows keyboard, and Ctrl+C must keep copying there. An application that wants another
/// keyboard's conventions names that platform where it resolves a stroke.
pub fn keyboard_platform() -> TargetPlatform {
    detected_target_platform().unwrap_or(TargetPlatform::of_build())
}

/// The browser's operating system on the web, read from the navigator the first time it is
/// asked for and kept; `None` on every other platform, where the build says it all.
///
/// Read here rather than only by the shell at start-up (milestone 847): an application is
/// made before the shell runs it, and a theme made then (`Theme::default()`, a seeded theme
/// in the application's constructor) was given the build's platform, Android, in every
/// browser, so a desktop browser stretched its lists past their ends like a phone.
fn detected_target_platform() -> Option<TargetPlatform> {
    if let Some(platform) = TargetPlatform::from_code(DETECTED.load(Ordering::Relaxed)) {
        return Some(platform);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(platform) = browser::navigator_platform() {
        DETECTED.store(platform.code(), Ordering::Relaxed);
        return Some(platform);
    }
    None
}

/// The navigator, read on the web.
#[cfg(target_arch = "wasm32")]
mod browser {
    use super::TargetPlatform;
    use wasm_bindgen::JsValue;

    /// What the page's navigator (a window's or a worker's) says the system is; `None`
    /// where there is no navigator.
    pub(super) fn navigator_platform() -> Option<TargetPlatform> {
        let navigator = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator"))
            .ok()
            .filter(|n| n.is_object())?;
        let get = |name: &str| {
            js_sys::Reflect::get(&navigator, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED)
        };
        let platform = get("platform").as_string().unwrap_or_default();
        let user_agent = get("userAgent").as_string().unwrap_or_default();
        let touch = get("maxTouchPoints").as_f64().unwrap_or(0.0) as i32;
        Some(TargetPlatform::from_browser(&platform, &user_agent, touch))
    }
}

/// The override of [`default_target_platform`], if one is set.
pub fn debug_default_target_platform_override() -> Option<TargetPlatform> {
    TargetPlatform::from_code(OVERRIDE.load(Ordering::Relaxed))
}

/// Makes [`default_target_platform`] report `platform`, or, with `None`, what it would
/// report on its own: for trying another platform's behaviour during development, and
/// for tests. It is process-wide, so a test that sets it resets it before it returns.
///
/// # Panics
///
/// In a release build (without debug assertions): an application that ships pretending
/// to be another platform should say so in its theme, where it is visible, rather than
/// through a switch meant for development.
pub fn set_debug_default_target_platform_override(platform: Option<TargetPlatform>) {
    if !cfg!(debug_assertions) {
        panic!(
            "the default target platform can only be overridden in a debug build; \
             set `Theme::platform` instead"
        );
    }
    OVERRIDE.store(platform.map_or(0, TargetPlatform::code), Ordering::Relaxed);
}

/// Records the platform the shell detected at start-up (the browser's operating system).
/// For the shell; an application sets `Theme::platform` instead.
#[doc(hidden)]
pub fn __set_detected_target_platform(platform: TargetPlatform) {
    DETECTED.store(platform.code(), Ordering::Relaxed);
}

/// Facts about the **machine** the application runs on, fixed for the life of the
/// process. Unlike [`TargetPlatform`], nothing overrides them.
pub struct Platform;

impl Platform {
    /// The operating system this binary was built for, in lower case: `"android"`,
    /// `"ios"`, `"linux"`, `"macos"`, `"windows"`… and `""` on the web, which is built for
    /// none.
    pub const fn operating_system() -> &'static str {
        std::env::consts::OS
    }

    /// Built for Android.
    pub const fn is_android() -> bool {
        cfg!(target_os = "android")
    }

    /// Built for iOS.
    pub const fn is_ios() -> bool {
        cfg!(target_os = "ios")
    }

    /// Built for Linux (not Android).
    pub const fn is_linux() -> bool {
        cfg!(target_os = "linux")
    }

    /// Built for macOS.
    pub const fn is_macos() -> bool {
        cfg!(target_os = "macos")
    }

    /// Built for Windows.
    pub const fn is_windows() -> bool {
        cfg!(target_os = "windows")
    }

    /// Built for Fuchsia.
    pub const fn is_fuchsia() -> bool {
        cfg!(target_os = "fuchsia")
    }

    /// Built for the web ([`IS_WEB`]).
    pub const fn is_web() -> bool {
        IS_WEB
    }

    /// How many threads the machine can run at once; 1 where it cannot say (a browser).
    pub fn number_of_processors() -> usize {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    }

    /// The separator between a path's components: `\` on Windows, `/` elsewhere.
    pub const fn path_separator() -> char {
        std::path::MAIN_SEPARATOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The build decides, natively**: this binary reports the platform it was built for.
    #[test]
    fn the_default_is_the_platform_built_for() {
        let expected = if cfg!(target_os = "windows") {
            TargetPlatform::Windows
        } else if cfg!(target_os = "macos") {
            TargetPlatform::MacOs
        } else if cfg!(target_os = "linux") {
            TargetPlatform::Linux
        } else {
            TargetPlatform::of_build()
        };
        assert_eq!(TargetPlatform::of_build(), expected);
        assert_eq!(Platform::operating_system(), std::env::consts::OS);
        assert_eq!(Platform::is_web(), IS_WEB);
    }

    /// **The override wins, and goes away**: set, it is what every reader is told; reset,
    /// they are told the truth again. The only test that touches the process-wide switch.
    #[test]
    fn the_debug_override_wins_until_it_is_cleared() {
        let native = default_target_platform();
        for platform in TargetPlatform::ALL {
            set_debug_default_target_platform_override(Some(platform));
            assert_eq!(default_target_platform(), platform);
            assert_eq!(debug_default_target_platform_override(), Some(platform));
        }
        set_debug_default_target_platform_override(None);
        assert_eq!(default_target_platform(), native);
        assert_eq!(debug_default_target_platform_override(), None);
    }

    /// **A browser is told apart by its navigator**: what real browsers report.
    #[test]
    fn a_browser_reports_the_system_it_runs_on() {
        let cases = [
            // Chrome on Windows.
            (
                "Win32",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
                0,
                TargetPlatform::Windows,
            ),
            // Safari on a Mac.
            (
                "MacIntel",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
                0,
                TargetPlatform::MacOs,
            ),
            // An iPad asking for the desktop page: a "Mac" with a touch screen.
            (
                "MacIntel",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
                5,
                TargetPlatform::Ios,
            ),
            // A phone emulated by a desktop browser: the user agent is the phone's, the
            // platform the computer's.
            (
                "Win32",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)",
                5,
                TargetPlatform::Ios,
            ),
            (
                "Win32",
                "Mozilla/5.0 (Linux; Android 14; Pixel 7)",
                5,
                TargetPlatform::Android,
            ),
            // An iPhone.
            (
                "iPhone",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)",
                5,
                TargetPlatform::Ios,
            ),
            // Chrome on Android, whose platform is a Linux.
            (
                "Linux armv8l",
                "Mozilla/5.0 (Linux; Android 14; Pixel 8)",
                5,
                TargetPlatform::Android,
            ),
            // Firefox on a Linux desktop.
            (
                "Linux x86_64",
                "Mozilla/5.0 (X11; Linux x86_64; rv:120.0)",
                0,
                TargetPlatform::Linux,
            ),
            // Firefox on Windows, with privacy settings that keep the platform empty: the
            // user agent says it.
            (
                "",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:133.0)",
                0,
                TargetPlatform::Windows,
            ),
            // The same on a Mac, on an iPad, and on a Chromebook.
            (
                "",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
                0,
                TargetPlatform::MacOs,
            ),
            (
                "",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
                5,
                TargetPlatform::Ios,
            ),
            (
                "",
                "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0)",
                0,
                TargetPlatform::Linux,
            ),
            // A BSD desktop.
            (
                "FreeBSD amd64",
                "Mozilla/5.0 (X11; FreeBSD amd64; rv:120.0)",
                0,
                TargetPlatform::Linux,
            ),
            // Something nobody recognises.
            ("", "", 0, TargetPlatform::Android),
        ];
        for (platform, agent, touch, expected) in cases {
            assert_eq!(
                TargetPlatform::from_browser(platform, agent, touch),
                expected,
                "{platform:?} / {agent:?} / {touch}"
            );
        }
    }

    /// **The families**: Apple's two, touch first, mouse first, and each named as people
    /// write it.
    #[test]
    fn platforms_fall_into_families() {
        let apple: Vec<_> = TargetPlatform::ALL
            .into_iter()
            .filter(|p| p.is_apple())
            .collect();
        assert_eq!(apple, [TargetPlatform::Ios, TargetPlatform::MacOs]);
        let mobile: Vec<_> = TargetPlatform::ALL
            .into_iter()
            .filter(|p| p.is_mobile())
            .collect();
        assert_eq!(
            mobile,
            [
                TargetPlatform::Android,
                TargetPlatform::Fuchsia,
                TargetPlatform::Ios
            ]
        );
        for platform in TargetPlatform::ALL {
            assert_ne!(platform.is_mobile(), platform.is_desktop());
            assert_eq!(TargetPlatform::from_code(platform.code()), Some(platform));
        }
        assert_eq!(TargetPlatform::from_code(0), None);
        assert_eq!(TargetPlatform::Ios.to_string(), "iOS");
        assert_eq!(TargetPlatform::MacOs.name(), "macOS");
    }
}
