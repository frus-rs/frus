# Milestone 787 — Material 3 expressive widgets, second batch

## Objective

Seven pieces of the Material 3 expressive set that frus did not have, each one builder with
themed defaults, every visual overridable per widget and per theme:

1. **Wavy progress indicators** — `LinearWavyProgressIndicator`, `CircularWavyProgressIndicator`
   (determinate and indeterminate) and `WavySliderTrackShape`, the wavy track a `Slider` can take.
2. **The Material 3 carousel** — `Carousel::multi_browse`, `::hero`, `::uncontained`,
   `::full_screen`: items that change size as they scroll, masked at the edges, snapping to an
   item, moved by the keyboard.
3. **Medium and large app bars** — `AppBar::medium()` / `::large()` and the collapsing
   `SliverAppBar::medium()` / `::large()`.
4. **The full modal date picker** — `DatePickerDialog` over `DatePickerModel`: calendar and
   text modes, a date range, the list of years, the locale's first day of the week, keyboard.
5. **The expandable navigation rail** — `ExpandableNavigationRail`, 96 wide collapsed, 220 wide
   expanded, a menu button and an action button.
6. **Glass** — `GlassSurface`, a frosted panel over what is behind it.
7. **Shapes, morphs and the loading indicator** — `RoundedPolygon`, `MaterialShapes`, `Morph`,
   `MorphingShape` and `LoadingIndicator`.

## Design

### Wavy progress (`wavy.rs`)

The covered part of the line is a sine wave sampled every 0.5 to 2 px and stroked with round
caps; the rest is a flat track a gap (plus both half caps) away, ending in a stop dot. Defaults:
a 10 px box, a 4 px stroke, a 3 px amplitude, a 40 px wavelength (20 px indeterminate), a 4 px
gap; the circular one is 48 px, 1.6 px amplitude, a 15 px wavelength rounded so that a whole
number of waves goes round. `wavy_amplitude(progress)` flattens the wave below 10 % and above
95 %, with a short smoothstep ramp instead of a jump. The wave flows at one wavelength a second
(`wave_speed`, 0 holds it). The indeterminate linear indicator reuses the two-line timing of
`LinearProgressIndicator` (now `pub(crate)`), the circular one the arc timing of
`CircularProgressIndicator`. Every one of them says `decorative()`, so a screen whose only
motion is a wave is drawn at the decorative rate of milestone 783. Theme: `WavyProgressTheme`
(`theme.widgets.wavy_progress`).

The slider track is a `SliderTrackShape`: the travelled part a wave, the rest a straight line.
It is still: a track shape is painted without the time.

### Carousel (`carouselm3.rs`)

The layout is pure arithmetic in `CarouselSizing::layout(offset, viewport, count, shrink)`:

- **Weighted** (multi-browse `[3, 2, 1]`, hero `[1, 7, 1]`): the view is shared out by
  weights; as the first item leaves, each item behind it grows toward the slot in front, so one
  stop later every item is one slot on. Hero "consumes the max weight": the first and the last
  item also rest in the large middle slot, with an empty slot beside them.
- **Fixed** (uncontained, full screen): items of one length; the leaving item shrinks down to
  the larger of the leftover at the far end and the shrink extent, then slides off; the last
  visible item takes what room is left.

Multi-browse ends with every slot filled (its last stop is `count − slots`), and an offset past
the end is held there.

The scrolling is a `PageView` of empty pages, one per stop, `viewport_fraction` = step / view,
with `pad_ends` so the offset runs from 0 to (stops − 1) × step. The items are drawn over it by
a `ScrollOverlay` from the offset of the frame being drawn, so nothing lags. Each item is a
clipped rounded container (radius 28, 4 px around it) whose content is laid out at the size of
the largest slot through an `OverflowBox`: a narrowing item **masks** its content, it does not
squeeze it. Taps go through an `InkWell`. Controlled with `index` + `on_index_changed`, the
carousel is wrapped in a focus stop whose arrow keys (mirrored in RTL; Up/Down when vertical),
Home and End ask for the next index. Theme: `CarouselTheme`.

The old arrow-button `CarouselView` stays as it was.

### Medium and large app bars (`appbar.rs`, `sliver.rs`)

