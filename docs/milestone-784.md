# Milestone 784 — Text for every script and every typographic need

## Objective

Five things text needed and frus could not do:

1. **OpenType features** per style and per span: small capitals, ligatures off, tabular or
   old-style figures, fractions, stylistic sets.
2. **Variable font axes**: `wght`, `wdth`, `slnt`, `opsz` and a font's own. A weight asked of a
   variable font must use its axis.
3. **A language per span**, so a font's localized glyphs are used and, for the Han characters
   Chinese, Japanese and Korean share, the right face draws them. Serbian Cyrillic and Han
   unification are the classic tests. Unset, the application's locale.
4. **Line breaking by dictionary** for Thai, Lao, Khmer and Myanmar, which write no space
   between words.
5. **The platform's fallback fonts** for a character no bundled or registered face has, instead
   of a box: lazily, never panicking when a bundled font feature is off.

## What the stack had, and what it lacked

frus shapes with cosmic-text 0.19 (the newest release), which shapes with harfrust and draws
with swash. Read before writing anything:

| need | cosmic-text 0.19 |
|---|---|
| features | `Attrs::font_features`, per span, but a run takes the features of its first character: a span changing only features inside a word was ignored |
| axes | `wght` only, driven by the weight, in the shaper and the rasterizer. No other axis anywhere, and the glyph cache key has no room for one |
| language | never given to the shaper: `locl` forms were never used |
| Han fallback | per-OS lists keyed by locale, but chosen once per script for the *device's* locale, and matching a bare `ja` only, so `ja-JP` fell to the Simplified Chinese face |
| Thai breaks | `unicode-linebreak` (UAX #14): `SA` letters get no break between them |
| fallback fonts | desktop: every system font in the database, plus per-script lists. Android: no list and no system font in the database (frus loads only the emoji faces and, as a last resort, Roboto). Web: nothing |

**Proven first.** Before the change, the Thai test (`thai_wraps_at_word_boundaries`) fails: with
no break opportunity the shaper cuts through words wherever a glyph overflows. It still fails
when the `dictionary-line-breaks` feature is turned off, which is how it is kept honest. The
cosmic-text unit test `thai_breaks_between_words_with_the_dictionary` shows the same on the
break list alone: one break, at the end, without the dictionary.

Features, axes other than `wght`, the language and per-span fallback all live inside
cosmic-text's shaping and rasterizing, behind no hook. So frus now carries a copy:
**`frus-cosmic-text`**, imported as `cosmic_text`, as `frus-glyphon` already is for glyphon. The
changes are small (about 440 lines over eleven files, each marked `frus`) and listed in its
README.

## What changed

### The API

```rust
TextStyle::new(14.0)
    .font_feature("tnum", 1)        // OpenType feature: 1 on, 0 off, n an alternate
    .font_variation("wdth", 75.0)   // variable font axis, in the axis's units
    .locale("ja");                  // BCP 47 language of the text
TextSpan::new("1/2").font_feature("frac", 1).locale("sr");
TextShaping::NONE.with_feature("smcp", 1).with_variation("opsz", 12.0).with_locale("zh-Hant");
frus_text::set_default_locale("ja-JP"); // done by the shell from the resolved locale
```

The three travel as one `TextShaping`, a field of `TextStyle`, `ResolvedTextStyle`, `TextRun`
and `Primitive::Text`. It is **four bytes and `Copy`**: the settings live in a process-wide
table and the value is their index, equal settings always getting the same one. A `TextStyle`
inside a `Theme` is handed down by value at every level of the tree (see the note on the theme's
stack cost), and inline arrays of features and axes would have doubled its size. Interning is
bounded in practice: by the distinct settings an application writes, and an animated axis is
rounded to a quarter unit.

- `merge` combines by tag (a child's `liga 0` overrides its parent's `liga 1` and keeps its
  `tnum 1`); the top language wins.
- `lerp` lets an axis **glide** (a variable weight animates smoothly, which `FontWeight::lerp`
  with four faces cannot), and switches features and language halfway.
- An invalid tag, a non-finite axis value or something that is not a language tag is ignored
  and the console says so, once.

`frus_text::shaped(attrs, shaping)` turns it into the shaper's attributes, at the one place
every measurement (`measure_*`, `line_spans*`, `TextLayout`, rich runs) and every paint
(`frus-gpu`) goes through. The measurement cache key holds the shaping and the application's
language.

### Variable axes

A font instance is now made per face, weight and axis set; the shaper measures that instance and
the glyph cache key carries the set's id to the rasterizer, which applies every axis clamped to
the face's range. `wght` set as an axis becomes the weight, so it is matched the way cosmic-text
matches weights: against a registered family's own faces (a variable one draws exactly 550, a
static one its nearest), the bundled families' 400 and 700, or as asked for a platform family on
the desktop (Bahnschrift, Segoe UI Variable), whose variable faces draw it exactly.

### The language

Set on the shaping buffer, so `locl` and language-system lookups apply (Serbian and Macedonian
Cyrillic in the bundled DejaVu Sans; Polish, Romanian, Turkish forms where a font has them).
Unset, the application's locale: the shell sets it every frame from the locale it resolved
(`install_ambient`), and changing it forgets the measurements.

