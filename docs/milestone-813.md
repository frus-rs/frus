# Milestone 813 — The framework's words in five more languages

## Objective

frus-widgets says a fair amount on an application's behalf: a back arrow's label, a date
picker's titles and month names, a table's pagination line, the selection bar's Cut, Copy,
Paste and Select all. Since milestone 449 those words came from a `Localizations` table, but the
framework shipped only two of them, English and French, and an application had to pick and
install one itself.

This milestone adds **German, Spanish, Japanese, Thai and Vietnamese**, makes the framework
install the right table on its own, and finishes moving the words that were still English
constants into the tables.

## What changed

### Five tables, and the parts that are not words

`German`, `Spanish`, `Japanese`, `Thai` and `Vietnamese` implement every entry of the trait.
The wording follows the platforms' own system strings — Android's, and the established
cross-platform toolkits' published translations — entry by entry. Where those sources disagree with the language's own date
conventions they were not copied blindly (see *Decisions*).

Each table also carries what a translation of the strings alone would leave behind:

| | first day | date typed | month and year | grouping | clock |
| --- | --- | --- | --- | --- | --- |
| en | Sunday | `08/17/2026` | August 2026 | `4,000` | 12 h |
| fr | Monday | `17/08/2026` | août 2026 | `4 000` | 24 h |
| de | Monday | `17.08.2026` | August 2026 | `4.000` | 24 h |
| es | Monday | `17/08/2026` | agosto de 2026 | `4000`, `40.000` | 24 h |
| ja | Sunday | `2026/08/17` | 2026年8月 | `4,000` | 24 h |
| th | Sunday | `17/08/2026` | สิงหาคม 2026 | `4,000` | 24 h |
| vi | Monday | `17/08/2026` | tháng 8 năm 2026 | `4.000` | 24 h |

### New entries on the trait

- `cut_button_label`, `copy_button_label`, `paste_button_label`, `select_all_button_label`:
  the selection bar's words were constants on `EditAction`. `EditAction::label` now returns a
  `String` and `ToolbarItem::label` a `Cow<str>`, built in the language in force. The two doc
  examples that filtered the bar by comparing words now compare `edit_action()`, which does not
  change with the language.
- `more_button_label` (an app bar's overflow dots said "More"), `remove_button_label` (a
  chip's cross said "Remove"), `licenses_label` and `license_count_label` (the licence page's
  button and its "1 licence" / "3 licences"; its Back and Close now come from the table too).
- `date_input_format`: the order a typed date is read and written in, always with the letters
  `dd`, `mm`, `yyyy` and the language's separator. `date_input_pattern` stays the hint shown to
  the reader. They had to be split: the date picker read the hint's letters to know the order,
  mapping `j` to the day — which is French's *jour* and German's *Jahr*. A German hint
  `tt.mm.jjjj` would have been read as day, month, day.
- `month_year`, `short_month_day`, `long_date`: a calendar's heading, a range's ends and the
  date-time pickers' summary were assembled as "month name, space, year" in four places, which
  is wrong in Spanish ("agosto de 2026"), Japanese ("2026年8月") and Vietnamese. The range
  headline guessed the order of day and month from whether the hint started with `m`.
- `uses_24_hour_clock`: whether the language's clock has twelve hours or twenty-four.

`parse_date` and `format_date` now split patterns on `/`, `.` and `-` and write the pattern's
own separator, so `dd.mm.yyyy` round-trips as `17.08.2026`.

The time picker's twelve-hour suffix came from a constant `"AM"` / `"PM"`; it now uses the
table's `ante_meridiem_label` / `post_meridiem_label`, which the AM/PM toggle already did.

### The shell installs the framework's table

`localizations::for_locale(&Locale)` picks a table by language — `es-MX` and `es-419` get
Spanish, a language not in `localizations::LANGUAGES` gets English — and keeps one per thread,
so asking every frame costs a reference count.

