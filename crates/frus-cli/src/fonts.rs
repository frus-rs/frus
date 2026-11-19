// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A release build carries its fonts cut down** (milestone 823): `frus build` keeps, of every
//! font embedded with `include_font!`, the characters the application can show, and nothing else.
//!
//! The bundled faces are about 3.4 MB, some 40% of a small application, and cover far more of
//! Unicode than one application draws. What it draws cannot be read from its sources alone: a name
//! typed by the reader, a title from a server, a translation loaded at run time are in none of
//! them. So what is kept is decided by **languages**, not by strings:
//!
//! - **Latin**, whole: every language written in it, Vietnamese included, with the punctuation,
//!   the currency signs and the replacement character.
//! - **The scripts of the application's languages**: those listed under
//!   `[package.metadata.frus.fonts] locales`, those its code names (`Locale::new("ar")`), and the
//!   languages the widgets speak by themselves (English, French, German, Spanish, Japanese, Thai,
//!   Vietnamese).
//! - **The symbols the widgets draw**: the minus of a stepper, the arrows, the check mark (see
//!   [`FRAMEWORK_SYMBOLS`]; a test keeps it in step with the widgets' sources).
//! - **Every character written in the application's sources**: its string and character
//!   literals, an icon font's code points included.
//! - **What the manifest adds**: `ranges` and `text`.
//!
//! A character outside all of that is drawn as the font's box, and the application's log names it
//! once. A font that must stay whole is listed under `keep`; `subset = false`, or
//! `frus build --no-font-subset`, keeps them all. A debug build never cuts anything.
//!
//! How the cut copy reaches the binary: `include_font!` is a macro that embeds the file at its
//! path, unless `FRUS_FONT_SUBSET_DIR` holds a cut copy of it, named by [`subset_name`]. This
//! module writes the copies and sets the variable for the cargo it starts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use read_fonts::collections::IntSet;
use read_fonts::types::{GlyphId, NameId, Tag};
use read_fonts::FontRef;

use crate::cargo::Mode;
use crate::json::{parse, Value};
use crate::project::{string_list, string_value, Project};

/// The variable `include_font!` reads: the directory of the cut copies.
pub(crate) const SUBSET_DIR_VAR: &str = "FRUS_FONT_SUBSET_DIR";

/// The manifest's section.
pub(crate) const SECTION: &str = "package.metadata.frus.fonts";

/// The file a build touches in the directory of the copies it embeds.
const LAST_USED: &str = "last-used";

/// Latin, whole, and the characters any text uses: what every cut keeps.
const BASE: &[(u32, u32)] = &[
    (0x0020, 0x007E), // Basic Latin
    (0x00A0, 0x024F), // Latin-1, Latin Extended-A and -B
    (0x02B0, 0x02FF), // spacing modifier letters: ʻ, ˆ, ˇ, ˜
    (0x0300, 0x036F), // combining marks: accents typed apart, Vietnamese
    (0x1E00, 0x1EFF), // Latin Extended Additional: Vietnamese
    (0x2000, 0x206F), // general punctuation: dashes, quotes, ellipsis, bullets
    (0x2070, 0x209F), // superscripts and subscripts
    (0x20A0, 0x20CF), // currency signs
    (0x2100, 0x214F), // letterlike symbols: ™, №, ℃
    (0xFFFD, 0xFFFD), // the replacement character
];

/// The symbols the widgets draw themselves, outside [`BASE`] and the scripts of the languages they
/// speak. `framework_symbols_cover_the_widgets` reads the widgets' sources and fails when one is
/// missing, so this list cannot fall behind them.
pub(crate) const FRAMEWORK_SYMBOLS: &[char] = &[
    '←', '↑', '→', '↓', '↔', '↕', '↩', '↵', '⇧', '⇥', '−', '∕', '√', '∞', '≈', '≠', '≤', '≥', '⋮',
    '⌃', '⌘', '⌥', '⌫', '⏎', '■', '□', '▲', '▶', '▸', '▼', '▾', '◀', '◂', '○', '●', '★', '☆', '✓',
    '✔', '✕', '✗', '✘',
];

/// The OpenType features the widgets ask for themselves (`RollingDigits`' figures of one width),
/// beyond those a shaper applies on its own, which a cut always keeps. Checked against the
/// widgets' sources by the same test as [`FRAMEWORK_SYMBOLS`].
pub(crate) const FRAMEWORK_FEATURES: &[&str] = &["tnum"];

/// The languages the widgets speak by themselves (milestone 813): kept whatever the application
/// declares, since `frus` installs their words for any application that lists them.
const WIDGET_LANGUAGES: &[&str] = &["en", "fr", "de", "es", "ja", "th", "vi"];

