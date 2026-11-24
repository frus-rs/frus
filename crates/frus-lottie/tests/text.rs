// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Text layers, written by hand: where their words must land, in what colour, and when.

use frus_core::{Alignment, AlignmentGeometry, BoxFit, Color, PathVerb, Primitive, Rect, Scene};
use frus_lottie::Animation;

/// A 400×200, 30 fps, 2-second animation with one text layer at `position`, whose `"t"` is
/// `text`, and a font list naming "Sans" (the default family, regular) and "Sans-Bold".
fn animation(text: &str, position: [f32; 2]) -> String {
    format!(
        r#"{{"v":"5.7.4","fr":30,"ip":0,"op":60,"w":400,"h":200,"nm":"t","ddd":0,"assets":[],
        "fonts":{{"list":[
          {{"fName":"Sans","fFamily":"sans-serif","fStyle":"Regular","ascent":75}},
          {{"fName":"Sans-Bold","fFamily":"sans-serif","fStyle":"Bold","ascent":75}}]}},
        "layers":[{{"ddd":0,"ind":1,"ty":5,"nm":"words","sr":1,"ip":0,"op":60,"st":0,"bm":0,
          "ks":{{"o":{{"a":0,"k":100}},"r":{{"a":0,"k":0}},"p":{{"a":0,"k":[{},{},0]}},
                "a":{{"a":0,"k":[0,0,0]}},"s":{{"a":0,"k":[100,100,100]}}}},
          "t":{text}}}]}}"#,
        position[0], position[1]
    )
}

/// A text property holding one document.
fn document(fields: &str) -> String {
    format!(
        r#"{{"d":{{"k":[{{"s":{{{fields}}},"t":0}}]}},"p":{{}},"m":{{"g":1,"a":{{"a":0,"k":[0,0]}}}},"a":[]}}"#
    )
}

fn painted(animation: &Animation, progress: f32) -> Vec<Primitive> {
    let mut scene = Scene::new();
    scene.set_clip(Rect::new(0.0, 0.0, 400.0, 200.0));
    animation.paint(
        &mut scene,
        Rect::new(0.0, 0.0, 400.0, 200.0),
        BoxFit::Fill,
        AlignmentGeometry::Physical(Alignment::CENTER),
        progress,
        1.0,
    );
    scene.primitives().to_vec()
}

/// Every path in `primitives`, layers opened.
fn paths(primitives: &[Primitive]) -> Vec<Primitive> {
    let mut out = Vec::new();
    for p in primitives {
        match p {
            Primitive::Path { .. } => out.push(p.clone()),
            Primitive::Layer { primitives, .. } => out.extend(paths(primitives)),
            _ => {}
        }
    }
    out
}

/// `(left, top, right, bottom)` of a path's end points.
fn extent(p: &Primitive) -> (f32, f32, f32, f32) {
    let Primitive::Path { path, .. } = p else {
        panic!("a path")
    };
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for verb in path.verbs() {
        let point = match *verb {
            PathVerb::MoveTo(p) | PathVerb::LineTo(p) => p,
            PathVerb::QuadTo { to, .. } | PathVerb::CubicTo { to, .. } => to,
            PathVerb::Close => continue,
        };
        b = (
            b.0.min(point.x),
            b.1.min(point.y),
            b.2.max(point.x),
            b.3.max(point.y),
        );
    }
    b
}

fn fill(p: &Primitive) -> Color {
    match p {
        Primitive::Path { fill: Some(c), .. } => *c,
        p => panic!("{p:?}"),
    }
}

