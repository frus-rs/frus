# Milestone 852 — The primary modifier, and a family per span

Two limitations, both of the kind an application meets on its first day on a second platform:
L-037 (a shortcut could not say "Cmd on a Mac, Ctrl elsewhere") and L-031 (a span of rich text
could not name its own font family).

## 1. The primary modifier (L-037)

### What was wrong

Milestone 736 made the framework's own editing keys follow the keyboard — Cmd+C on a Mac, Ctrl+C
elsewhere — inside the shell, with `primary_held(apple, ctrl, meta)`. An application's
`KeyStroke` had no such word: it named `ctrl` or `meta`, so a Save meant for Cmd+S on a Mac and
Ctrl+S on Windows was bound twice, and a page read in a browser on a Mac answered Ctrl+S, which
no Mac user presses. The editing keys themselves were `if` statements in the shell: an
application could not move Copy to another key, nor take Select all away from a field.

### What was done

- **`KeyStroke::primary()`**, a `primary` field: the platform's own modifier. A bound stroke says
  it; a pressed stroke never does, it names the keys held. `KeyStroke::resolve(keyboard)` spells
  it out — `meta` on macOS and iOS, `ctrl` on every other platform — and
  `KeyStroke::matches_on(bound, keyboard)` compares a pressed stroke with a bound one after
  resolving both. `matches` keeps its signature and reads the machine's keyboard.
- **`keyboard_platform()`** (frus-core, re-exported): which keyboard the application is typed
  on. The machine's, read as `default_target_platform` reads it — the browser's operating system
  on the web, the build's elsewhere — so a page read on a Mac answers Cmd. Unlike
  `default_target_platform`, the debug override does not change it: milestone 736's reasoning
  holds, a window standing in for an iPhone on a Windows machine is still typed on a Windows
  keyboard. The shell's `apple_keys` now comes from it, so the framework's keys and an
  application's primary bindings can never disagree about the keyboard. A test or an application
  that wants another keyboard's conventions passes a `TargetPlatform` to `matches_on`,
  `resolve`, `Ui::resolve_keystroke` or `default_editing_shortcuts`.
- **The editing keys are a table.** `default_editing_shortcuts(keyboard)` lists them as strokes
  and intents: primary+C, X, V, A, Z to `Intent::COPY`, `CUT`, `PASTE`, `SELECT_ALL`, `UNDO`;
  primary+Shift+Z to `REDO`, and Ctrl+Y to `REDO` as well off Apple keyboards (a browser on a Mac
  opens its history on Cmd+Y). The shell resolves a key against that table instead of its own
  `if`s.
- **Overridable.** `Ui::resolve_keystroke(stroke, focused, keyboard)` returns the messages and
  the intent the stroke was bound to, answered or not. When an application's `Shortcuts` binds a
  stroke to one of the editing intents and no `Actions` answers it, the shell performs it on the
  focused field or the selection area: another key can be Copy. `Intent::DO_NOTHING` takes the
  key: bound to primary+A, Select all is switched off under that scope. A stroke no binding took
  gets the default table's meaning. Bindings still come before the defaults, as before.
- **`KeyStroke::label_on(keyboard)`** writes a stroke as that keyboard has it, for a menu or a
  command palette: `⇧⌘Z` on an Apple keyboard, `Ctrl+Shift+Z` elsewhere.
- The command palette's documented key, the demo's palette and the guide use
  `KeyStroke::new(ShortcutKey::Char('k')).primary()`. DevTools' key names accept `primary+…`, and
  the test driver presses a primary stroke as its keyboard's modifier.

**One change of behaviour.** The default editing keys are matched exactly, as a stroke is: Ctrl+C
copies, and Ctrl+Shift+C or Ctrl+Alt+C no longer do (before, any stroke holding the primary
modifier with C copied). This is also why AltGr+C — Ctrl+Alt on Windows — can no longer copy
when it was meant to type a character.

### Tests

- `the_primary_modifier_is_cmd_on_apple_platforms_and_ctrl_elsewhere`: for each of the six
  `TargetPlatform`s, the resolution, `matches_on` with Cmd+S and Ctrl+S, a bare S, an extra
  modifier.
- `the_default_editing_shortcuts_follow_the_keyboard`: for each platform, the six intents on
  the platform's modifier, none on the other modifier or bare, Ctrl+Y only off Apple.
- `a_primary_binding_resolves_for_the_keyboard_it_is_typed_on`: a frame's interface resolves one
  primary binding as Cmd on macOS and Ctrl on Windows, and names an unanswered `COPY`.
