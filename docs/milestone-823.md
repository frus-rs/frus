# Milestone 823 — A release build carries only the glyphs it can show

## Objective

Issue #20: the bundled faces are 3.3 MB, about 40% of a small application, and cover far more of
Unicode than one application draws. Milestone 567 answered with a documented recipe (`pyftsubset`
by hand, the bundled faces turned off). This milestone makes it the build's job: a release
`frus build` cuts every font, frus's bundled faces and the application's own, down to the
characters the application can show, on every target, with nothing to install, and with a way to
say what else to keep. The issue asked for three things alongside the tool, and each is here:

- **a way to declare the ranges an application needs**: `[package.metadata.frus.fonts]`;
- **a fallback that degrades honestly**: a missing character is drawn as the font's box and the
  log names it, once;
- **no panic**, with any font feature on or off.

## What is kept: the coverage policy

What an application draws cannot be read from its sources. They hold its interface's own words;
they do not hold the name the reader types, the title a server sends, the translation loaded at
run time. A cut made from the sources alone breaks exactly the text a developer cannot test
ahead. So the policy is by **language**, with the sources as an addition:

| kept | why |
|---|---|
| Latin, whole: U+0020-007E, U+00A0-024F, U+02B0-036F (spacing modifiers, combining marks), U+1E00-1EFF (Vietnamese) | every language written in Latin, typed text of any of them |
| general punctuation, super- and subscripts, currency signs, letterlike symbols, U+FFFD | what any text carries: dashes, quotes, €, ™, the replacement character |
| the scripts of the application's languages | `locales` in the manifest, and every `Locale::new("…")`, `Locale::with_country`, `Locale::parse` its code names |
| the scripts of the widgets' own languages: English, French, German, Spanish, Japanese, Thai, Vietnamese | frus installs their words for any application that lists them (milestone 813) |
| the symbols the widgets draw: arrows, `−`, `√`, `≤`, `⋮`, `▾`, `✓`, `★`… (`FRAMEWORK_SYMBOLS`) | a release build would otherwise draw a box where a debug build drew the widget's symbol |
| every character in the application's string and character literals, `\u{…}` escapes decoded | the interface's own text, and the code points of an icon font it names |
| `ranges` and `text` from the manifest | anything else |
| OpenType features: those a shaper applies by itself, the widgets' `tnum`, every `font_feature("…")` the code names, `features` from the manifest | a stylistic set nobody turns on takes its alternate glyphs with it (`features = ["*"]` keeps them all) |

A locale's script comes from its script subtag when it has one (`sr-Latn`, `zh-Hant`) and from a
table of sixty-five languages otherwise; a language the table does not know is taken to be
written in Latin. Japanese keeps Kana and Han, Korean Hangul and Han, Chinese Han and Bopomofo.

