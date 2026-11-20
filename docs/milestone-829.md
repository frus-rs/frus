# Milestone 829 — The system's text services in a text field

## Objective

Issue #214: a frus text field got none of the services the system gives its own fields. A field
that draws its own text has to ask for each of them:

- **spell checking**: the line under a misspelled word, and the corrections a tap on it offers;
- **autocorrect and suggestions as switches**: a username field could not turn autocorrect
  off, and no field could say "no suggestion strip";
- **the system's items in the selection menu**: on Android, `ACTION_PROCESS_TEXT` lets other
  applications (a translator, a search, a dictionary) act on selected text.

The issue's "done when", on Android: a misspelled word in the demo's text field is underlined,
tapping it offers the system's suggestions, a field can turn autocorrect off, and the selection
menu lists the installed "process text" applications. See "How it was tested".

## The keyboard switches

`TextField::autocorrect(bool)` and `TextField::enable_suggestions(bool)` (and the same on `Ime`)
reach the keyboard as Android's `InputType` flags, worked out in `ime.rs` where they are tested:

- autocorrect is `TYPE_TEXT_FLAG_AUTO_CORRECT`. Untold, prose (`Text`, `Multiline`) asks for it
  and nothing else does. This is a **change**: a keyboard reads the flag's absence on a
  single-line field as "do not correct", so single-line frus fields were never corrected before;
- suggestions off is `TYPE_TEXT_FLAG_NO_SUGGESTIONS`, plus the *visible password* variation on
  plain text, because many keyboards only honour the variation. A field with a variation of its
  own (email, URL) keeps it, or it would lose its `@` key;
- a secret is never corrected and never suggests, whatever it asked.

On the web the same answer drives the hidden input's `autocorrect` and `spellcheck` attributes.

## Spell checking

`TextField::spell_check(true)`, off by default. The pieces:

- **The widget** (`frus-widgets/src/textservices.rs`) holds the checker's last answer for the
  focused field (`SpellCheckResults`: the text checked, and the misspelled words in characters
  with their suggestions). The field paints a line under each word, while it has focus and not
  under the word being composed. An answer about an older text is moved onto the new one
  (`spans_for`): words before and after the edit are kept and shifted, a word the edit touched
  is dropped, so typing does not make the lines flicker while the next answer is on its way.
- **The style** is overridable at three levels, like the rest of the field:
  `TextFieldStyle`/`TextFieldTheme` gain `misspelled_color` (default `#F44336`, the platform's
  red), `misspelled_wavy` (default a wave) and `misspelled_thickness` (default 1), with
  `TextField::misspelled_color` and `misspelled_wavy` builders.
- **The shell** (`frus-shell/src/textservices.rs`, tested off a device) keeps one request in
  flight, sends a new one whenever the text differs from what was last checked, drops stale
  answers and answers for a field that lost focus, and stops asking for good once the platform
  says it has no checker.
- **Android** (`FrusTextServices.java`, `android_text_services.rs`) opens one
  `SpellCheckerSession` (the person's spell checker language) on the first request, calls
  `getSentenceSuggestions`, and reports each word whose attributes say "looks like a typo" and
  not "in the dictionary", in UTF-16 offsets converted to characters on the Rust side.
- **A tap** that puts the caret in or against a misspelled word opens the spell checker's bar
  over it: up to three suggestions (`MAX_SPELL_SUGGESTIONS`), then *Delete*, as a list of rows.
  A suggestion replaces the word (through the message typing sends, so undo and filters see it);
  Delete removes it and one space beside it. `TextField::spell_check_toolbar` changes the list,
  as `selection_toolbar` does for the selection bar.

## Process text

The installed actions are asked for once, the first time a selection bar opens
(`queryIntentActivities(ACTION_PROCESS_TEXT, text/plain)`, the application's own activities and
unexported ones left out). A field's default bar then carries one `ToolbarItem::process_text`
per action whenever something is selected, never for a masked field. They are many on a real
phone, so they go behind a **More** button (three dots) at the end of the pill, as on the
platform's own bar; More shows them as a list in the bar's place.

Choosing one starts the activity with `EXTRA_PROCESS_TEXT`. A read-only field's text goes with
`EXTRA_PROCESS_TEXT_READONLY` and nothing comes back. Otherwise the answer replaces the
selection. A `NativeActivity` cannot override `onActivityResult`, so the request is started from
a fragment with no view, added for the request and removed once answered. Fragments saved with
the activity are brought back with the application's class loader, which cannot see the bundled
dex; so `android:fragments` is taken out of the saved state (frus adds no other fragment). An
answer that arrives after the activity was recreated is dropped.

From Android 11 an application sees another's activities only if its manifest says what it looks
for (a `<queries>` entry for `PROCESS_TEXT`); see "Not done".

## New public items

`frus_widgets`: `TextField::{autocorrect, enable_suggestions, spell_check, spell_check_toolbar,
misspelled_color, misspelled_wavy}`, `Ime::{autocorrect, suggestions, spell_check, corrects,
suggests}` and the three `Ime` fields, `EditAction::{More, ProcessText, ReplaceMisspelled,
DeleteMisspelled}`, `ToolbarItem::{process_text, suggestion}`, `SpellCheckResults`,
`SpellCheckSpan`, `set_spell_check_results`, `spell_check_results`, `misspelled_words`,
`misspelled_word_at`, `set_process_text_actions`, `process_text_actions`, `MISSPELLED_COLOR`,
`MAX_SPELL_SUGGESTIONS`, `Localizations::delete_button_label`, the three `misspelled_*` style
and theme fields, and the `Widget::{selection_toolbar_more, spell_check_toolbar}` hooks
(forwarded by `Box`, `Keyed`-style wrappers, `Responsive` and `OtpInput`). `ToolbarMark` gains
`more` and `spell`.

## Cost

Nothing runs until it is used: the session opens with the first spell check request, the
actions are queried when the first selection bar opens, and a field that did not ask for spell
checking reads nothing. The bundled dex grows from 64,600 to 71,228 bytes.

## How it was tested

- **Unit tests, `frus-widgets`** (`textservices.rs`, `textinput.rs` `text_service_tests`,
  `ime.rs`): the switches' `InputType` bits (prose asks for autocorrect, a username field turns
  it and suggestions off, a secret never gets either, an email field keeps its variation); the
  web attributes follow the switch; misspelled words are underlined only in a field that asked,
  only while focused, never in a secret, never under the composing word, and in the theme's or
  the caller's colour and form; an answer about an older text is moved onto the new one; the
  spell bar is three suggestions then Delete, one bar per word, and the application's builder
  wins; the process-text items sit behind More, never for a secret or without a selection.
