// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! OpenType features, variable font axes, the language of a run, and the platform's fallback
//! fonts (milestone 784).
//!
//! What a style says about shaping travels as a [`TextShaping`]; [`shaped`] turns it into the
//! shaper's attributes, at the one place every measurement and every paint goes through, so
//! the two cannot disagree.
//!
//! The fallback half answers *which font draws a character the application's fonts do not
//! have*. The desktop platforms keep every system font in the database from the start and
//! have per-script lists of the platform's own faces; what was missing there was the
//! language: Han characters were drawn in the Simplified Chinese face whatever the text's
//! language, because the lists were chosen once for the device's locale and matched only a
//! bare `ja`. Android has no list at all and its fonts are not in the database, so anything
//! outside the bundled faces was a box. Here the platform's own `fonts.xml` is read, and a
//! script's faces are loaded the first time text in it is shaped.

use std::sync::RwLock;

use cosmic_text::{Attrs, FeatureTag, FontFeatures, FontVariations, PlatformFallback, Weight};
use frus_core::TextShaping;
use unicode_script::Script;

/// The language text is shaped in when its style names none: the application's.
static DEFAULT_LOCALE: RwLock<Option<&'static str>> = RwLock::new(None);

/// Sets the language text is shaped in when its style names none — the application's locale,
/// a BCP 47 tag such as `ja` or `zh-Hant-TW`. The shell sets it from the locale the
/// application resolved, so an application does not call this itself.
///
/// It decides the glyph forms a font localizes and, for the Han characters Chinese, Japanese
/// and Korean share, which of the platform's fonts draws them.
///
/// ```
/// frus_text::set_default_locale("ja-JP");
/// assert_eq!(frus_text::default_locale(), Some("ja-JP"));
/// ```
pub fn set_default_locale(tag: &str) {
    let wanted = TextShaping::NONE.with_locale(tag).locale();
    let mut current = DEFAULT_LOCALE.write().unwrap_or_else(|e| e.into_inner());
    if *current != wanted {
        *current = wanted;
        drop(current);
        // Measurements taken in the old language may have used other faces.
        super::forget_measurements();
    }
}

/// The language text is shaped in when its style names none, if one was set.
pub fn default_locale() -> Option<&'static str> {
    *DEFAULT_LOCALE.read().unwrap_or_else(|e| e.into_inner())
}

/// The language `shaping` puts its text in: its own, or the application's.
pub(crate) fn locale_of(shaping: TextShaping) -> Option<&'static str> {
    if shaping.is_none() {
        default_locale()
    } else {
        shaping.locale().or_else(default_locale)
    }
}

/// `attrs` with what `shaping` says: its OpenType features, its variable font axes and its
/// language — the application's when it names none. Every place that shapes text goes through
/// this, so a measure and a paint shape the same words alike.
///
/// A `wght` axis takes over the weight, resolved like a weight is, against the faces of the
/// family `attrs` names when the application registered it: a variable face spanning it draws
/// exactly that, and a family of static faces draws its nearest, never a weight the shaper
/// would have to look for in the platform's fallback lists.
///
/// ```
/// use frus_core::TextStyle;
/// let style = TextStyle::new(14.0).font_feature("tnum", 1).resolved();
/// let attrs = frus_text::shaped(cosmic_text::Attrs::new(), style.shaping);
/// assert_eq!(attrs.font_features.features.len(), 1);
/// ```
pub fn shaped(mut attrs: Attrs<'static>, shaping: TextShaping) -> Attrs<'static> {
    let language = locale_of(shaping);
    if !shaping.is_none() {
        let mut features = FontFeatures::new();
        for (tag, value) in shaping.features() {
            features.set(FeatureTag::new(&tag.0), value);
        }
        let mut variations = FontVariations::new();
        for (tag, value) in shaping.variations() {
            if &tag.0 == b"wght" {
                let wanted = value.round().clamp(1.0, 1000.0) as u16;
                let weight = super::available_weight_in(attrs.family, wanted);
                attrs = attrs.weight(Weight(weight));
            } else {
                variations.set(FeatureTag::new(&tag.0), value);
            }
        }
        attrs = attrs.font_features(features).font_variations(variations);
    }
    attrs.language(language)
}

/// The locale `script` is looked up in: the one that decides which face draws the Han
/// characters, reduced to the four answers the platforms' lists know.
fn han_locale(locale: &str) -> &'static str {
    let mut parts = locale.split('-');
    let language = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    match language {
        "ja" => "ja",
        "ko" => "ko",
        "zh" => {
            let has = |subtag: &str| rest.iter().any(|part| part.eq_ignore_ascii_case(subtag));
            if has("HK") || has("MO") {
                "zh-HK"
            } else if has("Hant") || has("TW") {
                "zh-TW"
            } else {
                "zh-CN"
            }
        }
        _ => "zh-CN",
    }
}