/// A script and its blocks.
const SCRIPTS: &[(&str, &[(u32, u32)])] = &[
    ("Latin", &[]),
    ("Greek", &[(0x0370, 0x03FF), (0x1F00, 0x1FFF)]),
    (
        "Cyrillic",
        &[
            (0x0400, 0x052F),
            (0x1C80, 0x1C8F),
            (0x2DE0, 0x2DFF),
            (0xA640, 0xA69F),
        ],
    ),
    ("Armenian", &[(0x0530, 0x058F), (0xFB13, 0xFB17)]),
    ("Hebrew", &[(0x0590, 0x05FF), (0xFB1D, 0xFB4F)]),
    (
        "Arabic",
        &[
            (0x0600, 0x06FF),
            (0x0750, 0x077F),
            (0x0870, 0x08FF),
            (0xFB50, 0xFDFF),
            (0xFE70, 0xFEFF),
        ],
    ),
    ("Devanagari", &[(0x0900, 0x097F), (0xA8E0, 0xA8FF)]),
    ("Bengali", &[(0x0980, 0x09FF)]),
    ("Gurmukhi", &[(0x0A00, 0x0A7F)]),
    ("Gujarati", &[(0x0A80, 0x0AFF)]),
    ("Oriya", &[(0x0B00, 0x0B7F)]),
    ("Tamil", &[(0x0B80, 0x0BFF)]),
    ("Telugu", &[(0x0C00, 0x0C7F)]),
    ("Kannada", &[(0x0C80, 0x0CFF)]),
    ("Malayalam", &[(0x0D00, 0x0D7F)]),
    ("Sinhala", &[(0x0D80, 0x0DFF)]),
    ("Thai", &[(0x0E00, 0x0E7F)]),
    ("Lao", &[(0x0E80, 0x0EFF)]),
    ("Tibetan", &[(0x0F00, 0x0FFF)]),
    (
        "Myanmar",
        &[(0x1000, 0x109F), (0xA9E0, 0xA9FF), (0xAA60, 0xAA7F)],
    ),
    (
        "Georgian",
        &[(0x10A0, 0x10FF), (0x1C90, 0x1CBF), (0x2D00, 0x2D2F)],
    ),
    ("Ethiopic", &[(0x1200, 0x139F), (0x2D80, 0x2DDF)]),
    ("Khmer", &[(0x1780, 0x17FF), (0x19E0, 0x19FF)]),
    (
        "Hangul",
        &[
            (0x1100, 0x11FF),
            (0x3130, 0x318F),
            (0xA960, 0xA97F),
            (0xAC00, 0xD7FF),
        ],
    ),
    (
        "Kana",
        &[(0x3040, 0x30FF), (0x31F0, 0x31FF), (0xFF65, 0xFF9F)],
    ),
    ("Bopomofo", &[(0x3100, 0x312F), (0x31A0, 0x31BF)]),
    (
        "Han",
        &[
            (0x2E80, 0x2FDF),
            (0x3000, 0x303F),
            (0x3190, 0x319F),
            (0x31C0, 0x31EF),
            (0x3200, 0x33FF),
            (0x3400, 0x4DBF),
            (0x4E00, 0x9FFF),
            (0xF900, 0xFAFF),
            (0xFE30, 0xFE4F),
            (0xFF00, 0xFFEF),
            (0x20000, 0x2FA1F),
        ],
    ),
];

/// A language's scripts, by its ISO 639 code; a language not listed is written in Latin.
const LANGUAGE_SCRIPTS: &[(&[&str], &[&str])] = &[
    (&["el", "grc"], &["Greek"]),
    (
        &[
            "ru", "uk", "be", "bg", "mk", "sr", "kk", "ky", "mn", "tg", "tt", "ba", "cv", "ce",
            "os", "sah", "ab",
        ],
        &["Cyrillic"],
    ),
    (&["hy"], &["Armenian"]),
    (&["he", "iw", "yi"], &["Hebrew"]),
    (
        &["ar", "fa", "ur", "ps", "ug", "sd", "ckb", "ks", "dv"],
        &["Arabic"],
    ),
    (
        &["hi", "mr", "ne", "sa", "mai", "bho", "kok", "doi"],
        &["Devanagari"],
    ),
    (&["bn", "as", "mni"], &["Bengali"]),
    (&["pa"], &["Gurmukhi"]),
    (&["gu"], &["Gujarati"]),
    (&["or"], &["Oriya"]),
    (&["ta"], &["Tamil"]),
    (&["te"], &["Telugu"]),
    (&["kn"], &["Kannada"]),
    (&["ml"], &["Malayalam"]),
    (&["si"], &["Sinhala"]),
    (&["th"], &["Thai"]),
    (&["lo"], &["Lao"]),
    (&["bo", "dz"], &["Tibetan"]),
    (&["my"], &["Myanmar"]),
    (&["ka"], &["Georgian"]),
    (&["am", "ti"], &["Ethiopic"]),
    (&["km"], &["Khmer"]),
    (&["ko"], &["Hangul", "Han"]),
    (&["ja"], &["Kana", "Han"]),
    (&["zh", "yue", "wuu"], &["Han", "Bopomofo"]),
];

/// A locale's script subtag (`sr-Latn`, `zh-Hant`), which wins over its language's default.
const SCRIPT_SUBTAGS: &[(&str, &str)] = &[
    ("latn", "Latin"),
    ("cyrl", "Cyrillic"),
    ("grek", "Greek"),
    ("arab", "Arabic"),
    ("hebr", "Hebrew"),
    ("deva", "Devanagari"),
    ("hans", "Han"),
    ("hant", "Han"),
    ("jpan", "Kana"),
    ("kore", "Hangul"),
    ("thai", "Thai"),
    ("geor", "Georgian"),
    ("armn", "Armenian"),
];

/// What `[package.metadata.frus.fonts]` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    /// `subset = false` keeps every font whole.
    pub subset: bool,
    /// `locales = ["ar", "el"]`: languages the application shows besides those its code names.
    pub locales: Vec<String>,
    /// `ranges = ["U+0400-04FF", "U+2600"]`.
    pub ranges: Vec<(u32, u32)>,
    /// `text = "…"`: characters to keep, written out.
    pub text: String,
    /// `keep = ["assets/fonts/Icons.ttf"]`: fonts embedded whole, relative to the manifest.
    pub keep: Vec<String>,
    /// `features = ["ss01", "smcp"]`: OpenType features the application asks for beyond those
    /// its code names; `["*"]` keeps every one.
    pub features: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            subset: true,
            locales: Vec::new(),
            ranges: Vec::new(),
            text: String::new(),
            keep: Vec::new(),
            features: Vec::new(),
        }
    }
}

impl Settings {
    /// The section's lines, `(key, value as written)`.
    pub(crate) fn parse(lines: &[(String, String)]) -> Result<Settings, String> {
        let mut settings = Settings::default();
        for (key, value) in lines {
            let value = value.trim();
            match key.as_str() {
                "subset" => {
                    settings.subset = match value.split('#').next().unwrap_or("").trim() {
                        "true" => true,
                        "false" => false,
                        other => {
                            return Err(format!(
                                "[{SECTION}] subset is true or false, not `{other}`"
                            ))
                        }
                    }
                }
                "locales" => settings.locales = string_list(value),
                "keep" => settings.keep = string_list(value),
                "features" => settings.features = string_list(value),
                "text" => settings.text = string_value(value).unwrap_or_default(),
                "ranges" => {
                    for range in string_list(value) {
                        settings.ranges.push(parse_range(&range).ok_or_else(|| {
                            format!(
                                "[{SECTION}] ranges: `{range}` is not a code point or a range \
                                 (U+0400-04FF, U+2212)"
                            )
                        })?);
                    }
                }
                other => {
                    return Err(format!(
                        "[{SECTION}] has no `{other}`: subset, locales, ranges, text, keep and features"
                    ))
                }
            }
        }
        Ok(settings)
    }
}

