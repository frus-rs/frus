// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **How a run of text is shaped**: its OpenType features, its variable font axes and the
//! language it is written in (milestone 784).
//!
//! The three travel together as one small value, [`TextShaping`], so a [`TextStyle`]
//! (crate::TextStyle) stays `Copy` and cheap to hand down a tree: the settings themselves
//! live in a process-wide table and the value is their index. Two equal sets of settings
//! always get the same index, so comparing and hashing them is comparing a number.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

/// A four-letter OpenType tag: a feature (`liga`, `tnum`, `smcp`, `ss01`) or a variable font
/// axis (`wght`, `wdth`, `slnt`, `opsz`, or a font's own, such as `GRAD`).
///
/// Tags shorter than four letters are padded with spaces, as OpenType does (`cv1` is not a
/// tag, but `"kern"` and `"ss01"` are).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontTag(pub [u8; 4]);

impl FontTag {
    /// Reads a tag: one to four printable ASCII characters. `None` for anything else.
    ///
    /// ```
    /// # use frus_core::FontTag;
    /// assert_eq!(FontTag::parse("tnum").unwrap().as_str(), "tnum");
    /// assert!(FontTag::parse("tabular").is_none());
    /// ```
    pub fn parse(tag: &str) -> Option<FontTag> {
        let bytes = tag.as_bytes();
        if bytes.is_empty() || bytes.len() > 4 || !bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
            return None;
        }
        let mut out = [b' '; 4];
        out[..bytes.len()].copy_from_slice(bytes);
        Some(FontTag(out))
    }

    /// The tag as text.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl fmt::Debug for FontTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

/// The settings a [`TextShaping`] stands for.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
struct Settings {
    /// Features in the order they were set, each tag once.
    features: Vec<(FontTag, u32)>,
    /// Axes in the order they were set, each tag once; the value's bits.
    variations: Vec<(FontTag, u32)>,
    /// The language, a normalised BCP 47 tag.
    locale: Option<&'static str>,
}

struct Table {
    settings: Vec<Arc<Settings>>,
    index: HashMap<Arc<Settings>, u32>,
    locales: Vec<&'static str>,
}

fn table() -> &'static Mutex<Table> {
    static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let none = Arc::new(Settings::default());
        let mut index = HashMap::new();
        index.insert(none.clone(), 0);
        Mutex::new(Table {
            settings: vec![none],
            index,
            locales: Vec::new(),
        })
    })
}

fn intern(settings: Settings) -> TextShaping {
    let mut table = table()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(&id) = table.index.get(&settings) {
        return TextShaping(id);
    }
    let id = table.settings.len() as u32;
    let settings = Arc::new(settings);
    table.settings.push(settings.clone());
    table.index.insert(settings, id);
    TextShaping(id)
}

/// A language tag kept for the life of the program: there are few, and a style is `Copy`.
fn intern_locale(tag: &str) -> &'static str {
    let mut table = table()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(&known) = table.locales.iter().find(|known| **known == tag) {
        return known;
    }
    let leaked: &'static str = Box::leak(tag.to_owned().into_boxed_str());
    table.locales.push(leaked);
    leaked
}

/// Reads a BCP 47 language tag (`ja`, `zh-Hant`, `sr-Cyrl-RS`, `pt_BR`), normalising its
/// case and separators: `zh_hant_tw` is `zh-Hant-TW`. `None` for something that is not one.
fn normalise_locale(tag: &str) -> Option<String> {
    let mut parts = tag.split(['-', '_']).filter(|part| !part.is_empty());
    let language = parts.next()?;
    if !(2..=3).contains(&language.len()) || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = language.to_ascii_lowercase();
    for part in parts {
        if part.len() > 8 || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        out.push('-');
        match part.len() {
            // A script: title case.
            4 if part.chars().all(|c| c.is_ascii_alphabetic()) => {
                out.push_str(&part[..1].to_ascii_uppercase());
                out.push_str(&part[1..].to_ascii_lowercase());
            }
            // A region: upper case.
            2 => out.push_str(&part.to_ascii_uppercase()),
            _ => out.push_str(&part.to_ascii_lowercase()),
        }
    }
    Some(out)
}