/// The scripts whose fallback face depends on the language.
fn is_han_like(script: Script) -> bool {
    matches!(
        script,
        Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul | Script::Bopomofo
    )
}

/// The locale a script's fallback is chosen for: one of the four the Han lists know for the
/// scripts that depend on it, `und` for every other, whose faces do not.
fn fallback_locale(script: Script, locale: &str) -> &str {
    if is_han_like(script) {
        han_locale(locale)
    } else {
        "und"
    }
}

/// Fonts on disk that are not in the database until a script needs them, and the platform's
/// description of which serve which script: Android's `/system/fonts` and `fonts.xml`.
pub(crate) struct PlatformFonts {
    pub families: Vec<android::Family>,
    pub dir: std::path::PathBuf,
}

/// frus's font fallback: the platform's lists, chosen **per run's language**, and where the
/// platform's fonts are not loaded up front (Android), its own faces, loaded when a script
/// first needs them.
pub(crate) struct FrusFallback {
    pub platform: Option<PlatformFonts>,
}

impl FrusFallback {
    /// The fallback for the platform this runs on.
    pub(crate) fn for_platform() -> Self {
        #[cfg(target_os = "android")]
        let platform = Some(PlatformFonts {
            families: android::platform_fonts(),
            dir: std::path::PathBuf::from(super::ANDROID_FONT_DIR),
        });
        #[cfg(not(target_os = "android"))]
        let platform = None;
        FrusFallback { platform }
    }
}

impl cosmic_text::Fallback for FrusFallback {
    fn common_fallback(&self) -> &[&'static str] {
        PlatformFallback.common_fallback()
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        PlatformFallback.forbidden_fallback()
    }

    fn script_fallback(&self, script: Script, locale: &str) -> &[&'static str] {
        if self.platform.is_some() {
            let loaded = android::script_families(script, fallback_locale(script, locale));
            if !loaded.is_empty() {
                return loaded;
            }
        }
        let locale = if is_han_like(script) {
            han_locale(locale)
        } else {
            locale
        };
        PlatformFallback.script_fallback(script, locale)
    }

    fn load_faces(
        &self,
        script: Script,
        locale: &str,
        db: &mut cosmic_text::fontdb::Database,
    ) -> bool {
        match &self.platform {
            Some(platform) => android::load(
                &platform.families,
                &platform.dir,
                script,
                fallback_locale(script, locale),
                db,
            ),
            None => false,
        }
    }
}

/// Android's fallback fonts, read from the platform's `fonts.xml`.
pub(crate) mod android {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    use unicode_script::Script;

    /// One fallback family of `fonts.xml`: the languages it is for and its files.
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct Family {
        /// The `lang` attribute's tags: `und-Thai`, `ja`, `zh-Hans`.
        pub langs: Vec<String>,
        /// `compact` or `elegant`, when the family says.
        pub variant: Option<String>,
        /// The files and the face index inside each, regular first.
        pub fonts: Vec<(String, u32)>,
    }

