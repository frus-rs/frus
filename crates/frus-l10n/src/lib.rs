// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-l10n` — **localisation** for frus applications, built on
//! [Fluent](https://projectfluent.org/), Mozilla's i18n standard.
//!
//! Nothing is reinvented here: messages, plurals and selections live in `.ftl`
//! resources, embedded by the application through `include_str!`, and resolved per
//! locale with a **fallback** chain (`fluent-langneg` negotiation).
//!
//! ```
//! use frus_l10n::{Localizer, args};
//! let mut l10n = Localizer::new("en");
//! l10n.add("en", "hello = Hello, { $name }!\ntasks = { $n ->\n    [one] { $n } task\n   *[other] { $n } tasks\n }");
//! l10n.add("fr", "hello = Bonjour, { $name } !\ntasks = { $n ->\n    [one] { $n } tâche\n   *[other] { $n } tâches\n }");
//!
//! assert_eq!(l10n.format("hello", args![name: "Ada"]), "Hello, Ada!");
//! l10n.set_locale("fr");
//! assert_eq!(l10n.format("tasks", args![n: 2]), "2 tâches");
//! // Falls back to the default locale when the key is missing from the current one.
//! ```
//!
//! ## Typed messages
//!
//! [`messages!`] reads the application's `.ftl` files **when it compiles** and writes one
//! function per message, its arguments typed: a message missing from a locale, or a
//! translation using an argument the default locale does not give, does not compile.
//!
//! ```ignore
//! mod words {
//!     // i18n/en.ftl, i18n/fr.ftl, ... beside `src/`; the path is relative to this file.
//!     frus_l10n::messages!("../i18n", default = "en");
//! }
//!
//! let fr = words::messages("fr");
//! let title = fr.app_title(); // app-title = Mes tâches
//! let count = fr.task_count(3); // a plural selector's argument is a number
//! let hello = fr.hello("Ada"); // any other argument is text
//! ```

#![deny(missing_docs)]

// The README's example is compiled and run by `cargo test --doc`, so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

use std::collections::HashMap;

// The **concurrent** bundle — its memoiser sits behind a `Mutex` — is what makes
// `Localizer` `Send + Sync`, so an application can hold it in a `static`/`OnceLock`.
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// An argument value passed to a message: a string or a number.
#[derive(Clone, Debug)]
pub enum Arg<'a> {
    /// Text.
    Str(&'a str),
    /// A number, which takes part in the CLDR plural rules.
    Num(f64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(s: &'a str) -> Self {
        Arg::Str(s)
    }
}
impl From<i64> for Arg<'_> {
    fn from(n: i64) -> Self {
        Arg::Num(n as f64)
    }
}
impl From<i32> for Arg<'_> {
    fn from(n: i32) -> Self {
        Arg::Num(n as f64)
    }
}
impl From<usize> for Arg<'_> {
    fn from(n: usize) -> Self {
        Arg::Num(n as f64)
    }
}
impl From<f64> for Arg<'_> {
    fn from(n: f64) -> Self {
        Arg::Num(n)
    }
}

/// A number given to a message: any of Rust's integer and float types. It takes part in the
/// CLDR plural rules.
pub trait Number {
    /// The number as Fluent sees it.
    fn to_f64(self) -> f64;
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl Number for $t {
            fn to_f64(self) -> f64 {
                self as f64
            }
        }
    )*};
}
numbers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);

/// The application's messages, read from its Fluent files **when it compiles**: one function
/// per message, its arguments typed.
///
/// ```ignore
/// mod words {
///     frus_l10n::messages!("../i18n", default = "en", partial = ["de"]);
/// }
/// assert_eq!(words::messages("fr").task_count(2), "2 tâches");
/// ```
///
/// The folder, relative to the file the macro is written in, holds one file per locale named
/// after it: `en.ftl`, `fr.ftl`, `pt-BR.ftl`. The macro writes, where it is called:
///
/// - `messages(locale) -> Messages`: the messages in the closest locale there is a file for
///   (`fr-CA` gets `fr`), the default one when there is none;
/// - `Messages`, with one method per message, named after it (`task-count` is
///   `task_count`). An argument the message uses as a **plural selector**, or passes to
///   `NUMBER()`, takes any [`Number`]; any other argument takes a `&str`. The arguments come in
///   the order the default locale's message first uses them;
/// - `localizer() -> &'static Localizer`, holding every file, for the string lookups
///   ([`Localizer::format_for`] and the others), which keep working;
/// - `LOCALES` and `DEFAULT_LOCALE`.
///
/// **The rules**, each a compile error naming the message, the locale and its file:
///
/// - the default locale (`default`, `"en"` when not given) defines the messages and their
///   arguments;
/// - every other locale has every message of the default locale, unless it is listed in
///   `partial`: a message a partial locale lacks is shown in the default locale;
/// - no locale has a message the default locale lacks;
/// - a translation uses no argument the default locale's message does not give it;
/// - each file is valid Fluent.
///
/// The files are embedded: there is nothing to load at run time, and changing one compiles the
/// application again. A file added to the folder is seen on the next build of the crate.
#[macro_export]
macro_rules! messages {
    ($($input:tt)*) => {
        $crate::__private::messages! { $crate; $($input)* }
    };
}