fn warn_once(message: String) {
    static WARNED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut warned = WARNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !warned.contains(&message) {
        log::warn!("{message}");
        warned.push(message);
    }
}

/// **How a run of text is shaped**: OpenType features, variable font axes and the language,
/// each optional. [`TextShaping::NONE`] says nothing about any of them.
///
/// Usually set through a style rather than built directly:
///
/// ```
/// # use frus_core::TextStyle;
/// let figures = TextStyle::new(14.0)
///     .font_feature("tnum", 1) // figures of one width, for columns of numbers
///     .font_variation("wdth", 85.0) // a narrower cut of a variable font
///     .locale("ja"); // Japanese glyph forms for the shared Han characters
/// assert_eq!(figures.shaping.locale(), Some("ja"));
/// ```
///
/// - **Features** turn an OpenType feature on (`1`), off (`0`) or pick an alternate (`ss01`,
///   `salt`, `cv01` take an index). A feature the font does not have is ignored.
/// - **Axes** set a variable font's axis in its own units: `wght` 100 to 900, `wdth` as a
///   percentage, `slnt` in degrees, `opsz` in points, or any axis a font defines. An axis the
///   face does not have is ignored, and a value is clamped to the face's range. `wght` set here
///   takes over from the style's weight.
/// - **The language** picks the font's localized glyph forms (Serbian and Macedonian Cyrillic,
///   Polish accents, Turkish dotted i) and, for the Han characters Chinese, Japanese and Korean
///   share, which of the platform's fonts draws them. Unset, the application's locale.
///
/// It is `Copy` and four bytes: the settings are kept in a table for the life of the program,
/// and the value is their place in it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextShaping(u32);

impl TextShaping {
    /// No features, no axes, no language: everything is the font's and the application's
    /// default.
    pub const NONE: TextShaping = TextShaping(0);

