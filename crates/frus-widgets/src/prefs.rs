// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Preferences** (milestone 776): small settings that outlive the process — the theme the
//! person chose, whether the onboarding was seen, the last tab open.
//!
//! [`prefs`] is the application's one store. It is synchronous, and every value is anything
//! serde can write:
//!
//! ```
//! # frus_widgets::__use_memory_prefs();
//! use frus_widgets::prefs;
//!
//! prefs().set("onboarding_done", true).unwrap();
//! prefs().set("volume", 0.8).unwrap();
//! assert_eq!(prefs().get::<bool>("onboarding_done"), Some(true));
//! assert_eq!(prefs().get::<String>("missing"), None);
//! ```
//!
//! Where the values are kept is the framework's business, set up when the application starts:
//! a JSON file in the application's data folder on the desktop (written whole to a temporary
//! file, then renamed over the old one, so a crash never leaves half a file), the application's
//! own `files` folder on Android, the page's `localStorage` on the web.
//!
//! A component that shows a preference reads it with [`BuildContext::use_pref`], and is built
//! again whenever it changes, wherever the change was made.
//!
//! This is not the restoration of the interface's state ([`BuildContext::use_restorable_state`]),
//! which keeps what was on screen across a process the system ended and started again, and is
//! forgotten when the person closes the application. A preference stays until it is removed.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::component::BuildContext;

/// Every preference, by key, as JSON values. What a [`PrefsStorage`] loads and saves.
#[doc(hidden)]
pub type PrefsMap = BTreeMap<String, serde_json::Value>;

/// The change a [`PrefsStorage`] is asked to save, with the whole map it leads to.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum PrefsChange<'a> {
    /// `key` now holds this value.
    Set(&'a str, &'a serde_json::Value),
    /// `key` was removed.
    Remove(&'a str),
    /// Every key was removed.
    Clear,
}

/// Where the preferences are kept: a file, the page's `localStorage`. The platform layer
/// installs one when the application starts.
#[doc(hidden)]
pub trait PrefsStorage: Send + Sync {
    /// Every preference kept so far.
    fn load(&self) -> Result<PrefsMap, PrefsError>;
    /// Keeps `change`; `all` is every preference once it is made.
    fn save(&self, all: &PrefsMap, change: PrefsChange<'_>) -> Result<(), PrefsError>;
}

/// Why a preference could not be read or kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefsError {
    /// The value could not be written as JSON (a map whose keys are not strings, a float that
    /// is not a number).
    Encode(String),
    /// The storage refused it: the disk is full, the folder cannot be written, the browser's
    /// quota is reached.
    Storage(String),
    /// There is nowhere to keep it: the application has not started (preferences are set up by
    /// the framework's entry point), or the platform has no storage.
    Unavailable(&'static str),
}

impl fmt::Display for PrefsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(why) => write!(f, "the preference could not be written as JSON: {why}"),
            Self::Storage(why) => write!(f, "the preferences could not be kept: {why}"),
            Self::Unavailable(why) => write!(f, "no storage for the preferences: {why}"),
        }
    }
}

impl std::error::Error for PrefsError {}

/// The application's preferences: see [`prefs`].
pub struct Prefs {
    inner: Mutex<Inner>,
    /// Counts the changes, for whoever caches a value.
    generation: AtomicU64,
    /// Whether this is the application's store, whose changes rebuild the interface.
    global: bool,
}

struct Inner {
    storage: Option<Box<dyn PrefsStorage>>,
    /// What the storage held, read at the first use.
    cache: Option<PrefsMap>,
}

/// Kept in memory: the store of a test, which has no platform to keep it.
struct Memory;

impl PrefsStorage for Memory {
    fn load(&self) -> Result<PrefsMap, PrefsError> {
        Ok(PrefsMap::new())
    }
    fn save(&self, _: &PrefsMap, _: PrefsChange<'_>) -> Result<(), PrefsError> {
        Ok(())
    }
}

static GLOBAL: Prefs = Prefs::empty(true);

/// A preference changed since the shell last looked: the interface is built again.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// The application's **preferences**: settings that outlive the process, kept on the device.
///
/// ```
/// # frus_widgets::__use_memory_prefs();
/// use frus_widgets::prefs;
///
/// #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
/// enum ThemeChoice { System, Light, Dark }
///
/// prefs().set("theme", ThemeChoice::Dark).unwrap();
/// let theme = prefs().get("theme").unwrap_or(ThemeChoice::System);
/// assert_eq!(theme, ThemeChoice::Dark);
///
/// prefs().remove("theme").unwrap();
/// assert_eq!(prefs().get::<ThemeChoice>("theme"), None);
/// ```
pub fn prefs() -> &'static Prefs {
    &GLOBAL
}