### Fallback fonts

frus installs its own `Fallback` (`FrusFallback`):

- **Per span's language.** The lists are chosen per script *and* language, and a Han-like script
  (Han, Hiragana, Katakana, Hangul, Bopomofo) reduces the language to the four answers the
  platform lists know: `ja`, `ko`, `zh-TW` (also `zh-Hant`), `zh-HK` (also Macau), else `zh-CN`.
  On Windows the same 直 is drawn by Yu Gothic in Japanese, Microsoft YaHei UI in Chinese and
  Malgun Gothic in Korean (unit test, output below).
- **Windows, macOS, Linux** keep what they had: every system font in the database when the font
  system is built (fontdb's scan of the fonts folder, or fontconfig's directories on Linux) and
  the per-script lists of the platform's own faces (Segoe UI family, Nirmala UI, Leelawadee UI,
  Yu Gothic ... on Windows; the CoreText-era names on macOS; Noto on Linux).
- **Android, lazily.** `fonts.xml` (`/system/etc/fonts.xml`) is read once, the first time a
  script needs it. Its fallback families are matched by script subtag (`und-Thai`, `und-Deva`)
  or, for Han, by language (`ja`, `ko`, `zh-Hans`, `zh-Hant`), the `compact` variant first. The
  files are loaded into the database the first time text in that script is shaped, **once per
  script and language per font system**, and never again; a collection (`NotoSansCJK-Regular.ttc`)
  is loaded once and its face for the language chosen by index. Nothing is read for Latin text.
  The new cosmic-text hook is `Fallback::load_faces`, called before the faces are matched.
- **Web**: there is no platform font a page can reach from WebAssembly. A character none of the
  application's fonts has is drawn as the face's missing-glyph box; the guide says to register a
  face for every script a web build shows.

Turning off a bundled font feature changes none of this, and never panics: the fallback only adds
faces.

### Dictionary line breaking

Feature `dictionary-line-breaks` (on by default in `frus-text`, forwarded by `frus`,
`frus-shell`, `frus-widgets`, `frus-gpu`). The break list is `unicode-linebreak`'s, plus the breaks
ICU4X's dictionary segmenter (`icu_segmenter` 2.4, compiled data, Unicode-3.0 licence, the
Unicode Consortium's own) finds **between two letters of the `SA` scripts**: every other break is
unchanged, so Latin, CJK and Arabic lay out as before. The dictionary model, not the LSTM: it is
the one ICU itself uses for these scripts, and faster.

**Size.** Measured on a minimal program that shapes one line with the fork (release, `opt-level =
"s"`, fat LTO, one codegen unit, stripped, Windows x64): 975,360 bytes without the feature,
2,062,848 with it, so **+1,087,488 bytes (about 1.04 MiB)** for the dictionaries of the four
scripts and the segmenter. That is a third of what the bundled fonts weigh, so it is a feature
like they are: on by default, because a Thai reader's paragraph cut through words is a defect,
and droppable by an application that never shows those scripts.

### Smaller fixes found on the way

- cosmic-text's fast path for an ASCII word checked the spans inside it but not the default
  attributes around them, so a span ending mid-word shaped the rest of the word with its own
  attributes. Found by the per-span feature test.
- `FontFeatures::set` replaced nothing: a tag set twice was shaped with both values.
- `TextSpan::measure_hash` now covers letter spacing and shaping.

## How it was tested

Unit tests (`frus-cosmic-text`, `frus-core`, `frus-text`), all on Windows:

- `a_feature_changes_the_glyphs`: "fi" is one glyph, two with `liga 0`; per span, `fi` (liga off)
  then `fi` gives three glyphs.
- `the_language_selects_a_localized_glyph`: "б" in `sr` is another glyph than in `ru`.
- `an_axis_changes_advance_widths_and_the_drawn_glyph`: in a small variable test font, `wdth 50`
  halves the advances, weight 700 widens them by a fifth and 550 lands between; the rasterized
  `H` at `wdth 50` is narrower than at 100.
- `a_wght_axis_becomes_the_weight`.
- `a_missing_glyph_resolves_to_a_platform_font`: Hebrew in DejaVu Sans Mono is glyph 0; with a
  stand-in `fonts.xml` naming a face that has it, every letter is drawn by that face, loaded on
  first use, and Latin text loads nothing more.
- `fonts_xml_fallback_families_are_read`, `a_platform_face_is_loaded_for_a_script_on_first_use`.
- `han_characters_take_the_face_of_their_language` (real platform faces).
- `thai_wraps_at_word_boundaries` and the cosmic-text `linebreak` tests.
- `frus-core`: interning, merge by tag, lerp, invalid input ignored, tags normalised.

The variable test font is `crates/frus-text/tests/fonts/FrusTestVariable.ttf` (17.8 kB), built by
`scripts/make-test-variable-font.py` from the bundled DejaVu Sans's ASCII, renamed as its licence
asks of a modified font, with a `wdth` axis that halves and a `wght` axis that widens every glyph
by a fifth, so a test can predict the widths.

