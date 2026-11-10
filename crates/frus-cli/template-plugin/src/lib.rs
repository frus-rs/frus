//! `{{project-name}}` — a frus plugin: something the platform does, called from Rust.
//!
//! One function for the application, the same on every platform:
//!
//! ```no_run
//! match {{crate_name}}::platform_version() {
//!     Ok(version) => println!("running on {version}"),
//!     Err({{crate_name}}::PluginError::Unsupported(platform)) => println!("not on {platform}"),
//!     Err(error) => eprintln!("{error}"),
//! }
//! ```
//!
//! Each platform has its side: Android's is Java (`android/java`), reached through JNI from
//! [`android`]; the others are Rust, below. A platform where the plugin cannot work returns
//! [`PluginError::Unsupported`] — never a panic, never a value that pretends it worked.

use std::fmt;

#[cfg(target_os = "android")]
mod android;

/// Why the plugin could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// The platform has no such thing, or the plugin has no side for it yet.
    Unsupported(&'static str),
    /// The platform was asked and failed: what it said.
    Failed(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::Unsupported(platform) => {
                write!(f, "{{project-name}} does not work on {platform}")
            }
            PluginError::Failed(why) => write!(f, "{{project-name}} failed: {why}"),
        }
    }
}

impl std::error::Error for PluginError {}

/// The platform's own name for its version: `Android 10 (API 29)`.
///
/// Android only, for now; the desktops and the web say [`PluginError::Unsupported`]. Fill in
/// their side below to support them.
pub fn platform_version() -> Result<String, PluginError> {
    #[cfg(target_os = "android")]
    {
        android::platform_version()
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(PluginError::Unsupported(platform()))
    }
}

/// The platform this was built for, as a person names it.
pub fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "Android"
    } else if cfg!(target_arch = "wasm32") {
        "the web"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "ios") {
        "iOS"
    } else {
        "Linux"
    }
}
