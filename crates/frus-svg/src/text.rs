// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **`<text>` drawn as glyph outlines**, shaped by frus's own text stack.
//!
//! The parser resolves everything about a picture but its words: it would lay them out with
//! a font stack of its own, which frus does not carry. So text is laid out here, before the
//! parser sees the file, and every `<text>` element is replaced by what the parser does
//! understand — groups carrying the element's and its `<tspan>`s' attributes (paint,
//! transform, opacity, classes, styles), each holding a placeholder rectangle with a
//! reserved `id`. The parser then resolves for each placeholder what it resolves for any
//! shape: its fill and stroke (gradients, inherited values and style sheets included), its
//! transform from any depth of groups and `<use>`, its place among the other shapes, its
//! clip and mask. Where the picture is turned into shapes, a placeholder is swapped for the
//! glyph outlines laid out here.
//!
//! A placeholder covers the whole text element's glyphs, so a gradient or a filter region
//! given in `objectBoundingBox` units spans the text, as it would.
//!
//! What is laid out: `x`, `y`, `dx` and `dy` (single values and lists, one per character),
//! `<tspan>` and `<a>` with their own positions and styles, `text-anchor` per text chunk,
//! `font-family` lists, `font-size` (lengths, `em`, percentages and keywords), `font-weight`
//! (numbers, `bold`, `bolder`, `lighter`), `font-style`, `letter-spacing`, and
//! `xml:space`. These are read from attributes, `style` attributes and the picture's
//! `<style>` sheets.

use std::collections::HashMap;
use std::fmt::Write as _;

use frus_core::{FillRule, Path, PathVerb};
use usvg::roxmltree::{self, Node, NodeId};

/// The `id` prefix of a placeholder; the number after it indexes [`Prepared::spans`].
pub(crate) const MARKER: &str = "__frus_text_";

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// A picture's text laid out, and its source rewritten for the parser.
pub(crate) struct Prepared {
    /// The source, every `<text>` replaced by placeholders.
    pub source: String,
    /// The glyph outlines of each placeholder, in its text element's own coordinates.
    pub spans: Vec<Path>,
    /// What the text uses that is not drawn.
    pub notes: Vec<&'static str>,
}

/// Lays out the text of `source`. `None` when it has no text, or is not XML the
/// parser would read: it is then parsed as it is.
pub(crate) fn prepare(source: &str) -> Option<Prepared> {
    if !source.contains("text") {
        return None;
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(source, options).ok()?;
    let texts: Vec<Node> = doc.descendants().filter(|n| is_svg(*n, "text")).collect();
    if texts.is_empty() {
        return None;
    }
    let css: String = doc
        .descendants()
        .filter(|n| is_svg(*n, "style"))
        .flat_map(|n| n.children().filter_map(|c| c.text()))
        .collect::<Vec<_>>()
        .join("\n");
    let styles = Styles::new(&css);
    let mut out = Prepared {
        source: String::with_capacity(source.len()),
        spans: Vec::new(),
        notes: Vec::new(),
    };
    let mut copied = 0;
    for text in texts {
        let range = text.range();
        if range.start < copied {
            continue; // inside a text already replaced (not valid SVG, but not ours to draw)
        }
        out.source.push_str(&source[copied..range.start]);
        let markup = layout(text, &styles, &mut out.spans, &mut out.notes);
        out.source.push_str(&markup);
        copied = range.end;
    }
    out.source.push_str(&source[copied..]);
    Some(out)
}

fn is_svg(node: Node, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node.tag_name().namespace() == Some(SVG_NS)
}

fn note(notes: &mut Vec<&'static str>, what: &'static str) {
    if !notes.contains(&what) {
        notes.push(what);
    }
}

// --- Style: attributes, `style` attributes and style sheets ---

/// The picture's style sheets, their rules in the order they apply.
struct Styles<'css> {
    rules: Vec<simplecss::Rule<'css>>,
}

impl<'css> Styles<'css> {
    fn new(css: &'css str) -> Self {
        let mut rules = simplecss::StyleSheet::parse(css).rules;
        // Later and more specific rules win: applied last.
        rules.sort_by_key(|rule| rule.selector.specificity());
        Styles { rules }
    }

