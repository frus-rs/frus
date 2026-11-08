// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The secure store** (milestone 776): secrets — a session token, a password — kept by the
//! system, encrypted at rest and readable by this application only.
//!
//! ```no_run
//! # fn f() -> Result<(), frus_shell::SecureStoreError> {
//! let store = frus_shell::secure_store();
//! store.set("token", "eyJhbGciOi…")?;
//! if let Some(token) = store.get("token")? {
//!     // signed in
//! #   let _ = token;
//! }
//! store.delete("token")?;
//! # Ok(()) }
//! ```
//!
//! What each platform keeps them in:
//!
//! - **Windows**: the Credential Manager, a generic credential per key, named
//!   `frus.<program>/<key>` (`cmdkey /list` shows them). Protected by the person's Windows
//!   logon. A value is at most 2560 bytes.
//! - **macOS and iOS**: the Keychain, a generic password per key, the service being
//!   `frus.<program>`.
//! - **Linux**: the desktop's Secret Service (GNOME Keyring, KWallet) over D-Bus, an item per key
//!   in the default collection. A locked collection is unlocked first, which may show the
//!   desktop's own prompt. Without a Secret Service — a server, a bare window manager — the
//!   calls answer [`SecureStoreError::Platform`].
//! - **Android**: an AES-256-GCM key made in the Android Keystore, which never leaves it (in the
//!   phone's secure hardware where it has some); the ciphertext of each value is kept in the
//!   application's own folder. Uninstalling the application, or "Clear storage", removes both.
//! - **The web**: [`SecureStoreError::Unsupported`]. A page has nowhere to keep a secret that
//!   any script running in it cannot read; keep it on the server, in an `HttpOnly` cookie.
//!
//! The calls block while the system answers — on Linux, while the person answers an unlock
//! prompt — so a value read at the start belongs in `init`, and one written after a sign-in in
//! an `async` handler.

use std::fmt;

/// Why a secret could not be kept or read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecureStoreError {
    /// The platform has no secure storage: the web, where any script of the page could read it.
    Unsupported(&'static str),
    /// The system refused or failed: no Secret Service on this Linux desktop, a locked keyring
    /// whose prompt was dismissed, a value too large for the Credential Manager.
    Platform(String),
}

impl fmt::Display for SecureStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(why) => write!(f, "no secure storage here: {why}"),
            Self::Platform(why) => write!(f, "the secure storage failed: {why}"),
        }
    }
}

impl std::error::Error for SecureStoreError {}

/// The system's store for secrets: see [`secure_store`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SecureStore {
    _private: (),
}

/// The application's **secure store**: tokens and passwords, kept encrypted by the system.
///
/// ```no_run
/// # fn f() -> Result<(), frus_shell::SecureStoreError> {
/// frus_shell::secure_store().set("refresh_token", "r-123")?;
/// let token: Option<String> = frus_shell::secure_store().get("refresh_token")?;
/// # Ok(()) }
/// ```
pub fn secure_store() -> SecureStore {
    SecureStore::default()
}

impl SecureStore {
    /// Keeps `value` under `key`, replacing what was there.
    ///
    /// ```no_run
    /// # fn f() -> Result<(), frus_shell::SecureStoreError> {
    /// frus_shell::secure_store().set("password", "correct horse")?;
    /// # Ok(()) }
    /// ```
    pub fn set(&self, key: &str, value: &str) -> Result<(), SecureStoreError> {
        check_key(key)?;
        backend::set(&service(), key, value)
    }

    /// The value kept under `key`, or `None` when there is none.
    ///
    /// ```no_run
    /// # fn f() -> Result<(), frus_shell::SecureStoreError> {
    /// let token = frus_shell::secure_store().get("token")?;
    /// # Ok(()) }
    /// ```
    pub fn get(&self, key: &str) -> Result<Option<String>, SecureStoreError> {
        check_key(key)?;
        backend::get(&service(), key)
    }