/// `U+0400-04FF`, `U+0400-U+04FF`, `U+2212`, `0x2212`.
fn parse_range(text: &str) -> Option<(u32, u32)> {
    let point = |t: &str| {
        let t = t.trim();
        let t = t
            .strip_prefix("U+")
            .or_else(|| t.strip_prefix("u+"))
            .or_else(|| t.strip_prefix("0x"))
            .unwrap_or(t);
        u32::from_str_radix(t, 16).ok().filter(|&p| p <= 0x10FFFF)
    };
    let (start, end) = match text.split_once('-') {
        Some((a, b)) => (point(a)?, point(b)?),
        None => (point(text)?, point(text)?),
    };
    (start <= end).then_some((start, end))
}

/// The scripts of a locale tag (`ar`, `sr-Latn`, `zh_Hant_TW`); None for a language this table
/// does not know, which is taken to be written in Latin.
fn scripts_of(tag: &str) -> Option<Vec<&'static str>> {
    let mut parts = tag.split(['-', '_']);
    let language = parts.next()?.to_ascii_lowercase();
    for part in parts {
        let part = part.to_ascii_lowercase();
        if let Some((_, script)) = SCRIPT_SUBTAGS.iter().find(|(code, _)| *code == part) {
            return Some(vec![*script]);
        }
    }
    LANGUAGE_SCRIPTS
        .iter()
        .find(|(languages, _)| languages.contains(&language.as_str()))
        .map(|(_, scripts)| scripts.to_vec())
}

/// What a cut keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Coverage {
    /// Sorted, merged, inclusive.
    pub ranges: Vec<(u32, u32)>,
    /// The scripts kept besides Latin, for the summary.
    pub scripts: Vec<&'static str>,
    /// The characters the sources add outside everything else.
    pub from_sources: usize,
    /// The OpenType features kept besides those a shaper applies by itself, sorted; None keeps
    /// every feature.
    pub features: Option<Vec<String>>,
}

impl Coverage {
    /// The coverage of `settings`, the locales the code names and the characters its literals
    /// hold.
    pub(crate) fn new(
        settings: &Settings,
        code_locales: &[String],
        literal_chars: &BTreeSet<char>,
    ) -> Coverage {
        let mut ranges: Vec<(u32, u32)> = BASE.to_vec();
        ranges.extend(FRAMEWORK_SYMBOLS.iter().map(|&c| (c as u32, c as u32)));
        let mut scripts: Vec<&'static str> = Vec::new();
        let tags = WIDGET_LANGUAGES
            .iter()
            .map(|s| s.to_string())
            .chain(settings.locales.iter().cloned())
            .chain(code_locales.iter().cloned());
        for tag in tags {
            for script in scripts_of(&tag).unwrap_or_default() {
                if !scripts.contains(&script) {
                    scripts.push(script);
                }
            }
        }
        for script in &scripts {
            if let Some((_, blocks)) = SCRIPTS.iter().find(|(name, _)| name == script) {
                ranges.extend_from_slice(blocks);
            }
        }
        ranges.extend_from_slice(&settings.ranges);
        let declared = merge(ranges.clone());
        let extra: BTreeSet<u32> = literal_chars
            .iter()
            .chain(settings.text.chars().collect::<Vec<_>>().iter())
            .map(|&c| c as u32)
            .filter(|&p| p >= 0x20 && !covers(&declared, p))
            .collect();
        let from_sources = extra.len();
        ranges.extend(extra.into_iter().map(|p| (p, p)));
        Coverage {
            ranges: merge(ranges),
            scripts,
            from_sources,
            features: Some(FRAMEWORK_FEATURES.iter().map(|f| f.to_string()).collect()),
        }
    }

    /// The features the manifest lists and those the code asks for with `font_feature("…")`.
    pub(crate) fn with_features(mut self, settings: &Settings, code: &[String]) -> Coverage {
        if settings.features.iter().any(|f| f == "*") {
            self.features = None;
        } else if let Some(features) = &mut self.features {
            features.extend(settings.features.iter().cloned());
            features.extend(code.iter().cloned());
            features.sort();
            features.dedup();
        }
        self
    }

    #[cfg(test)]
    fn contains(&self, c: char) -> bool {
        covers(&self.ranges, c as u32)
    }

    /// A name for the coverage, which the directory of the cut copies is called: another coverage
    /// writes another directory, so a build never reuses a copy cut for a different one.
    fn key(&self) -> String {
        let mut text = String::from(env!("CARGO_PKG_VERSION"));
        for (a, b) in &self.ranges {
            text.push_str(&format!(",{a:x}-{b:x}"));
        }
        match &self.features {
            Some(features) => text.push_str(&format!(";{}", features.join(","))),
            None => text.push_str(";*"),
        }
        format!("{:016x}", fnv(text.as_bytes()))
    }

    /// `Latin, Japanese, Thai + 12 characters from the sources`.
    fn describe(&self) -> String {
        let mut text = String::from("Latin");
        for script in &self.scripts {
            text.push_str(", ");
            text.push_str(script);
        }
        if self.from_sources > 0 {
            text.push_str(&format!(
                " + {} character{} from the sources",
                self.from_sources,
                if self.from_sources == 1 { "" } else { "s" }
            ));
        }
        text
    }
}