- `a_stroke_is_labelled_as_its_keyboard_writes_it`.
- Through the shell's driver (`platform_shortcut_tests.rs`), on both keyboards: a primary Save is
  Ctrl+S and not the logo key off Apple, Cmd+S and not Ctrl+S on Apple;
  `an_application_rebinds_and_switches_off_editing_keys` (primary+U bound to `UNDO` undoes,
  primary+Z and primary+Shift+Z still work, primary+A bound to `DO_NOTHING` selects nothing);
  `ctrl_y_redoes_off_an_apple_keyboard_only`.
- Milestone 736's clipboard tests now go through the default table, and its driver tests
  (Cmd+C and Cmd+A on an Apple keyboard) pass unchanged.

## 2. A family per span (L-031)

### What was wrong

`TextStyle` had a `family`, and a plain `Text` honoured it, but `TextRun` — what a rich text is
measured and drawn from — had none: `RichText` dropped the family its spans resolved, and every
run was shaped with `family_for(text)`, the default family. There was no `TextSpan::family`, and
`TextSpan::measure_hash` ignored the family, so the cache could not have told two faces apart
anyway. Nothing anywhere could name families for the characters a face lacks.

### What was done

- **`TextSpan::family(FontFamily)`**, and `TextRun::family`: the run's family reaches the
  measurement (`measure_runs`, `runs_cut_at`, `TextLayout::of_runs_in`) and the renderer
  (frus-gpu, which draws every target, the web included), through one function.
- **A fallback list**: `TextStyle::family_fallback(&'static [FontFamily])`, on `TextSpan`, `Text`,
  `ResolvedTextStyle`, `TextRun` and the text primitive. It is inherited and merged like every
  other field, and swaps at the halfway point in a `lerp`, as the family does.
- **frus-cosmic-text** got `Attrs::fallback_families`: the shaper's fallback iterator already took
  a list of default families and tried them glyph by glyph; it was given only the one. A span's
  list now follows its family, before the font system's script and common fallbacks, and a run is
  shaped with one list (`Attrs::compatible`). The fork's README lists the change.
- **`frus_text::families_for_style` and `with_families`** turn a style's family and list into
  the shaper's, the one place measurement and paint both call. Milestone 626's rules hold:
  every family is resolved to what is loaded (a near miss forgiven), a family that is not loaded
  is **skipped** — so when the family itself is missing, the first loaded one of the list takes
  its place and the text is never blank — and a run containing Arabic keeps the Arabic face
  first. The application's default family closes the chain, so a character none of them has is
  looked for in a face frus knows is loaded before the platform's lists, which are empty on
  Android. With no list, nothing changes: the shaper gets the family alone, as before.
- The measurement caches know the family and the list: `MeasureKey`, `TextSpan::measure_hash`,
  `RichText`'s and `Text`'s measure keys.

### Tests

- `a_runs_family_reaches_the_measurement`: "illicit lilies" in a monospaced run is measured more
  than 1.3 times as wide as in the default family. Before, both were the same width.
- `missing_characters_are_drawn_in_the_fallback_families`: Latin letters asked of the bundled
  Arabic face, with the monospaced family as its fallback, are shaped in DejaVu Sans Mono, glyph
  by glyph.
- `the_fallback_chain_skips_what_is_not_loaded`: no list changes nothing; a missing family and a
  missing first fallback give the first loaded member, then the default family; the default is
  not repeated; one chain is one `'static` copy.
- `TextSpan::family`'s doctest: the span's run carries the family and its neighbours do not.
- A golden, `rich_text_span_family`, draws a paragraph with a monospaced span and a span whose
  family is not loaded and falls back.

## Not done

- **Not tried on a Mac**, nor in a browser on one: no Mac was available. The Apple keyboard is
  exercised through the driver's `set_apple_keyboard` and through `TargetPlatform::MacOs` in the
  widget tests.
- **No phone or browser run** for the families: the renderer path is the one every target uses
  and is covered by the golden on the desktop renderer only.
- The fallback list is **static** (`&'static [FontFamily]`), as `FontFamily::Named` holds a
  `&'static str`: a list built at run time has to be leaked by the application.
- The editing intents cover the six clipboard and history keys. The caret's movements (word
  jumps, Cmd+arrow, Home/End) and word deletion still follow the keyboard inside the shell and
  are not yet in the table, so they cannot be rebound.
- The web's `<input>` handles Cmd+C, X and V itself in a focused field (milestone 711): a binding
  of those exact keys to another intent does not reach the browser's own handling there.
- `CommandPalette`'s and `MenuItem`'s shortcut labels are still free text; `KeyStroke::label_on`
  is there for an application to fill them.
