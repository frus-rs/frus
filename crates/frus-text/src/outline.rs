// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Text as outlines**: words shaped by the same font system as every `Text`, each glyph
//! turned into the path its font draws it with.
//!
//! Vector artwork — an SVG picture, a Lottie animation — places its words under transforms,
//! fills them with gradients and clips them like any other shape. Drawn as glyph outlines
//! they are shapes like the rest: sharp at any size, turned and filled as the artwork says.

use cosmic_text::{
    Attrs, Buffer, CacheKey, CacheKeyFlags, Command, Metrics, Shaping, SwashCache, Weight,
};
use frus_core::{FillRule, FontFamily, Path, Point};

use crate::{
    available_style, available_weight_value, contains_arabic, fallback_for, family_for,
    family_for_style, font_system, interned, match_family, monospace_family, sans_family, spaced,
    LOADED,
};

/// How a text is outlined: its font, its size, its spacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineStyle<'a> {
    /// The families asked for, in order of preference, as a CSS `font-family` list names
    /// them: the first one loaded draws. `serif`, `sans-serif` and `monospace` are the
    /// generic families; an empty list, or one naming nothing loaded, is the default family
    /// (and the console says which name was not found).
    pub families: &'a [&'a str],
    /// The font size, in the artwork's units.
    pub size: f32,
    /// The OpenType weight, 1 to 1000; resolved to a weight the fonts have, as text's is.
    pub weight: u16,
    /// Italic.
    pub italic: bool,
    /// Room added after every character, in the artwork's units.
    pub letter_spacing: f32,
}

impl Default for OutlineStyle<'_> {
    fn default() -> Self {
        OutlineStyle {
            families: &[],
            size: 16.0,
            weight: 400,
            italic: false,
            letter_spacing: 0.0,
        }
    }
}

/// One line of text as glyph outlines.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlinedLine {
    /// Every glyph's outline, filled non-zero, in the line's coordinates: the line starts
    /// at `x = 0` and its baseline is `y = 0`, `y` growing downwards.
    pub path: Path,
    /// How far the line advances: where the next text would start.
    pub advance: f32,
}

/// **`text` on one line, as outlines.** Line breaks in it are read as spaces.
pub fn outline_text(text: &str, style: &OutlineStyle) -> OutlinedLine {
    let one_line: String = text
        .chars()
        .map(|c| if is_line_break(c) { ' ' } else { c })
        .collect();
    outline_paragraph(&one_line, style, None)
        .into_iter()
        .next()
        .unwrap_or(OutlinedLine {
            path: Path::new().with_fill_rule(FillRule::NonZero),
            advance: 0.0,
        })
}

