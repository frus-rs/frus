# frus-cosmic-text

A copy of [cosmic-text](https://github.com/pop-os/cosmic-text) 0.19.0, the text shaping and
layout library frus measures and draws its text with, changed in the places below. It is
imported as `cosmic_text` (`[lib] name`), so the code that uses it reads as it would with the
original. The licence is cosmic-text's own: MIT or Apache-2.0, at your option (the two files
are here).

The `vi` editor and `syntect` highlighting modules are left out; nothing in frus uses them.

## What is changed, and why

Every change is marked `frus` in the source.

- **Variable font axes** (`Attrs::font_variations`, `FontVariations`). cosmic-text drives a
  variable face's `wght` axis from the weight and no other axis. A font instance is now made per
  face, weight and set of axis settings (`FontSystem::get_font_instance`), the shaper measures
  that instance, and the rasterizer draws it: `CacheKey::font_variations` carries the set's id
  to `SwashCache`, which applies every axis, each clamped to the face's range.
- **A language per span** (`Attrs::language`). The shaper was never told the text's language, so
  a font's localized forms (`locl`: Serbian Cyrillic, Polish accents, Turkish i) were never
  used. The run's language is now set on the shaping buffer.
- **Script fallback per span's language.** The fallback lists were chosen once per script for
  the font system's locale, so the Han characters Chinese, Japanese and Korean share were drawn
  by the same face whatever the text's language. They are now chosen per script and language.
- **Fonts loaded when a script first needs them** (`Fallback::load_faces`). A fallback can load
  faces into the database the first time text in a script is shaped, once per script and
  language; frus uses it on Android, whose fonts are not in the database up front.
- **A run is shaped with one set of features, axes and one language.** `Attrs::compatible` now
  compares them, so a span that changes only a feature starts a new run instead of taking the
  features of the run it falls in. `FontFeatures::set` replaces a tag set twice.
- **Dictionary line breaking** (feature `dictionary-line-breaks`). Thai, Lao, Khmer and Myanmar
  put no space between words, and the Unicode line breaking algorithm finds no break inside
  them. With the feature, the breaks between two letters of those scripts come from ICU4X's
  dictionary segmenter (`icu_segmenter`, Unicode licence); every other break is unchanged.
- `FontSystem::set_fallback` replaces the fallback lists of a font system already built.