/// Installs where the application's preferences are kept. Called by the platform layer when
/// the application starts; a later call replaces the storage and reads it again.
#[doc(hidden)]
pub fn __install_prefs_storage(storage: Box<dyn PrefsStorage>) {
    let mut inner = GLOBAL.lock();
    inner.storage = Some(storage);
    inner.cache = None;
}

/// Whether the application's preferences have somewhere to be kept yet.
#[doc(hidden)]
pub fn __prefs_storage_installed() -> bool {
    GLOBAL.lock().storage.is_some()
}

/// Keeps the application's preferences in memory, starting empty, unless a storage is already
/// installed: what a test without a platform uses.
#[doc(hidden)]
pub fn __use_memory_prefs() {
    let mut inner = GLOBAL.lock();
    if inner.storage.is_none() {
        inner.storage = Some(Box::new(Memory));
        inner.cache = None;
    }
}

/// Whether a preference changed since the last call, and forgets it. The shell reads this at
/// the top of every frame, beside [`take_rebuild_request`](crate::take_rebuild_request): a
/// change made on another thread rebuilds the interface too.
#[doc(hidden)]
pub fn __take_prefs_change() -> bool {
    CHANGED.swap(false, Ordering::AcqRel)
}

impl Prefs {
    const fn empty(global: bool) -> Self {
        Self {
            inner: Mutex::new(Inner {
                storage: None,
                cache: None,
            }),
            generation: AtomicU64::new(0),
            global,
        }
    }

    /// A store of its own, on `storage`: for the tests.
    #[cfg(test)]
    fn with_storage(storage: Box<dyn PrefsStorage>) -> Self {
        let prefs = Self::empty(false);
        prefs.lock().storage = Some(storage);
        prefs
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The value kept under `key`, read as a `T`; `None` when there is none, or when it is not
    /// a `T` (a value kept by an older version of the application, say).
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// # use frus_widgets::prefs;
    /// prefs().set("last_tab", 2).unwrap();
    /// assert_eq!(prefs().get::<usize>("last_tab"), Some(2));
    /// assert_eq!(prefs().get::<String>("last_tab"), None); // not a string
    /// ```
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let value = self.with_map(|map| map.get(key).cloned()).flatten()?;
        match serde_json::from_value(value) {
            Ok(value) => Some(value),
            Err(err) => {
                log::debug!("preference {key:?} is not of the type asked for: {err}");
                None
            }
        }
    }

    /// Whether a value is kept under `key`.
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// # use frus_widgets::prefs;
    /// prefs().set("seen_intro", true).unwrap();
    /// assert!(prefs().contains("seen_intro"));
    /// ```
    pub fn contains(&self, key: &str) -> bool {
        self.with_map(|map| map.contains_key(key)).unwrap_or(false)
    }

    /// Every key a value is kept under, in order.
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// # use frus_widgets::prefs;
    /// prefs().set("a.key", 1).unwrap();
    /// assert!(prefs().keys().contains(&"a.key".to_string()));
    /// ```
    pub fn keys(&self) -> Vec<String> {
        self.with_map(|map| map.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Keeps `value` under `key`, replacing what was there. It is on the device when this
    /// returns `Ok`; on an error nothing changed.
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// # use frus_widgets::prefs;
    /// prefs().set("username", "ada").unwrap();
    /// prefs().set("recent", vec!["a", "b"]).unwrap();
    /// ```
    pub fn set<T: Serialize>(&self, key: &str, value: T) -> Result<(), PrefsError> {
        let value =
            serde_json::to_value(value).map_err(|err| PrefsError::Encode(err.to_string()))?;
        self.change(|map| {
            if map.get(key) == Some(&value) {
                return None;
            }
            map.insert(key.to_owned(), value.clone());
            Some(PrefsChange::Set(key, &value))
        })
    }

    /// Removes what is kept under `key`; nothing kept there is not an error.
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// # use frus_widgets::prefs;
    /// prefs().remove("token_hint").unwrap();
    /// ```
    pub fn remove(&self, key: &str) -> Result<(), PrefsError> {
        self.change(|map| map.remove(key).map(|_| PrefsChange::Remove(key)))
    }

    /// Removes every preference.
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// # use frus_widgets::prefs;
    /// prefs().clear().unwrap();
    /// assert!(prefs().keys().is_empty());
    /// ```
    pub fn clear(&self) -> Result<(), PrefsError> {
        self.change(|map| {
            if map.is_empty() {
                return None;
            }
            map.clear();
            Some(PrefsChange::Clear)
        })
    }

    /// How many changes were made to this store: a number that grows with each one.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Reads the map, loading it the first time; `None` without a storage.
    fn with_map<R>(&self, read: impl FnOnce(&PrefsMap) -> R) -> Option<R> {
        let mut inner = self.lock();
        Self::loaded(&mut inner).ok().map(|map| read(map))
    }

    fn loaded(inner: &mut Inner) -> Result<&mut PrefsMap, PrefsError> {
        let Inner { storage, cache } = inner;
        let Some(storage) = storage else {
            return Err(PrefsError::Unavailable(
                "the application has not started: the preferences are set up by its entry point",
            ));
        };
        if cache.is_none() {
            let map = storage.load().unwrap_or_else(|err| {
                log::warn!("{err}; starting with none");
                PrefsMap::new()
            });
            *cache = Some(map);
        }
        Ok(cache.as_mut().expect("loaded above"))
    }

    /// Makes a change on a copy of the map, keeps the copy, and only then takes it: a change
    /// the storage refuses leaves everything as it was. `edit` answers `None` for no change.
    fn change<'c>(
        &self,
        edit: impl FnOnce(&mut PrefsMap) -> Option<PrefsChange<'c>>,
    ) -> Result<(), PrefsError> {
        {
            let mut inner = self.lock();
            let current = Self::loaded(&mut inner)?;
            let mut next = current.clone();
            let Some(change) = edit(&mut next) else {
                return Ok(());
            };
            let storage = inner.storage.as_ref().expect("checked by loaded");
            storage.save(&next, change)?;
            inner.cache = Some(next);
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        if self.global {
            CHANGED.store(true, Ordering::Release);
            crate::component::request_rebuild();
            crate::local_task::request_frame();
        }
        Ok(())
    }
}

impl fmt::Debug for Prefs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prefs").field("keys", &self.keys()).finish()
    }
}