fn merge(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (a, b) in ranges {
        match out.last_mut() {
            Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn covers(ranges: &[(u32, u32)], point: u32) -> bool {
    let at = ranges.partition_point(|&(_, end)| end < point);
    ranges.get(at).is_some_and(|&(start, _)| start <= point)
}

/// FNV-1a, 64 bits: stable across Rust versions and platforms, unlike `DefaultHasher`.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The name the cut copy of the font at `canonical` has in the subset directory. frus-macros
/// computes the same: both are tested on the same input, and must agree, or a release build
/// quietly embeds the whole font again.
pub(crate) fn subset_name(canonical: &Path) -> String {
    let hash = fnv(canonical.to_string_lossy().as_bytes());
    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{hash:016x}-{name}")
}

// --- Reading the sources ---

/// What the sources hold.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Found {
    /// The fonts `include_font!` embeds, resolved against the file that names them, each with the
    /// cargo feature its item is behind (`#[cfg(feature = "bundled-sans")]`), if any.
    pub fonts: Vec<(PathBuf, Option<String>)>,
    /// The characters outside ASCII in string and character literals.
    pub chars: BTreeSet<char>,
    /// The tags given to `Locale::new`, `Locale::with_country`, `Locale::parse`.
    pub locales: Vec<String>,
    /// The OpenType features given to `font_feature` and `with_feature`.
    pub features: Vec<String>,
    /// The files of the modules declared `#[cfg(test)] mod name;`: tests, not read.
    pub test_files: Vec<PathBuf>,
}

/// Reads one source file's text. The item after `#[cfg(test)]` (a test module, a helper) is not
/// read: its strings are not what the application shows. Comments are not read either.
pub(crate) fn scan(text: &str, dir: &Path, literals: bool, found: &mut Found) {
    let chars: Vec<char> = text.chars().collect();
    // The code read so far, without its whitespace: what came before a literal says what the
    // literal is.
    let mut code = String::new();
    let mut i = 0;
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    // Braces open, the depth a test item's body started at while it is skipped, and whether a
    // `#[cfg(test)]` waits for its item.
    let mut depth = 0usize;
    let mut skipping: Option<usize> = None;
    let mut test_item = false;
    // Where the code after the last test attribute starts.
    let mut after_attribute = 0usize;
    // The feature the item being read is behind, and the depth the item is at.
    let mut gated: Option<(String, usize)> = None;
    let mut scratch = Found::default();
    while i < chars.len() {
        let found: &mut Found = if skipping.is_some() {
            &mut scratch
        } else {
            &mut *found
        };
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let before_ident = i > 0 && ident(chars[i - 1]);
        // A raw string: r"…", r#"…"#, br"…".
        if (c == 'r' || (c == 'b' && next == Some('r'))) && !before_ident {
            let start = if c == 'b' { i + 2 } else { i + 1 };
            let mut j = start;
            while chars.get(j) == Some(&'#') {
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                let hashes = j - start;
                let mut k = j + 1;
                let mut value = String::new();
                while k < chars.len() {
                    if chars[k] == '"' && (1..=hashes).all(|h| chars.get(k + h) == Some(&'#')) {
                        break;
                    }
                    value.push(chars[k]);
                    k += 1;
                }
                i = k + 1 + hashes;
                literal(&value, &code, dir, literals, feature(&gated), found);
                code.push('"');
                continue;
            }
        }
        if c == '"' {
            let mut value = String::new();
            let mut k = i + 1;
            while k < chars.len() && chars[k] != '"' {
                if chars[k] == '\\' {
                    let (decoded, used) = escape(&chars[k + 1..]);
                    value.extend(decoded);
                    k += 1 + used;
                } else {
                    value.push(chars[k]);
                    k += 1;
                }
            }
            i = k + 1;
            if code.ends_with("#[cfg(feature=") {
                gated = Some((value.clone(), depth));
            }
            literal(&value, &code, dir, literals, feature(&gated), found);
            code.push('"');
            continue;
        }
        if c == '\'' {
            // A character literal, or a lifetime.
            if next == Some('\\') {
                let (decoded, used) = escape(&chars[i + 2..]);
                if chars.get(i + 2 + used) == Some(&'\'') {
                    if literals {
                        found.chars.extend(decoded.filter(|c| !c.is_ascii()));
                    }
                    i += 3 + used;
                    continue;
                }
            } else if chars.get(i + 2) == Some(&'\'') {
                if let Some(ch) = next.filter(|c| literals && !c.is_ascii()) {
                    found.chars.insert(ch);
                }
                i += 3;
                continue;
            }
        }
        match c {
            '{' => {
                if test_item && skipping.is_none() {
                    skipping = Some(depth);
                }
                test_item = false;
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if skipping == Some(depth) {
                    skipping = None;
                }
                if gated.as_ref().is_some_and(|(_, at)| *at == depth) {
                    gated = None;
                }
            }
            ';' => {
                if gated.as_ref().is_some_and(|(_, at)| *at == depth) {
                    gated = None;
                }
                // `#[cfg(test)] mod name;`: the module's file holds tests.
                if test_item && skipping.is_none() {
                    if let Some(name) = code
                        .get(after_attribute..)
                        .map(|item| {
                            item.strip_prefix("pub(crate)")
                                .or_else(|| item.strip_prefix("pub"))
                                .unwrap_or(item)
                        })
                        .and_then(|item| item.strip_prefix("mod"))
                        .filter(|name| !name.is_empty() && name.chars().all(ident))
                    {
                        found.test_files.push(dir.join(format!("{name}.rs")));
                        found.test_files.push(dir.join(name).join("mod.rs"));
                    }
                }
                test_item = false;
            }
            _ => {}
        }
        if !c.is_whitespace() {
            code.push(c);
            // `#[cfg(test)]`, or `#[cfg(all(test, …))]`: compiled for the tests only.
            if c == ']' {
                if let Some(attribute) = code.rfind("#[").map(|at| &code[at..]) {
                    if attribute == "#[cfg(test)]" || attribute.starts_with("#[cfg(all(test,") {
                        test_item = true;
                        after_attribute = code.len();
                    }
                }
            }
        }
        i += 1;
    }
}

/// The feature of the item being read, if it is behind one.
fn feature(gated: &Option<(String, usize)>) -> Option<&str> {
    gated.as_ref().map(|(f, _)| f.as_str())
}

/// A string literal's value, with what the code before it says it is.
fn literal(
    value: &str,
    code: &str,
    dir: &Path,
    literals: bool,
    feature: Option<&str>,
    found: &mut Found,
) {
    if literals {
        found.chars.extend(value.chars().filter(|c| !c.is_ascii()));
    }
    if code.ends_with("include_font!(") {
        found
            .fonts
            .push((dir.join(value), feature.map(str::to_string)));
    }
    if code.ends_with("font_feature(") || code.ends_with("with_feature(") {
        found.features.push(value.to_string());
    }
    if ["Locale::new(", "Locale::with_country(", "Locale::parse("]
        .iter()
        .any(|call| code.ends_with(call))
    {
        found.locales.push(value.to_string());
    }
}

/// An escape after its backslash: the characters it stands for, and how many it took.
fn escape(rest: &[char]) -> (Option<char>, usize) {
    match rest.first() {
        Some('u') if rest.get(1) == Some(&'{') => {
            let close = rest.iter().position(|&c| c == '}').unwrap_or(rest.len());
            let hex: String = rest[2..close.min(rest.len())].iter().collect();
            let decoded = u32::from_str_radix(&hex.replace('_', ""), 16)
                .ok()
                .and_then(char::from_u32);
            (decoded, close + 1)
        }
        Some('x') => (None, 3),
        // A backslash at the end of a line continues the string: the newline and the indentation
        // after it are not part of it, and are skipped as ordinary characters would be.
        Some(c) => (
            match c {
                'n' | 't' | 'r' | '0' | '\n' | '\r' => None,
                other => Some(*other),
            },
            1,
        ),
        None => (None, 0),
    }
}

/// Reads every `.rs` file below `dir`, but those of test modules.
fn scan_dir(dir: &Path, literals: bool, found: &mut Found) {
    let mut files = Vec::new();
    rust_files(dir, &mut files);
    let mut each = Vec::new();
    let mut tests = Vec::new();
    for path in files {
        let mut one = Found::default();
        if let Ok(text) = std::fs::read_to_string(&path) {
            scan(&text, path.parent().unwrap_or(dir), literals, &mut one);
        }
        tests.append(&mut one.test_files);
        each.push((path, one));
    }
    for (path, mut one) in each {
        if tests.contains(&path) {
            continue;
        }
        found.fonts.append(&mut one.fonts);
        found.chars.append(&mut one.chars);
        found.locales.append(&mut one.locales);
        found.features.append(&mut one.features);
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A package of the build.
struct Package {
    name: String,
    dir: PathBuf,
    /// On this machine (a path or the workspace), not from a registry or git.
    local: bool,
    /// The features the build turns on, as cargo resolves them.
    features: Vec<String>,
}

/// The packages of the application's build, and its target directory, as `cargo metadata` says.
fn metadata(root: &Path) -> Result<(Vec<Package>, PathBuf), String> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let json = parse(&String::from_utf8_lossy(&output.stdout))?;
    let target = json
        .get("target_directory")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or("cargo metadata did not say where the target directory is")?;
    let nodes = json
        .get("resolve")
        .and_then(|r| r.get("nodes"))
        .and_then(Value::as_array)
        .unwrap_or_default();
    let packages = json
        .get("packages")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let manifest = p.get("manifest_path")?.as_str()?;
            let id = p.get("id")?.as_str()?;
            Some(Package {
                name: p.get("name")?.as_str()?.to_string(),
                dir: Path::new(manifest).parent()?.to_path_buf(),
                local: !matches!(p.get("source"), Some(Value::String(_))),
                features: nodes
                    .iter()
                    .find(|n| n.get("id").and_then(Value::as_str) == Some(id))
                    .and_then(|n| n.get("features")?.as_array())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect(),
            })
        })
        .collect();
    Ok((packages, target))
}

// --- Cutting ---

/// One font, before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cut {
    /// `frus-text/DejaVuSans.ttf`.
    pub shown: String,
    pub before: u64,
    /// None: embedded whole, for `reason`.
    pub after: Option<u64>,
    pub reason: Option<String>,
}

