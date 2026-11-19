// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `messages!` on a folder of three locales, one of them partial: the typed functions, and the
//! string lookups beside them.

mod words {
    frus_l10n::messages!("i18n", default = "en", partial = ["de"]);
}

use frus_l10n::args;
use words::{localizer, messages, DEFAULT_LOCALE, LOCALES};

#[test]
fn every_message_is_a_function_with_typed_arguments() {
    let en = messages("en");
    assert_eq!(en.app_title(), "My Tasks");
    assert_eq!(en.hello("Ada"), "Hello, Ada!");
    assert_eq!(en.task_count(0), "No tasks");
    assert_eq!(en.task_count(1u8), "1 task");
    assert_eq!(en.task_count(2.0), "2 tasks");
    // A key that is a Rust keyword.
    assert_eq!(en.r#type(), "A keyword as a key");
    // A message that refers to others takes their arguments, in the order it meets them.
    assert_eq!(en.welcome("Ada", 3usize), "Hello, Ada! You have 3 tasks.");
}

#[test]
fn a_locale_is_negotiated_and_a_partial_one_falls_back() {
    assert_eq!(messages("fr").task_count(2), "2 tâches");
    assert_eq!(messages("fr-CA").app_title(), "Mes tâches");
    assert_eq!(messages("de").app_title(), "Meine Aufgaben");
    // `de` is partial: what it lacks is shown in the default locale.
    assert_eq!(messages("de").hello("Ada"), "Hello, Ada!");
    // A locale with no file is the default one.
    assert_eq!(messages("ja").app_title(), "My Tasks");
    assert_eq!(LOCALES, ["de", "en", "fr"]);
    assert_eq!(DEFAULT_LOCALE, "en");
}

#[test]
fn the_string_lookups_keep_working() {
    let l10n = localizer();
    let fr = l10n.langid("fr");
    assert_eq!(
        l10n.format_for(&fr, "hello", args![name: "Zoé"]),
        "Bonjour, Zoé !"
    );
    assert_eq!(l10n.get("app-title"), "My Tasks");
    assert_eq!(l10n.get("no-such-key"), "no-such-key");
}