/// A preference a component shows: see [`BuildContext::use_pref`].
#[derive(Clone, Debug)]
pub struct UsePref<T> {
    key: String,
    default: T,
}

impl<T: Clone + Serialize + DeserializeOwned> UsePref<T> {
    /// The value kept, or the default when there is none.
    pub fn get(&self) -> T {
        prefs()
            .get(&self.key)
            .unwrap_or_else(|| self.default.clone())
    }

    /// Keeps `value`; every component that shows this preference is built again.
    pub fn set(&self, value: T) -> Result<(), PrefsError> {
        prefs().set(&self.key, value)
    }

    /// Removes the value kept: [`get`](Self::get) answers the default again.
    pub fn remove(&self) -> Result<(), PrefsError> {
        prefs().remove(&self.key)
    }

    /// The preference's key.
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl BuildContext<'_> {
    /// A **preference** this component shows: the value kept under `key`, or `default`. Any
    /// change to it — from here, from another component, from a background task — builds the
    /// interface again, so what is shown is always what is kept.
    ///
    /// ```
    /// # frus_widgets::__use_memory_prefs();
    /// use frus_widgets::{BuildContext, Switch, Widget};
    ///
    /// fn dark_mode(cx: &BuildContext) -> Box<dyn Widget> {
    ///     let dark = cx.use_pref("dark_mode", false);
    ///     let toggle = dark.clone();
    ///     Box::new(Switch::new(dark.get()).on_toggle(move |on| {
    ///         let _ = toggle.set(on);
    ///     }))
    /// }
    /// ```
    pub fn use_pref<T>(&self, key: &str, default: T) -> UsePref<T>
    where
        T: Clone + Serialize + DeserializeOwned + 'static,
    {
        UsePref {
            key: key.to_owned(),
            default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Keeps what it is given, and can be told to refuse.
    #[derive(Clone, Default)]
    struct Recording {
        saved: Arc<Mutex<PrefsMap>>,
        refuse: Arc<AtomicBool>,
        saves: Arc<AtomicU64>,
    }

    impl PrefsStorage for Recording {
        fn load(&self) -> Result<PrefsMap, PrefsError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        fn save(&self, all: &PrefsMap, _: PrefsChange<'_>) -> Result<(), PrefsError> {
            if self.refuse.load(Ordering::SeqCst) {
                return Err(PrefsError::Storage("disk full".into()));
            }
            self.saves.fetch_add(1, Ordering::SeqCst);
            *self.saved.lock().unwrap() = all.clone();
            Ok(())
        }
    }

    #[test]
    fn values_of_any_serde_type_round_trip() {
        let prefs = Prefs::with_storage(Box::new(Recording::default()));
        prefs.set("flag", true).unwrap();
        prefs.set("count", 3u32).unwrap();
        prefs.set("name", "ada").unwrap();
        prefs.set("list", vec![1, 2, 3]).unwrap();
        assert_eq!(prefs.get::<bool>("flag"), Some(true));
        assert_eq!(prefs.get::<u32>("count"), Some(3));
        assert_eq!(prefs.get::<String>("name"), Some("ada".into()));
        assert_eq!(prefs.get::<Vec<i32>>("list"), Some(vec![1, 2, 3]));
        assert_eq!(prefs.get::<bool>("missing"), None);
        // Another type than the one kept: none, not a panic.
        assert_eq!(prefs.get::<bool>("name"), None);
        assert_eq!(prefs.keys(), vec!["count", "flag", "list", "name"]);
    }

    /// A second store on the same storage reads what the first kept: the process started again.
    #[test]
    fn a_new_store_reads_what_was_kept() {
        let storage = Recording::default();
        let first = Prefs::with_storage(Box::new(storage.clone()));
        first.set("theme", "dark").unwrap();
        first.set("gone", 1).unwrap();
        first.remove("gone").unwrap();
        let again = Prefs::with_storage(Box::new(storage));
        assert_eq!(again.get::<String>("theme"), Some("dark".into()));
        assert!(!again.contains("gone"));
        again.clear().unwrap();
        assert!(again.keys().is_empty());
    }

    /// A refused write changes nothing, and says so.
    #[test]
    fn a_refused_write_changes_nothing() {
        let storage = Recording::default();
        let prefs = Prefs::with_storage(Box::new(storage.clone()));
        prefs.set("kept", 1).unwrap();
        storage.refuse.store(true, Ordering::SeqCst);
        let before = prefs.generation();
        assert_eq!(
            prefs.set("kept", 2),
            Err(PrefsError::Storage("disk full".into()))
        );
        assert_eq!(prefs.get::<i32>("kept"), Some(1));
        assert!(prefs.remove("kept").is_err());
        assert!(prefs.contains("kept"));
        assert_eq!(prefs.generation(), before);
    }

    /// Setting what is already kept writes nothing and changes nothing.
    #[test]
    fn the_same_value_is_not_written_again() {
        let storage = Recording::default();
        let prefs = Prefs::with_storage(Box::new(storage.clone()));
        prefs.set("x", 1).unwrap();
        let generation = prefs.generation();
        prefs.set("x", 1).unwrap();
        prefs.remove("absent").unwrap();
        assert_eq!(storage.saves.load(Ordering::SeqCst), 1);
        assert_eq!(prefs.generation(), generation);
        prefs.set("x", 2).unwrap();
        assert_eq!(prefs.generation(), generation + 1);
    }

    #[test]
    fn a_value_json_cannot_hold_is_refused() {
        let prefs = Prefs::with_storage(Box::new(Recording::default()));
        let mut odd = BTreeMap::new();
        odd.insert(vec![1u8], 1);
        assert!(matches!(prefs.set("odd", odd), Err(PrefsError::Encode(_))));
        assert!(!prefs.contains("odd"));
    }

    /// Without a storage nothing pretends to be kept.
    #[test]
    fn without_storage_nothing_is_kept() {
        let prefs = Prefs::empty(false);
        assert!(matches!(prefs.set("x", 1), Err(PrefsError::Unavailable(_))));
        assert_eq!(prefs.get::<i32>("x"), None);
        assert!(prefs.keys().is_empty());
    }

    /// A storage that cannot be read starts empty rather than failing every read.
    #[test]
    fn an_unreadable_storage_starts_empty() {
        struct Broken;
        impl PrefsStorage for Broken {
            fn load(&self) -> Result<PrefsMap, PrefsError> {
                Err(PrefsError::Storage("unreadable".into()))
            }
            fn save(&self, _: &PrefsMap, _: PrefsChange<'_>) -> Result<(), PrefsError> {
                Ok(())
            }
        }
        let prefs = Prefs::with_storage(Box::new(Broken));
        assert_eq!(prefs.get::<i32>("x"), None);
        prefs.set("x", 1).unwrap();
        assert_eq!(prefs.get::<i32>("x"), Some(1));
    }

    /// The application's store asks for a rebuild on a change, from any thread, and the hook
    /// reads the value kept or its default.
    #[test]
    fn a_change_to_the_global_store_rebuilds_the_interface() {
        __use_memory_prefs();
        let key = "prefs.tests.rebuild";
        let _ = __take_prefs_change();
        std::thread::spawn(move || prefs().set(key, 5).unwrap())
            .join()
            .unwrap();
        assert!(__take_prefs_change(), "a change on another thread is seen");
        assert!(!__take_prefs_change(), "and taken once");

        let pref = UsePref {
            key: key.to_owned(),
            default: 0,
        };
        assert_eq!(pref.get(), 5);
        pref.remove().unwrap();
        assert_eq!(pref.get(), 0, "the default once removed");
        assert!(__take_prefs_change());
    }
}