`AppBarSize` (`Small`, `Medium`, `Large`). A medium bar is a 64 toolbar over a 48 band holding
the title in `headline_small`, bottom padding 20; a large one an 88 band, `headline_medium`,
bottom padding 28 (112 and 152 in all). `AppBar::medium()` moves the title out of the toolbar
into a band given as the bar's `bottom` (above any bottom the caller set). `SliverAppBar::medium()`
/ `::large()` are pinned by default and open to that height; while the bar collapses, the band
between the toolbar and the bar's current bottom shrinks and clips the big title, which stays
anchored to its bottom; once the bar is fully collapsed the toolbar's own title fades in over
500 ms on the emphasized-decelerate curve. Theme: `AppBarTheme::medium_title_style`,
`large_title_style`.

### Date picker (`datepickerdialog.rs`)

`DatePicker`, the month grid of earlier milestones, stays. The new dialog is controlled the way
every overlay here is: the application keeps `Option<DatePickerModel>` (`Some` is open), sends
each `DatePickerEvent` to `DatePickerModel::update`, and gets a `DatePickerOutcome` (`Picked`,
`PickedRange`, `Cancelled`) when it ends. The model is pure and carries the logic: the month
shown, the year list, picking (a range is start then end; an earlier day or a third tap starts
again), the keyboard cursor (arrows ±1/±7 days, Page Up/Down ±1 month, Home/End to the edges of
the reader's week, Enter/Space to pick), the bounds (days outside are disabled, the month and
cursor cannot leave them), and the text mode (a date typed in the locale's order, checked for
format and range).

`CalendarDate` is a plain Gregorian date with exact day arithmetic (days since 1970, leap years
by the full Gregorian rule), `add_months` that clamps to the month's end, and `weekday()`.

The panel is 360 wide: the heading (`label_large`), the headline (`headline_large`, the date as
the locale says it — "Mon, Aug 17", "lun. 17 août"), the mode switch, the month button opening
the year list, the arrows, the weekday initials rotated to the reader's first day, six rows of
40 px day circles (picked: `primary`; today: outlined; range: a `secondary_container` band),
Cancel / OK. `body(screen)` puts it in the existing modal `Dialog`. New localized strings
(`date_picker_title`, `date_input_pattern`, `short_months`, `medium_date`, …) with their French
translations. Theme: `DatePickerDialogTheme`.

### Expandable rail (`expandrail.rs`)

`ExpandableNavigationRail` is controlled (`selected`, `expanded`, `on_toggle`). Collapsed:
96 wide, each destination a 56×32 pill around the icon with the label (`label_medium`) under it.
Expanded: 220 wide, each destination a 56-tall pill with the icon and the label (`label_large`)
side by side. A menu button toggles it, and the action button (`fab`) shows its label when
open. The width change is the container's own animated size (350 ms, emphasized
decelerate); without a given height the rail reads its parent's through a `LayoutBuilder`. Each
destination reads as a tab, selected or not. Theme: `ExpandableRailTheme`. The existing
`NavigationRail` (with its `extended` form) is unchanged.

### Glass (`glass.rs`)

