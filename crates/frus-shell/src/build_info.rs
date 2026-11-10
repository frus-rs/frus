// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **What was built** (milestone 790): the build mode, the flavor, the version, and the values
//! given with `frus run --define` / `frus build --define`.
//!
//! The command-line tool hands them to the compiler as environment variables, so each is a
//! constant of the build: reading one costs nothing, and a build with another value compiles
//! again by itself (cargo follows the variables a crate reads).
//!
//! | `frus` flag | variable | read with |
//! |---|---|---|
//! | `--debug` / `--profile` / `--release` | `FRUS_BUILD_MODE` (profile only) | [`build_mode`] |
//! | `--flavor dev` | `FRUS_FLAVOR` | [`flavor`] |
//! | `--build-name 1.2.3`, `--build-number 42` | `FRUS_BUILD_NAME`, `FRUS_BUILD_NUMBER` | [`app_version`] |
//! | `--define KEY=VALUE` | `FRUS_DEFINE_KEY` | [`define!`](crate::define) |

use std::fmt;
use std::sync::OnceLock;

/// How the application was built: what `frus run` and `frus build` were given.
///
/// ```
/// if frus_shell::build_mode() == frus_shell::BuildMode::Debug {
///     frus_shell::log::debug!("a development build");
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildMode {
    /// `frus run` without a flag: checks on, hot reload, the build that is quick to make.
    Debug,
    /// `--profile`: optimised as a release is, with the frame timings logged and the symbols
    /// kept, so that what is measured is what ships. No hot reload.
    Profile,
    /// `--release`, and `frus build`: what ships.
    Release,
}

impl BuildMode {
    /// The mode's name, as the command line spells it: `debug`, `profile` or `release`.
    ///
    /// ```
    /// assert_eq!(frus_shell::BuildMode::Profile.name(), "profile");
    /// ```
    pub fn name(self) -> &'static str {
        match self {
            BuildMode::Debug => "debug",
            BuildMode::Profile => "profile",
            BuildMode::Release => "release",
        }
    }
}

impl fmt::Display for BuildMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The mode this build was made in.
///
/// A debug build is one with debug assertions; a profile build is a release one the tool marks
/// with `FRUS_BUILD_MODE=profile` (a build made by hand with `cargo build --profile profile`
/// gets the mark by setting that variable).
///
/// ```
/// let mode = frus_shell::build_mode();
/// frus_shell::log::info!("built in {mode}");
/// ```
pub fn build_mode() -> BuildMode {
    mode_of(cfg!(debug_assertions), option_env!("FRUS_BUILD_MODE"))
}

fn mode_of(debug_assertions: bool, marked: Option<&str>) -> BuildMode {
    match marked {
        Some("profile") => BuildMode::Profile,
        Some("release") => BuildMode::Release,
        Some("debug") => BuildMode::Debug,
        _ if debug_assertions => BuildMode::Debug,
        _ => BuildMode::Release,
    }
}

/// The application's version: `--build-name` and `--build-number` when given, otherwise the
/// `version` of its `Cargo.toml`, whose build metadata is the number (`1.2.3+42`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppVersion {
    /// What people see: `1.2.3`.
    pub name: &'static str,
    /// What stores order releases by (Android's version code), when one was given.
    pub number: Option<u32>,
}

impl fmt::Display for AppVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.number {
            Some(number) => write!(f, "{}+{number}", self.name),
            None => f.write_str(self.name),
        }
    }
}

/// What [`main!`](crate::main) records of the application's own crate, which the shell cannot
/// read itself. Not for applications.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct __BuildInfo {
    pub version: &'static str,
    pub flavor: Option<&'static str>,
    pub build_name: Option<&'static str>,
    pub build_number: Option<&'static str>,
}

static BUILD: OnceLock<__BuildInfo> = OnceLock::new();

/// Called by [`main!`](crate::main) before the application is made.
#[doc(hidden)]
pub fn __register_build(info: __BuildInfo) {
    let _ = BUILD.set(info);
}

/// The flavor the application was built as: `frus run --flavor dev` gives `Some("dev")`.
/// `None` for a build without one, and in a test, which does not start through
/// [`main!`](crate::main).
///
/// ```
/// let api = match frus_shell::flavor() {
///     Some("dev") => "https://dev.example.com",
///     _ => "https://example.com",
/// };
/// # let _ = api;
/// ```
pub fn flavor() -> Option<&'static str> {
    BUILD
        .get()
        .and_then(|info| info.flavor.filter(|f| !f.is_empty()))
}