    /// The value `node` itself gives `name`: its `style` attribute, then the style sheets,
    /// then its attribute.
    fn specified(&self, node: Node, name: &str) -> Option<String> {
        if let Some(style) = node.attribute("style") {
            let declared = simplecss::DeclarationTokenizer::from(style)
                .filter(|d| d.name == name)
                .last();
            if let Some(d) = declared {
                return Some(d.value.trim().to_string());
            }
        }
        let element = XmlElement(node);
        let mut found = None;
        for rule in &self.rules {
            if rule.selector.matches(&element) {
                if let Some(d) = rule.declarations.iter().rev().find(|d| d.name == name) {
                    found = Some(d.value);
                }
            }
        }
        found
            .or_else(|| node.attribute(name))
            .map(|v| v.trim().to_string())
    }

    /// The value `node` has for an inherited property: its own, or its nearest ancestor's.
    fn inherited(&self, node: Node, name: &str) -> Option<String> {
        let mut at = Some(node);
        while let Some(node) = at {
            match self.specified(node, name) {
                Some(v) if v != "inherit" => return Some(v),
                _ => at = node.parent_element(),
            }
        }
        None
    }

    fn font_size(&self, node: Node) -> f32 {
        let parent = node
            .parent_element()
            .map_or(DEFAULT_FONT_SIZE, |p| self.font_size(p));
        match self.specified(node, "font-size") {
            Some(v) => font_size(&v, parent).unwrap_or(parent),
            None => parent,
        }
    }

    fn font_weight(&self, node: Node) -> u16 {
        let parent = node.parent_element().map_or(400, |p| self.font_weight(p));
        match self.specified(node, "font-weight").as_deref() {
            None | Some("inherit") => parent,
            Some("normal") => 400,
            Some("bold") => 700,
            // The relative weights, as CSS defines them.
            Some("bolder") => match parent {
                0..=349 => 400,
                350..=549 => 700,
                _ => 900,
            },
            Some("lighter") => match parent {
                0..=549 => 100,
                550..=749 => 400,
                _ => 700,
            },
            Some(v) => v
                .parse::<f32>()
                .ok()
                .filter(|w| (1.0..=1000.0).contains(w))
                .map_or(parent, |w| w as u16),
        }
    }

    fn font(&self, node: Node) -> Font {
        let size = self.font_size(node);
        let families = self
            .inherited(node, "font-family")
            .map(|list| {
                list.split(',')
                    .map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
                    .filter(|f| !f.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let italic = matches!(
            self.inherited(node, "font-style").as_deref(),
            Some("italic" | "oblique")
        );
        let letter_spacing = self
            .inherited(node, "letter-spacing")
            .and_then(|v| length(&v, size))
            .unwrap_or(0.0);
        let anchor = match self.inherited(node, "text-anchor").as_deref() {
            Some("middle") => Anchor::Middle,
            Some("end") => Anchor::End,
            _ => Anchor::Start,
        };
        Font {
            families,
            size,
            weight: self.font_weight(node),
            italic,
            letter_spacing,
            anchor,
        }
    }
}

/// A browser's `medium`.
const DEFAULT_FONT_SIZE: f32 = 16.0;

/// A `font-size` value, `parent` being the inherited size.
fn font_size(value: &str, parent: f32) -> Option<f32> {
    let keyword = match value {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "larger" => Some(parent * 1.2),
        "smaller" => Some(parent / 1.2),
        _ => None,
    };
    let size = match keyword {
        Some(size) => size,
        None => match value.strip_suffix('%') {
            Some(p) => p.trim().parse::<f32>().ok()? / 100.0 * parent,
            None => length(value, parent)?,
        },
    };
    (size.is_finite() && size >= 0.0).then_some(size)
}

/// A length in user units, `em` being `font_size`. Percentages are not read.
fn length(value: &str, font_size: f32) -> Option<f32> {
    let value = value.trim();
    if value == "normal" {
        return Some(0.0);
    }
    let split = value
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let n: f32 = number.trim().parse().ok()?;
    let scale = match unit.trim() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        "em" => font_size,
        "ex" => font_size / 2.0,
        _ => return None,
    };
    Some(n * scale).filter(|v| v.is_finite())
}

/// A list of lengths, as `x`, `y`, `dx` and `dy` give them.
fn lengths(value: &str, font_size: f32) -> Vec<Option<f32>> {
    value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| length(s, font_size))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
struct Font {
    families: Vec<String>,
    size: f32,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    anchor: Anchor,
}

/// A roxmltree element as the style sheet matcher sees one.
#[derive(Clone, Copy)]
struct XmlElement<'a, 'input>(Node<'a, 'input>);

impl simplecss::Element for XmlElement<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(XmlElement)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(XmlElement)
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.0.tag_name().name() == name
    }

    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator) -> bool {
        self.0
            .attribute(local_name)
            .is_some_and(|v| operator.matches(v))
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass) -> bool {
        matches!(class, simplecss::PseudoClass::FirstChild)
            && self.0.prev_sibling_element().is_none()
    }
}

