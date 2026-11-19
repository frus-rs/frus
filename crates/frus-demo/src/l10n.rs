// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Translation: the three languages the demo offers, and the lookups the views
//! call. One place, so that no screen has to know how Fluent is wired up.

/// The demo's words: `i18n/en.ftl`, `fr.ftl` and `ar.ftl`, read when the demo compiles. Each
/// message is a function of [`words::Messages`], its arguments typed: a message missing from one
/// file, or a translation naming an argument English does not give, does not compile.
pub(crate) mod words {
    frus_l10n::messages!("../i18n", default = "en");
}

/// The languages the demo offers (menu label, locale code). The last one, Arabic, is
/// **right-to-left**: selecting it also mirrors the layout (bidi + mirroring).
pub(crate) const LANGS: [(&str, &str); 3] =
    [("English", "en"), ("Français", "fr"), ("العربية", "ar")];

/// **The language in force**: the one the reader picked in this application, or — the
/// default — the one the framework resolved from the device.
///
/// Before milestone 454 there was no second half to this: the demonstration opened in
/// English on a French phone, because nothing carried the device's answer to it.
pub(crate) fn lang_index(picked: Option<usize>) -> usize {
    match picked {
        Some(index) => index,
        None => {
            let resolved = frus_widgets::locale::of();
            LANGS
                .iter()
                .position(|(_, tag)| *tag == resolved.language_code())
                .unwrap_or(0)
        }
    }
}

/// The label of the language action, which names the stop it switches **to**. The cycle
/// runs through the three languages and back to the device's own.
pub(crate) fn lang_label(picked: Option<usize>) -> &'static str {
    match picked {
        None => LANGS[0].0,
        Some(index) if index + 1 < LANGS.len() => LANGS[index + 1].0,
        Some(_) => "System",
    }
}

/// The next stop of that cycle.
pub(crate) fn next_lang(picked: Option<usize>) -> Option<usize> {
    match picked {
        None => Some(0),
        Some(index) if index + 1 < LANGS.len() => Some(index + 1),
        Some(_) => None,
    }
}

/// Is the language at index `lang` written right to left?
pub(crate) fn lang_is_rtl(lang: usize) -> bool {
    LANGS[lang].1 == "ar"
}

/// The demo's words in the language at index `lang`: `text_in(lang).app_title()`.
pub(crate) fn text_in(lang: usize) -> words::Messages {
    words::messages(LANGS[lang].1)
}