Goldens (`frus-test`, `tests/text_shaping.rs`, llvmpipe under WSL, a test binary of its own
because it registers a font for the process):

- `text_features_language`: "fi fl office" with ligatures, then with `liga 0`; бгдпт in Russian,
  then in Serbian. The test asserts the pairs differ before comparing the image.
- `text_variable_axes`: the test font at `wdth` 50, 75, 100, then `wght` 550 and 700. The test
  asserts the ink widens step by step and that `wdth 50` is about half of 100.
- `text_thai_wrapping`: a 37-letter Thai sentence without a space in a 120 px box breaks into
  ข้าวที่ / บ้านบ้านของ / ฉันอยู่ใกล้ / ตลาด, each line a run of whole words (Noto Sans Thai from
  the system, through the fallback lists).

A real Windows window: `cargo run -p frus-demo --example text_shaping` (Windows 11, the
bundled DejaVu faces plus the system's). What it showed, read off the screenshot:

- Calibri with `smcp`, `frac` and `onum`: small capitals, ½ and ¾ as fractions.
- A rich text whose spans differ only in a feature and a language: `fi` as a ligature then as
  two letters (`liga 0` on its span), and Russian бдп then Serbian бдп, whose forms differ.
- Bahnschrift at `wght` 300, 450 and 650, and at `wdth 75 wght 700`: four distinct weights
  and a condensed cut, from one variable file.
- 直 骨 逸 海 in `ja`, `zh-Hans`, `zh-Hant` and `ko`: four different faces (骨 shows it best).
- Russian and Serbian бгдпт: the Serbian б, г, д, п, т take their Serbian forms.
- The Thai sentence in a 220 px box wraps after ของ, between two words.
- Devanagari, Ethiopic, Khmer and Hebrew, none of them in the bundled faces, drawn by
  Nirmala UI, Ebrima, Leelawadee UI / Khmer UI and Segoe UI from the platform lists.

## Evidence

```text
cargo test -p frus-text --lib -- han_characters --nocapture
ja: Yu Gothic, zh: Microsoft YaHei UI, ko: Malgun Gothic

# without the dictionary (cargo test -p frus-text --no-default-features --features bundled-sans,bundled-mono)
at 34, a line breaks inside a word: [0..9, 9..18, 18..30, 30..48, 48..51] (words at [0, 9, 18, 30, 39, 51])
test result: FAILED. 0 passed; 1 failed
# with it (default features)
test result: ok. 1 passed

cargo test -p frus-core -p frus-text -p frus-cosmic-text
frus-core 255 passed; frus-cosmic-text 6 passed; frus-text 68 passed; doctests 25, 3, 3 passed
```

The screenshot of the Windows run is kept with the milestone's working files
(`text_shaping_windows.png`); the PR shows it.

Also checked: `cargo clippy --workspace --all-targets -D warnings`, `cargo clippy -p frus-text
-p frus-cosmic-text` for `wasm32-unknown-unknown` and `aarch64-linux-android`, `cargo check -p
frus-text --target aarch64-apple-ios`, `cargo doc --all-features -D warnings` for the three
crates, `cargo test -p frus-widgets --test core_exports`.

## Not done

- **Not run on the phone.** The Android loader is tested with a stand-in `fonts.xml` and a face
  on disk, and `fonts.xml`'s format is the one Android 10 ships; nobody has looked at Thai or
  Devanagari on a device yet. The machine was saturated by other builds for most of this
  milestone, and an APK build did not fit.
- **Rich text spans keep no family.** `TextRun` has never carried a font family (a rich text is
  drawn in the default family, or the Arabic one for Arabic), so a `TextSpan` cannot name one,
  and a feature on a span works only if the default family has it. Features, axes and the
  language do travel per span.
- **No automatic optical size.** A face with an `opsz` axis is drawn at its default `opsz` unless
  the style sets one; matching it to the font size, as browsers do, would make a font instance per
  size.
- **The desktop still reads every system font at start-up**, as it did before (fontdb's scan,
  measured in milestone 683). Only Android is lazy. Making the desktop lazy as well would need the
  platform's own fallback service (DirectWrite's `IDWriteFontFallback`, CoreText's cascade
  list), not a static list, and is its own milestone.
- **macOS and Linux** keep cosmic-text's static per-script lists, now chosen per language; neither
  was run here. The lists name the platform faces; CoreText's cascade list is not queried.
- **The web** has no platform fonts: a script the application ships no face for is a box there.
- **A weight between two static faces** of a platform family (a desktop font that is not
  variable) is matched by cosmic-text among the platform's faces; frus does not inventory system
  families.
- **Line breaking** uses the dictionaries only between two letters of the `SA` scripts; Chinese
  and Japanese keep the Unicode algorithm's per-character breaks, which is what they need.
- **The fork** must be published to crates.io before the next release (`frus-cosmic-text`,
  before `frus-glyphon` and `frus-text`). Its changes are worth offering upstream; that was not
  done here.
