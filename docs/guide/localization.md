# Languages

An application in several languages needs three things: to know which language the reader
wants, to say its own words in it, and to have the framework say its words (a back arrow's
label, a date picker's months, the Copy bar over a selection) in it too. frus does the first
and the last, and works with Fluent files for the second.

## Which language

List the languages the application has, best first. frus compares them with the languages the
device is set to and picks the closest: a phone set to Mexican Spanish gets `es` if that is what
the application has. A reader whose languages are all missing gets the first one in the list.

```rust
use frus::{text, FrusApp, Locale};

fn app() -> FrusApp {
    FrusApp::from_fn(|_cx| Box::new(text(format!("Language: {}", frus::locale::of()))))
        .supported_locales(vec![Locale::new("en"), Locale::new("fr"), Locale::new("es")])
}
```

While it builds, any widget reads the language with `frus::locale::of()`. The same comparison is
`frus::locale::resolve`, which a test can call:

```rust
use frus::Locale;

let supported = [Locale::new("en"), Locale::new("es"), Locale::new("ja")];
let resolved = frus::locale::resolve(&[Locale::with_country("es", "MX")], &supported);
assert_eq!(resolved, Locale::new("es"));
// Nothing in common: the application's first language.
assert_eq!(frus::locale::resolve(&[Locale::new("sw")], &supported), Locale::new("en"));
// Tags as the platforms write them.
assert_eq!(Locale::parse("zh_Hant_TW").map(|l| l.to_string()), Some("zh-Hant-TW".into()));
```

**A language setting inside the application** pins one over the device's, and `None` goes back
to following the device. The change shows on the next frame:

```rust
use frus::{host, Button, Locale};

let french: Button = Button::new("Français")
    .on_press(|| host::app().set_locale(Some(Locale::new("fr"))));
let device: Button = Button::new("As the device").on_press(|| host::app().set_locale(None));
# let _ = (french, device);
```

## The application's own words: Fluent

The `frus-l10n` crate keeps the application's words in [Fluent](https://projectfluent.org)
files, one per language: plurals, genders and arguments are written by the translator, not in
the code. Add it beside `frus`:

```toml
frus-l10n = "0.5"
```

Load the files once and look each message up in the language frus resolved:

```rust
use frus_l10n::{args, Arg, Localizer};
use std::sync::OnceLock;

const EN: &str = "
greeting = Hello, { $name }!
unread = { $count ->
    [one] { $count } unread message
   *[other] { $count } unread messages
}";
const FR: &str = "
greeting = Bonjour, { $name } !
unread = { $count ->
    [one] { $count } message non lu
   *[other] { $count } messages non lus
}";

fn words() -> &'static Localizer {
    static WORDS: OnceLock<Localizer> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut words = Localizer::new("en");
        // In an application: include_str!("../i18n/en.ftl")
        words.add("en", EN);
        words.add("fr", FR);
        words
    })
}

/// A message in the language the application is showing.
fn tr(key: &str, args: &[(&str, Arg)]) -> String {
    let words = words();
    let language = frus::locale::of();
    words.format_for(&words.langid(language.language_code()), key, args)
}

// What a widget writes:
let title = frus::text(tr("greeting", args![name: "Ada"]));
# let _ = title;

let words = words();
assert_eq!(words.format_for(&words.langid("fr"), "unread", args![count: 2]), "2 messages non lus");
assert_eq!(words.format_for(&words.langid("en"), "unread", args![count: 1]), "1 unread message");
```

A message missing in one language falls back to the default language, and one missing in every
language shows its key, so a gap is visible rather than blank. Keep the `.ftl` files in an
`i18n/` folder beside `src/` and embed them with `include_str!`: there is nothing to load at
run time, on any platform.

## The framework's own words

frus has its words in **English, French, German, Spanish, Japanese, Thai and Vietnamese**. An
application that lists one of them in its supported languages gets it with nothing more to do:

```rust
use frus::{localizations, Locale, Localizations};

let words = localizations::for_locale(&Locale::new("es"));
assert_eq!(words.cancel_button_label(), "Cancelar");
assert_eq!(words.month_year(8, 2026), "agosto de 2026");
// The week starts on Monday in Spain.
assert_eq!(words.first_day_of_week_index(), 1);
// A language frus has no words in gets English.
assert_eq!(localizations::for_locale(&Locale::new("sw")).cancel_button_label(), "Cancel");
```

The tables carry more than words: which day a calendar's week starts on, the order a date is
typed in (`17.08.2026` in German, `2026/08/17` in Japanese), how a month and year are written
together, how a number's thousands are grouped, and whether the clock has twenty-four hours
(`uses_24_hour_clock`, which an application can use for a time picker's `hour12()`).

**To change some of the words, or to add a language**, implement `Localizations`. Every entry
has an English default, so a table writes only what differs, and the application hands it over:

```rust
use frus::{text, FrusApp, Locale, Localizations};
use std::rc::Rc;

/// English and Swahili: frus has no Swahili table, so the application brings the words.
struct Words;

fn swahili() -> bool {
    frus::locale::of().language_code() == "sw"
}

impl Localizations for Words {
    fn cancel_button_label(&self) -> &str {
        if swahili() { "Ghairi" } else { "Cancel" }
    }
    fn ok_button_label(&self) -> &str {
        if swahili() { "Sawa" } else { "OK" }
    }
    fn back_button_label(&self) -> &str {
        if swahili() { "Rudi" } else { "Back" }
    }
}

fn app() -> FrusApp {
    FrusApp::from_fn(|_cx| Box::new(text("Habari")))
        .supported_locales(vec![Locale::new("en"), Locale::new("sw")])
        .localizations(Rc::new(Words))
}
# let _ = app;
```

The table the application gives replaces frus's own for every language, which is why this one
answers both. Look at `Localizations` in the API reference for the whole list of entries.

## Fonts for every script

The bundled font has the letters of English, French, German, Spanish, Vietnamese and the other
Latin, Greek and Cyrillic languages, and Arabic. Japanese, Chinese, Korean, Thai and the other
scripts are drawn by the system's fonts on the desktop and on Android, so there is nothing to
ship. A **web** build has no system fonts: register a face for each such script with
`frus::fonts::add_font` (a subset of Noto Sans JP or Noto Sans Thai, say), or its letters are
drawn as boxes. See [Text and fonts](text.md).

## Right to left

Arabic, Hebrew, Persian and Urdu also turn the layout around. That follows the theme's
direction, which the application sets for those languages: see
[Text and fonts](text.md#right-to-left).