    /// Removes the value kept under `key`; nothing kept there is not an error.
    ///
    /// ```no_run
    /// # fn f() -> Result<(), frus_shell::SecureStoreError> {
    /// frus_shell::secure_store().delete("token")?; // signed out
    /// # Ok(()) }
    /// ```
    pub fn delete(&self, key: &str) -> Result<(), SecureStoreError> {
        check_key(key)?;
        backend::delete(&service(), key)
    }
}

fn check_key(key: &str) -> Result<(), SecureStoreError> {
    if key.is_empty() {
        return Err(SecureStoreError::Platform("a key cannot be empty".into()));
    }
    Ok(())
}

/// Whom the secrets belong to: `frus.` and the program's name, as its folders are named. On
/// Android and the web the system keeps each application apart already.
fn service() -> String {
    #[cfg(any(desktop, ios))]
    {
        let program = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.file_stem()?.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "app".to_owned());
        service_for(&program)
    }
    #[cfg(not(any(desktop, ios)))]
    {
        "frus".to_owned()
    }
}

#[cfg_attr(not(any(desktop, ios, test)), allow(dead_code))]
fn service_for(program: &str) -> String {
    format!("frus.{program}")
}

/// The Credential Manager's name for `key`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn target_name(service: &str, key: &str) -> String {
    format!("{service}/{key}")
}

#[cfg(windows)]
mod backend {
    use super::{target_name, SecureStoreError};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, FILETIME};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    /// `CRED_MAX_CREDENTIAL_BLOB_SIZE`: five 512-byte pages.
    const MAX_BLOB: usize = 5 * 512;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn failed(what: &str) -> SecureStoreError {
        let code = unsafe { GetLastError() };
        SecureStoreError::Platform(format!("{what} failed (Windows error {code})"))
    }

    pub(super) fn set(service: &str, key: &str, value: &str) -> Result<(), SecureStoreError> {
        if value.len() > MAX_BLOB {
            return Err(SecureStoreError::Platform(format!(
                "the Credential Manager keeps at most {MAX_BLOB} bytes; this value has {}",
                value.len()
            )));
        }
        let mut target = wide(&target_name(service, key));
        let mut user = wide(key);
        let mut blob = value.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: std::ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(failed("CredWriteW"));
        }
        Ok(())
    }

    pub(super) fn get(service: &str, key: &str) -> Result<Option<String>, SecureStoreError> {
        let target = wide(&target_name(service, key));
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) } == 0 {
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(failed("CredReadW"));
        }
        let bytes = unsafe {
            let credential = &*found;
            let bytes = if credential.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(found as *const _);
            bytes
        };
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| SecureStoreError::Platform("the credential is not text".into()))
    }

    pub(super) fn delete(service: &str, key: &str) -> Result<(), SecureStoreError> {
        let target = wide(&target_name(service, key));
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(());
            }
            return Err(failed("CredDeleteW"));
        }
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod backend {
    use super::SecureStoreError;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    fn failed(err: security_framework::base::Error) -> SecureStoreError {
        SecureStoreError::Platform(format!("the Keychain refused it: {err}"))
    }

    pub(super) fn set(service: &str, key: &str, value: &str) -> Result<(), SecureStoreError> {
        set_generic_password(service, key, value.as_bytes()).map_err(failed)
    }

    pub(super) fn get(service: &str, key: &str) -> Result<Option<String>, SecureStoreError> {
        match get_generic_password(service, key) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| SecureStoreError::Platform("the item is not text".into())),
            Err(err) if err.code() == NOT_FOUND => Ok(None),
            Err(err) => Err(failed(err)),
        }
    }

    pub(super) fn delete(service: &str, key: &str) -> Result<(), SecureStoreError> {
        match delete_generic_password(service, key) {
            Err(err) if err.code() != NOT_FOUND => Err(failed(err)),
            _ => Ok(()),
        }
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
mod backend {
    use std::collections::HashMap;

    use super::SecureStoreError;
    use secret_service::blocking::SecretService;
    use secret_service::EncryptionType;

    fn failed(err: secret_service::Error) -> SecureStoreError {
        SecureStoreError::Platform(format!("the Secret Service: {err}"))
    }

    fn attributes<'a>(service: &'a str, key: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([("service", service), ("key", key), ("application", "frus")])
    }

    pub(super) fn set(service: &str, key: &str, value: &str) -> Result<(), SecureStoreError> {
        let secrets = SecretService::connect(EncryptionType::Dh).map_err(failed)?;
        let collection = secrets.get_default_collection().map_err(failed)?;
        collection.ensure_unlocked().map_err(failed)?;
        collection
            .create_item(
                &format!("{service} {key}"),
                attributes(service, key),
                value.as_bytes(),
                true,
                "text/plain",
            )
            .map_err(failed)?;
        Ok(())
    }

    pub(super) fn get(service: &str, key: &str) -> Result<Option<String>, SecureStoreError> {
        let secrets = SecretService::connect(EncryptionType::Dh).map_err(failed)?;
        let found = secrets
            .search_items(attributes(service, key))
            .map_err(failed)?;
        let Some(item) = found.unlocked.first().or(found.locked.first()) else {
            return Ok(None);
        };
        item.ensure_unlocked().map_err(failed)?;
        let bytes = item.get_secret().map_err(failed)?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| SecureStoreError::Platform("the item is not text".into()))
    }

    pub(super) fn delete(service: &str, key: &str) -> Result<(), SecureStoreError> {
        let secrets = SecretService::connect(EncryptionType::Dh).map_err(failed)?;
        let found = secrets
            .search_items(attributes(service, key))
            .map_err(failed)?;
        for item in found.unlocked.iter().chain(found.locked.iter()) {
            item.ensure_unlocked().map_err(failed)?;
            item.delete().map_err(failed)?;
        }
        Ok(())
    }
}