/// The application's version. `None` in a test, which does not start through
/// [`main!`](crate::main).
///
/// ```
/// if let Some(version) = frus_shell::app_version() {
///     frus_shell::log::info!("version {version}");
/// }
/// ```
pub fn app_version() -> Option<AppVersion> {
    BUILD.get().map(version_of)
}

fn version_of(info: &__BuildInfo) -> AppVersion {
    // `1.2.3+42`: semver's build metadata, the number when it is one.
    let (cargo_name, metadata) = match info.version.split_once('+') {
        Some((name, meta)) => (name, meta.parse().ok()),
        None => (info.version, None),
    };
    AppVersion {
        name: info
            .build_name
            .filter(|n| !n.is_empty())
            .unwrap_or(cargo_name),
        number: info
            .build_number
            .and_then(|n| n.trim().parse().ok())
            .or(metadata),
    }
}

/// A value given at build time with `--define KEY=VALUE` (or `--define-from-file`, or a
/// flavor's `defines`): `Option<&'static str>`, `None` when the build was not given it. With a
/// second argument, that is the value when none was given, and the result is a `&'static str`
/// that can be a constant.
///
/// The key is letters, digits and `_`. A build given another value compiles again by itself,
/// on every platform: desktop, Android and the web.
///
/// ```
/// const API_URL: &str = frus_shell::define!("API_URL", "https://example.com");
/// let analytics = frus_shell::define!("ANALYTICS_KEY"); // Option<&str>
/// # let _ = (API_URL, analytics);
/// ```
#[macro_export]
macro_rules! define {
    ($key:literal $(,)?) => {
        ::core::option_env!(::core::concat!("FRUS_DEFINE_", $key))
    };
    ($key:literal, $default:expr $(,)?) => {
        match ::core::option_env!(::core::concat!("FRUS_DEFINE_", $key)) {
            ::core::option::Option::Some(value) => value,
            ::core::option::Option::None => $default,
        }
    };
}

/// The application crate's part of [`__BuildInfo`], read where [`main!`](crate::main) expands:
/// in the application's crate, whose version it is.
#[doc(hidden)]
#[macro_export]
macro_rules! __build_info {
    () => {
        $crate::__BuildInfo {
            version: ::core::env!("CARGO_PKG_VERSION"),
            flavor: ::core::option_env!("FRUS_FLAVOR"),
            build_name: ::core::option_env!("FRUS_BUILD_NAME"),
            build_number: ::core::option_env!("FRUS_BUILD_NUMBER"),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mode_follows_the_mark_then_the_assertions() {
        assert_eq!(mode_of(true, None), BuildMode::Debug);
        assert_eq!(mode_of(false, None), BuildMode::Release);
        assert_eq!(mode_of(false, Some("profile")), BuildMode::Profile);
        assert_eq!(mode_of(true, Some("release")), BuildMode::Release);
        assert_eq!(mode_of(false, Some("other")), BuildMode::Release);
        assert_eq!(BuildMode::Profile.to_string(), "profile");
    }

    #[test]
    fn the_version_comes_from_the_flags_then_the_manifest() {
        let info = __BuildInfo {
            version: "0.3.1+7",
            flavor: None,
            build_name: None,
            build_number: None,
        };
        let plain = version_of(&info);
        assert_eq!(plain.name, "0.3.1");
        assert_eq!(plain.number, Some(7));
        assert_eq!(plain.to_string(), "0.3.1+7");
        let given = version_of(&__BuildInfo {
            build_name: Some("1.2.3"),
            build_number: Some("42"),
            ..info
        });
        assert_eq!(given.to_string(), "1.2.3+42");
        let bare = version_of(&__BuildInfo {
            version: "2.0.0",
            ..info
        });
        assert_eq!(bare.number, None);
        assert_eq!(bare.to_string(), "2.0.0");
    }

    #[test]
    fn a_define_is_a_build_constant() {
        // Nothing sets this one: absent, and its default where one is given.
        const MISSING: &str = crate::define!("FRUS_TEST_NEVER_SET", "fallback");
        assert_eq!(MISSING, "fallback");
        assert_eq!(crate::define!("FRUS_TEST_NEVER_SET"), None);
    }
}
