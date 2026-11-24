// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Text layers as the shapes they draw**, laid out by frus's own text stack.
//!
//! velato has no text layer. Before it reads the file, a text layer (type 5) becomes a shape
//! layer (type 4) with everything else it had — transform, parent, in and out points,
//! masks, mattes, blend — and, for shapes, its words: each text document shaped with the
//! font the file names, its glyphs turned into Lottie paths, filled with the document's
//! colour and stroked with its stroke. A text whose document is keyframed gets a group per
//! document, each shown from its keyframe to the next by a held opacity: the document
//! changes on its frame, as text documents do, never in between.
//!
//! Read from a document: the text (lines broken at `\r` or `\n`), the font (`f`, looked up
//! in the file's font list for its family and style), the size (`s`), the justification
//! (`j`), the line height (`lh`), the tracking (`tr`), the baseline shift (`ls`), the fill
//! (`fc`), the stroke (`sc`, `sw`, `of`) and a paragraph box (`sz`, `ps`), in which lines
//! wrap.

use frus_core::{Path, PathVerb, Point};
use serde_json::{json, Map, Value};

/// Turns the text layer `layer` into a shape layer drawing its words. `fonts` is the
/// file's font list (`fonts.list`). `false` when it has no document to draw.
pub(crate) fn text_as_shape(
    layer: &mut Map<String, Value>,
    fonts: &[Value],
    notes: &mut Vec<&'static str>,
) -> bool {
    let Some(text) = layer.get("t") else {
        return false;
    };
    if text
        .get("a")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        note(notes, "text animator");
    }
    if text.get("p").and_then(|p| p.get("m")).is_some() {
        note(notes, "text on a path");
    }
    let documents = documents(text);
    if documents.is_empty() {
        return false;
    }
    let times: Vec<f64> = documents.iter().map(|(t, _)| *t).collect();
    let shapes: Vec<Value> = documents
        .iter()
        .enumerate()
        .map(|(i, (_, document))| {
            let mut items = document_items(document, fonts);
            let opacity = if times.len() == 1 {
                json!({"a": 0, "k": 100})
            } else {
                // Held: this document shows from its keyframe to the next one's.
                let keyframes: Vec<Value> = times
                    .iter()
                    .enumerate()
                    .map(|(j, t)| json!({"t": t, "s": [if i == j { 100 } else { 0 }], "h": 1}))
                    .collect();
                json!({"a": 1, "k": keyframes})
            };
            items.push(
                json!({"ty": "tr", "p": fixed(json!([0, 0])), "a": fixed(json!([0, 0])),
                "s": fixed(json!([100, 100])), "r": fixed(json!(0)), "o": opacity}),
            );
            json!({"ty": "gr", "nm": "text", "it": items})
        })
        .collect();
    layer.remove("t");
    layer.insert("ty".to_string(), json!(4));
    layer.insert("shapes".to_string(), Value::Array(shapes));
    true
}

fn note(notes: &mut Vec<&'static str>, what: &'static str) {
    if !notes.contains(&what) {
        notes.push(what);
    }
}

fn fixed(k: Value) -> Value {
    json!({"a": 0, "k": k})
}

/// The layer's text documents with the frame each starts at, in order.
fn documents(text: &Value) -> Vec<(f64, Value)> {
    let Some(d) = text.get("d") else {
        return Vec::new();
    };
    match d.get("k") {
        Some(Value::Array(keyframes)) => {
            let mut out: Vec<(f64, Value)> = keyframes
                .iter()
                .filter_map(|k| {
                    let document = k.get("s")?.clone();
                    Some((k.get("t").and_then(Value::as_f64).unwrap_or(0.0), document))
                })
                .collect();
            out.sort_by(|a, b| a.0.total_cmp(&b.0));
            out
        }
        // An unanimated document, as a few exporters write it.
        Some(document @ Value::Object(_)) => vec![(0.0, document.clone())],
        _ => Vec::new(),
    }
}

/// The font a document names: its family and style from the file's font list.
struct Face {
    family: String,
    weight: u16,
    italic: bool,
    /// The face's ascent, in hundredths of the size, where the file gives it.
    ascent: Option<f64>,
}