/// **`text` as outlined lines**: broken at its line breaks and, given a `max_width`,
/// wrapped between words to fit it. Each line is in its own coordinates (see
/// [`OutlinedLine`]); placing them — line height, alignment — is the caller's.
pub fn outline_paragraph(
    text: &str,
    style: &OutlineStyle,
    max_width: Option<f32>,
) -> Vec<OutlinedLine> {
    let size = style.size;
    if !(size.is_finite() && size > 0.0) {
        return Vec::new();
    }
    let mut font_system = font_system().lock().expect("FontSystem lock");
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(size, size * 1.2));
    match max_width.filter(|w| w.is_finite() && *w > 0.0) {
        Some(width) => buffer.set_size(Some(width), None),
        None => {
            buffer.set_wrap(cosmic_text::Wrap::None);
            buffer.set_size(None, None);
        }
    }
    let attrs = spaced(
        Attrs::new()
            .family(family_of_list(text, style.families))
            .weight(Weight(available_weight_value(style.weight.clamp(1, 1000))))
            .style(available_style(style.italic)),
        style.letter_spacing,
        size,
    );
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut font_system, false);

    let mut swash = SwashCache::new();
    let mut lines = Vec::new();
    for run in buffer.layout_runs() {
        // A line may be placed anywhere in the buffer (a right-to-left one at its end):
        // it is given here from its own start.
        let start = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
        let start = if start.is_finite() { start } else { 0.0 };
        let mut path = Path::new().with_fill_rule(FillRule::NonZero);
        for glyph in run.glyphs {
            let (mut key, _, _) = CacheKey::new(
                glyph.font_id,
                glyph.glyph_id,
                glyph.font_size,
                (0.0, 0.0),
                glyph.font_weight,
                // Unhinted: the outline is scaled with the artwork, and hinting is a fit
                // to one size's pixel grid.
                glyph.cache_key_flags | CacheKeyFlags::DISABLE_HINTING,
            );
            key.font_variations = glyph.font_variations;
            let Some(commands) = swash.get_outline_commands_uncached(&mut font_system, key) else {
                continue;
            };
            let ox = glyph.x + glyph.font_size * glyph.x_offset - start;
            let oy = glyph.y - glyph.font_size * glyph.y_offset;
            // The font's outlines go up from the baseline; the artwork's `y` goes down.
            let at = |x: f32, y: f32| Point::new(ox + x, oy - y);
            for command in commands.iter() {
                path = match *command {
                    Command::MoveTo(p) => path.move_to(at(p.x, p.y)),
                    Command::LineTo(p) => path.line_to(at(p.x, p.y)),
                    Command::QuadTo(c, p) => path.quad_to(at(c.x, c.y), at(p.x, p.y)),
                    Command::CurveTo(c1, c2, p) => {
                        path.cubic_to(at(c1.x, c1.y), at(c2.x, c2.y), at(p.x, p.y))
                    }
                    Command::Close => path.close(),
                };
            }
        }
        lines.push(OutlinedLine {
            path,
            advance: run.line_w,
        });
    }
    lines
}

fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{2028}' | '\u{2029}' | '\u{0b}' | '\u{0c}'
    )
}

/// The family to draw `text` in, from a `font-family` list.
fn family_of_list(text: &str, families: &[&str]) -> cosmic_text::Family<'static> {
    // As for a style naming a family: a run with Arabic keeps the face that draws it.
    if contains_arabic(text) {
        return family_for(text);
    }
    let mut first_named = None;
    for name in families {
        let name = name.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if name.is_empty() {
            continue;
        }
        match name.to_ascii_lowercase().as_str() {
            "sans-serif" | "system-ui" | "ui-sans-serif" | "-apple-system" => return sans_family(),
            "serif" | "ui-serif" => return family_for_style(text, Some(FontFamily::Serif)),
            "monospace" | "ui-monospace" => return monospace_family(),
            // No face is either: the default, unless a later name is loaded.
            "cursive" | "fantasy" | "emoji" | "math" | "fangsong" => continue,
            _ => {}
        }
        if let Some(found) = loaded(name) {
            return cosmic_text::Family::Name(found);
        }
        first_named.get_or_insert(name);
    }
    if let Some(name) = first_named {
        if let Some(fallback) = fallback_for("artwork's", name) {
            return cosmic_text::Family::Name(fallback);
        }
    }
    sans_family()
}