// --- Layout ---

/// One character of a text element, after its white space is handled.
struct Char {
    c: char,
    /// The span (a run of characters from one text node) it belongs to.
    span: usize,
    x: Option<f32>,
    y: Option<f32>,
    dx: f32,
    dy: f32,
}

/// The characters of one text node, and the element that styles them.
struct Span<'a, 'input> {
    owner: Node<'a, 'input>,
    font: Font,
    /// Its glyphs, as they are laid out.
    path: Path,
}

/// Lays out `text`, appends its spans' outlines to `spans`, and returns the markup that
/// stands in for it.
fn layout(
    text: Node,
    styles: &Styles,
    spans: &mut Vec<Path>,
    notes: &mut Vec<&'static str>,
) -> String {
    let mut chars = Vec::new();
    let mut local = Vec::new();
    let mut ranges = HashMap::new();
    collect(text, styles, &mut chars, &mut local, &mut ranges, notes);
    // Default white space: a trailing space goes, as the leading ones did.
    if chars.last().is_some_and(|c: &Char| c.c == ' ') && !preserves_space(text) {
        chars.pop();
    }
    positions(text, styles, &mut chars, &ranges, notes);

    // Runs of characters shaped together: one span, no position of their own after the
    // first. Chunks — what `text-anchor` moves — start at every absolute position.
    struct Run {
        span: usize,
        text: String,
        x: f32,
        y: f32,
        chunk: usize,
    }
    // Each run's outline in place, with its span and chunk.
    let mut placed: Vec<(usize, usize, Path)> = Vec::new();
    // Each chunk's anchor, start and end.
    let mut chunks: Vec<(Anchor, f32, f32)> = Vec::new();
    let (mut cx, mut cy) = (0.0_f32, 0.0_f32);
    let mut open: Option<Run> = None;
    let mut flush = |open: &mut Option<Run>, cx: &mut f32, local: &[Span]| {
        if let Some(run) = open.take() {
            let font = &local[run.span].font;
            let families: Vec<&str> = font.families.iter().map(String::as_str).collect();
            let line = frus_text::outline_text(
                &run.text,
                &frus_text::OutlineStyle {
                    families: &families,
                    size: font.size,
                    weight: font.weight,
                    italic: font.italic,
                    letter_spacing: font.letter_spacing,
                },
            );
            *cx = run.x + line.advance;
            placed.push((run.span, run.chunk, line.path.translated(run.x, run.y)));
        }
    };
    for (i, ch) in chars.iter().enumerate() {
        let new_chunk = i == 0 || ch.x.is_some() || ch.y.is_some();
        let breaks = new_chunk
            || ch.dx != 0.0
            || ch.dy != 0.0
            || open.as_ref().is_some_and(|run| run.span != ch.span);
        if breaks {
            flush(&mut open, &mut cx, &local);
            if let Some(chunk) = chunks.last_mut() {
                chunk.2 = cx;
            }
        }
        if let Some(x) = ch.x {
            cx = x;
        }
        if let Some(y) = ch.y {
            cy = y;
        }
        cx += ch.dx;
        cy += ch.dy;
        if new_chunk {
            chunks.push((local[ch.span].font.anchor, cx, cx));
        }
        if breaks {
            open = Some(Run {
                span: ch.span,
                text: String::new(),
                x: cx,
                y: cy,
                chunk: chunks.len() - 1,
            });
        }
        if let Some(run) = open.as_mut() {
            run.text.push(ch.c);
        }
    }
    flush(&mut open, &mut cx, &local);
    if let Some(chunk) = chunks.last_mut() {
        chunk.2 = cx;
    }

    // The anchors: each chunk moved by its share of its own advance.
    let shifts: Vec<f32> = chunks
        .iter()
        .map(|&(anchor, start, end)| match anchor {
            Anchor::Start => 0.0,
            Anchor::Middle => -(end - start) / 2.0,
            Anchor::End => -(end - start),
        })
        .collect();
    for (span, chunk, path) in placed {
        let span = &mut local[span];
        span.path = std::mem::take(&mut span.path).append(path.translated(shifts[chunk], 0.0));
    }

    // The text's box, which every placeholder covers.
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for span in &local {
        if let Some(b) = path_bounds(&span.path) {
            bounds = Some(match bounds {
                None => b,
                Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
            });
        }
    }

    let mut markup = String::new();
    // A clip path takes shapes, not groups: there each placeholder is a bare path carrying
    // the text's transform, and the clip is the glyphs whatever their paint.
    let in_clip = text.ancestors().any(|n| is_svg(n, "clipPath"));
    let transform = text
        .attribute("transform")
        .map(|t| format!(r#" transform="{}""#, escaped(t)))
        .unwrap_or_default();
    if !in_clip {
        open_group(&mut markup, text, true);
    }
    if let Some((x0, y0, x1, y1)) = bounds {
        // A box with no width or height would not be a shape.
        let (x1, y1) = (x1.max(x0 + 0.01), y1.max(y0 + 0.01));
        for span in local {
            if span.path.is_empty() {
                continue;
            }
            let wrappers: Vec<Node> = if in_clip {
                Vec::new()
            } else {
                span.owner
                    .ancestors()
                    .take_while(|n| *n != text)
                    .filter(|n| n.is_element())
                    .collect()
            };
            let own_transform = if in_clip { transform.as_str() } else { "" };
            for wrapper in wrappers.iter().rev() {
                open_group(&mut markup, *wrapper, false);
            }
            let _ = write!(
                markup,
                r#"<path id="{MARKER}{}"{own_transform} d="M{x0} {y0}H{x1}V{y1}H{x0}Z"/>"#,
                spans.len()
            );
            spans.push(span.path);
            for _ in &wrappers {
                markup.push_str("</g>");
            }
        }
    }
    if !in_clip {
        markup.push_str("</g>");
    }
    markup
}

/// The characters under `node`, with their white space handled, and each element's range
/// of them.
fn collect<'a, 'input>(
    node: Node<'a, 'input>,
    styles: &Styles,
    chars: &mut Vec<Char>,
    spans: &mut Vec<Span<'a, 'input>>,
    ranges: &mut HashMap<NodeId, (usize, usize)>,
    notes: &mut Vec<&'static str>,
) {
    let start = chars.len();
    if styles
        .specified(node, "text-decoration")
        .is_some_and(|v| v != "none")
    {
        note(notes, "text decoration");
    }
    if styles
        .inherited(node, "writing-mode")
        .is_some_and(|v| v.starts_with("tb") || v.starts_with("vertical"))
    {
        note(notes, "vertical text");
    }
    for child in node.children() {
        if child.is_text() {
            let Some(content) = child.text() else {
                continue;
            };
            let preserve = preserves_space(node);
            let span = spans.len();
            spans.push(Span {
                owner: node,
                font: styles.font(node),
                path: Path::new().with_fill_rule(FillRule::NonZero),
            });
            for c in content.chars() {
                let c = match c {
                    '\n' | '\r' if !preserve => continue,
                    '\n' | '\r' | '\t' => ' ',
                    c => c,
                };
                // Default white space: no leading space, and no two in a row.
                if c == ' ' && !preserve && chars.last().is_none_or(|last: &Char| last.c == ' ') {
                    continue;
                }
                chars.push(Char {
                    c,
                    span,
                    x: None,
                    y: None,
                    dx: 0.0,
                    dy: 0.0,
                });
            }
        } else if is_svg(child, "tspan") || is_svg(child, "a") {
            collect(child, styles, chars, spans, ranges, notes);
        } else if is_svg(child, "textPath") {
            note(notes, "text on a path");
        }
    }
    ranges.insert(node.id(), (start, chars.len()));
}

fn preserves_space(node: Node) -> bool {
    node.ancestors()
        .find_map(|n| n.attribute((XML_NS, "space")))
        .is_some_and(|v| v == "preserve")
}

/// Every element's `x`, `y`, `dx` and `dy` given to its characters, an element's own
/// lists over its ancestors'.
fn positions(
    text: Node,
    styles: &Styles,
    chars: &mut [Char],
    ranges: &HashMap<NodeId, (usize, usize)>,
    notes: &mut Vec<&'static str>,
) {
    for element in text.descendants().filter(|n| n.is_element()) {
        let Some(&(start, end)) = ranges.get(&element.id()) else {
            continue;
        };
        let end = end.min(chars.len());
        let size = styles.font_size(element);
        for (name, set) in [
            ("x", (|c: &mut Char, v| c.x = Some(v)) as fn(&mut Char, f32)),
            ("y", |c, v| c.y = Some(v)),
            ("dx", |c, v| c.dx = v),
            ("dy", |c, v| c.dy = v),
        ] {
            let Some(list) = element.attribute(name) else {
                continue;
            };
            for (k, value) in lengths(list, size).into_iter().enumerate() {
                if start + k >= end {
                    break;
                }
                if let Some(v) = value {
                    set(&mut chars[start + k], v);
                }
            }
        }
        if element
            .attribute("rotate")
            .is_some_and(|r| lengths(r, size).iter().any(|v| v.is_some_and(|v| v != 0.0)))
        {
            note(notes, "rotated glyphs");
        }
        if element.attribute("textLength").is_some() {
            note(notes, "text length");
        }
    }
}

// --- Paths ---

fn path_bounds(path: &Path) -> Option<(f32, f32, f32, f32)> {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for verb in path.verbs() {
        let points = match *verb {
            PathVerb::MoveTo(p) | PathVerb::LineTo(p) => [Some(p), None, None],
            PathVerb::QuadTo { ctrl, to } => [Some(ctrl), Some(to), None],
            PathVerb::CubicTo { c1, c2, to } => [Some(c1), Some(c2), Some(to)],
            PathVerb::Close => [None, None, None],
        };
        for p in points.into_iter().flatten() {
            bounds = Some(match bounds {
                None => (p.x, p.y, p.x, p.y),
                Some(b) => (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)),
            });
        }
    }
    bounds
}

// --- Markup ---

/// Opens a `<g>` carrying what the parser reads of `element`: everything but its text
/// positions — paint, opacity, transform, classes, style. A `<tspan>`'s `id` is not
/// repeated, and neither is what does not apply to one (opacity, filters, masks, clips).
fn open_group(markup: &mut String, element: Node, outer: bool) {
    markup.push_str("<g");
    for attribute in element.attributes() {
        if attribute.namespace().is_some() {
            continue;
        }
        let name = attribute.name();
        let positional = matches!(
            name,
            "x" | "y" | "dx" | "dy" | "rotate" | "textLength" | "lengthAdjust"
        );
        let element_only = matches!(
            name,
            "id" | "opacity" | "filter" | "mask" | "clip-path" | "transform"
        );
        if positional || (!outer && element_only) {
            continue;
        }
        let _ = write!(markup, r#" {name}="{}""#, escaped(attribute.value()));
    }
    markup.push('>');
}

fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}