#[doc(hidden)]
pub mod __private {
    pub use frus_macros::messages;
    pub use unic_langid::LanguageIdentifier;
}

/// Builds a message's argument list: `args![name: "Ada", n: 3]`.
#[macro_export]
macro_rules! args {
    ($($key:ident : $value:expr),* $(,)?) => {
        &[$((stringify!($key), $crate::Arg::from($value))),*]
    };
    () => { &[] };
}

/// The **localiser**: Fluent bundles per locale, a current locale and a default
/// locale, the latter being the last resort.
pub struct Localizer {
    bundles: HashMap<LanguageIdentifier, FluentBundle<FluentResource>>,
    default: LanguageIdentifier,
    current: LanguageIdentifier,
}

impl Localizer {
    /// Creates a localiser whose default — and current — locale is `default`, for
    /// instance `"en"` or `"fr-FR"`. A malformed locale falls back to `und`.
    pub fn new(default: &str) -> Self {
        let default: LanguageIdentifier = default.parse().unwrap_or_default();
        Self {
            bundles: HashMap::new(),
            current: default.clone(),
            default,
        }
    }

    /// Adds to, or extends, a locale's messages from an `.ftl` source. Syntax errors
    /// are reported through `debug_assert` but do not stop the rest of the file from
    /// loading.
    pub fn add(&mut self, locale: &str, ftl: &str) {
        let langid: LanguageIdentifier = match locale.parse() {
            Ok(id) => id,
            Err(_) => return,
        };
        let resource = match FluentResource::try_new(ftl.to_owned()) {
            Ok(res) => res,
            Err((res, _errors)) => {
                debug_assert!(false, "Fluent syntax errors in \"{locale}\"");
                res
            }
        };
        let bundle = self
            .bundles
            .entry(langid.clone())
            .or_insert_with(|| new_bundle(langid));
        // An existing key is not overwritten: add_resource skips duplicates and
        // reports an error, which we deliberately ignore.
        let _ = bundle.add_resource(resource);
    }

    /// The available locales — those for which messages have been loaded.
    pub fn available(&self) -> Vec<LanguageIdentifier> {
        self.bundles.keys().cloned().collect()
    }

    /// Sets the current locale. The best match among the available locales is
    /// **negotiated**: an exact match first (`fr-FR`), then by language (`fr-CA` →
    /// `fr`), and failing that the default locale. Returns the locale actually
    /// chosen.
    pub fn set_locale(&mut self, locale: &str) -> LanguageIdentifier {
        self.current = self.negotiate(locale);
        self.current.clone()
    }

    /// The locale [`Localizer::set_locale`] would choose for `locale`, without choosing it: an
    /// exact match, then the same language (`fr-CA` → `fr`), then the default locale.
    pub fn negotiate(&self, locale: &str) -> LanguageIdentifier {
        let Ok(requested) = locale.parse::<LanguageIdentifier>() else {
            return self.default.clone();
        };
        // 1) An exact match.
        let exact = self.bundles.keys().find(|id| **id == requested).cloned();
        // 2) The same language, ignoring region and variant. The keys are sorted first, so
        // the answer does not depend on the hash map's order.
        let by_lang = || {
            let mut same: Vec<&LanguageIdentifier> = self
                .bundles
                .keys()
                .filter(|id| id.language == requested.language)
                .collect();
            same.sort_by_key(|id| id.to_string());
            same.first().map(|id| (*id).clone())
        };
        exact
            .or_else(by_lang)
            .unwrap_or_else(|| self.default.clone())
    }

    /// The current locale.
    pub fn locale(&self) -> &LanguageIdentifier {
        &self.current
    }