The renderer already had a GPU backdrop filter (`BackdropFilter`, milestone 700s): it copies the
frame drawn so far, blurs it in two passes, and draws it back inside the layer's clip. So
`GlassSurface` is a composition: a `ClipRRect` (the blur's region), a `BackdropFilter::blur`,
and a container with the tint, the rim and the content. Defaults: σ = 24, radius 28, a tint of
`surface` at 55 % (45 % in dark), a 1 px white rim at 45 % (14 % in dark). The tint, like every
translucent colour in frus, blends in linear light; the golden samples the rendered pixel.
`grouped()` shares one blur between panels that do not overlap; `enabled(false)` drops the blur
and its cost. Theme: `GlassTheme`.

### Shapes and the loading indicator (`shapes.rs`, `loadingindicator.rs`)

`RoundedPolygon::from_vertices` rounds each corner with a circular arc of the asked radius,
tangent to both edges, approximated by one cubic; a radius too large for the edges is reduced so
the cut never passes the middle of either edge — which is what makes a "cookie" a star whose
corners are rounded as far as they go. `regular` and `star` build on it. `MaterialShapes` gives
the thirty shapes of the Material 3 set (circle, square, slanted, arch, semicircle, oval, pill,
triangle, arrow, diamond, clam shell, pentagon, gem, sunny, very sunny, cookies 4/6/7/9/12,
clovers 4/8, burst, soft burst, boom, soft boom, flower, puffy, puffy diamond, heart), each
normalised for the morph.

`Morph` normalises both outlines to a 2×2 box, resamples each at 180 points evenly spaced by
arc length, and turns the second so that the sum of squared distances to the first is least;
in between, each point moves in a straight line. `MorphingShape` paints one.

`LoadingIndicator` morphs through soft burst, cookie 9, pentagon, pill, sunny, cookie 4, oval:
each morph 650 ms on a spring (damping 0.6, stiffness 200, ≈ 9 % overshoot), plus a quarter
turn per morph over a steady spin of 4.666 s a turn. 48 px box, 38 px shape, `primary`, or
`on_primary_container` on a `primary_container` circle when `contained`. It is decorative.
`determinate(v)` morphs the first shape into the second as `v` goes 0→1.

## How it was tested

- Unit tests (frus-widgets): wavy amplitude ramp, wave geometry, gap and stop dot, flowing
  phase, circular wave closure, theme and caller overrides, semantics; carousel layouts at rest,
  half-way, with shrink, at the end, hero, uncontained, full screen, snapping index; date
  arithmetic (round trips, weekdays, leap years incl. 1900/2000/2100, month-end clamping),
  locale first day (Monday-first grid, Home/End), range picking and band, bounds, keyboard,
  year list, typed dates (format, range, locale order), localized headline; rail widths and
  indicator; glass defaults and the backdrop layer; corner tangency, oversized rounding, morph
  endpoints, half-way interpolation, best rotation, every shape closed and clockwise; the
  loading spring, phase and paint.
- Goldens (frus-test `tests/m3_expressive.rs`): wavy progress, the three carousels, medium and
  large bars, a large sliver bar open and collapsed, the date picker (single and range), the rail
  collapsed and expanded, glass over stripes (with pixel assertions that the panel blurred and
  tinted what was under it and left the rest alone), the shapes and loading indicator.
- A real Windows window: `cargo run -p frus-demo --example m3_expressive`, one page per widget
  (`FRUS_M3_PAGE`, `FRUS_M3_PICKER`, `FRUS_M3_RAIL` open a page, the picker or the rail
  directly), each page captured from the screen: the waves flowing, the three carousels, the
  medium and large bars and a large sliver bar collapsed with its toolbar title shown, the
  single and range pickers, the rail collapsed and expanded, glass over stripes, the thirty
  shapes and the loading indicators.

## Not done

- **The full-screen range picker** (a vertical list of months with a Save button) is not
  there: a range is picked in the same dialog as a date, one month at a time, or typed in two
  fields.
- **The year list does not open scrolled to the shown year**: it opens at its top. The scroll
  view has no initial offset, and a `ScrollTo` from inside the picker was not wired.
- **The modal form of the expandable rail** (expanding over the content with a scrim) is not
  there; the rail pushes the content aside as it widens.
- **The wavy slider track is still**: a slider track is painted without the time, so its wave
  does not flow. The amplitude of a wavy indicator follows the value (a ramp near 10 % and
  95 %) rather than animating over time when it crosses them.
- **The shapes are drawn to the published Material 3 shapes, not ported from their exact
  definitions**: no source for them was available here, so each was built from its
  description (points, inner radius, rounding) and checked by eye. The morph resamples the
  outlines rather than matching their features (corners to corners), so a morph between two
  very different shapes can show a brief twist the feature-matched morph would not.
- **A plain `AppBar::medium()` / `::large()` does not collapse**; the collapsing one is the
  sliver. Medium and large bars ignore `center_title` for the big title (it is always at the
  start edge, as the specification has it).
- **The carousel** has no infinite mode, and its keyboard works only when it is controlled
  (`index` + `on_index_changed`). Items cannot be focused one by one.
- **Not driven by input in the real window**: the screenshots show each widget drawn in a
  live window, but no drag, fling, click or key was sent to it (the desktop was in use); the
  scrolling, snapping, keyboard and picking are covered by the unit tests and the model
  only.
- **Not checked on Android, iOS or the Web**; only Windows and the goldens.