    fn settings(self) -> Arc<Settings> {
        let table = table()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        table
            .settings
            .get(self.0 as usize)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether this says nothing at all.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The same settings with the OpenType feature `tag` set to `value` (`0` off, `1` on, or
    /// an alternate's index), replacing what was set for that tag. An invalid tag is ignored,
    /// and the console says so.
    ///
    /// ```
    /// # use frus_core::TextShaping;
    /// let no_ligatures = TextShaping::NONE.with_feature("liga", 0);
    /// assert_eq!(no_ligatures.features()[0].1, 0);
    /// ```
    #[must_use]
    pub fn with_feature(self, tag: &str, value: u32) -> TextShaping {
        let Some(tag) = FontTag::parse(tag) else {
            warn_once(format!(
                "frus: {tag:?} is not an OpenType feature tag (one to four ASCII letters); \
                 the feature is ignored"
            ));
            return self;
        };
        let mut settings = (*self.settings()).clone();
        match settings.features.iter_mut().find(|(t, _)| *t == tag) {
            Some(feature) => feature.1 = value,
            None => settings.features.push((tag, value)),
        }
        intern(settings)
    }

    /// The same settings with the variable font axis `tag` at `value`, in the axis's own
    /// units, replacing what was set for that axis. An invalid tag or a value that is not a
    /// number is ignored, and the console says so.
    ///
    /// ```
    /// # use frus_core::TextShaping;
    /// let condensed = TextShaping::NONE.with_variation("wdth", 75.0);
    /// assert_eq!(condensed.variation("wdth"), Some(75.0));
    /// ```
    #[must_use]
    pub fn with_variation(self, tag: &str, value: f32) -> TextShaping {
        let Some(parsed) = FontTag::parse(tag).filter(|_| value.is_finite()) else {
            warn_once(format!(
                "frus: the variable font axis {tag:?} = {value} is not valid (a tag of one to \
                 four ASCII letters and a finite number); it is ignored"
            ));
            return self;
        };
        let mut settings = (*self.settings()).clone();
        let bits = (value + 0.0).to_bits();
        match settings.variations.iter_mut().find(|(t, _)| *t == parsed) {
            Some(variation) => variation.1 = bits,
            None => settings.variations.push((parsed, bits)),
        }
        intern(settings)
    }

    /// The same settings in the language `tag`, a BCP 47 tag such as `ja`, `zh-Hant` or
    /// `sr-Cyrl`. A tag that is not one is ignored, and the console says so.
    ///
    /// ```
    /// # use frus_core::TextShaping;
    /// assert_eq!(TextShaping::NONE.with_locale("zh_hant").locale(), Some("zh-Hant"));
    /// ```
    #[must_use]
    pub fn with_locale(self, tag: &str) -> TextShaping {
        let Some(normalised) = normalise_locale(tag) else {
            warn_once(format!(
                "frus: {tag:?} is not a language tag (such as \"ja\" or \"sr-Cyrl\"); the \
                 text keeps the application's language"
            ));
            return self;
        };
        let locale = intern_locale(&normalised);
        let mut settings = (*self.settings()).clone();
        settings.locale = Some(locale);
        intern(settings)
    }

    /// The OpenType features set, in the order they were set.
    pub fn features(self) -> Vec<(FontTag, u32)> {
        self.settings().features.clone()
    }

    /// The variable font axes set, in the order they were set, in each axis's own units.
    pub fn variations(self) -> Vec<(FontTag, f32)> {
        self.settings()
            .variations
            .iter()
            .map(|(tag, bits)| (*tag, f32::from_bits(*bits)))
            .collect()
    }

    /// The value set for the axis `tag`, if one was.
    pub fn variation(self, tag: &str) -> Option<f32> {
        let tag = FontTag::parse(tag)?;
        self.variations()
            .into_iter()
            .find_map(|(t, value)| (t == tag).then_some(value))
    }

    /// The language set, normalised (`zh-Hant`, `sr-Cyrl`), if one was.
    pub fn locale(self) -> Option<&'static str> {
        self.settings().locale
    }

    /// **Merges** `over` on top of `self`: a feature or axis `over` sets replaces the same tag
    /// here and every other one survives; `over`'s language wins if it names one.
    #[must_use]
    pub fn merge(self, over: TextShaping) -> TextShaping {
        if over.is_none() || over == self {
            return self;
        }
        if self.is_none() {
            return over;
        }
        let mut settings = (*self.settings()).clone();
        let over = over.settings();
        for &(tag, value) in &over.features {
            match settings.features.iter_mut().find(|(t, _)| *t == tag) {
                Some(feature) => feature.1 = value,
                None => settings.features.push((tag, value)),
            }
        }
        for &(tag, bits) in &over.variations {
            match settings.variations.iter_mut().find(|(t, _)| *t == tag) {
                Some(variation) => variation.1 = bits,
                None => settings.variations.push((tag, bits)),
            }
        }
        settings.locale = over.locale.or(settings.locale);
        intern(settings)
    }

    /// The settings `t` of the way from `self` to `other`. An axis both set **glides**
    /// between the two values (to a quarter of a unit), which is what makes a weight or a
    /// width animate smoothly on a variable font; an axis only one side set is held. Features
    /// and the language are choices, so they switch halfway.
    #[must_use]
    pub fn lerp(self, other: TextShaping, t: f32) -> TextShaping {
        if self == other {
            return self;
        }
        let t = t.clamp(0.0, 1.0);
        let (near, far) = (self.settings(), other.settings());
        let base = if t < 0.5 { &near } else { &far };
        let mut settings = Settings {
            features: base.features.clone(),
            variations: Vec::new(),
            locale: base.locale,
        };
        let value = |list: &Settings, tag: FontTag| {
            list.variations
                .iter()
                .find_map(|(t, bits)| (*t == tag).then(|| f32::from_bits(*bits)))
        };
        for &(tag, _) in near.variations.iter().chain(far.variations.iter()) {
            if settings.variations.iter().any(|(t, _)| *t == tag) {
                continue;
            }
            let v = match (value(&near, tag), value(&far, tag)) {
                (Some(a), Some(b)) => ((a + (b - a) * t) * 4.0).round() / 4.0,
                (a, b) => a.or(b).unwrap_or_default(),
            };
            settings.variations.push((tag, (v + 0.0).to_bits()));
        }
        intern(settings)
    }
}

impl fmt::Debug for TextShaping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return f.write_str("TextShaping::NONE");
        }
        let settings = self.settings();
        f.debug_struct("TextShaping")
            .field("features", &settings.features)
            .field("variations", &self.variations())
            .field("locale", &settings.locale)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_settings_are_the_same_value() {
        let a = TextShaping::NONE.with_feature("tnum", 1).with_locale("ja");
        let b = TextShaping::NONE.with_feature("tnum", 1).with_locale("ja-");
        assert_eq!(a, b);
        assert_ne!(
            a,
            TextShaping::NONE.with_feature("tnum", 0).with_locale("ja")
        );
        assert!(TextShaping::NONE.is_none());
        assert!(!a.is_none());
    }

    #[test]
    fn a_tag_set_twice_keeps_the_last_value() {
        let s = TextShaping::NONE
            .with_feature("liga", 1)
            .with_feature("liga", 0)
            .with_variation("wdth", 100.0)
            .with_variation("wdth", 75.0);
        assert_eq!(s.features(), vec![(FontTag(*b"liga"), 0)]);
        assert_eq!(s.variations(), vec![(FontTag(*b"wdth"), 75.0)]);
    }

    #[test]
    fn invalid_settings_are_ignored_not_panics() {
        let s = TextShaping::NONE
            .with_feature("tabular", 1)
            .with_feature("", 1)
            .with_variation("wght", f32::NAN)
            .with_locale("not a tag")
            .with_locale("x");
        assert!(s.is_none());
    }

    #[test]
    fn locale_tags_are_normalised() {
        assert_eq!(
            normalise_locale("ZH_hant_tw").as_deref(),
            Some("zh-Hant-TW")
        );
        assert_eq!(normalise_locale("sr-cyrl").as_deref(), Some("sr-Cyrl"));
        assert_eq!(normalise_locale("en-US").as_deref(), Some("en-US"));
        assert_eq!(normalise_locale("1a"), None);
    }

    #[test]
    fn merge_combines_by_tag_and_the_top_language_wins() {
        let below = TextShaping::NONE
            .with_feature("tnum", 1)
            .with_feature("liga", 1)
            .with_locale("en");
        let over = TextShaping::NONE.with_feature("liga", 0).with_locale("sr");
        let merged = below.merge(over);
        assert_eq!(
            merged.features(),
            vec![(FontTag(*b"tnum"), 1), (FontTag(*b"liga"), 0)]
        );
        assert_eq!(merged.locale(), Some("sr"));
        assert_eq!(below.merge(TextShaping::NONE), below);
        assert_eq!(TextShaping::NONE.merge(over), over);
    }

    #[test]
    fn an_axis_glides_and_a_feature_switches_halfway() {
        let light = TextShaping::NONE.with_variation("wght", 300.0);
        let heavy = TextShaping::NONE
            .with_variation("wght", 700.0)
            .with_feature("smcp", 1);
        let quarter = light.lerp(heavy, 0.25);
        assert_eq!(quarter.variation("wght"), Some(400.0));
        assert!(quarter.features().is_empty());
        let three_quarters = light.lerp(heavy, 0.75);
        assert_eq!(three_quarters.variation("wght"), Some(600.0));
        assert_eq!(three_quarters.features().len(), 1);
    }
}
