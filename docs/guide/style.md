# Text, colour and the theme

Every widget draws itself from the **theme**: its colours, its type, its corners. An
application chooses a theme once, and each widget can still be told otherwise. Nothing on
screen is styled only one way: what the theme says is a default, and what you write on a widget
wins.

## Text

`Text` takes its size, weight and colour from the theme's body style unless you say otherwise.

```rust
use frus::{Column, FontWeight, Text, TextAlign, Widget};

fn article() -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .spacing(8.0)
            .child(Text::new("A quiet morning").size(28.0).weight(FontWeight::Bold).heading())
            .child(Text::new("Published at 06:30").size(13.0).italic())
            .child(
                Text::new("The first paragraph, long enough to wrap onto several lines.")
                    .wrap()
                    .align(TextAlign::Justify),
            )
            .child(Text::new("Read more").underline()),
    )
}
```

`.heading()` also tells screen readers that the line is a heading. `.max_lines(2).ellipsis()`
cuts long text with "…"; `Text::new(..).selectable()` lets the reader select and copy it.

The theme's **type roles** — `display_large` down to `label_small` — are what a screen should
use for its titles and body text, so that every screen of the application agrees:

```rust
use frus::{component, Column, Text, Widget};

fn titled() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let text = &cx.theme().text;
        Box::new(
            Column::new()
                .child(Text::styled("Settings", text.headline_small))
                .child(Text::styled("Choose how the application looks.", text.body_medium)),
        )
    }))
}
```

### OpenType features, variable fonts and the language

A style can turn a font's OpenType features on or off, set a variable font's axes, and say which
language its text is in. A span of rich text can do the same for its own words.

```rust
use frus::{RichText, Text, TextSpan, TextStyle};

let price = TextStyle::new(16.0)
    .font_feature("tnum", 1) // figures of one width, so columns of numbers line up
    .font_feature("liga", 0); // no ligatures
let heading = TextStyle::new(32.0)
    .font_variation("wght", 650.0) // a variable font's weight axis, any value it spans
    .font_variation("wdth", 85.0); // and its width axis
let japanese = TextStyle::new(16.0).locale("ja");

let total = Text::styled("1 234,50", price);
let title = Text::styled("Chapter one", heading);
let mixed = RichText::new(
    TextSpan::new("One and a half: ")
        .child(TextSpan::new("1/2").font_feature("frac", 1))
        .child(TextSpan::new(" \u{76f4}").locale("zh-Hans")),
);
```

- **Features** take a four-letter tag and a value: `1` on, `0` off, or an alternate's number.
  Common ones are `smcp` (small capitals), `liga` (ligatures), `tnum` (tabular figures), `onum`
  (old-style figures), `frac` (fractions), `zero` (slashed zero) and `ss01` to `ss20` (stylistic
  sets). A feature the font does not have changes nothing.
- **Axes** are in each axis's own units: `wght` 1 to 1000, `wdth` a percentage, `slnt` degrees,
  `opsz` points, or any axis the font defines. A value is clamped to the font's range and an axis
  it does not have is ignored. `wght` set this way replaces the style's weight: a variable font
  draws exactly that weight, and a family of static faces its nearest.
- **The language** is a tag such as `ja`, `zh-Hant`, `ko` or `sr-Cyrl`. It picks the font's local
  forms (Serbian and Macedonian Cyrillic, Turkish dotted i) and, for the Han characters Chinese,
  Japanese and Korean share, which of the platform's fonts draws them. Text whose style names no
  language is in the application's, the locale it resolved.

**Thai, Lao, Khmer and Myanmar** wrap between words, although they write no space between them:
frus finds the words with a dictionary. That is the `dictionary-line-breaks` feature, on by
default. It adds about 1.1 MB to a binary, so an application that never shows those scripts can
turn it off, and they then break only at spaces and punctuation.

**A character none of your fonts has** is drawn by the platform's own font for its script: the
desktop's (Windows, macOS, and Linux through fontconfig) and, on Android, the one the system's
`fonts.xml` names, read from `/system/fonts` the first time text in that script is shown. On the
**web** there is no such font to reach: a page draws only the fonts the application bundles or
registers, so register a face for every script it shows, or the character is drawn as a box.

## Colours from the theme

`cx.theme().scheme` holds the colour **roles**: `primary` and `on_primary` (what is drawn on
top of it), `surface`, `error`, `outline_variant`, and the rest. A widget built from roles
looks right in the light theme, the dark theme and any theme the application makes:

```rust
use frus::{component, Container, Text, Widget};

fn notice() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let scheme = &cx.theme().scheme;
        Box::new(
            Container::new()
                .padding(12.0)
                .radius(8.0)
                .color(scheme.primary_container)
                .child(Text::new("Saved").color(scheme.on_primary_container)),
        )
    }))
}
```

For a status there are three more families beside `error`: `success`, `warning` and `info`,
each with its `on_` colour, a `_container` and an `on_..._container`. A theme made from a seed
turns them slightly toward the seed's hue, so they belong to its palette:

```rust
use frus::{component, Container, Text, Widget};

fn saved_banner() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let scheme = &cx.theme().scheme;
        Box::new(
            Container::new()
                .padding(12.0)
                .color(scheme.success_container)
                .child(Text::new("All changes saved").color(scheme.on_success_container)),
        )
    }))
}
```

`Alert` and `SnackBar` use them for their success, warning and info kinds.