**`FRAMEWORK_SYMBOLS` cannot fall behind the widgets.** A test reads the sources of `frus`,
`frus-widgets`, `frus-shell`, `frus-text` and `frus-gpu` the way the build reads an
application's, and fails, naming each character, when a literal the widgets can draw is outside
what a cut always keeps. Its first run found `▾` (the tree's open branch); writing it found the
reader had to skip test modules declared `#[cfg(all(test, desktop))] mod name;`, whose Hebrew is
not what a widget shows.

### The alternatives

- **Scanning string literals only** (the 567 recipe's `--text-file`). The smallest result, and
  wrong for exactly the text that matters most: the reader's. Kept as an addition, not the rule.
- **Keeping everything the bundled faces have for the scripts frus supports.** Safe, but DejaVu
  Sans alone covers Greek, Cyrillic, Armenian, Georgian, the IPA, math, dingbats and box drawing:
  the cut would save little.
- **A list the developer must write.** Correct, and a step everyone forgets until a build draws
  boxes in production. The policy above asks for nothing until an application shows a script
  outside its declared languages, and the log says which character when it does.
- **Dropping the hinting.** The hinting (`fpgm`, `prep`, the glyph programs) is about
  15% of a cut DejaVu face; it is kept, because the rasterizer uses it and a cut face must draw
  every kept glyph exactly as the whole one does.

## How it reaches the binary

A font is in the binary through `include_bytes!`, which `frus build` cannot reach into. Three
pieces:

- **`include_font!`** (new crate `frus-macros`, a procedural macro with no dependencies,
  re-exported as `frus::fonts::include_font!` and `frus_text::include_font!`). It resolves its
  path the way `include_bytes!` does, against the file that writes it, and expands to
  `{ let _ = option_env!("FRUS_FONT_SUBSET_DIR"); include_bytes!("<path>") }`. When that variable
  names a directory holding a cut copy of the file, the path is the copy's. The `option_env!` is
  what makes the compiler record the dependency on the variable, so a build that sets it, or stops
  setting it, compiles the crate again rather than reusing the other build's bytes.
- **`frus-text` embeds its seven faces through it**, so the bundled faces are cut like any other.
- **`frus-cli`** (`fonts.rs`), before a release or profile build of any target: reads
  `cargo metadata`, scans the sources of the local packages and of frus's crates for
  `include_font!(…)`, literals and locale tags, computes the coverage, cuts each font into
  `target/frus-fonts/<hash of the coverage>/<hash of the font's path>-<name>`, and sets
  `FRUS_FONT_SUBSET_DIR` for the cargo it starts. A copy is rewritten only when its bytes change,
  so an unchanged application recompiles nothing. The copy's name is an FNV-1a hash of the font's
  canonical path, computed by the macro and by the tool alike (each tests the same input and
  answer).

A font that cannot be cut is embedded whole and the build says why; nothing in the cut fails a
build. `frus build --no-font-subset`, `subset = false` in the manifest, a debug build,
`frus run` and `cargo build` by hand all embed the fonts whole.

### The subsetter

`skera` 0.10 (MIT OR Apache-2.0), the font subsetter of Google's fontations project: a Rust port
of HarfBuzz's subsetter, maintained with `read-fonts`, `write-fonts` and `skrifa`, which the
shaper already uses. Called with every layout script kept, the features above, and the closure
over the substitutions on: the glyphs an Arabic letter's joining forms, a ligature or a contextual
alternate reach are kept although no character maps to them, and the positioning tables stay.
Variable fonts keep their axes and variations. The names are all kept (frus finds a family by
the name inside the file) and so is `.notdef`, the box. `frus-cli` had no dependencies until now,
on purpose; this one is a subsetter that keeps what a shaper needs, which is not worth writing
again.

Rejected: `subsetter` (Typst's) drops the layout tables and the character map, being made for PDF
embedding, and says so; `allsorts` subsets for PDF too, without GSUB/GPOS; calling `pyftsubset`
would need Python on every developer's machine.

**The Thai line-break dictionary** is not a font: it is ICU4X data in `frus-text`
(`dictionary-line-breaks`) and is unaffected. The bundled faces have no Thai or Japanese glyphs to
cut; Thai and Japanese are drawn by the system's fonts on the desktop and Android.

## The honest fallback

`frus-text` names a character no loaded font has, once per character, the first time text holding
it is measured:

```text
WARN frus_text: U+0416 (Ж) is drawn as a box: no loaded font has it. A release build from `frus build` keeps of each font only the characters the application can show; `frus help fonts` says how to keep more, and a face for the script can be added with `frus::fonts::add_font`.
```

It looks for `.notdef` (glyph 0) in the shaped runs of a measurement, which is cached, so a frame
that reuses one pays nothing.

## Commands

- `frus build <target>` cuts in release and profile, prints what it kept (`Fonts: 3.3 MB → 607 kB,
  cut to the characters the application can show (Latin, Kana, Han, Thai)`, with `+ N characters
  from the sources` when its literals add some), and names any font embedded whole and why.
  `--no-font-subset` turns it off. A copy already cut for the same coverage from the same file is
  reused.
- `frus fonts [--locale <tag>]…` prints, without building, what a release build keeps of each font
  and what it weighs, and writes the cut copies.
- `frus size` / `--analyze-size` build with the fonts cut, as they ship, and list the cut copies
  found inside the binary as `fonts (cut)/<name>`.

## What it weighs

Each pair is one `frus build <target>` with `--no-font-subset` and one without, on this branch, in
release, rustc 1.96.1, Windows 11 (Android with NDK 27.0, Linux under WSL 2). Each number is the
file's size in bytes, read from the disk; "gzip" is `gzip -9` of the `.wasm`, what a server
compressing it sends.

**`frus-hello`**, the counter, every bundled face on (the default). Its fonts: 3.3 MB → 607 kB
(Latin, the widgets' languages; Japanese and Thai cost nothing, the bundled faces have neither).

| target | file | fonts whole | fonts cut | difference |
|---|---|---|---|---|
| Android, arm64 | `frus-hello.apk` | 6,587,251 | 5,182,323 | −1,404,928 (−21%) |
| | `libfrus_hello.so` | 13,903,936 | 11,073,088 | −2,830,848 (−20%) |
| Web | `frus_hello_bg.wasm` | 12,170,807 | 9,339,959 | −2,830,848 (−23%) |
| | gzip | 5,178,047 | 3,766,320 | −1,411,727 (−27%) |
| Windows | `frus-hello.exe` | 15,996,416 | 13,165,568 | −2,830,848 (−18%) |
| Linux, x86-64 | `frus-hello` | 20,312,264 | 17,481,288 | −2,830,976 (−14%) |

**Forge** (`frus-examples/forge`), which turns the bundled faces off and embeds four static Inter
weights (Regular, Medium, SemiBold, Bold) with `include_font!`: 1.3 MB → 566 kB.

| target | file | fonts whole | fonts cut | difference |
|---|---|---|---|---|
| Android, arm64 | `forge.apk` | 6,005,614 | 5,612,398 | −393,216 (−7%) |
| | `libforge.so` | 13,396,448 | 12,601,824 | −794,624 (−6%) |
| Web | `forge_bg.wasm` | 11,031,635 | 10,235,509 | −796,126 (−7%) |
| | gzip | 4,125,312 | 3,732,021 | −393,291 (−10%) |
| Windows | `forge.exe` | 15,002,624 | 14,208,000 | −794,624 (−5%) |

The uncompressed difference is the bytes the fonts lost (2,830,884 for the counter, 794,620 for
Forge), give or take the alignment of a section: the fonts are data, and no code moves. A
compressed package saves about half of it, as fonts compress by about half. For the counter that is a fifth of the APK and more than a quarter of what a
browser downloads. Inter keeps more per face than DejaVu (142 kB against 122 to 136 kB).
`ci/size-budget.toml` measures the library a plain `cargo build` makes, which embeds the fonts
whole, and is unchanged.

## How it was tested

- **The cut keeps what the shaper needs.** `frus-cli`'s tests cut the real bundled faces: DejaVu
  Sans keeps `A é ệ − € ✓` and loses `Ж Ω ب`, and keeps GSUB, GPOS, `name`, `OS/2` and the
  hinting (`fpgm`); Noto Naskh with Arabic declared keeps more glyphs than a cut that follows only
  the character map (the joining forms are reached through the substitutions).
- **The widgets' characters are always kept.** `framework_symbols_cover_the_widgets` reads the
  sources of `frus`, `frus-widgets`, `frus-shell`, `frus-text` and `frus-gpu` (the seven
  languages' tables included) and fails, naming each character or feature, when one is outside
  what a cut always keeps. It failed first on `▾` and on Hebrew from test modules the reader had
  not yet learnt to skip; both are fixed.
- **The source reader**: literals with escapes and raw strings, comments skipped, test items
  skipped (`#[cfg(test)]`, `#[cfg(all(test, …))]`, test modules in their own files), the feature
  an `include_font!` is behind, locale tags, feature tags; the manifest section and its errors; the
  coverage of a locale (script subtags, Japanese, Korean, Chinese); the copy's name, against the
  macro's own test of the same input.
- **The goldens, every one, with the faces cut** (WSL, the CI's pinned mesa): 280 tests in 26
  files pass with the whole faces and the same 280 pass with the faces cut to Latin, the widgets'
  languages and Arabic, Hebrew, Greek, Cyrillic, Devanagari, Hangul and Chinese (the scripts the
  goldens draw): Arabic and Hebrew right to left, Thai wrapping, the French calendar, mixed-script
  paragraphs, every widget screen. The pixels are compared against the committed images, so a
  glyph drawn differently, moved, or boxed would fail. That each test binary carried the cut faces
  and not the whole ones was checked by looking for each file's bytes inside it. The first attempt
  had not: the cut copies had been named for a path Git Bash rewrote, so the macro found none and
  embedded the whole faces, and the run proved nothing. Looking inside the binary is what caught
  it.
- **The missing character is named, once**: a `frus-text` test measures U+0378 (unassigned, in no
  font) twice and finds one warning that names it.
- **Dropping a feature still does not panic**: `frus-text`'s tests in seven configurations (all
  features; none; each group alone; sans and Arabic), each with the cut faces and with the whole
  ones. The two give the same results in every configuration. With features off, two to four
  tests fail in both, the same ones: they assert on what the configuration leaves out (a named
  family's face, a justified paragraph's edges, and Thai breaking between words, which needs the
  `dictionary-line-breaks` feature `--no-default-features` also drops); none is a panic of the
  font system. A face behind a feature this build leaves off is neither cut nor counted (Forge
  turns the bundled faces off, and `frus fonts` lists only its Inter).
- **Release binaries hold the cut faces**: each was searched for the bytes of every whole face
  and every cut copy; the whole faces are absent, the cut copies present.

## Not done

- **macOS was not measured**, nor the cut checked in a Mac's binary: there is no Mac here. Nothing
  in the mechanism is specific to a platform (a macro, an environment variable, files), and the
  Windows, Linux, web and Android builds all carried the cut faces.
- **No phone check.** No device was attached to this machine during the milestone; the release
  APK was built, measured and searched for the cut faces, not installed.
- **`frus run --release` embeds the fonts whole.** Only `frus build` (and `frus size`) cut them.
  A release run is for trying an application, and a cut there would make a missing script look
  like a bug in the code rather than a declaration; the trade-off is that the size a developer
  sees in a release run is not the shipped one.
- **A font embedded with `include_bytes!` is not cut.** The examples and the guides now use
  `include_font!`; an application written before this milestone keeps its whole faces until it
  switches.
- **`frus-macros` is a new crate to publish**, before `frus-text`, at the next release.
- **The literal reader is a lexer, not a parser.** It knows strings, raw strings, characters,
  comments and test items; a literal built by another macro (`concat!`) is not seen. The coverage
  does not depend on it for anything but the extra characters a source spells out.
- **Locales chosen at run time from data** (a list fetched from a server) are not seen; declare
  them in the manifest. The log names each character drawn as a box.
