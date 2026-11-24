# Milestone 842 — Editing steps by grapheme cluster

## Objective

Part of #56 (text: bidi, complex scripts, fallback chains and emoji), whose last comment left
"multi-codepoint emoji sequences (skin tones, flags, ZWJ families) and grapheme-aware editing"
open. What a reader sees as one character can be several `char`s:

| Cluster | Characters |
|---|---|
| a family: man, ZWJ, woman, ZWJ, girl | 5 |
| a flag (Togo): two regional indicators | 2 |
| a thumb with a skin tone | 2 |
| `e` and a combining acute accent | 2 |
| Devanagari "ki" (KA + vowel sign I), Thai "kam" | 2 each |
| an Arabic letter carrying its vowel mark | 2 |

Editing indices in frus are counts of `char`s, and every step of editing was one `char`:
the arrows stopped inside a family emoji four times, Backspace took the girl off it and left
a dangling joiner, a double click on an emoji selected one of its parts, a word jump stopped
between `e` and its accent, a press on an emoji could put the caret inside it, and a length
limit could let half of a thumb in. The goal: every one of these steps by **extended grapheme
cluster** (Unicode UAX #29), in left-to-right and right-to-left text alike.

## Root cause

Two places, each counting `char`s where it should count clusters.

1. **The layout** (`frus_text::TextLayout`). Each character of a line got an extent, and a
   glyph covering several characters had its width split evenly between them. A caret
   boundary existed between every two characters, and `visual_step` and `position_at` walked
   those boundaries. So a ZWJ family drawn as one glyph was five fifths, each with its own
   boundary, and a press on its second fifth put the caret after the man. Worse, when a
   cluster's characters were covered by several glyphs, the last glyph's share overwrote the
   others, so a family could end up with **no width at all** on the test machine and be
   skipped by the arrows altogether.
2. **The editing model** (`frus-widgets`' `textinput.rs`, shared by `TextField`, selectable
   `Text` and `RichText`). Backspace removed `chars[cursor - 1]`, Delete `chars[cursor]`, the
   layout-less arrows moved by one, `word_range` and the word jumps tested one `char` at a
   time (and a combining mark is not alphanumeric), and the length limit truncated at a
   `char` count.

A third, found on the way: Android's `deleteSurroundingText(before, after)` counts **UTF-16
units**, and the shell turned it into that many Backspace presses. With Backspace taking a
cluster, a keyboard deleting a thumb with a skin tone (four units) would have taken four
clusters; before this milestone it already took one character too many for any emoji (two
units, one `char`).

## Design

### One definition of a cluster

`frus_text::grapheme_stops(text)` gives the cluster boundaries in characters, from
`unicode-segmentation` (extended clusters, already in the tree through cosmic-text).
`grapheme_before`, `grapheme_after` and `grapheme_floor` step over a sorted list of stops by
binary search. Editing indices stay counts of `char`s, as everywhere else in frus: only the
places a caret may stand change.

### The layout knows its clusters

`LayoutLine` gains `stops`, whether each boundary of the line is a cluster boundary. While
reading the shaped glyphs:

- a glyph's width is split between the **clusters** it draws, not the characters — a
  ligature of two letters still has two halves, a ZWJ sequence drawn as one glyph is not cut;
- a character drawn by several glyphs (a base and its mark) spans all of them;
- every character of a cluster is then given the cluster's whole extent and direction.

`position_at` considers only cluster starts and lands on the end of the cluster under the
press for its trailing half; `visual_step` crosses one cluster and lands after all of it.
Since every character of a cluster shares its extent, `caret_rect`, `selection_rects` and the
selection handles need no change: an index inside a cluster is drawn at one of its edges.

### The editing model steps by cluster

Backspace and Delete remove the whole cluster before or after the caret; the arrows without a
layout go to the previous or next stop; a word jump and a double click treat a cluster as a
word when its first character is a letter, digit or `_` (so a letter carries its marks, and
an emoji sequence is one separator); the word jump on screen judges the cluster it crossed
by its first character; and the length limit cuts at the last stop that fits, so an emoji
that does not fit whole does not land.

### Android's surrounding deletion counts units

`ime::plan` turns `Delete { before, after }` into the exact stretches it names: the units are
converted to characters, the stretch after the selection is selected and deleted, then the
stretch before it, and the caret and selection are put back, moved by what went before them.
Deleting a selection is exact, so the input method's count is honoured whatever Backspace
does on its own.

## How it was tested

Each test below was run against the code before the fix and failed, then passes after it.

- `frus-text`:
  - `visual_steps_stop_only_at_grapheme_boundaries`: steps across seven texts (family, two
    flags, skin tone, combining accent, Devanagari, Thai, Arabic with harakat — the last right
    to left) both ways and requires exactly the cluster boundaries. Before: `[0, 1, 7]` for
    `a` + family + `b`, the family having no width at all.
  - `a_press_lands_on_a_grapheme_boundary`: sweeps a press across each text every half
    pixel; every position must be a cluster boundary. Before: a press 18.5 px into
    `x` + two flags landed at 2, between the two regional indicators of the first flag.
  - `a_cluster_has_one_box`: the family's caret edges are apart, its selection is one
    rectangle between them, and every index inside it is drawn at one of its edges. Before:
    both edges at the same `x`.
  - `grapheme::tests`: the boundaries themselves, and the stepping helpers.
- `frus-widgets` (`textinput::grapheme_tests`): Backspace and Delete over each cluster kind,
  two flags in a row deleted one at a time, the layout-less arrows, the arrows on screen in
  a field (LTR and RTL), word jumps and double clicks over a decomposed `é` and over emoji,
  and the length limit refusing half a thumb.
- `frus-shell` (`ime::tests::deleting_around_the_caret_counts_utf16_units`): four units
  before the caret remove one thumb with its skin tone and nothing else; two after it one
  emoji; around a selection, the selection is kept and moved.

The whole library suites pass after the change: `frus-text` 75, `frus-widgets` 2584 (built
without optimisation; its `a_deep_tree_builds_on_a_platform_s_smallest_stack` was left out of
that run, since it needs the optimised build's frame sizes to fit its deliberately small
stack), `frus-shell` 639. `cargo clippy --workspace --all-targets -- -D warnings` is clean.

## Not done

- **Emoji and complex-script fallback rendering** is unchanged: a machine with no emoji face
  draws a ZWJ sequence as whatever the fallback finds. On the Windows test machine the family
  shapes but its glyphs left it without width before this change; that it is drawn as one
  colour picture is not verified here.
- **The length limit counts characters**, not clusters: a family emoji costs five of a
  `max_length`. It no longer splits one, but a limit of 10 does not hold ten emoji.
- **CR LF** is one cluster, but Home/End without a layout look for `\n` only and can stop
  between the two. Fields do not normally hold `\r`.
- **Text cut short by a line limit** (`TextLayout::elided`) is cut at a character count; a
  cut through a cluster is not prevented.
- **Vertical caret movement and the selection handles** use `position_at` and so land on
  cluster boundaries, but have no tests of their own here.
- Not run on a phone or in a browser (the web field edits in the browser's own `<input>`, which
  already steps by cluster). Android's `deleteSurroundingText` path is covered by the plan's
  unit test, not by a keyboard on a device.
- The rest of #56 — bidi reordering details beyond what milestones before handled, per-run
  versus per-character fallback — is left as it was.