Write a colour by hand — `Color::hex("#6366F1")`, `Color::rgb8(99, 102, 241)`,
`Colors::WHITE` — only where it is meant to stay the same whatever the theme.

## The application's theme

`Theme::from_seed(colour, dark)` makes a whole scheme from one colour. Give the application a
light theme, a dark one, and let the system choose:

```rust
use frus::{text, Color, FrusApp, Theme, ThemeMode};

fn app() -> FrusApp {
    let brand = Color::hex("#0F766E");
    FrusApp::from_fn(|_cx| Box::new(text("Hello")))
        .title("Tides")
        .theme(Theme::from_seed(brand, false))
        .dark_theme(Theme::from_seed(brand, true))
        .theme_mode(ThemeMode::System)
}
```

The application hands the result to `frus::main!(app())`. While it runs,
`host::app().set_theme_mode(ThemeMode::Dark)` switches it — from a settings switch, say.

## Changing how one kind of widget looks, everywhere

`theme.widgets` holds the defaults of each kind of widget: buttons, fields, cards, app bars,
dialogs, and the rest. Set a field and every widget of that kind follows, unless the widget
itself says otherwise:

```rust
use frus::{BorderRadius, Color, Theme};

fn square_buttons() -> Theme {
    let mut theme = Theme::from_seed(Color::hex("#0F766E"), false);
    theme.widgets.button.radius = Some(BorderRadius::uniform(6.0));
    theme.widgets.button.height = Some(44.0);
    theme
}
```

## Changing one widget

What you write on a widget wins over the theme:

```rust
use frus::{Button, Color, Colors, Icon, Icons, Variant, Widget};

fn buttons() -> Vec<Box<dyn Widget>> {
    vec![
        Box::new(Button::new("Save").on_press(|| {})),
        Box::new(Button::new("Cancel").variant(Variant::Outlined).on_press(|| {})),
        Box::new(Button::new("Delete").variant(Variant::Danger).on_press(|| {})),
        Box::new(Button::new("Add").icon(Icon::new(Icons::ADD)).variant(Variant::Tonal).on_press(|| {})),
        Box::new(
            Button::new("Custom")
                .color(Color::hex("#EA580C"))
                .label_color(Colors::WHITE)
                .radius(4.0)
                .on_press(|| {}),
        ),
    ]
}
```

A button with no `on_press` is disabled, and looks it.

When a look depends on the state — pressed, hovered, focused, disabled — a `ButtonStyle` says
it state by state, and `WidgetStateProperty::all` is the same value in every state:

```rust
use frus::{Button, ButtonStyle, Color, Widget, WidgetStateProperty};

fn styled() -> Box<dyn Widget> {
    Box::new(
        Button::new("Styled")
            .style(
                ButtonStyle::new()
                    .background_color(WidgetStateProperty::all(Color::hex("#1E293B")))
                    .foreground_color(WidgetStateProperty::all(Color::hex("#F8FAFC"))),
            )
            .on_press(|| {}),
    )
}
```

## Tokens of your own

A design system has values the theme has no field for: a brand gradient's colours, a card's
corner, a spacing scale. Put them in a type of your own that implements `ThemeExtension`,
store it with `Theme::with_extension`, and read it anywhere with `theme.extension::<T>()`.
It follows the theme into every subtree, and with a `lerp` it fades between light and dark
with everything else:

```rust
use frus::{Color, Theme, ThemeExtension};

#[derive(Clone, Debug, PartialEq)]
struct Brand {
    highlight: Color,
    card_radius: f32,
}

impl ThemeExtension for Brand {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Brand {
            highlight: self.highlight.lerp(other.highlight, t),
            card_radius: self.card_radius + (other.card_radius - self.card_radius) * t,
        }
    }
}

fn theme(dark: bool) -> Theme {
    let brand = Brand {
        highlight: if dark { Color::hex("#FDBA74") } else { Color::hex("#EA580C") },
        card_radius: 14.0,
    };
    Theme::from_seed(Color::hex("#0F766E"), dark).with_extension(brand)
}

let radius = theme(false).extension::<Brand>().map(|b| b.card_radius);
assert_eq!(radius, Some(14.0));
```

Leave `lerp` out for tokens that cannot be in between, such as a font family: they switch half
way through the fade.

## A different theme for one part of the screen

`Themed` gives a subtree a theme of its own — a dark panel in a light page, a branded card:

```rust
use frus::{Button, Color, Themed, Theme, Widget};

fn dark_panel() -> Box<dyn Widget> {
    Box::new(Themed::new(
        Theme::from_seed(Color::hex("#7C3AED"), true),
        Button::new("Inside a dark theme").on_press(|| {}),
    ))
}
```

## Icons and pictures

`Icon::new(Icons::NAME)` draws one of the bundled Material symbols; `.size()` and `.color()`
change it. `Image::network(url)` shows a picture from the network,
`Image::memory(bytes)` one already in memory, and `asset!("photo.png")` one embedded in the
program from a file beside the source. `.fit(BoxFit::Cover)` fills its box.

```rust
use frus::{Colors, Icon, Icons, Image, Row, Widget};

fn icons_and_a_picture() -> Box<dyn Widget> {
    Box::new(
        Row::new()
            .spacing(8.0)
            .child(Icon::new(Icons::FAVORITE).color(Colors::RED).size(32.0))
            .child(Image::network("https://example.com/photo.png").size(96.0, 96.0)),
    )
}
```
