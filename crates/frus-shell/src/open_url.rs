// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Opening an address in the system's handler** (milestone 775): a web page in the browser,
//! `mailto:` in the mail application, `tel:` in the dialer, `sms:` in the messages application,
//! and any other scheme in whatever application claims it.
//!
//! ```no_run
//! match frus_shell::open_url("https://frus.dev") {
//!     Ok(()) => {}
//!     Err(frus_shell::OpenUrlError::NoHandler) => { /* say that nothing here opens it */ }
//!     Err(error) => log::warn!("the page did not open: {error}"),
//! }
//! ```
//!
//! What each platform does:
//!
//! - **Windows**: `ShellExecute`, the same as typing the address in the Run box.
//! - **macOS**: `open`, which hands the address to Launch Services.
//! - **Linux**: `xdg-open`, which asks the desktop. It is not waited for — on some desktops it
//!   lasts as long as the browser it started — so an address nothing opens is not reported.
//! - **Android**: an `Intent.ACTION_VIEW` with `FLAG_ACTIVITY_NEW_TASK`, from the activity. An
//!   `https` address may open an installed application that claims it instead of the browser,
//!   as on any Android application. An address the application itself claims opens a second
//!   activity of it, which hands the address to the running one.
//! - **The web**: `http` and `https` in a new tab; any other scheme through the page's
//!   location, which the browser hands to the system without leaving the page.
//! - **iOS**: not yet; [`OpenUrlError::Unsupported`].
//!
//! `javascript:` addresses are refused everywhere: they run code rather than open anything.

use std::fmt;

/// Why [`open_url`] did not open an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenUrlError {
    /// Not an address that can be opened: no scheme, characters an address cannot hold, or a
    /// `javascript:` address.
    InvalidUrl,
    /// Nothing on this device opens addresses of this kind.
    NoHandler,
    /// This platform cannot open addresses.
    Unsupported,
    /// The system refused, for the reason given.
    Failed(String),
}

impl fmt::Display for OpenUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenUrlError::InvalidUrl => f.write_str("not an address that can be opened"),
            OpenUrlError::NoHandler => f.write_str("nothing on this device opens this address"),
            OpenUrlError::Unsupported => f.write_str("this platform cannot open addresses"),
            OpenUrlError::Failed(why) => write!(f, "the address could not be opened: {why}"),
        }
    }
}

impl std::error::Error for OpenUrlError {}

/// Opens `url` in the system's handler for its scheme: the browser for `https://`, the mail
/// application for `mailto:`, the dialer for `tel:`, the messages application for `sms:`.
///
/// Returns once the system has been asked; the page itself opens on its own time.
///
/// ```no_run
/// # fn f() -> Result<(), frus_shell::OpenUrlError> {
/// frus_shell::open_url("https://frus.dev")?;
/// frus_shell::open_url("mailto:hello@example.com?subject=Hi")?;
/// frus_shell::open_url("tel:+15551234567")?;
/// # Ok(())
/// # }
/// ```
pub fn open_url(url: &str) -> Result<(), OpenUrlError> {
    let (scheme, url) = checked(url)?;
    open_checked(&scheme, url)
}

/// The address, trimmed, and its scheme in lower case — or why it cannot be opened.
pub(crate) fn checked(url: &str) -> Result<(String, &str), OpenUrlError> {
    let url = url.trim();
    let scheme = scheme_of(url).ok_or(OpenUrlError::InvalidUrl)?;
    if url.chars().any(char::is_control) || url.len() == scheme.len() + 1 {
        return Err(OpenUrlError::InvalidUrl);
    }
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "javascript" {
        return Err(OpenUrlError::InvalidUrl);
    }
    Ok((scheme, url))
}

/// The scheme of `url` (`https` for `https://…`), if it starts with one.
///
/// A scheme starts with a letter and holds letters, digits, `+`, `-` and `.`. **One letter is
/// not taken for a scheme**: `C:\Users` is a Windows path, and handing a path to the system's
/// handler would run what it names.
pub(crate) fn scheme_of(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let well_formed = scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    well_formed.then_some(scheme)
}

/// Whether the browser opens this scheme in a page of its own, rather than handing it on.
#[cfg_attr(not(web), allow(dead_code))]
pub(crate) fn is_web(scheme: &str) -> bool {
    matches!(scheme, "http" | "https")
}

#[cfg(windows)]
fn open_checked(_scheme: &str, url: &str) -> Result<(), OpenUrlError> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// `SE_ERR_NOASSOC`: no application is associated with the scheme.
    const NO_ASSOCIATION: isize = 31;

    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (verb, target) = (wide("open"), wide(url));
    // SAFETY: both strings are NUL-terminated and outlive the call; no window is named.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    match result {
        code if code > 32 => Ok(()),
        NO_ASSOCIATION => Err(OpenUrlError::NoHandler),
        code => Err(OpenUrlError::Failed(format!(
            "ShellExecute answered {code}"
        ))),
    }
}