- **Unit tests, `frus-shell`** (`textservices.rs`): one request in flight, a new one only for a
  changed text, a stale answer or one for a field that lost focus dropped, and **a platform with
  no spell checker** (the Java side answers with no words) asked once and never again, with
  nothing shown; UTF-16 offsets to characters past surrogate pairs; a correction and a deletion.
- **Driven through the shell** (`app.rs` `spell_check_tests`, the real tap path): a tap in a
  correctly spelled word opens nothing; a tap in the misspelled one opens the bar with its
  suggestions and Delete; choosing a suggestion replaces the word through the field's message;
  Delete takes the word and one space.
- The routine check: `cargo fmt`, clippy on the host and on wasm32, the tests of
  `frus-widgets`, `frus-shell` and `frus`, the docs with warnings denied, Android clippy of
  `frus-shell` (its one finding is an existing `thread_local` lint in `video/controller.rs`, not
  this change), and the eight `frus-examples` projects plus the gallery's wasm build. The iOS
  `cargo check` could not run on this Windows machine (`ring`'s build script needs Apple's
  tools); this change adds no `cfg` an iOS build would take differently.
- The bundled dex was rebuilt with `scripts/build-input-dex.sh`, and a release APK of the demo
  built with cargo-apk (WSL).
- **On the phone** (Huawei STK-L21, Android 10, SwiftKey; release APK of the demo, `/fields`):
  - **autocorrect switch**: `dumpsys input_method` showed `inputType=0xc001` for *Notes*
    (text, sentence caps, `AUTO_CORRECT`) and `inputType=0x84091` for *Username* (text,
    `NO_SUGGESTIONS`, visible-password variation): the username field turned both off;
  - **process text**: a hold on *quick* selected it and the bar read *Cut, Copy, Select all, ⋮*;
    ⋮ opened the installed actions as a list (*Ask Gemini, Read aloud, Search, Petal Search,
    Petal Translate, Ask Bing, Ask Perplexity*). Choosing *Petal Translate* opened Huawei's
    search with *quick* in its box; the demo kept running (same process, no exception);
  - **no spell checker**: this phone has **no spell checker service** (`dumpsys textservices`
    lists none, and the system logged "no available spell checker services found"). frus logged
    "spell check: the platform offers no spell checker" once and drew nothing, as the shell's
    unit test says it must. So the underline and the suggestions were **not** seen on a device.

## Not done

- **The spell checker's underline and suggestions are owed on a device**: the test phone has no
  spell checker service, and nothing was installed on it. They are proven by unit tests and by
  the shell driven through its own tap path only. Until a phone with a checker (Gboard, the AOSP
  keyboard) shows them, #214 stays open.
- **A process-text answer written back** was not seen on the phone: the action tried opened a
  search, which returns nothing.
- **The `<queries>` manifest entry** an Android 11+ device needs to see the process-text
  activities is not added. cargo-apk's manifest schema refused the form tried
  (`[[package.metadata.android.queries.intent]]`), and adding it in `frus build` would make
  every package be made twice. On Android 11 and later, until an application declares it, the
  menu shows no process-text items (nothing breaks).
- **Spell checking keeps only the focused field's results**; an unfocused field shows no
  underline. The checker is the person's language, not the field's locale.
- **A tap selects nothing**: the suggestions bar opens at the caret; the word is not highlighted.
- **iOS and macOS** (`UITextChecker`, `NSSpellChecker`) and the desktop's look-up, share and
  translate are not wired. **The web** only gets `autocorrect`/`spellcheck` following the
  switch; the browser's own spell check runs on the hidden input, where nothing shows it.
- **SelectionArea and selectable `Text`** get no process-text items, only text fields.
- An answer from a process-text action replaces the range that was selected when it was
  started, even if the text changed meanwhile.
- The goldens were not re-run: nothing changes in a frame unless spell checking is asked for or
  process-text actions exist, which no golden sets up.