fn face(name: &str, fonts: &[Value]) -> Face {
    let entry = fonts
        .iter()
        .find(|f| f.get("fName").and_then(Value::as_str) == Some(name));
    let field = |key: &str| entry.and_then(|e| e.get(key)).and_then(Value::as_str);
    let style = field("fStyle").unwrap_or("").to_ascii_lowercase();
    let weight = match () {
        _ if style.contains("thin") || style.contains("hairline") => 100,
        _ if style.contains("extralight") || style.contains("ultralight") => 200,
        _ if style.contains("semibold") || style.contains("demibold") => 600,
        _ if style.contains("extrabold") || style.contains("ultrabold") => 800,
        _ if style.contains("black") || style.contains("heavy") => 900,
        _ if style.contains("bold") => 700,
        _ if style.contains("medium") => 500,
        _ if style.contains("light") => 300,
        _ => 400,
    };
    let weight = entry
        .and_then(|e| e.get("fWeight"))
        .and_then(|w| {
            w.as_f64()
                .or_else(|| w.as_str().and_then(|s| s.parse().ok()))
        })
        .filter(|w| (1.0..=1000.0).contains(w))
        .map_or(weight, |w| w as u16);
    Face {
        family: field("fFamily").unwrap_or(name).to_string(),
        weight,
        italic: style.contains("italic") || style.contains("oblique"),
        ascent: entry.and_then(|e| e.get("ascent")).and_then(Value::as_f64),
    }
}

/// A colour as Lottie writes one, `[r, g, b]` or `[r, g, b, a]` from 0 to 1 (or to 255 in a
/// few old files), as a fill's or a stroke's `"c"`.
fn colour(value: Option<&Value>) -> Option<Value> {
    let channels: Vec<f64> = value?
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    if channels.len() < 3 {
        return None;
    }
    let scale = if channels.iter().take(3).any(|c| *c > 1.0) {
        255.0
    } else {
        1.0
    };
    let c = |i: usize| (channels[i] / scale).clamp(0.0, 1.0);
    Some(fixed(json!([c(0), c(1), c(2), 1])))
}