    /// Resolves a message **with no arguments**. The fallback chain is current
    /// locale → default → the key itself: never a panic, never a surprise empty string.
    pub fn get(&self, key: &str) -> String {
        self.format(key, &[])
    }

    /// Resolves a message **with arguments** (`args![…]`) in the **current locale**,
    /// with the same fallback chain as [`Localizer::get`].
    pub fn format(&self, key: &str, args: &[(&str, Arg)]) -> String {
        self.format_for(&self.current, key, args)
    }

    /// Resolves a message in an **explicit locale**, without changing the current
    /// one — handy for a pure `view` that receives the language as a parameter. The
    /// fallback chain is `locale` → the default locale → the raw key.
    pub fn format_for(
        &self,
        locale: &LanguageIdentifier,
        key: &str,
        args: &[(&str, Arg)],
    ) -> String {
        for langid in [locale, &self.default] {
            if let Some(bundle) = self.bundles.get(langid) {
                if let Some(text) = format_in(bundle, key, args) {
                    return text;
                }
            }
        }
        // Last resort: the raw key, which makes the gap visible without breaking the UI.
        key.to_owned()
    }

    /// Parses a locale tag (`"fr"`, `"fr-FR"`), or returns the default locale when
    /// it is malformed — a utility for callers of [`Localizer::format_for`].
    pub fn langid(&self, locale: &str) -> LanguageIdentifier {
        locale.parse().unwrap_or_else(|_| self.default.clone())
    }
}

/// Builds a bundle for a locale, without **bidi isolation** — no FSI/PDI marks
/// around arguments — which keeps the output readable and testable. RTL direction
/// is handled at the layout level; see `Theme::direction`.
fn new_bundle(langid: LanguageIdentifier) -> FluentBundle<FluentResource> {
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    bundle.set_use_isolating(false);
    bundle
}

/// Formats `key` in `bundle` when the message exists, `None` otherwise.
fn format_in(
    bundle: &FluentBundle<FluentResource>,
    key: &str,
    args: &[(&str, Arg)],
) -> Option<String> {
    let message = bundle.get_message(key)?;
    let pattern = message.value()?;
    let mut fargs = FluentArgs::new();
    for (name, value) in args {
        let v = match value {
            Arg::Str(s) => FluentValue::from(*s),
            Arg::Num(n) => FluentValue::from(*n),
        };
        fargs.set(*name, v);
    }
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, Some(&fargs), &mut errors);
    Some(text.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Localizer {
        let mut l = Localizer::new("en");
        l.add(
            "en",
            "hello = Hello, { $name }!\n\
             tasks = { $n ->\n    [one] { $n } task\n   *[other] { $n } tasks\n }\n\
             only-en = English only",
        );
        l.add(
            "fr",
            "hello = Bonjour, { $name } !\n\
             tasks = { $n ->\n    [one] { $n } tâche\n   *[other] { $n } tâches\n }",
        );
        l
    }

    #[test]
    fn resolves_arguments() {
        let l = sample();
        assert_eq!(l.format("hello", args![name: "Ada"]), "Hello, Ada!");
    }

    #[test]
    fn plural_rules_per_locale() {
        let mut l = sample();
        assert_eq!(l.format("tasks", args![n: 1]), "1 task");
        assert_eq!(l.format("tasks", args![n: 5]), "5 tasks");
        l.set_locale("fr");
        assert_eq!(l.format("tasks", args![n: 1]), "1 tâche");
        assert_eq!(l.format("tasks", args![n: 3]), "3 tâches");
    }

    #[test]
    fn negotiates_region_to_base_language() {
        let mut l = sample();
        // fr-CA does not exist, so it is negotiated down to fr.
        let got = l.set_locale("fr-CA");
        assert_eq!(got.to_string(), "fr");
        assert_eq!(l.format("hello", args![name: "Zoé"]), "Bonjour, Zoé !");
    }

    #[test]
    fn falls_back_to_default_then_key() {
        let mut l = sample();
        l.set_locale("fr");
        // "only-en" exists only in English, so it falls back to the default locale.
        assert_eq!(l.get("only-en"), "English only");
        // A key that exists nowhere yields the raw key.
        assert_eq!(l.get("missing-key"), "missing-key");
    }

    #[test]
    fn unknown_locale_stays_on_default() {
        let mut l = sample();
        let got = l.set_locale("xx");
        assert_eq!(got.to_string(), "en");
    }
}
