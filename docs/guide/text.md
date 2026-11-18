# Text and fonts

Text in frus is shaped and drawn by frus itself: the same shaping engine, the same line breaking
and the same fonts on every platform. This page covers how text is written, how a reader selects
it, which fonts are drawn and how to bring your own, right-to-left writing, and the reader's text
size. The theme's type roles and colours are in [Text, colour and the theme](style.md).

## Plain and rich text

`Text::new` has chosen nothing: it takes its size, weight and colour from the part of the screen
it is in (an app bar's title, a button's label), and otherwise 16 pixels, regular, in the
theme's `on_surface`. Every builder you call on it wins over that.

```rust
use frus::{Column, FontFamily, FontWeight, Text, TextOverflow, Widget};

fn card_text(title: &str, body: &str) -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .spacing(4.0)
            // One line, cut with "…" when it does not fit.
            .child(Text::new(title).size(18.0).weight(FontWeight::Medium).ellipsis())
            // Wraps, but never more than three lines, the last one cut.
            .child(
                Text::new(body)
                    .wrap()
                    .max_lines(3)
                    .overflow(TextOverflow::Ellipsis)
                    .height(1.4),
            )
            .child(Text::new("ID 4417").family(FontFamily::Monospace).size(12.0)),
    )
}
```

`.ellipsis()` always means **one line**; for a cut after several lines, `.wrap().max_lines(n)`
with `.overflow(TextOverflow::Ellipsis)`. `.height(h)` is the line height as a multiple of the
size.

Words in different styles inside one paragraph are a `RichText` made of `TextSpan`s. A span
says only what changes; the rest comes from its parent span and from the text's base style:

```rust
use frus::{Colors, RichText, TextSpan, TextStyle};

let receipt = RichText::new(
    TextSpan::new("Order ")
        .child(TextSpan::new("4417").bold())
        .child(TextSpan::new(" is ready. "))
        .child(TextSpan::new("Overdue").color(Colors::RED).underline()),
)
.base_style(TextStyle::new(15.0))
.wrap();
# let _ = receipt;
```