#[cfg(target_os = "macos")]
fn open_checked(_scheme: &str, url: &str) -> Result<(), OpenUrlError> {
    // `open` returns at once, with a failure when no application claims the address.
    let output = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| OpenUrlError::Failed(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        log::debug!(
            "open: {}",
            String::from_utf8_lossy(&output.stderr).trim_end()
        );
        Err(OpenUrlError::NoHandler)
    }
}

#[cfg(all(desktop, not(any(windows, target_os = "macos"))))]
fn open_checked(_scheme: &str, url: &str) -> Result<(), OpenUrlError> {
    use std::process::{Command, Stdio};
    let child = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            // Reaped where waiting costs nothing: `xdg-open` may last as long as the browser.
            std::thread::spawn(move || {
                if let Ok(status) = child.wait() {
                    if !status.success() {
                        log::warn!("xdg-open could not open the address ({status})");
                    }
                }
            });
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log::warn!("xdg-open is not installed: addresses cannot be opened");
            Err(OpenUrlError::Unsupported)
        }
        Err(error) => Err(OpenUrlError::Failed(error.to_string())),
    }
}

#[cfg(android)]
fn open_checked(_scheme: &str, url: &str) -> Result<(), OpenUrlError> {
    crate::android_share::open_url(url)
}

#[cfg(web)]
fn open_checked(scheme: &str, url: &str) -> Result<(), OpenUrlError> {
    let window = web_sys::window().ok_or(OpenUrlError::Unsupported)?;
    if is_web(scheme) {
        // A new tab, which the browser allows from a press or a key; anywhere else it may block
        // it, and says so with no window.
        match window.open_with_url_and_target(url, "_blank") {
            Ok(Some(tab)) => {
                // The page opened must not reach back into this one.
                let _ = js_sys::Reflect::set(&tab, &"opener".into(), &wasm_bindgen::JsValue::NULL);
                Ok(())
            }
            Ok(None) => Err(OpenUrlError::Failed(
                "the browser blocked the new tab".to_string(),
            )),
            Err(error) => Err(OpenUrlError::Failed(format!("{error:?}"))),
        }
    } else {
        // `mailto:`, `tel:` and the like: the browser hands them to the system and the page
        // stays where it is.
        window
            .location()
            .assign(url)
            .map_err(|error| OpenUrlError::Failed(format!("{error:?}")))
    }
}

#[cfg(ios)]
fn open_checked(_scheme: &str, _url: &str) -> Result<(), OpenUrlError> {
    Err(OpenUrlError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_with_a_scheme_is_accepted_and_trimmed() {
        assert_eq!(
            checked("  https://frus.dev/a?b=1 "),
            Ok(("https".to_string(), "https://frus.dev/a?b=1"))
        );
        assert_eq!(
            checked("mailto:hello@example.com"),
            Ok(("mailto".to_string(), "mailto:hello@example.com"))
        );
        assert_eq!(
            checked("tel:+15551234567"),
            Ok(("tel".to_string(), "tel:+15551234567"))
        );
        assert_eq!(
            checked("HTTPS://frus.dev"),
            Ok(("https".to_string(), "HTTPS://frus.dev")),
            "a scheme's case does not matter"
        );
    }

    #[test]
    fn what_is_not_an_address_is_refused() {
        for url in ["", "frus.dev", "/orders/42", "https:", "42:x", "a b:c"] {
            assert_eq!(checked(url), Err(OpenUrlError::InvalidUrl), "{url:?}");
        }
    }

    /// A one-letter scheme is a drive: the system's handler would run the program a path names.
    #[test]
    fn a_windows_path_is_not_an_address() {
        assert_eq!(
            checked(r"C:\Windows\System32\calc.exe"),
            Err(OpenUrlError::InvalidUrl)
        );
        assert_eq!(checked("c:/x"), Err(OpenUrlError::InvalidUrl));
    }

    #[test]
    fn javascript_and_control_characters_are_refused() {
        assert_eq!(
            checked("javascript:alert(1)"),
            Err(OpenUrlError::InvalidUrl)
        );
        assert_eq!(
            checked("JavaScript:alert(1)"),
            Err(OpenUrlError::InvalidUrl)
        );
        assert_eq!(
            checked("https://a.example/\nother"),
            Err(OpenUrlError::InvalidUrl)
        );
    }

    #[test]
    fn only_http_and_https_open_in_the_browser_itself() {
        assert!(is_web("https"));
        assert!(is_web("http"));
        assert!(!is_web("mailto"));
        assert!(!is_web("tel"));
    }

    #[test]
    fn the_errors_say_what_happened() {
        assert_eq!(
            OpenUrlError::NoHandler.to_string(),
            "nothing on this device opens this address"
        );
        assert_eq!(
            OpenUrlError::Failed("refused".into()).to_string(),
            "the address could not be opened: refused"
        );
    }
}