/// The fonts of a build, cut.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub dir: PathBuf,
    pub coverage: Coverage,
    pub fonts: Vec<Cut>,
}

impl Prepared {
    /// `Fonts: 3.4 MB → 0.6 MB, cut to …`.
    pub(crate) fn summary(&self) -> String {
        let before: u64 = self.fonts.iter().map(|f| f.before).sum();
        let after: u64 = self.fonts.iter().map(|f| f.after.unwrap_or(f.before)).sum();
        let mut text = format!(
            "Fonts: {} → {}, cut to the characters the application can show ({}).",
            size(before),
            size(after),
            self.coverage.describe()
        );
        for font in self.fonts.iter().filter(|f| f.after.is_none()) {
            if let Some(reason) = &font.reason {
                text.push_str(&format!("\n  {} is embedded whole: {reason}.", font.shown));
            }
        }
        text.push_str(&format!(
            "\n  Another language, a range or a feature goes in [{SECTION}] (locales, ranges, \
             text, features, keep); --no-font-subset keeps them whole."
        ));
        text
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} kB", bytes.div_ceil(1024))
    }
}

/// Cuts the fonts of `project`'s build into `target/frus-fonts/<coverage>/`, the locales given
/// added to the manifest's.
pub(crate) fn prepare(project: &Project, extra_locales: &[String]) -> Result<Prepared, String> {
    let mut settings = Settings::parse(&project.font_lines)
        .map_err(|e| format!("{e} (in {})", project.root.join("Cargo.toml").display()))?;
    settings.locales.extend(extra_locales.iter().cloned());
    let (packages, target) = metadata(&project.root)?;
    let mut fonts = Found::default();
    let mut sources = Found::default();
    for package in &packages {
        // Only local packages and frus's own can embed a font with the macro. The application's
        // text is in its own local packages: frus's crates are not, even when an application
        // depends on a checkout of them by path, so a build cuts the same way from a checkout
        // as from crates.io (what the widgets draw is in FRAMEWORK_SYMBOLS).
        let framework = package.name.starts_with("frus") && package.name != project.package;
        if !(package.local || framework) {
            continue;
        }
        let mut found = Found::default();
        scan_dir(
            &package.dir.join("src"),
            package.local && !framework,
            &mut found,
        );
        // A font behind a feature this build leaves off is not in the binary.
        found.fonts.retain(|(_, feature)| {
            feature
                .as_ref()
                .is_none_or(|f| package.features.contains(f))
        });
        if package.local && !framework {
            sources.chars.append(&mut found.chars);
            sources.locales.append(&mut found.locales);
            sources.features.append(&mut found.features);
        }
        fonts.fonts.append(&mut found.fonts);
    }
    let coverage = Coverage::new(&settings, &sources.locales, &sources.chars)
        .with_features(&settings, &sources.features);
    let base = target.join("frus-fonts");
    let dir = base.join(coverage.key());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // The copies cut for another coverage go once no build has used them for a day. Not at once:
    // two applications sharing a target directory each have their own, and one build must not
    // take away the other's.
    let _ = std::fs::write(dir.join(LAST_USED), b"");
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.filter_map(Result::ok) {
            let stale = std::fs::metadata(entry.path().join(LAST_USED))
                .and_then(|m| m.modified())
                .map_or(true, |at| {
                    at.elapsed().is_ok_and(|age| age.as_secs() > 24 * 60 * 60)
                });
            if entry.path() != dir && stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let keep: Vec<PathBuf> = settings
        .keep
        .iter()
        .filter_map(|k| std::fs::canonicalize(project.root.join(k)).ok())
        .collect();
    let mut seen = BTreeSet::new();
    let mut cuts = Vec::new();
    for (font, _) in &fonts.fonts {
        // A path in a comment's example, or a font behind a feature this build leaves out of a
        // package that is not there: nothing to cut.
        let Ok(canonical) = std::fs::canonicalize(font) else {
            continue;
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }
        let shown = shown_name(&canonical, &packages);
        let Ok(bytes) = std::fs::read(&canonical) else {
            continue;
        };
        let out = dir.join(subset_name(&canonical));
        let mut cut = Cut {
            shown,
            before: bytes.len() as u64,
            after: None,
            reason: None,
        };
        if !settings.subset {
            cut.reason = Some("`subset = false`".into());
        } else if keep.contains(&canonical) {
            cut.reason = Some("listed in `keep`".into());
        } else if let Some(len) = fresh(&out, &canonical) {
            // Cut already, for this coverage, from this file as it is now.
            cut.after = Some(len);
        } else {
            match cut_font(&bytes, &coverage) {
                Ok(smaller) if smaller.len() < bytes.len() => {
                    write_if_changed(&out, &smaller)?;
                    cut.after = Some(smaller.len() as u64);
                }
                Ok(_) => cut.reason = Some("nothing to leave out".into()),
                Err(error) => cut.reason = Some(format!("it could not be cut ({error})")),
            }
        }
        if cut.after.is_none() {
            let _ = std::fs::remove_file(&out);
        }
        cuts.push(cut);
    }
    Ok(Prepared {
        dir,
        coverage,
        fonts: cuts,
    })
}

/// The size of the copy at `out` when it was cut after `font` last changed. The directory is
/// named for the coverage and the tool's version, so the copy is the one this build would cut.
fn fresh(out: &Path, font: &Path) -> Option<u64> {
    let copy = std::fs::metadata(out).ok()?;
    let source = std::fs::metadata(font).ok()?.modified().ok()?;
    (copy.modified().ok()? >= source).then_some(copy.len())
}

/// Writes `bytes` unless the file holds them already: a file rewritten is a file cargo sees as
/// changed, and every crate that embeds it is compiled again.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// `frus-text/DejaVuSans.ttf`: the package and the file.
fn shown_name(canonical: &Path, packages: &[Package]) -> String {
    let file = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    packages
        .iter()
        .filter_map(|p| {
            let dir = std::fs::canonicalize(&p.dir).ok()?;
            canonical
                .starts_with(&dir)
                .then(|| (dir.components().count(), &p.name))
        })
        .max()
        .map(|(_, name)| format!("{name}/{file}"))
        .unwrap_or(file)
}

/// The font in `bytes` with the characters of `coverage` and nothing else.
///
/// What a shaper needs of the rest stays: the substitutions and positionings of every script
/// and feature (an Arabic letter's joining forms, a ligature, a mark's place), followed from
/// the characters kept to the glyphs they reach; the hinting, so a glyph is drawn exactly as in
/// the whole font; a variable font's axes; every name, which is how a family is found; and the
/// box drawn for a missing character.
pub(crate) fn cut_font(bytes: &[u8], coverage: &Coverage) -> Result<Vec<u8>, String> {
    cut_with(
        bytes,
        coverage,
        skera::SubsetFlags::SUBSET_FLAGS_NOTDEF_OUTLINE,
    )
}

fn cut_with(
    bytes: &[u8],
    coverage: &Coverage,
    flags: skera::SubsetFlags,
) -> Result<Vec<u8>, String> {
    let font = FontRef::new(bytes).map_err(|e| e.to_string())?;
    let mut unicodes = IntSet::<u32>::empty();
    for &(start, end) in &coverage.ranges {
        unicodes.insert_range(start..=end);
    }
    // The features a shaper applies by itself, and those the application asks for: a stylistic
    // set nobody turns on takes its alternate glyphs with it.
    let features = match &coverage.features {
        None => IntSet::<Tag>::all(),
        Some(asked) => skera::DEFAULT_LAYOUT_FEATURES
            .iter()
            .copied()
            .chain(
                asked
                    .iter()
                    .filter_map(|f| <[u8; 4]>::try_from(f.as_bytes()).ok().map(|b| Tag::new(&b))),
            )
            .collect(),
    };
    let plan = skera::Plan::new(
        &IntSet::<GlyphId>::empty(),
        &unicodes,
        &font,
        flags,
        &IntSet::<Tag>::empty(),
        &IntSet::<Tag>::all(),
        &features,
        &IntSet::<NameId>::all(),
        &IntSet::<u16>::all(),
    );
    skera::subset_font(&font, &plan).map_err(|e| e.to_string())
}

/// Before a build: cuts the fonts and points `include_font!` at them in a release (or profile)
/// build, unless the build or the manifest says not to; clears the variable otherwise. A font that
/// cannot be cut is embedded whole, and nothing here fails the build.
pub(crate) fn for_build(project: &Project, mode: Mode, wanted: bool) {
    std::env::remove_var(SUBSET_DIR_VAR);
    if mode == Mode::Debug || !wanted {
        return;
    }
    match prepare(project, &[]) {
        Ok(prepared) if prepared.fonts.iter().any(|f| f.after.is_some()) => {
            println!("{}", prepared.summary());
            std::env::set_var(SUBSET_DIR_VAR, &prepared.dir);
        }
        Ok(prepared) if !prepared.fonts.is_empty() => println!("{}", prepared.summary()),
        Ok(_) => {}
        Err(error) => println!("The fonts are embedded whole: {error}"),
    }
}

/// `frus fonts [--locale <tag>]…`: what a release build keeps, written out.
pub(crate) fn report(locales: &[String]) -> Result<std::process::ExitCode, String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let project = Project::find(&here)?;
    let prepared = prepare(&project, locales)?;
    if prepared.fonts.is_empty() {
        println!(
            "No font is embedded with include_font!: nothing to cut. frus's bundled faces are \
             (unless every `bundled-*` feature is off)."
        );
        return Ok(std::process::ExitCode::SUCCESS);
    }
    for font in &prepared.fonts {
        match font.after {
            Some(after) => println!(
                "  {:<44} {:>9} → {:>9}",
                font.shown,
                size(font.before),
                size(after)
            ),
            None => println!(
                "  {:<44} {:>9}   whole: {}",
                font.shown,
                size(font.before),
                font.reason.as_deref().unwrap_or("")
            ),
        }
    }
    println!("{}", prepared.summary());
    println!(
        "The cut fonts: {} (a release `frus build` embeds them).",
        prepared.dir.display()
    );
    Ok(std::process::ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subset_name_is_the_one_the_macro_reads() {
        // The same input and answer as frus-macros' test of its copy.
        assert_eq!(
            subset_name(Path::new("/fonts/Inter-Regular.ttf")),
            "c2186180b0c7d968-Inter-Regular.ttf"
        );
    }

    #[test]
    fn the_manifest_section_is_read() {
        let lines = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let settings = Settings::parse(&lines(&[
            ("locales", r#"["ar", "el"]"#),
            ("ranges", r#"["U+0400-04FF", "U+2600", "0x2212"]"#),
            ("text", r#""★""#),
            ("keep", r#"["assets/fonts/Icons.ttf"]"#),
            ("features", r#"["ss01"]"#),
        ]))
        .unwrap();
        assert!(settings.subset);
        assert_eq!(settings.locales, ["ar", "el"]);
        assert_eq!(
            settings.ranges,
            [(0x400, 0x4FF), (0x2600, 0x2600), (0x2212, 0x2212)]
        );
        assert_eq!(settings.text, "★");
        assert_eq!(settings.keep, ["assets/fonts/Icons.ttf"]);
        assert_eq!(settings.features, ["ss01"]);
        assert!(
            !Settings::parse(&lines(&[("subset", "false")]))
                .unwrap()
                .subset
        );
        assert!(Settings::parse(&lines(&[("ranges", r#"["Cyrillic"]"#)])).is_err());
        assert!(Settings::parse(&lines(&[("subset", "maybe")])).is_err());
        assert!(Settings::parse(&lines(&[("scripts", "[]")])).is_err());
    }

    #[test]
    fn a_locale_brings_its_script() {
        assert_eq!(scripts_of("ar"), Some(vec!["Arabic"]));
        assert_eq!(scripts_of("ar-EG"), Some(vec!["Arabic"]));
        assert_eq!(scripts_of("sr-Latn"), Some(vec!["Latin"]));
        assert_eq!(scripts_of("zh_Hant_TW"), Some(vec!["Han"]));
        assert_eq!(scripts_of("ja"), Some(vec!["Kana", "Han"]));
        assert_eq!(scripts_of("pt-BR"), None);

        let none = BTreeSet::new();
        let plain = Coverage::new(&Settings::default(), &[], &none);
        // Latin, the widgets' languages and symbols are always there.
        for c in [
            'A', 'é', 'ß', 'ł', 'ệ', '€', '…', '−', '✓', 'あ', '漢', 'ก', '\u{FFFD}',
        ] {
            assert!(plain.contains(c), "{c}");
        }
        // Scripts no language asked for are not.
        for c in ['Ж', 'Ω', 'ب', 'א', '한'] {
            assert!(!plain.contains(c), "{c}");
        }
        let settings = Settings {
            locales: vec!["el".into()],
            ..Settings::default()
        };
        let declared = Coverage::new(&settings, &["ar".into(), "ru".into()], &none);
        for c in ['Ж', 'Ω', 'ب', 'ﻻ'] {
            assert!(declared.contains(c), "{c}");
        }
        assert!(!declared.contains('א'));
        assert_ne!(plain.key(), declared.key());

        // A character of the sources is kept alone, and counted.
        let from_code = Coverage::new(&Settings::default(), &[], &BTreeSet::from(['א', 'é']));
        assert!(from_code.contains('א') && !from_code.contains('ב'));
        assert_eq!(from_code.from_sources, 1);
    }

    #[test]
    fn sources_are_read_for_fonts_locales_and_characters() {
        let source = r##"
            // include_font!("commented.ttf") and "Ω in a comment"
            /* "Ж in a block" /* nested */ */
            const F: &[u8] = frus::fonts::include_font!("../assets/Inter.ttf");
            const G: &[u8] = include_font! ( r#"raw.ttf"# );
            /// A face behind a feature.
            #[cfg(feature = "bundled-sans")]
            const H: &[u8] = include_font!("gated.ttf");
            const I: &[u8] = include_font!("after.ttf");
            fn view<'a>(x: &'a str) {
                let a = "Café — €";
                let b = '\u{2212}';
                let c = '✓';
                let d = "\u{e5cd}";
                let tag = Locale::new("ar");
                let more = Locale::parse("he-IL");
                let figures = TextStyle::new(14.0).font_feature("tnum", 1);
                let lifetime: &'static str = "\"quoted\" ünder";
                let multi = "first line \
                    ñ";
            }
            #[cfg(test)]
            mod tests { const T: &str = "日本語"; fn f() { let _ = "Ω"; } }
            const AFTER: &str = "ø";
        "##;
        let mut found = Found::default();
        scan(source, Path::new("/app/src"), true, &mut found);
        assert_eq!(
            found.fonts,
            [
                (Path::new("/app/src").join("../assets/Inter.ttf"), None),
                (Path::new("/app/src").join("raw.ttf"), None),
                (
                    Path::new("/app/src").join("gated.ttf"),
                    Some("bundled-sans".to_string())
                ),
                (Path::new("/app/src").join("after.ttf"), None),
            ]
        );
        assert_eq!(found.locales, ["ar", "he-IL"]);
        assert_eq!(found.features, ["tnum"]);
        let chars: String = found.chars.iter().collect();
        assert_eq!(chars, "éñøü—€−✓\u{e5cd}");
    }

    #[test]
    fn a_test_module_s_file_is_not_read() {
        let dir = std::env::temp_dir().join(format!("frus-fonts-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("lib.rs"),
            "#[cfg(all(test, desktop))]\nmod modal_tests;\nmod shown;\nconst A: &str = \"é\";\n",
        )
        .unwrap();
        std::fs::write(dir.join("modal_tests.rs"), "const T: &str = \"Ж\";\n").unwrap();
        std::fs::write(dir.join("shown.rs"), "const S: &str = \"ñ\";\n").unwrap();
        let mut found = Found::default();
        scan_dir(&dir, true, &mut found);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.chars.iter().collect::<String>(), "éñ");
    }

    #[test]
    fn ranges_merge_and_answer() {
        let merged = merge(vec![(10, 20), (5, 9), (30, 30), (15, 25)]);
        assert_eq!(merged, [(5, 25), (30, 30)]);
        assert!(covers(&merged, 5) && covers(&merged, 25) && covers(&merged, 30));
        assert!(!covers(&merged, 26) && !covers(&merged, 4) && !covers(&merged, 31));
        assert_eq!(parse_range("U+0400-04FF"), Some((0x400, 0x4FF)));
        assert_eq!(parse_range("U+0400-U+04FF"), Some((0x400, 0x4FF)));
        assert_eq!(parse_range("U+04FF-0400"), None);
        assert_eq!(parse_range("U+110000"), None);
    }

    /// The widgets' own literals, read as an application's are: every character they can draw
    /// must be in what a cut always keeps, or a release build draws a box where a debug build drew
    /// the widget's symbol (the `−` of milestone 567).
    #[test]
    fn framework_symbols_cover_the_widgets() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = Found::default();
        for name in [
            "frus",
            "frus-widgets",
            "frus-shell",
            "frus-text",
            "frus-gpu",
        ] {
            scan_dir(&crates.join(name).join("src"), true, &mut found);
        }
        assert!(found.chars.len() > 20, "the widgets' sources were not read");
        let coverage = Coverage::new(&Settings::default(), &[], &BTreeSet::new());
        let asked: Vec<_> = found
            .features
            .iter()
            .filter(|f| !FRAMEWORK_FEATURES.contains(&f.as_str()))
            .collect();
        assert!(
            asked.is_empty(),
            "the widgets ask for features a cut leaves out; add them to FRAMEWORK_FEATURES: {asked:?}"
        );
        let missing: String = found
            .chars
            .iter()
            .filter(|&&c| !coverage.contains(c) && !c.is_control())
            .map(|c| format!("{c} U+{:04X}\n", *c as u32))
            .collect();
        assert!(
            missing.is_empty(),
            "the widgets draw characters a cut leaves out; add them to FRAMEWORK_SYMBOLS:\n{missing}"
        );
    }

    /// A real cut of the bundled sans: far smaller, the kept letters there, the others gone.
    #[test]
    fn the_bundled_sans_is_cut_to_what_is_kept() {
        use read_fonts::TableProvider;
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../frus-text/assets/DejaVuSans.ttf");
        let bytes = std::fs::read(path).unwrap();
        let coverage = Coverage::new(&Settings::default(), &[], &BTreeSet::new());
        let cut = cut_font(&bytes, &coverage).unwrap();
        assert!(
            cut.len() * 4 < bytes.len(),
            "{} of {}",
            cut.len(),
            bytes.len()
        );
        let font = FontRef::new(&cut).unwrap();
        let cmap = font.cmap().unwrap();
        for c in ['A', 'é', 'ệ', '−', '€', '✓'] {
            assert!(cmap.map_codepoint(c).is_some(), "{c} was cut");
        }
        for c in ['Ж', 'Ω', 'ب'] {
            assert!(cmap.map_codepoint(c).is_none(), "{c} was kept");
        }
        // The layout tables a shaper reads, and the family's name, survive.
        for tag in [b"GSUB", b"GPOS", b"name", b"OS/2", b"fpgm"] {
            assert!(
                font.table_data(Tag::new(tag)).is_some(),
                "{}",
                String::from_utf8_lossy(tag)
            );
        }
    }

    /// The Arabic face, with Arabic kept: the joining forms the shaper substitutes are reached
    /// from the letters, though no character maps to them.
    #[test]
    fn arabic_keeps_its_joining_forms() {
        use read_fonts::TableProvider;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../frus-text/assets/NotoNaskhArabic-Regular.ttf");
        let bytes = std::fs::read(path).unwrap();
        let whole = FontRef::new(&bytes).unwrap();
        let settings = Settings {
            locales: vec!["ar".into()],
            ..Settings::default()
        };
        let cut = cut_font(&bytes, &Coverage::new(&settings, &[], &BTreeSet::new())).unwrap();
        let font = FontRef::new(&cut).unwrap();
        let glyphs = |f: &FontRef| f.maxp().unwrap().num_glyphs();
        // What the character map alone reaches, without following the substitutions: fewer
        // glyphs, the ligatures and contextual forms missing.
        let mapped_only = cut_with(
            &bytes,
            &Coverage::new(&settings, &[], &BTreeSet::new()),
            skera::SubsetFlags::SUBSET_FLAGS_NOTDEF_OUTLINE
                | skera::SubsetFlags::SUBSET_FLAGS_NO_LAYOUT_CLOSURE,
        )
        .unwrap();
        let mapped_only = FontRef::new(&mapped_only).unwrap();
        assert!(
            glyphs(&font) > glyphs(&mapped_only),
            "{} glyphs with the substitutions followed, {} without",
            glyphs(&font),
            glyphs(&mapped_only)
        );
        assert!(glyphs(&font) <= glyphs(&whole));
        assert!(font.table_data(Tag::new(b"GSUB")).is_some());
    }
}
