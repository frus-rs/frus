// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Calling the platform yourself** (milestone 839): the bridge an application or a crate
//! uses for a system API frus does not wrap.
//!
//! frus wraps the common platform services (sharing, haptics, notifications, the camera…).
//! For everything else, Rust reaches the system directly, and this module is the documented
//! way in, per target:
//!
//! - **Android**: `android::with_env` gives a `JNIEnv` and the activity on any thread,
//!   attaching the thread to the Java VM for the call when it is not already. Listeners,
//!   callbacks and broadcast receivers are implemented in Rust with `android::JavaProxy`,
//!   with no Java file and no dex of the crate's own. `jni` is re-exported as
//!   `android::jni`, so a crate needs no version of its own.
//! - **The web**: `web::window` and `web::document`, with `web_sys`, `js_sys`,
//!   `wasm_bindgen` and `wasm_bindgen_futures` re-exported in `web`.
//! - **Windows, macOS, Linux and iOS**: no bridge is needed, and none is offered. Their
//!   system interfaces are C, COM or Objective-C, which Rust calls directly (the `windows`,
//!   `objc2` and D-Bus crates), on any thread the API allows; there is no VM to attach to.
//!   [`BRIDGE`] says which case a build is in, for code that must decide at run time.
//!
//! On every target, a callback that runs on a system thread hands what it learned to the
//! interface's thread with [`channel`]: the value is queued, the task awaiting it is woken,
//! and that wake asks the window for a frame, which runs the task.
//!
//! Nothing here costs an application that does not call it: no thread, no listener, no
//! class loaded. On Android the bridge's one class is loaded by the first `android::JavaProxy`.

mod channel;

pub use channel::{channel, Receiver, Sender};

#[cfg(android)]
pub mod android;
#[cfg(web)]
pub mod web;

/// Which way into the platform this build has: what [`BRIDGE`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bridge {
    /// Android: the Java VM, through `frus::platform::android` (`with_env`, `JavaProxy`).
    Android,
    /// The browser: `frus::platform::web` (`window`, `document`, `web_sys`).
    Web,
    /// Windows, macOS, Linux or iOS: no bridge, the system's interfaces being called from
    /// Rust directly.
    Native,
}

/// Which way into the platform this build has. On Android and the web, the modules of the
/// same names exist; elsewhere neither does, and code reaching the system directly is gated on
/// its own `cfg`.
pub const BRIDGE: Bridge = if cfg!(target_os = "android") {
    Bridge::Android
} else if cfg!(target_arch = "wasm32") {
    Bridge::Web
} else {
    Bridge::Native
};

/// Why a call into the platform did not complete.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlatformError {
    /// The platform cannot be reached from here: no activity yet (or any more), no browser
    /// window, or the bridge's class could not be loaded. The text says which.
    Unavailable(String),
    /// The platform threw: the exception's text (`toString()` on Android). The exception has
    /// been cleared, so the next call starts clean.
    Exception(String),
    /// The binding itself failed (a JNI error that is not a Java exception: a method that
    /// does not exist with that signature, a wrong type).
    Binding(String),
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformError::Unavailable(why) => write!(f, "the platform is unavailable: {why}"),
            PlatformError::Exception(text) => write!(f, "the platform threw: {text}"),
            PlatformError::Binding(text) => write!(f, "the platform call failed: {text}"),
        }
    }
}

impl std::error::Error for PlatformError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_test_build_has_no_bridge() {
        assert_eq!(BRIDGE, Bridge::Native);
    }

    #[test]
    fn an_error_says_what_failed() {
        assert_eq!(
            PlatformError::Exception("java.lang.SecurityException: no".into()).to_string(),
            "the platform threw: java.lang.SecurityException: no"
        );
    }
}
