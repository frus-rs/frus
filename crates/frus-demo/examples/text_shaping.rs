// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! OpenType features, variable font axes, the language of a span, Thai line breaking and the
//! platform's fallback fonts (milestone 784).
//!
//! `cargo run -p frus-demo --example text_shaping` on the desktop. The Windows faces it names
//! (Calibri, Bahnschrift) are drawn by whatever the platform has instead elsewhere.

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> frus_shell::anyhow::Result<()> {
    use frus_shell::FrusApp;
    use frus_widgets::{Container, Flex, FontFamily, RichText, Text, TextSpan, TextStyle, Widget};

    let app = FrusApp::from_fn(move |cx| {
        let theme = cx.theme();
        let base = TextStyle::new(20.0);
        let calibri = base.family(FontFamily::Named("Calibri"));
        let bahnschrift = TextStyle::new(24.0).family(FontFamily::Named("Bahnschrift"));
        let heading =
            |label: &str| Text::styled(label, TextStyle::new(13.0).color(theme.scheme.primary));

        let mut left = Flex::column().gap(6.0);
        left = left
            .child(heading("OpenType features"))
            .child(Text::styled("office fi fl (liga on)", base))
            .child(Text::styled(
                "office fi fl (liga off)",
                base.font_feature("liga", 0),
            ))
            .child(Text::styled("Small Capitals 1/2 3/4 2026", calibri))
            .child(Text::styled(
                "Small Capitals 1/2 3/4 2026",
                calibri
                    .font_feature("smcp", 1)
                    .font_feature("frac", 1)
                    .font_feature("onum", 1),
            ))
            // Per span, in the bundled face: ligatures on, then off; Russian, then Serbian.
            .child(RichText::new(
                TextSpan::new("Per span: fi ")
                    .style(base)
                    .child(TextSpan::new("fi").font_feature("liga", 0))
                    .child(TextSpan::new(" \u{0431}\u{0434}\u{043f} "))
                    .child(TextSpan::new("\u{0431}\u{0434}\u{043f}").locale("sr")),
            ))
            .child(heading("Variable font axes (Bahnschrift: wght, wdth)"));
        for (label, style) in [
            ("wght 300", bahnschrift.font_variation("wght", 300.0)),
            ("wght 450", bahnschrift.font_variation("wght", 450.0)),
            ("wght 650", bahnschrift.font_variation("wght", 650.0)),
            (
                "wdth 75 wght 700",
                bahnschrift
                    .font_variation("wdth", 75.0)
                    .font_variation("wght", 700.0),
            ),
        ] {
            left = left.child(Text::styled(format!("Variable {label}"), style));
        }

        let mut right = Flex::column().gap(6.0);
        right = right.child(heading("The language of a span"));
        let han = "\u{76f4} \u{9aa8} \u{9038} \u{6d77}";
        for tag in ["ja", "zh-Hans", "zh-Hant", "ko"] {
            right = right.child(Text::styled(
                format!("{tag}: {han}"),
                TextStyle::new(28.0).locale(tag),
            ));
        }
        let cyrillic = "\u{0431}\u{0433}\u{0434}\u{043f}\u{0442}";
        right = right
            .child(Text::styled(format!("ru: {cyrillic}"), base.locale("ru")))
            .child(Text::styled(format!("sr: {cyrillic}"), base.locale("sr")))
            .child(heading("Thai, wrapped between words (no spaces)"));
        let thai = "\u{0e09}\u{0e31}\u{0e19}\u{0e01}\u{0e34}\u{0e19}\u{0e02}\u{0e49}\u{0e32}\u{0e27}\
                    \u{0e17}\u{0e35}\u{0e48}\u{0e1a}\u{0e49}\u{0e32}\u{0e19}\u{0e1a}\u{0e49}\u{0e32}\
                    \u{0e19}\u{0e02}\u{0e2d}\u{0e07}\u{0e09}\u{0e31}\u{0e19}\u{0e2d}\u{0e22}\u{0e39}\
                    \u{0e48}\u{0e43}\u{0e01}\u{0e25}\u{0e49}\u{0e15}\u{0e25}\u{0e32}\u{0e14}";
        right = right
            .child(
                Container::new()
                    .width(220.0)
                    .color(theme.scheme.surface_container_high)
                    .padding(8.0)
                    .child(Text::styled(thai, TextStyle::new(22.0))),
            )
            .child(heading("Platform fallback: Devanagari, Ethiopic, Khmer, Hebrew"))
            .child(Text::styled(
                "\u{0939}\u{093f}\u{0928}\u{094d}\u{0926}\u{0940} \u{1295}\u{1308}\u{122d} \
                 \u{1781}\u{17d2}\u{1798}\u{17c2}\u{179a} \u{05e9}\u{05dc}\u{05d5}\u{05dd}",
                TextStyle::new(24.0),
            ));

        Box::new(
            Container::new()
                .color(theme.background)
                .padding(16.0)
                .child(Flex::row().gap(32.0).child(left).child(right)),
        ) as Box<dyn Widget>
    })
    .title("frus text shaping")
    .window_size(1000.0, 720.0);
    frus_shell::run(app)
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn main() {}