/// The loaded family `name` means, matched as a style's named family is. Only a name that
/// is found is kept: a picture fetched from anywhere does not grow the registry of names.
fn loaded(name: &str) -> Option<&'static str> {
    let guard = LOADED.read().expect("family registry");
    let loaded = guard.as_ref()?;
    match_family(name, &loaded.families, &loaded.app).map(interned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(path: &Path) -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for verb in path.verbs() {
            let points: Vec<Point> = match *verb {
                frus_core::PathVerb::MoveTo(p) | frus_core::PathVerb::LineTo(p) => vec![p],
                frus_core::PathVerb::QuadTo { ctrl, to } => vec![ctrl, to],
                frus_core::PathVerb::CubicTo { c1, c2, to } => vec![c1, c2, to],
                frus_core::PathVerb::Close => vec![],
            };
            for p in points {
                b = (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y));
            }
        }
        b
    }

    /// **A letter stands on the baseline**: an `H` at 100 units goes up from `y = 0` by its
    /// cap height and not below it, starts near `x = 0` and ends before its advance.
    #[test]
    fn a_letter_stands_on_its_baseline_from_its_start() {
        let line = outline_text(
            "H",
            &OutlineStyle {
                size: 100.0,
                ..OutlineStyle::default()
            },
        );
        assert!(!line.path.is_empty());
        assert_eq!(line.path.fill_rule(), FillRule::NonZero);
        let (x0, y0, x1, y1) = bounds(&line.path);
        assert!(y1.abs() < 0.5, "the H sits on the baseline: bottom {y1}");
        assert!(
            (-80.0..-60.0).contains(&y0),
            "a cap height above it: top {y0}"
        );
        assert!((0.0..20.0).contains(&x0), "starts near 0: {x0}");
        assert!(
            x1 < line.advance && line.advance > 50.0,
            "{x1} {}",
            line.advance
        );
    }

    /// **The outline scales with the size, unhinted**: twice the size is the same letter
    /// twice as large, so artwork can be scaled after it is outlined.
    #[test]
    fn twice_the_size_is_the_same_outline_twice_as_large() {
        let at = |size| {
            outline_text(
                "a",
                &OutlineStyle {
                    size,
                    ..OutlineStyle::default()
                },
            )
        };
        let (small, large) = (at(50.0), at(100.0));
        let (a, b) = (bounds(&small.path), bounds(&large.path));
        for (s, l) in [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)] {
            assert!((s * 2.0 - l).abs() < 0.05, "{s} x 2 vs {l}");
        }
        assert!((small.advance * 2.0 - large.advance).abs() < 0.05);
    }

    /// **Weight, spacing and the family list reach the glyphs**: bold is wider, spacing
    /// adds its room after every character, and an unknown family in front of a loaded
    /// generic one draws in that one.
    #[test]
    fn weight_spacing_and_families_reach_the_glyphs() {
        let base = OutlineStyle {
            size: 40.0,
            ..OutlineStyle::default()
        };
        let regular = outline_text("Wide", &base);
        let bold = outline_text(
            "Wide",
            &OutlineStyle {
                weight: 700,
                ..base
            },
        );
        assert!(bold.advance > regular.advance, "bold is wider");
        let spaced = outline_text(
            "Wide",
            &OutlineStyle {
                letter_spacing: 5.0,
                ..base
            },
        );
        assert!(
            (spaced.advance - regular.advance - 20.0).abs() < 0.5,
            "four letters, 5 each: {} vs {}",
            spaced.advance,
            regular.advance
        );
        let mono = outline_text(
            "iiii",
            &OutlineStyle {
                families: &["No Such Family", "monospace"],
                ..base
            },
        );
        let proportional = outline_text("iiii", &base);
        assert!(
            mono.advance > proportional.advance * 1.3,
            "monospace i's are as wide as any letter: {} vs {}",
            mono.advance,
            proportional.advance
        );
    }

    /// **A paragraph breaks at its line breaks and wraps to a width**, every line given
    /// from its own start.
    #[test]
    fn a_paragraph_breaks_and_wraps() {
        let style = OutlineStyle {
            size: 20.0,
            ..OutlineStyle::default()
        };
        assert_eq!(outline_paragraph("one\ntwo", &style, None).len(), 2);
        let wrapped = outline_paragraph("one two three four", &style, Some(60.0));
        assert!(wrapped.len() >= 3, "{} lines", wrapped.len());
        for line in &wrapped {
            assert!(line.advance <= 60.5, "{}", line.advance);
            let (x0, ..) = bounds(&line.path);
            assert!((0.0..5.0).contains(&x0), "from its own start: {x0}");
        }
        assert!(outline_text("one\ntwo", &style).advance > 0.0);
        assert!(outline_paragraph("x", &OutlineStyle { size: 0.0, ..style }, None).is_empty());
    }
}
