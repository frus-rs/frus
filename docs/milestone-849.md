# Milestone 849 — Word deletion, a length limit in characters, cuts and CR LF by cluster

## Objective

Part of #56 (text: bidi, complex scripts, fallback chains and emoji), and the last of its
editing items. Milestone 842 made the caret, Backspace, Delete and the double click step by
grapheme cluster and listed what it left: the length limit counted code points, the ellipsis cut
a code point at a time, and Home and End could stop inside a CR LF. LIMITATIONS L-038 added the
missing key: no text field could delete a word. This milestone closes all four.

1. **Word and line deletion** (L-038). Ctrl+Backspace and Ctrl+Delete delete a word on Windows,
   Linux and an Android hardware keyboard; Option+Backspace and Option+Delete on an Apple
   keyboard. Cmd+Backspace and Cmd+Delete delete to the start or the end of the line on an Apple
   keyboard, Alt+Backspace and Alt+Delete elsewhere. One undo brings a deleted word back.
2. **`max_length` counts what a reader counts**: grapheme clusters, not code points. A paste
   past the limit is cut at a cluster boundary; a composition is cut, or waits, as the platform
   does it.
3. **An ellipsis never cuts through a cluster**, in left-to-right and right-to-left text.
4. **CR LF is one line break**: on screen, for the caret, for deletion; a single-line field
   leaves the line breaks of a paste out.

## Root causes

- **No deletion key but one cluster.** `Key` had `Backspace` and `Delete`, and the shell mapped
  the physical keys to them whatever the modifiers. On Android the bridge dropped the key's meta
  state on its way to Rust and did not take forward Delete at all.
- **The limit counted `char`s.** `TextField::on_edit` compared `chars.len()` with `max_length`,
  and the counter showed `value.chars().count()`: a family emoji cost five. It also cut the
  **end** of the whole text: a paste into the middle of a field near its limit dropped what was
  already after the caret, and a keystroke into a full field was emitted as an unchanged value.
- **The ellipsis popped `char`s.** `text::truncate`, `text::ellipsise` and the rich-text loop
  removed one code point at a time and stopped as soon as the rest fitted — after the man of a
  family, between two regional indicators, between an Arabic letter and its vowel.
- **Two splitters disagreed about lines.** The shaper splits a text at CR LF, a lone CR and a
  lone LF (`LineIter`), and rich text at the bidirectional paragraph separators. `TextLayout`
  split the same text again with `split('\n')`, and `line_spans` and `runs_cut_at` counted each
  line as its text plus one byte. So in `"ab\r\ncd"` the layout's first line was `"ab\r"`: End
  stopped at 3, between the CR and the LF, and typing there split the break in two. Every line
  after a CR LF was placed one byte early in the spans a line limit cuts at. A lone CR, which the
  shaper breaks on, gave the shaper one more line than the layout had, and the layout indexed
  past the end of its list: a panic.

## Design

### Deletion keys

`Key` gains `DeleteWord { forward }` and `DeleteLine { forward }`. The shell's
`deletion_key(apple, ctrl, alt, meta, forward)` reads the modifiers the way milestone 736 reads
them for the arrows: the word modifier is Option on an Apple keyboard and Ctrl elsewhere, the line
modifier Cmd on an Apple keyboard and Alt elsewhere. Android's bridge now passes the meta state
and takes `KEYCODE_FORWARD_DEL`, and the same function reads it as a keyboard that is not
Apple's. On the web a focused field edits in the browser's own `<input>`, which already deletes
words with the platform's keys.

`TextField` deletes, with nothing selected, from the caret to `word_boundary_left` or
`word_boundary_right` — the boundaries of milestone 842's word jumps, so Ctrl+Backspace takes
exactly what Ctrl+Shift+Left would select — or to the start or the end of the logical line. A
selection is deleted as Backspace deletes it. At the start of a line a line deletion takes
nothing: the break above belongs to no line. In an **obscured** field the words and the lines
cannot be seen, and stopping between two would tell where the dots hide a space, so both take
everything on that side. A read-only field refuses them as it refuses every change.

The undo history records a word or a line deletion as a **chunk**: one deliberate act, one step,
never joined to the backspaces before it.

### The limit in clusters

`limit_clusters` holds the text to `max_length` grapheme clusters after an edit. What the edit
inserted gives way first, from its end and a whole cluster at a time, so the text around a paste
stays and what fits of the paste lands where the caret was; if that is not enough (a value over
the limit to begin with), the text is cut after its `max`-th cluster. An edit the limit refuses
whole is nothing done: no value is emitted and the caret stays. The counter counts clusters too.

A composition that has not finished is held to the limit throughout on Android and Windows, as
those platforms' own fields hold it, and may run over until it is finished on macOS, Linux, iOS
and the web, so that a word composed in Chinese or Japanese is not cut under the writer's
fingers. On the web, where frus fields do compose, the shell now passes the browser's open
composition to the field, and when the browser reports the composition finished with the text
unchanged, the field is told so with an empty `Key::Text`, at which the limit applies.

### Ellipsis by cluster