#[cfg(android)]
mod backend {
    pub(super) use crate::android_keystore::{delete, get, set};
}

#[cfg(web)]
mod backend {
    use super::SecureStoreError;

    const WHY: &str = "a browser has no storage that the page's other scripts cannot read; keep \
                       secrets on the server, in an HttpOnly cookie";

    pub(super) fn set(_: &str, _: &str, _: &str) -> Result<(), SecureStoreError> {
        Err(SecureStoreError::Unsupported(WHY))
    }

    pub(super) fn get(_: &str, _: &str) -> Result<Option<String>, SecureStoreError> {
        Err(SecureStoreError::Unsupported(WHY))
    }

    pub(super) fn delete(_: &str, _: &str) -> Result<(), SecureStoreError> {
        Err(SecureStoreError::Unsupported(WHY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_named_after_the_program() {
        assert_eq!(service_for("notes"), "frus.notes");
        assert_eq!(target_name("frus.notes", "token"), "frus.notes/token");
    }

    #[test]
    fn an_empty_key_is_refused() {
        assert!(matches!(
            secure_store().set("", "x"),
            Err(SecureStoreError::Platform(_))
        ));
    }

    /// The real store, round trip: set, read back, replace, delete, read nothing. Writes one
    /// credential under a name of its own and removes it.
    #[cfg(windows)]
    #[test]
    fn the_credential_manager_round_trips() {
        let store = secure_store();
        let key = format!("frus-test-{}", std::process::id());
        store.set(&key, "first").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("first"));
        store.set(&key, "second, longer — with ünïcode").unwrap();
        assert_eq!(
            store.get(&key).unwrap().as_deref(),
            Some("second, longer — with ünïcode")
        );
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
        store.delete(&key).unwrap();
        assert!(store.set(&key, &"x".repeat(2561)).is_err());
        assert_eq!(store.get(&key).unwrap(), None);
    }
}