OpenType features, a variable font's axes and the language of a span are in
[the style page](style.md#opentype-features-variable-fonts-and-the-language).

## Letting the reader select and copy

A text is not selectable unless you say so. `.selectable()` makes one text selectable: a drag
or a double click selects, a long press on a phone selects a word and shows its handles, and the
bar over the selection offers Copy and Select all, in the reader's language.

`SelectionArea` makes everything under it selectable as one: a selection can start in one
text and end in another, as on a web page. Buttons and fields inside it keep their presses.

```rust
use frus::{column, EditAction, SelectionArea, Text, ToolbarItem};

let order_id = Text::new("ORDER-4417").selectable();

let terms: SelectionArea<()> = SelectionArea::around(column![
    Text::new("Terms of service").heading(),
    Text::new("You may copy any of this."),
    Text::new("Drag from one line to another."),
]);

// The bar over a selection is yours to change: here, Copy and nothing else.
let copy_only: SelectionArea<()> = SelectionArea::around(Text::new("A1B2-C3D4"))
    .selection_toolbar(|_, items| {
        items
            .into_iter()
            .filter(|item| item.edit_action() == Some(EditAction::Copy))
            .collect::<Vec<ToolbarItem<()>>>()
    });
# let _ = (order_id, terms, copy_only);
```

`.selection_color(..)` and `.handle_color(..)` change how the selection looks; `theme.widgets.text_selection` changes it everywhere.

## The fonts frus draws with

frus bundles a small set of faces, so that text looks the same everywhere and nothing is
missing on a platform with few fonts. Each is a cargo feature of `frus`, all on by default:

| Feature | What it adds | Size | Without it |
| --- | --- | --- | --- |
| `bundled-sans` | DejaVu Sans, regular and bold | about 1.5 MB | the platform's sans-serif (Roboto on Android) |
| `bundled-italic` | DejaVu Sans Oblique and Bold Oblique | about 1.3 MB | italic is drawn upright |
| `bundled-mono` | DejaVu Sans Mono | about 340 KB | monospace falls back to the default family |
| `bundled-arabic` | Noto Naskh Arabic, regular and bold | about 360 KB | Arabic cannot be shaped on Android |
| `dictionary-line-breaks` | word dictionaries for Thai, Lao, Khmer and Myanmar | about 1.1 MB | those scripts break only at spaces |

An application that brings its own font can drop the ones it does not need. Turning the
defaults off drops images too, so name what you keep:

```toml
frus = { version = "0.5", default-features = false, features = ["bundled-sans", "images"] }
```

Dropping a face never panics: text falls back to the next font that has its letters.

**A character none of your fonts has** is drawn by the platform's own font for its script on
the desktop and on Android. The **web** has no such fallback: register a face for every script a
web build shows, or its characters are drawn as boxes.

### Your own font

Register the font's files once, before the application starts, then name its family. The usual
place is the block given to `frus::main!`:

```rust
use frus::{FontFamily, FrusApp, Text};

fn app(regular: Vec<u8>, bold: Vec<u8>) -> FrusApp {
    // In an application: include_bytes!("../fonts/Inter-Regular.ttf").to_vec()
    frus::fonts::add_font(regular);
    frus::fonts::add_font(bold);
    // Every text that names no family is drawn in Inter...
    frus::fonts::set_default_family("Inter");
    FrusApp::from_fn(|_cx| {
        // ...and one text can still name another.
        Box::new(Text::new("Display").family(FontFamily::Named("Inter")))
    })
}
```

With `frus::main!({ /* add_font, set_default_family */ app() })` the fonts are registered on
every platform, the web included. Register one file per weight you use; a weight you did not
register is drawn in the nearest one you did. `set_monospace_family` does the same for
`FontFamily::Monospace`.

The family is the name written **inside** the font file. A near miss is forgiven (other case or
spacing, or one extra word: `"Inter"` finds `"Inter 24pt"`), and a name that matches nothing logs
a warning and draws in the first family you registered rather than nothing.

### Making the binary smaller: a subset

A font cut down to the letters an application shows weighs a few dozen kilobytes instead of a
megabyte or more. With `fontTools` (`pip install fonttools`), once per face:

```sh
cat src/*.rs > used.txt
python -m fontTools.subset DejaVuSans.ttf \
  --unicodes=U+0020-007E,U+00A0-00FF --text-file=used.txt \
  --no-hinting --notdef-outline --output-file=fonts/DejaVuSans-Subset.ttf
```

Then turn the bundled face off and register the subset as above. Text that comes from a server
or from the reader is not in your sources: name its ranges in `--unicodes`. A letter the subset
lacks is drawn as a box, never a crash, so look at every screen once in each language before
shipping. [Starting a frus application](../getting-started.md) has the measured sizes.

## Right to left

Arabic, Hebrew, Persian and Urdu are written from right to left. Shaping and the order of
mixed-direction words inside a line are automatic. What faces which way on the screen, rows,
padding, icons and the back arrow, follows the theme's `direction`:

```rust
use frus::{Color, Container, FrusApp, InsetsDirectional, Row, Text, Theme, Themed};

// The whole application, right to left.
fn arabic_app() -> FrusApp {
    FrusApp::from_fn(|_cx| Box::new(Text::new("مرحبا")))
        .theme(Theme::from_seed(Color::hex("#0F766E"), false).rtl())
}

// Or follow the language the application resolved: `builder` wraps every page.
fn follows_language(app: FrusApp) -> FrusApp {
    app.builder(|cx, pages| {
        let rtl = matches!(frus::locale::of().language_code(), "ar" | "he" | "fa" | "ur");
        let theme = if rtl { cx.theme().clone().rtl() } else { cx.theme().clone() };
        Box::new(Themed::new(theme, pages))
    })
}

// Sides that follow the direction: start is the left in English, the right in Arabic.
fn indented() -> Container {
    Container::new()
        .padding_insets(InsetsDirectional::only_start(24.0))
        .child(Row::new().child(Text::new("1")).child(Text::new("2")))
}
# let _ = (arabic_app, follows_language, indented);
```

Write `start` and `end` rather than left and right wherever the side should flip:
`InsetsDirectional`, `AlignmentDirectional`, `BorderRadiusDirectional`. A `Row`, `Column` or
`Stack` can also be given a direction of its own with `.text_direction(..)`.

## The reader's text size

On Android the reader's font size reaches every text: frus scales it all, and a layout that
leaves room for longer lines keeps working. Read the factor with `MediaQuery::of().text_scaler`.
Where a few pixels must not grow (a badge, a tab's label), cap the factor for that style:

```rust
use frus::{MediaQuery, Text, TextStyle};

let badge = Text::styled("99+", TextStyle::new(11.0).clamp_scale(1.3));
// A size of your own, scaled as text is.
let gap = MediaQuery::of().scaled(8.0);
# let _ = (badge, gap);
```

On the desktop and the web the factor is 1 for now. Under `frus run`, the `T` key steps through
larger sizes so you can see a screen as such a reader does.