`install_ambient` used to install a table only when `Application::localizations` returned one;
now it installs `for_locale(resolved)` otherwise. An application that lists `de` in
`supported_locales` gets German calendars, dialogs and selection bars without writing a table,
and the resolution is the one milestone 454 already runs (`es-MX` against `[en, es]` gives
`es`; anything unmatched gives the application's first, normally `en`).

This replaces one assertion of `a_shell_installs_the_application_s_words`: "an application that
says nothing leaves whatever is in force alone". Nothing outside the shell installs a table, so
the only thing it protected was a table lingering after the application stopped giving it — a
German table staying in force after the reader switched the application to English. Saying
nothing now means "the framework's words for the resolved language".

## Decisions

- **Thai uses the Gregorian year.** Thai calendars often count from the Buddhist era (2026 is
  2569), but the platform date symbols this follows use the Gregorian year for `th`, so the
  table does too. An application that wants the Buddhist year overrides `month_year` and
  `long_date`.
- **Day-first hints for Thai and Vietnamese.** The source strings give both a month-first hint
  (`ดด/วว/ปปปป`, `mm/dd/yyyy`) while their own date parsing reads `d/M/y` for those languages:
  a hint that contradicts the field. Here hint and parsing agree, day first.
- **Spanish groups from five digits** (`4000`, `40.000`), as CLDR's minimum grouping says and
  Android shows; Spain's wording (*Introduce una fecha*) for every Spanish locale.
- **Spanish "OK" is "Aceptar"**, not the upper-case "ACEPTAR" one source still has.
- **Short "Clear"**: frus's English says "Clear" on a field's cross, not "Clear text"; the new
  tables follow it ("Löschen", "Borrar", "消去", "ล้าง", "Xoá"), as French did.
- **Selected-row counts agree with *fila*** in Spanish ("3 seleccionadas"), as French agrees
  with *ligne*.
- **No new font.** DejaVu Sans (`bundled-sans`) has every letter of the German, Spanish and
  Vietnamese tables (checked character by character against its cmap). Japanese and Thai have
  none of theirs in it. Desktop and Android draw them with the system's fonts (milestone 784);
  the web cannot reach system fonts, so an application shown in those languages in a browser
  registers a face with `frus::fonts::add_font`. Bundling a CJK face would be megabytes for
  every application; that is documented in the module and in the style guide instead.
- **No cargo feature for the tables.** They are a few kilobytes of strings and small functions
  (see *Size*); a feature would add a configuration where a missing table falls back to English
  for no measurable saving.

## How it was tested

- `every_table_answers_every_entry_the_trait_declares` reads the source and checks every table
  writes out every entry of the trait — a new entry fails until all six non-English tables say
  it — and that every shipped table is in `LANGUAGES`.
- `every_table_says_its_own_words` compares every string entry (and the formatted ones) of
  each table with English, with a short per-language list of words that are rightly the same
  ("OK", "AM", German's "August").
- `a_table_is_picked_by_its_language_and_english_answers_the_rest`: `es-MX`, `es-419`, `de-AT`,
  `fr-CA`, `ja-JP`, `th`, `vi-VN` get their tables; `zh-Hant-TW` and `en-GB` English; one table
  per thread.
- `each_language_gets_its_calendar_clock_and_numbers_right`: first days, clocks, typed dates
  in each order (`07.08.2026` parsed back day first), written dates, grouping, agreement.
- `a_calendar_heading_is_written_the_language_s_way`: a `DatePicker` built under the Japanese,
  Spanish and German tables draws `2026年1月`, `enero de 2026`, `Januar 2026`.
- `the_built_in_words_follow_the_language_in_force` (selection bar).
- `the_framework_s_languages_resolve_by_language_and_fall_back_to_english` (locale resolution).
- `a_shell_installs_the_framework_s_own_words_for_the_resolved_language` drives the shell's
  `install_ambient`: `es-MX` → Spanish, `ja-JP` → Japanese, an application language frus has no
  words for → English.
- The style guide's example is a doctest.

## Size

RESULTS_SIZE

## Not done

- **The time pickers do not follow the clock on their own.** They stay twenty-four hour unless
  `hour12()` is asked for, as before; `uses_24_hour_clock` lets an application decide. Making it
  the default would change every English application's picker.
- **No regional variants**: one Spanish table (Spain's wording) for `es-MX` and `es-419`, one
  German for `de-AT` and `de-CH`.
- **No Thai digits, no Buddhist era**, deliberately (see *Decisions*).
- **Not seen on a device or in a browser.** The words were checked through the scene and the
  accessibility tree in tests; Japanese and Thai rendering through the system fonts is
  milestone 784's path and was not re-checked on a phone here.
- Words still spoken in English that no table covers yet: anything an application passes in
  itself, and developer-facing messages (the licence page's "No licences are registered").