`text::cluster_ends(text)` gives the byte offsets where a grapheme cluster ends. `truncate` and
`ellipsise` try those, from the end, instead of every code point, and the rich-text loop drops a
whole cluster per step. The ellipsis is appended at the logical end of the text, so in a
right-to-left line the bidirectional algorithm draws it at the left, after the last word read.

### Lines as the shaper made them

`buffer_line_starts(text, buffer)` reads each line's start off the shaped buffer itself: the
line's text, then its own line ending (`CrLf`, `Lf`, `Cr`) when the text continues with it, or
any single paragraph separator otherwise. `TextLayout::from_buffer`, `line_spans_resolved` and
`runs_cut_at` all use it, and the layout takes each line's text from the buffer. Rich text,
whose lines are split at the LF alone, keeps the CR at the end of its line; the layout leaves a
CR that ends a line before a separator out of the line's characters, so its caret does not stop
between the two either.

Rich text that is not plain ASCII is split by the bidirectional algorithm, which ended a
paragraph at the CR and another at the LF, so a CR LF drew an empty line between the two. The
fork's `BidiParagraphs` now takes a paragraph that is only the LF after a CR together with it.

The editing model's `line_start` and `line_end`, used without a layout, stop at any line break
character, so End goes before the whole of a CR LF. Backspace, Delete and the arrows already
took a CR LF whole, since it is one grapheme cluster.

A **single-line** field leaves the line breaks of what is typed or pasted out, a CR LF whole, and
the lines run on: a CR left behind would be a line break on screen of its own. A multi-line field
keeps them as they came. A value the application sets is its own and is not rewritten.

## How it was tested

- `frus-text`: `a_crlf_is_one_line_break` (two lines like LF, the first line ends before the CR,
  the arrows cross from 2 to 4, a press never lands at 3, `line_spans` gives `[0..2, 4..6]`),
  `a_lone_cr_is_a_line_break_too`, and `rich_text_breaks_once_at_a_crlf` (ASCII, Latin with an
  accent, Arabic; and `runs_cut_at` after two CR LFs). All three were run against the code
  before the change and failed: End stopped at 3, the lone CR panicked with "index out of
  bounds: the len is 1 but the index is 1", and rich text let the caret stand at 3. The rich
  text test also caught the bidirectional paragraph splitter giving a non-ASCII CR LF three
  lines, fixed in `BidiParagraphs` (marked `frus:` in the fork). Whole suite: 82 passed.
- `frus-widgets`: word deletion both ways over plain words, a decomposed `é`, Devanagari and
  Arabic, emoji and flags; a word deletion stopping at a line break and at a CR LF; over a
  selection; line deletion in a multi-line field with a CR LF; an obscured field; a read-only
  field; the length limit in clusters (family, flag, skin tone, `é`, Devanagari, Arabic; ten
  families in a limit of ten), a paste into the middle and over a selection, the composition
  rule both ways and the empty-text settle; CR LF crossed and deleted whole, End before it, on
  screen too; a single-line field leaving line breaks out; the ellipsis sweep over seven
  clustered texts at every 3 px (`an_ellipsis_never_cuts_a_cluster`) and an RTL line ending in
  its ellipsis on the left; undo of a deleted word in one step. Whole lib suite: 2601 passed
  (built at opt-level 0; `a_deep_tree_builds_on_a_platform_s_smallest_stack`, which needs the
  optimised build's frames, left out of that run as in milestone 842). Two existing limit tests
  changed on purpose: a keystroke into a full field is now `None` rather than the unchanged
  value.
- `frus-shell`: `deletion_key`'s table on both keyboards; Android's plan for KEYCODE_DEL and
  KEYCODE_FORWARD_DEL with Ctrl and Alt meta; the web's finished composition cut to the limit;
  and, through the test driver, `ctrl_backspace_deletes_a_word_and_ctrl_z_brings_it_back`
  (Ctrl+Backspace twice, two undos, Ctrl+Delete, Alt+Backspace) and
  `option_and_cmd_backspace_on_an_apple_keyboard`. Whole lib suite: 664 passed, 2 ignored.

The `frus-widgets` and `frus-shell` tests were written with the change and were not run
against the code before it (they use the new keys and functions); the `frus-text` ones were.

## Not done

- **Words are the word jumps' words.** A word is a run of letters, digits and `_`, so an emoji
  or a symbol such as `+` is a separator: Ctrl+Backspace after `say 👋` deletes `say 👋`, where
  word segmentation by UAX #29 deletes the emoji alone. Changing it would change Ctrl+Arrow and
  the double click too, and is left for a step of its own. A run of Chinese or Thai has no
  dictionary segmentation here either: it is one word.
- **Line deletion goes to the logical line**, not the row a wrapped line is drawn on: in a
  wrapped multi-line field Cmd+Backspace deletes back to the last line break.
- An obscured field's Backspace still deletes a whole grapheme cluster.
- The composition rule is unit-tested; no input method was driven in a browser, on a Mac or on
  Linux, and on the test machine (Windows) the field holds compositions to the limit.
- Not run on a phone (Android's hardware keyboard path is covered by the plan's unit test) or
  on a Mac.
- L-038 is marked `fixed in 0.7.0`, assuming the next release is 0.7.0.
- The rest of #56 — per-character versus per-run fallback, emoji rendering where no emoji face
  exists, bidi details beyond earlier milestones — is unchanged.