/// One document's glyphs as paths, then its stroke and its fill, as group items.
fn document_items(document: &Value, fonts: &[Value]) -> Vec<Value> {
    let number = |key: &str| document.get(key).and_then(Value::as_f64);
    let text = document.get("t").and_then(Value::as_str).unwrap_or("");
    let size = number("s").unwrap_or(0.0);
    let face = face(
        document.get("f").and_then(Value::as_str).unwrap_or(""),
        fonts,
    );
    let families = [face.family.as_str()];
    let style = frus_text::OutlineStyle {
        families: &families,
        size: size as f32,
        weight: face.weight,
        italic: face.italic,
        // Tracking is in thousandths of an em.
        letter_spacing: (number("tr").unwrap_or(0.0) * size / 1000.0) as f32,
    };
    let line_height = number("lh").filter(|h| *h > 0.0).unwrap_or(size * 1.2);
    let shift = number("ls").unwrap_or(0.0);
    let justification = document.get("j").and_then(Value::as_u64).unwrap_or(0);
    let pair = |key: &str| -> Option<(f64, f64)> {
        let a = document.get(key)?.as_array()?;
        Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    // A paragraph box: lines wrap in its width, and the first baseline is an ascent below
    // its top.
    let paragraph = pair("sz").zip(pair("ps"));

    let mut items = Vec::new();
    let hard_lines = text.split(['\r', '\n', '\u{3}']);
    let mut lines = Vec::new();
    for line in hard_lines {
        match paragraph {
            Some(((width, _), _)) => {
                let wrapped = frus_text::outline_paragraph(line, &style, Some(width as f32));
                if wrapped.is_empty() {
                    lines.push(None);
                }
                lines.extend(wrapped.into_iter().map(Some));
            }
            None => lines.push(Some(frus_text::outline_text(line, &style))),
        }
    }
    let ascent = face.ascent.map_or(size * 0.75, |a| a * size / 100.0);
    for (i, line) in lines.into_iter().enumerate() {
        let Some(line) = line else { continue };
        let advance = f64::from(line.advance);
        let y = i as f64 * line_height - shift;
        let x = match paragraph {
            Some(((width, _), (left, _))) => match justification {
                1 | 4 => left + width - advance,
                2 | 5 => left + (width - advance) / 2.0,
                _ => left,
            },
            None => match justification {
                1 | 4 => -advance,
                2 | 5 => -advance / 2.0,
                _ => 0.0,
            },
        };
        let y = match paragraph {
            Some((_, (_, top))) => top + ascent + y,
            None => y,
        };
        let path = line.path.translated(x as f32, y as f32);
        items.extend(
            contours(&path)
                .into_iter()
                .map(|bezier| json!({"ty": "sh", "ks": {"a": 0, "k": bezier}})),
        );
    }
    if items.is_empty() {
        return items;
    }
    let fill = colour(document.get("fc"))
        .map(|c| json!({"ty": "fl", "c": c, "o": fixed(json!(100)), "r": 1}));
    let stroke = colour(document.get("sc"))
        .zip(number("sw").filter(|w| *w > 0.0))
        .map(|(c, w)| {
            json!({"ty": "st", "c": c, "o": fixed(json!(100)), "w": fixed(json!(w)),
                "lc": 2, "lj": 2, "ml": 4})
        });
    // The first item draws on top: the stroke over the fill unless the document says
    // otherwise.
    let stroke_over = document.get("of").and_then(Value::as_bool).unwrap_or(true);
    let (top, bottom) = if stroke_over {
        (stroke, fill)
    } else {
        (fill, stroke)
    };
    items.extend(top);
    items.extend(bottom);
    items
}

/// A path's contours as Lottie Béziers: vertices, and the in and out tangents relative to
/// them.
fn contours(path: &Path) -> Vec<Value> {
    struct Contour {
        v: Vec<Point>,
        i: Vec<Point>,
        o: Vec<Point>,
        closed: bool,
    }
    let zero = Point::new(0.0, 0.0);
    let mut out: Vec<Contour> = Vec::new();
    let mut current: Option<Contour> = None;
    let finish = |contour: Option<Contour>, out: &mut Vec<Contour>| {
        if let Some(mut c) = contour {
            // A contour closed by a curve back to its start: that vertex once.
            if c.v.len() > 1 {
                let (first, last) = (c.v[0], c.v[c.v.len() - 1]);
                if (first.x - last.x).abs() < 1e-4 && (first.y - last.y).abs() < 1e-4 {
                    c.v.pop();
                    let in_tangent = c.i.pop().unwrap_or(zero);
                    c.o.pop();
                    c.i[0] = in_tangent;
                    c.closed = true;
                }
            }
            if c.v.len() > 1 {
                out.push(c);
            }
        }
    };
    let rel = |p: Point, to: Point| Point::new(p.x - to.x, p.y - to.y);
    for verb in path.verbs() {
        match *verb {
            PathVerb::MoveTo(p) => {
                finish(current.take(), &mut out);
                current = Some(Contour {
                    v: vec![p],
                    i: vec![zero],
                    o: vec![zero],
                    closed: false,
                });
            }
            PathVerb::LineTo(p) => {
                if let Some(c) = current.as_mut() {
                    c.v.push(p);
                    c.i.push(zero);
                    c.o.push(zero);
                }
            }
            PathVerb::QuadTo { ctrl, to } => {
                if let Some(c) = current.as_mut() {
                    let from = c.v[c.v.len() - 1];
                    // The same curve as a cubic: its handles two thirds of the way to the
                    // quadratic's control point.
                    let c1 = Point::new(
                        from.x + 2.0 / 3.0 * (ctrl.x - from.x),
                        from.y + 2.0 / 3.0 * (ctrl.y - from.y),
                    );
                    let c2 = Point::new(
                        to.x + 2.0 / 3.0 * (ctrl.x - to.x),
                        to.y + 2.0 / 3.0 * (ctrl.y - to.y),
                    );
                    let last = c.o.len() - 1;
                    c.o[last] = rel(c1, from);
                    c.v.push(to);
                    c.i.push(rel(c2, to));
                    c.o.push(zero);
                }
            }
            PathVerb::CubicTo { c1, c2, to } => {
                if let Some(c) = current.as_mut() {
                    let from = c.v[c.v.len() - 1];
                    let last = c.o.len() - 1;
                    c.o[last] = rel(c1, from);
                    c.v.push(to);
                    c.i.push(rel(c2, to));
                    c.o.push(zero);
                }
            }
            PathVerb::Close => {
                if let Some(c) = current.as_mut() {
                    c.closed = true;
                }
            }
        }
    }
    finish(current, &mut out);
    let xy = |points: &[Point]| -> Vec<Value> {
        points
            .iter()
            .map(|p| json!([f64::from(p.x), f64::from(p.y)]))
            .collect()
    };
    out.iter()
        .map(|c| json!({"c": c.closed, "v": xy(&c.v), "i": xy(&c.i), "o": xy(&c.o)}))
        .collect()
}