/// **A text layer draws its words** — it used to be left out — on the baseline at the
/// layer's position, in the document's size and fill colour.
#[test]
fn a_text_layer_draws_its_words_at_its_position() {
    let a = Animation::parse(
        animation(
            &document(r#""s":40,"f":"Sans","t":"Hi","j":0,"tr":0,"lh":48,"ls":0,"fc":[1,0,0]"#),
            [50.0, 100.0],
        )
        .as_bytes(),
    )
    .unwrap();
    assert!(a.unsupported().is_empty(), "{:?}", a.unsupported());
    let drawn = paths(&painted(&a, 0.0));
    assert_eq!(drawn.len(), 1, "the words, one compound path");
    let c = fill(&drawn[0]);
    assert_eq!((c.r, c.g, c.b, c.a), (1.0, 0.0, 0.0, 1.0));
    let (x0, y0, x1, y1) = extent(&drawn[0]);
    assert!((y1 - 100.0).abs() < 0.5, "on the baseline: {y1}");
    assert!((65.0..80.0).contains(&y0), "the i's dot above: {y0}");
    assert!((50.0..56.0).contains(&x0) && x1 < 110.0, "{x0}..{x1}");
}

/// **Justification is about the layer's origin**: right-justified text ends there,
/// centred text is centred on it; and a second line is a line height below.
#[test]
fn justification_and_lines() {
    let at = |j: u32, text: &str| {
        let a = Animation::parse(
            animation(
                &document(&format!(
                    r#""s":30,"f":"Sans","t":"{text}","j":{j},"tr":0,"lh":50,"ls":0,"fc":[0,0,0]"#
                )),
                [200.0, 80.0],
            )
            .as_bytes(),
        )
        .unwrap();
        extent(&paths(&painted(&a, 0.0))[0])
    };
    let left = at(0, "Hello");
    let right = at(1, "Hello");
    let centre = at(2, "Hello");
    assert!((200.0..205.0).contains(&left.0), "{left:?}");
    assert!(right.2 <= 200.0 && right.2 > 195.0, "{right:?}");
    assert!(
        ((centre.0 + centre.2) / 2.0 - 200.0).abs() < 3.0,
        "{centre:?}"
    );
    let two = at(0, "Hello\\rHello");
    assert!((two.3 - 130.0).abs() < 0.5, "the second baseline: {two:?}");
}

/// **The font list's style reaches the glyphs, tracking spaces them**, and a stroke is
/// drawn over the fill.
#[test]
fn font_style_tracking_and_stroke() {
    let width = |fields: &str| {
        let a = Animation::parse(animation(&document(fields), [10.0, 100.0]).as_bytes()).unwrap();
        let drawn = paths(&painted(&a, 0.0));
        let (x0, _, x1, _) = extent(&drawn[0]);
        (x1 - x0, drawn)
    };
    let (regular, _) = width(r#""s":40,"f":"Sans","t":"Wide","j":0,"fc":[0,0,0]"#);
    let (bold, _) = width(r#""s":40,"f":"Sans-Bold","t":"Wide","j":0,"fc":[0,0,0]"#);
    assert!(bold > regular * 1.05, "{bold} vs {regular}");
    // 100 thousandths of 40 is 4 after each of the three gaps.
    let (tracked, _) = width(r#""s":40,"f":"Sans","t":"Wide","j":0,"tr":100,"fc":[0,0,0]"#);
    assert!(
        (tracked - regular - 12.0).abs() < 0.5,
        "{tracked} vs {regular}"
    );
    let (_, drawn) =
        width(r#""s":40,"f":"Sans","t":"Wide","j":0,"fc":[0,0,1],"sc":[1,0,0],"sw":2,"of":true"#);
    assert_eq!(drawn.len(), 2, "fill, then stroke");
    assert_eq!(fill(&drawn[0]).b, 1.0);
    assert_eq!(fill(&drawn[1]).r, 1.0);
}

/// **A keyframed document changes on its frame**: the first text until frame 30, the
/// second from then on — never both, never between.
#[test]
fn a_keyframed_document_switches_on_its_frame() {
    let text = r#"{"d":{"k":[
        {"s":{"s":40,"f":"Sans","t":"I","j":0,"fc":[1,0,0]},"t":0},
        {"s":{"s":40,"f":"Sans","t":"IIIIII","j":0,"fc":[0,0,1]},"t":30}]},
        "p":{},"m":{"g":1,"a":{"a":0,"k":[0,0]}},"a":[]}"#;
    let a = Animation::parse(animation(text, [10.0, 100.0]).as_bytes()).unwrap();
    let visible = |progress: f32| -> Vec<Primitive> {
        paths(&painted(&a, progress))
            .into_iter()
            .filter(|p| fill(p).a > 0.0)
            .collect()
    };
    let early = visible(0.2); // frame 12
    assert_eq!(early.len(), 1);
    assert_eq!(fill(&early[0]).r, 1.0);
    let late = visible(0.75); // frame 45
    assert_eq!(late.len(), 1);
    assert_eq!(fill(&late[0]).b, 1.0);
    assert!(
        extent(&late[0]).2 - extent(&late[0]).0 > (extent(&early[0]).2 - extent(&early[0]).0) * 3.0
    );
}

/// **A paragraph box wraps its lines in its width**, the first baseline an ascent below its
/// top.
#[test]
fn a_paragraph_box_wraps_its_lines() {
    let a = Animation::parse(
        animation(
            &document(
                r#""s":20,"f":"Sans","t":"one two three four five six","j":0,"lh":24,"fc":[0,0,0],"sz":[80,200],"ps":[0,0]"#,
            ),
            [100.0, 0.0],
        )
        .as_bytes(),
    )
    .unwrap();
    let (x0, y0, x1, y1) = extent(&paths(&painted(&a, 0.0))[0]);
    assert!(x0 >= 100.0 && x1 <= 181.0, "inside the box: {x0}..{x1}");
    assert!(y0 > 0.0 && y0 < 15.0, "the first line's top: {y0}");
    assert!(y1 > 15.0 + 2.0 * 24.0, "three lines and more: {y1}");
}

/// **What a text layer uses that is not drawn is named**: its animators.
#[test]
fn text_animators_are_named() {
    let text = r#"{"d":{"k":[{"s":{"s":40,"f":"Sans","t":"A","j":0,"fc":[0,0,0]},"t":0}]},
        "p":{},"m":{"g":1,"a":{"a":0,"k":[0,0]}},
        "a":[{"nm":"Animator 1","s":{"t":0,"xe":{"a":0,"k":0},"ne":{"a":0,"k":0},"a":{"a":0,"k":100},"b":1,"rn":0,"sh":1,"r":1},
              "a":{"o":{"a":0,"k":0}}}]}"#;
    let a = Animation::parse(animation(text, [10.0, 100.0]).as_bytes()).unwrap();
    assert_eq!(a.unsupported(), ["text animator"]);
    assert_eq!(paths(&painted(&a, 0.0)).len(), 1, "drawn without it");
}