    /// Reads the fallback families — those with a `lang` — of a `fonts.xml`. A tolerant
    /// reader of the few elements that matter: a file it does not understand gives fewer
    /// families, never an error.
    // Read only on Android, where the platform has a `fonts.xml`, and by the tests.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn parse(xml: &str) -> Vec<Family> {
        let mut families = Vec::new();
        let mut rest = xml;
        while let Some(start) = rest.find("<family") {
            rest = &rest[start + "<family".len()..];
            // `<familyset>` is not a family.
            if !rest.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
                continue;
            }
            let Some(head_end) = rest.find('>') else {
                break;
            };
            let head = &rest[..head_end];
            let self_closing = head.ends_with('/');
            let body_end = if self_closing {
                head_end
            } else {
                rest.find("</family>").unwrap_or(rest.len())
            };
            let body = &rest[head_end.min(body_end)..body_end];
            if let Some(lang) = attribute(head, "lang") {
                let mut fonts = Vec::new();
                let mut inner = body;
                while let Some(font_start) = inner.find("<font") {
                    inner = &inner[font_start + "<font".len()..];
                    let Some(open_end) = inner.find('>') else {
                        break;
                    };
                    let open = &inner[..open_end];
                    let Some(close) = inner.find("</font>") else {
                        break;
                    };
                    let file = inner[open_end + 1..close].trim();
                    // A `<font>` element may hold `<axis>` children after its file name.
                    let file = file.split('<').next().unwrap_or("").trim();
                    let index = attribute(open, "index")
                        .and_then(|i| i.parse().ok())
                        .unwrap_or(0);
                    let regular = attribute(open, "weight").is_none_or(|w| w == "400")
                        && attribute(open, "style").is_none_or(|s| s == "normal");
                    if !file.is_empty() {
                        if regular {
                            fonts.insert(0, (file.to_string(), index));
                        } else {
                            fonts.push((file.to_string(), index));
                        }
                    }
                    inner = &inner[close..];
                }
                families.push(Family {
                    langs: lang
                        .split([' ', ','])
                        .filter(|l| !l.is_empty())
                        .map(String::from)
                        .collect(),
                    variant: attribute(head, "variant").map(String::from),
                    fonts,
                });
            }
            rest = &rest[body_end..];
        }
        families
    }

    /// The value of `name="..."` in an element's opening tag.
    // Read only on Android, where the platform has a `fonts.xml`, and by the tests.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
        let mut at = 0;
        while let Some(found) = tag[at..].find(name) {
            let start = at + found;
            let before = tag[..start].chars().next_back();
            let after = &tag[start + name.len()..];
            at = start + name.len();
            if before.is_some_and(|c| !c.is_whitespace()) {
                continue;
            }
            let after = after.trim_start();
            let Some(after) = after.strip_prefix('=') else {
                continue;
            };
            let after = after.trim_start();
            let quote = after.chars().next()?;
            if quote != '"' && quote != '\'' {
                continue;
            }
            let value = &after[1..];
            return value.find(quote).map(|end| &value[..end]);
        }
        None
    }

    /// Whether `family` is for `script` written in `locale` (already reduced to `ja`, `ko`,
    /// `zh-CN`, `zh-TW` or `zh-HK` for the Han-like scripts).
    fn serves(family: &Family, script: Script, locale: &str) -> bool {
        let code = script.short_name();
        family.langs.iter().any(|lang| {
            let mut parts = lang.split('-');
            let language = parts.next().unwrap_or("");
            let subtags: Vec<&str> = parts.collect();
            if subtags.iter().any(|s| s.eq_ignore_ascii_case(code)) {
                return true;
            }
            match script {
                Script::Han | Script::Bopomofo => match locale {
                    "ja" => language == "ja",
                    "ko" => language == "ko",
                    "zh-TW" | "zh-HK" => language == "zh" && subtags.contains(&"Hant"),
                    _ => language == "zh" && subtags.contains(&"Hans"),
                },
                Script::Hiragana | Script::Katakana => language == "ja",
                Script::Hangul => language == "ko",
                _ => false,
            }
        })
    }

    /// The families for `script` in `locale`, the `compact` variant first: it is the one
    /// Android draws interface text with.
    fn families_for<'a>(all: &'a [Family], script: Script, locale: &str) -> Vec<&'a Family> {
        let mut found: Vec<&Family> = all.iter().filter(|f| serves(f, script, locale)).collect();
        found.sort_by_key(|f| f.variant.as_deref() != Some("compact"));
        found
    }

    /// Android's fallback families, read once from `/system/etc/fonts.xml`.
    #[cfg(target_os = "android")]
    pub(crate) fn platform_fonts() -> Vec<Family> {
        static FAMILIES: OnceLock<Vec<Family>> = OnceLock::new();
        FAMILIES
            .get_or_init(|| {
                ["/system/etc/fonts.xml", "/system/etc/system_fonts.xml"]
                    .iter()
                    .find_map(|path| std::fs::read_to_string(path).ok())
                    .map(|xml| parse(&xml))
                    .unwrap_or_default()
            })
            .clone()
    }

    /// Family names per (script, locale).
    type Names = HashMap<(Script, String), &'static [&'static str]>;

    /// The family names loaded for each (script, locale), for the fallback lists.
    fn names() -> &'static Mutex<Names> {
        static NAMES: OnceLock<Mutex<Names>> = OnceLock::new();
        NAMES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// The family names [`load`] found for `script` in `locale`; empty before it ran.
    pub(crate) fn script_families(script: Script, locale: &str) -> &'static [&'static str] {
        names()
            .lock()
            .ok()
            .and_then(|names| names.get(&(script, locale.to_string())).copied())
            .unwrap_or(&[])
    }

    /// Loads the faces of `families` that serve `script` in `locale` from `dir` into `db`,
    /// once per file, and remembers their family names for [`script_families`]. Returns
    /// whether a family name was found to fall back to.
    pub(crate) fn load(
        families: &[Family],
        dir: &Path,
        script: Script,
        locale: &str,
        db: &mut cosmic_text::fontdb::Database,
    ) -> bool {
        let mut found: Vec<&'static str> = Vec::new();
        for family in families_for(families, script, locale) {
            for (file, index) in &family.fonts {
                let path: PathBuf = dir.join(file);
                let loaded = |db: &cosmic_text::fontdb::Database| {
                    db.faces()
                        .any(|face| face_path(face) == Some(path.as_path()))
                };
                if !loaded(db) {
                    if db.load_font_file(&path).is_err() {
                        continue;
                    }
                    log::info!(
                        "frus: loaded the platform font {} for {script:?} text",
                        path.display()
                    );
                }
                let name = db
                    .faces()
                    .find(|face| face_path(face) == Some(path.as_path()) && face.index == *index)
                    .and_then(|face| face.families.first().map(|(name, _)| name.clone()));
                if let Some(name) = name {
                    if !found.iter().any(|known| *known == name) {
                        found.push(Box::leak(name.into_boxed_str()));
                    }
                }
            }
        }
        if found.is_empty() {
            return false;
        }
        if let Ok(mut names) = names().lock() {
            names.insert(
                (script, locale.to_string()),
                Box::leak(found.into_boxed_slice()),
            );
        }
        true
    }

    fn face_path(face: &cosmic_text::fontdb::FaceInfo) -> Option<&Path> {
        match &face.source {
            cosmic_text::fontdb::Source::File(path) => Some(path.as_path()),
            cosmic_text::fontdb::Source::SharedFile(path, _) => Some(path.as_path()),
            cosmic_text::fontdb::Source::Binary(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn han_locales_reduce_to_the_four_lists() {
        assert_eq!(han_locale("ja-JP"), "ja");
        assert_eq!(han_locale("ko-KR"), "ko");
        assert_eq!(han_locale("zh-Hant-TW"), "zh-TW");
        assert_eq!(han_locale("zh-TW"), "zh-TW");
        assert_eq!(han_locale("zh-HK"), "zh-HK");
        assert_eq!(han_locale("zh-Hans-CN"), "zh-CN");
        assert_eq!(han_locale("en-US"), "zh-CN");
    }

    const FONTS_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<familyset version="23">
    <family name="sans-serif">
        <font weight="400" style="normal">Roboto-Regular.ttf</font>
    </family>
    <family lang="und-Thai" variant="elegant">
        <font weight="400" style="normal">NotoSansThai-Regular.ttf</font>
    </family>
    <family lang="und-Thai" variant="compact">
        <font weight="700" style="normal">NotoSansThaiUI-Bold.ttf</font>
        <font weight="400" style="normal">NotoSansThaiUI-Regular.ttf</font>
    </family>
    <family lang="zh-Hans">
        <font weight="400" style="normal" index="2">NotoSansCJK-Regular.ttc</font>
    </family>
    <family lang="ja">
        <font weight="400" style="normal" index="0">NotoSansCJK-Regular.ttc
            <axis tag="wght" stylevalue="400" />
        </font>
    </family>
    <family lang="und-Zsye">
        <font weight="400" style="normal">NotoColorEmoji.ttf</font>
    </family>
</familyset>"#;

    #[test]
    fn fonts_xml_fallback_families_are_read() {
        let families = android::parse(FONTS_XML);
        assert_eq!(
            families.len(),
            5,
            "the named family has no lang and is not a fallback"
        );
        assert_eq!(families[1].variant.as_deref(), Some("compact"));
        assert_eq!(
            families[1].fonts[0],
            ("NotoSansThaiUI-Regular.ttf".to_string(), 0),
            "regular first"
        );
        assert_eq!(
            families[2].fonts,
            vec![("NotoSansCJK-Regular.ttc".to_string(), 2)]
        );
        assert_eq!(
            families[3].fonts,
            vec![("NotoSansCJK-Regular.ttc".to_string(), 0)]
        );
    }

    /// The Android loader with a stand-in `fonts.xml`: a script no loaded face covers is
    /// served by the platform face its `fonts.xml` names, loaded on first use.
    #[test]
    fn a_platform_face_is_loaded_for_a_script_on_first_use() {
        let dir = std::env::temp_dir().join(format!("frus-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // DejaVu Sans covers Georgian, which nothing else in this database does.
        std::fs::write(
            dir.join("Georgian.ttf"),
            include_bytes!("../assets/DejaVuSans.ttf"),
        )
        .unwrap();
        let families = android::parse(
            r#"<familyset><family lang="und-Geor"><font weight="400" style="normal">Georgian.ttf</font></family></familyset>"#,
        );
        let mut db = cosmic_text::fontdb::Database::new();
        assert!(android::load(
            &families,
            &dir,
            Script::Georgian,
            "en",
            &mut db
        ));
        assert_eq!(db.faces().count(), 1);
        assert_eq!(
            android::script_families(Script::Georgian, "en"),
            ["DejaVu Sans"]
        );
        // Once per file: asking again loads nothing more.
        assert!(android::load(
            &families,
            &dir,
            Script::Georgian,
            "en",
            &mut db
        ));
        assert_eq!(db.faces().count(), 1);
        // A script nothing serves loads nothing.
        assert!(!android::load(&families, &dir, Script::Thai, "en", &mut db));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use cosmic_text::{
        fontdb, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
    };

    const DEJAVU: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
    const DEJAVU_MONO: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");
    const VARIABLE: &[u8] = include_bytes!("../tests/fonts/FrusTestVariable.ttf");

    /// A font system holding exactly `faces`, with frus's fallback and nothing of the
    /// platform's.
    fn system_of(faces: &[&[u8]], platform: Option<PlatformFonts>) -> FontSystem {
        let mut db = fontdb::Database::new();
        for face in faces {
            db.load_font_data(face.to_vec());
        }
        FontSystem::new_with_locale_and_db_and_fallback(
            "en-US".into(),
            db,
            FrusFallback { platform },
        )
    }

    /// The glyphs `text` shapes to: id, font, width in pixels at 100 px.
    fn glyphs(fs: &mut FontSystem, text: &str, attrs: &Attrs) -> Vec<(u16, fontdb::ID, f32)> {
        let mut buffer = Buffer::new(fs, Metrics::new(100.0, 120.0));
        buffer.set_text(text, attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(fs, false);
        buffer
            .layout_runs()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(|g| (g.glyph_id, g.font_id, g.w))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn family_of(fs: &FontSystem, id: fontdb::ID) -> String {
        fs.db()
            .face(id)
            .map(|face| face.families[0].0.clone())
            .unwrap_or_default()
    }

    /// **A feature changes the shaping**: DejaVu Sans draws "fi" as one ligature glyph, and
    /// with `liga` off as two.
    #[test]
    fn a_feature_changes_the_glyphs() {
        let mut fs = system_of(&[DEJAVU], None);
        let sans = Attrs::new().family(Family::Name("DejaVu Sans"));
        let style = |shaping: TextShaping| shaped(sans.clone(), shaping);
        assert_eq!(
            glyphs(&mut fs, "fi", &style(TextShaping::NONE)).len(),
            1,
            "a ligature by default"
        );
        assert_eq!(
            glyphs(
                &mut fs,
                "fi",
                &style(TextShaping::NONE.with_feature("liga", 0))
            )
            .len(),
            2,
            "liga off: two letters"
        );
        // Per span: the feature applies to its own run of a line and not to the next.
        let mut buffer = Buffer::new(&mut fs, Metrics::new(100.0, 120.0));
        let off = style(TextShaping::NONE.with_feature("liga", 0));
        let on = style(TextShaping::NONE);
        buffer.set_rich_text([("fi", off), ("fi", on)], &sans, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut fs, false);
        let count: usize = buffer.layout_runs().map(|run| run.glyphs.len()).sum();
        assert_eq!(count, 3, "two letters, then a ligature");
    }

    /// **The language selects the glyphs**: DejaVu Sans has Serbian forms of the Cyrillic
    /// letters (its `locl` for `SRB`), so "б" shapes to another glyph in Serbian than in
    /// Russian.
    #[test]
    fn the_language_selects_a_localized_glyph() {
        let mut fs = system_of(&[DEJAVU], None);
        let sans = Attrs::new().family(Family::Name("DejaVu Sans"));
        let in_language = |fs: &mut FontSystem, tag: &str| {
            glyphs(
                fs,
                "б",
                &shaped(sans.clone(), TextShaping::NONE.with_locale(tag)),
            )[0]
            .0
        };
        let russian = in_language(&mut fs, "ru");
        let serbian = in_language(&mut fs, "sr");
        assert_ne!(russian, serbian, "Serbian Cyrillic has its own be");
        assert_eq!(in_language(&mut fs, "sr-Cyrl-RS"), serbian);
        // No language named and none set for the application: the default form.
        let attrs = sans.clone().language(None);
        assert_eq!(glyphs(&mut fs, "б", &attrs)[0].0, russian);
    }

    /// **An axis changes the advances, and the drawn glyph**: the test font's `wdth` 50 is
    /// half as wide, and its `wght` 700 a fifth wider — reached through the weight, which a
    /// variable face spans.
    #[test]
    fn an_axis_changes_advance_widths_and_the_drawn_glyph() {
        let mut fs = system_of(&[VARIABLE], None);
        let face = Attrs::new().family(Family::Name("Frus Test Variable"));
        let condensed = || {
            let mut v = FontVariations::new();
            v.set(FeatureTag::new(b"wdth"), 50.0);
            face.clone().font_variations(v)
        };
        let width = |fs: &mut FontSystem, attrs: &Attrs| -> f32 {
            glyphs(fs, "HHHH", attrs).iter().map(|g| g.2).sum()
        };
        let normal = width(&mut fs, &face);
        let narrow = width(&mut fs, &condensed());
        let heavy = width(&mut fs, &face.clone().weight(Weight(700)));
        let between = width(&mut fs, &face.clone().weight(Weight(550)));
        assert!(
            (narrow / normal - 0.5).abs() < 0.01,
            "wdth 50: {narrow} vs {normal}"
        );
        assert!(
            (heavy / normal - 1.2).abs() < 0.01,
            "wght 700: {heavy} vs {normal}"
        );
        assert!(
            between > normal && between < heavy,
            "wght 550 is in between: {between}"
        );

        // The rasterizer draws the instance the shaper measured: a narrower bitmap.
        let mut cache = SwashCache::new();
        let mut image_width = |fs: &mut FontSystem, attrs: &Attrs| {
            let mut buffer = Buffer::new(fs, Metrics::new(100.0, 120.0));
            buffer.set_text("H", attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(fs, false);
            let run = buffer.layout_runs().next().unwrap();
            let key = run.glyphs[0].physical((0.0, 0.0), 1.0).cache_key;
            let image = cache.get_image_uncached(fs, key).expect("an image");
            assert_eq!(image.content, SwashContent::Mask);
            image.placement.width
        };
        let wide = image_width(&mut fs, &face);
        let narrow_image = image_width(&mut fs, &condensed());
        assert!(
            (narrow_image as f32) < wide as f32 * 0.7,
            "the condensed H is drawn condensed: {narrow_image} vs {wide}"
        );
    }

    /// **A `wght` axis set on a style takes over the weight**, and is resolved like a weight:
    /// never one the default family cannot serve.
    #[test]
    fn a_wght_axis_becomes_the_weight() {
        let attrs = shaped(
            Attrs::new(),
            TextShaping::NONE.with_variation("wght", 700.0),
        );
        assert_eq!(attrs.weight, Weight(700));
        assert!(
            attrs.font_variations.is_empty(),
            "wght is not passed as an axis"
        );
        let attrs = shaped(Attrs::new(), TextShaping::NONE.with_variation("wdth", 80.0));
        assert_eq!(attrs.font_variations.variations.len(), 1);
    }

    /// **A missing glyph is drawn by a platform face, loaded when first needed**: Hebrew is
    /// not in DejaVu Sans Mono, and the stand-in `fonts.xml` names a face that has it.
    /// Without the platform's fonts, the same text has no glyphs (a box).
    #[test]
    fn a_missing_glyph_resolves_to_a_platform_font() {
        let text = "\u{05e9}\u{05dc}\u{05d5}\u{05dd}";
        let mono = Attrs::new().family(Family::Name("DejaVu Sans Mono"));

        let mut bare = system_of(&[DEJAVU_MONO], None);
        assert!(
            glyphs(&mut bare, text, &mono).iter().all(|g| g.0 == 0),
            "nothing draws Hebrew"
        );

        let dir = std::env::temp_dir().join(format!("frus-fallback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Hebrew.ttf"), DEJAVU).unwrap();
        let families = android::parse(
            r#"<familyset><family lang="und-Hebr"><font weight="400">Hebrew.ttf</font></family></familyset>"#,
        );
        let platform = PlatformFonts {
            families,
            dir: dir.clone(),
        };
        let mut fs = system_of(&[DEJAVU_MONO], Some(platform));
        assert_eq!(fs.db().faces().count(), 1, "nothing is loaded up front");
        let drawn = glyphs(&mut fs, text, &mono);
        assert!(
            drawn.iter().all(|g| g.0 != 0),
            "every letter has a glyph: {drawn:?}"
        );
        assert!(drawn.iter().all(|g| family_of(&fs, g.1) == "DejaVu Sans"));
        assert_eq!(fs.db().faces().count(), 2, "the platform face was loaded");
        // Latin text loads nothing more.
        glyphs(&mut fs, "Hello", &mono);
        assert_eq!(fs.db().faces().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Han unification follows the language**: the same character is drawn by the
    /// platform's Japanese face in Japanese and its Simplified Chinese face in Chinese.
    /// Needs the platform's CJK faces, so where they are missing it says so and stops.
    #[test]
    fn han_characters_take_the_face_of_their_language() {
        let mut fs = FontSystem::new();
        fs.set_fallback(FrusFallback::for_platform());
        fs.db_mut().load_font_data(DEJAVU.to_vec());
        let sans = Attrs::new().family(Family::Name("DejaVu Sans"));
        let face_for = |fs: &mut FontSystem, tag: &str| {
            let g = glyphs(
                fs,
                "\u{76f4}",
                &shaped(sans.clone(), TextShaping::NONE.with_locale(tag)),
            );
            (g[0].0 != 0).then(|| family_of(fs, g[0].1))
        };
        let (Some(japanese), Some(chinese), Some(korean)) = (
            face_for(&mut fs, "ja-JP"),
            face_for(&mut fs, "zh-Hans"),
            face_for(&mut fs, "ko"),
        ) else {
            eprintln!("no CJK faces on this machine; Han unification not checked");
            return;
        };
        eprintln!("ja: {japanese}, zh: {chinese}, ko: {korean}");
        if cfg!(any(windows, target_os = "macos")) {
            assert_ne!(japanese, chinese, "ja and zh differ");
            assert_ne!(korean, chinese, "ko and zh differ");
        }
    }

    /// **Thai breaks between words.** "I eat rice at home" in Thai has no space in it.
    /// Given the width of its first two words, every line must start at a word. Without the
    /// dictionary there is no break opportunity at all, and the shaper cuts through words
    /// wherever a glyph overflows.
    #[test]
    fn thai_wraps_at_word_boundaries() {
        let words = [
            "\u{0e09}\u{0e31}\u{0e19}",
            "\u{0e01}\u{0e34}\u{0e19}",
            "\u{0e02}\u{0e49}\u{0e32}\u{0e27}",
            "\u{0e17}\u{0e35}\u{0e48}",
            "\u{0e1a}\u{0e49}\u{0e32}\u{0e19}",
        ];
        let text: String = words.concat();
        let mut boundaries = vec![0];
        for word in &words {
            boundaries.push(boundaries.last().unwrap() + word.len());
        }
        let style = frus_core::TextStyle::new(20.0).resolved();
        let whole = crate::measure_resolved(&text, &style).width;
        // A line narrower than a word must cut it; from the widest word to the whole
        // sentence, every line must break between words.
        let widest = words
            .iter()
            .map(|word| crate::measure_resolved(word, &style).width)
            .fold(0.0_f32, f32::max);
        for step in 0..8 {
            let width = widest + 1.0 + (whole - widest - 2.0) * step as f32 / 8.0;
            let spans = crate::line_spans_resolved(&text, &style, Some(width), true);
            assert!(spans.len() >= 2, "it wraps at {width}: {spans:?}");
            for span in &spans {
                assert!(
                    boundaries.contains(&span.start) && boundaries.contains(&span.end),
                    "at {width}, a line breaks inside a word: {spans:?} (words at {boundaries:?})"
                );
            }
        }
    }
}
