# Milestone 778 — Motion tokens, status colours, theme extensions, animation speed and reduced motion

## Objective

Four gaps in the theme and the animation layer, each one something every real application
ends up working around:

1. **Motion tokens.** Every animated widget carried its own duration and curve. There was no
   place where an application could say "our motion is a little slower" once.
2. **Status colours.** The colour scheme stopped at `error`. `Alert` hard-coded a green, an
   amber and a red; `SnackBar` hard-coded a green for its success stripe.
3. **Typed theme extensions.** An application or a design system with tokens of its own (a
   brand gradient, a card corner, a spacing scale) had nowhere to put them that follows a
   subtree's theme and fades between light and dark.
4. **Animation speed and reduced motion.** There was no way to slow every animation down to
   look at it, and the system's *reduce motion* setting was read on Android only.

## What changed

### `theme.motion`: `MotionTheme`

`Theme::motion` is an `Arc<MotionTheme>` (one pointer: a theme is 11.5 KB and copied in
recursive walks, so the tokens are shared, not copied). `MotionTheme::M3`, the default, holds:

- the duration scale, in seconds like every duration here: `short1..4` (50 to 200 ms),
  `medium1..4` (250 to 400 ms), `long1..4` (450 to 600 ms), `extra_long1..4` (700 ms to 1 s);
- the easings: `standard`, `standard_decelerate`, `standard_accelerate`, `emphasized`,
  `emphasized_decelerate`, `emphasized_accelerate`, and `legacy` (the earlier "fast out, slow
  in" curve that many established components still use);
- three springs: `spring_fast` (damping ratio 0.9, stiffness 1400), `spring_default` (0.9,
  700) and `spring_slow` (0.9, 300).

`Theme::with_motion(MotionTheme { .. })` replaces them. During a theme fade the tokens switch at
the half, like the platform: they are timings, not looks.

Widgets moved to the tokens: **`BottomNavigationBar`**, whose 200 ms fast-out-slow-in
selection change is exactly `short4` along `legacy`. The others are listed under "Not done".

### Status colours: `success`, `warning`, `info`

`ColorScheme` gains twelve roles, built the way `error` is: `success`, `on_success`,
`success_container`, `on_success_container`, and the same for `warning` and `info`.

- **From a seed** (`ColorScheme::from_seed`): each family is a tonal palette of its own hue (a
  green, an amber, a blue, with the source colours' chroma clamped to 36..60), **harmonised**
  toward the seed: turned by half the angle between the two hues, at most 15 degrees, so a
  green next to a purple seed leans a little purple and stays a green. The tones are the
  error family's: 40 / 100 / 90 / 10 in light, 80 / 20 / 30 / 90 in dark, which keeps every
  `X` / `on_X` pair at AA contrast (a test checks it for three seeds).
- **The hand-written `light()` and `dark()`** carry the same computation's result for their
  own primary's hue, written out as literals so building a theme does not solve twelve HCT
  colours; a test holds the literals to the computation.
- `lerp` interpolates them with the rest.

Widgets moved to them:

- **`Alert`**: info, success, warning and error now take `info`, `success`, `warning` and
  `error` from the scheme, instead of `theme.primary` and three hard-coded colours.
- **`SnackBar`**: a new `warning()` kind (`SnackBarKind::Warning`) takes `warning`; `success()`
  takes `success` instead of a hard-coded green (the theme's `snack_bar.success_color` still
  outranks it). The info kind keeps `inverse_primary`: it is the default kind of every snack
  bar, and its stripe stands on the inverted surface, which is what that role is for.

### Typed theme extensions

```rust
#[derive(Clone, Debug, PartialEq)]
struct Brand { highlight: Color, card_radius: f32 }
impl ThemeExtension for Brand {
    fn lerp(&self, other: &Self, t: f32) -> Self { /* .. */ }
}
let theme = Theme::light().with_extension(Brand { .. });
let brand: Option<&Brand> = theme.extension::<Brand>();
```

`ThemeExtension: Clone + PartialEq + Debug + Send + Sync + 'static` has one method, `lerp`,
whose default switches at the half. `Theme::extensions` is a `ThemeExtensions`, one pointer
wide (`Option<Arc<Vec<..>>>`, copy on write), so an empty set costs nothing and a theme copy
does not copy the tokens. `Theme::lerp` interpolates every type present on both sides with its
own `lerp`; a type present on one side only is kept until the half, like the theme's other
discrete settings. `PartialEq` compares the values, so a changed extension is a changed theme.

### Animation speed: `frus::set_animation_speed`

`set_animation_speed(f32)` (clamped to `0.01..=100`, NaN read as 1) and `animation_speed()` live
in `frus-core` and are re-exported by `frus-widgets` and `frus`. The factor is a process-wide
atomic. The shell multiplies **the frame's time step** by it, once, right after measuring it:
every animation the frames move takes that step (the application's `tick`, so animation
controllers and the router's page transitions; every implicit-animation family in the
`Runtime`; scroll flings, overscroll, ripples, sheets, dismissals; the theme fade; the
continuous clock of spinners and placeholders). That one multiplication is the whole design,
and it is why nothing can be missed by a widget forgetting to ask. Gesture timing, timers and
the caret's blink stay on the wall clock, so input is not slowed. The test driver and
`frus-test`'s `Stage` apply the same factor.

### Reduced motion on every platform

`Accessibility::disable_animations` (in `MediaQuery`, and `Runtime::still`) was only filled on
Android. A new `reduce_motion` module in the shell reads it:

- **Windows**: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` (*Show animations in
  Windows*), already in the `windows-sys` features.
- **macOS**: `NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion`, through
  `objc2`, already a dependency.
- **Linux**: `gtk-enable-animations` from GTK 4's then GTK 3's `settings.ini`, else GNOME's
  `enable-animations` through `gsettings`; read once a process (a process spawn is not
  repeated on every focus).
- **iOS**: `UIAccessibilityIsReduceMotionEnabled()`.
- **The Web**: `matchMedia("(prefers-reduced-motion: reduce)")`.

It is read where the brightness is (when the application resumes) and again whenever the window
regains the focus, which is how a trip to the system settings comes back. The application's
`AccessibilityOverrides` still outrank it.

`MediaQuery::reduce_motion()` is the one-line question for a widget. Decorative animations now
honour it: **`Skeleton`**'s shimmer holds at its middle and stops asking for frames, and a
**`Chart`**'s pulsing halo is not drawn. (Implicit animations already completed at once under
it.) Indeterminate progress indicators keep moving: they say that something is happening, which
is information, not decoration.

## How it was tested

- **Unit tests** (`cargo test -p frus-core`, `-p frus-widgets`, `-p frus-shell --lib`,
  `-p frus-test --lib`, the `frus` doctests with `testing`, which compile the two guides):
  - token defaults: the sixteen durations follow the scale, every easing runs from 0 to 1 and
    has its shape, the springs are the ratios and stiffnesses they say (`motion.rs`);
  - status colours: every `X` / `on_X` pair, the three new families and their containers
    included, is at AA contrast for three seeds in light and dark; harmonisation turns by half
    the angle, at most 15 degrees, the short way round; the literals in `light()` and `dark()`
    equal the computation (`theme.rs`);
  - extensions: read back by type, replaced by a second value of the same type, not shared by
    a theme copy, compared by value; half way between light and dark a radius of 4 and one of
    8 make 6 and a token with the default `lerp` switches at the half; one present on a single
    side switches at the half (`themeextension.rs`);
  - `Alert`'s four kinds and `SnackBar`'s success and warning kinds take their roles
    (`alert.rs`, `toast.rs`);
  - the speed: clamped to its range, NaN read as 1 (`speed.rs`);
  - **the speed through the test driver** (`crates/frus-shell/tests/animation_speed.rs`, its own
    binary since the factor is process-wide): an `AnimationHandle` run over half a second
    finishes in 30 frames of 1/60 s at speed 1, in about 300 at 0.1 (asserted within 10x ± 0.5),
    stands at 0.1 after 30 slowed frames, and finishes in 8 frames at 4x. Without the
    multiplication in `Driver::frame` the slowed run would take the same 30 frames, a ratio of 1;
  - **reduced motion turns off a decoration**: with a surface installed whose accessibility asks
    for it, two instants of a `Skeleton` paint the same alpha and it stops asking for frames;
    the pulse comes back once the surface is gone (`skeleton.rs`).
- **Windows, for real**: `system_reduce_motion()` was printed by the shell's test
  (`--nocapture`) and compared with `SystemParametersInfo(SPI_GETCLIENTAREAANIMATION)` called
  from PowerShell on the same machine: animations on, so `Some(false)` (see the report).
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.

## Goldens

- **`toast_host`, regenerated.** CI's render differs from the committed golden in 180 pixels
  past the tolerance, all of them in one 4 x 45 rectangle at x 271..274, y 122..166: the stripe
  down the leading edge of the *success* snack bar. It was the crate's hard-coded green
  `(70, 190, 120)` and is now the dark scheme's `success` role, `(106, 221, 140)`. That is the
  change this milestone makes on purpose. The remaining differences are one or two levels in a
  channel, within the tolerance. The new golden is CI's own render, as the GPU job wrote it
  (`toast_host.actual.png`).
- **`alert_kinds`** (in `widgets.rs`) is expected to change too: its info, success, warning and
  error boxes now take `info`, `success`, `warning` and `error` from the scheme. Its new golden is taken from CI's render once the GPU job reaches it.

## Not done

- **Goldens were not run under WSL.** WSL failed with I/O errors and then stopped altogether
  (`E_UNEXPECTED`), because drive C:, which holds WSL's disk, had no free space left. The
  goldens were rendered by CI's GPU job instead, which uses the same pinned Mesa as the WSL
  recipe; see "Goldens" above.
- **Reduced motion was read for real on Windows only.** The macOS, Linux, iOS and Web readers
  compile (`cargo check` for `x86_64-apple-darwin` and `aarch64-apple-ios`, clippy for
  `wasm32-unknown-unknown`) but were not run on those systems. On Linux the answer is read once
  a process, so a change while the application runs is not followed; on the Web it is read
  again when the window regains the focus, not on the media query's change event.
- **Widgets still on their own timing**, because their duration is asked without a theme
  (`Widget::anim_duration`) or because their numbers are platform-specific specifications
  rather than the motion scale: `Checkbox`, `Radio` and `Switch` (`TOGGLE_SECONDS`), `Avatar`,
  the hover, focus and press transitions (`ANIM_DURATION`, `PRESS_DURATION`), the ink ripple,
  the scrollbar's fades, `Dismissible`, `RefreshIndicator`, the overscroll glow and stretch, the
  page transitions (per platform), the theme fade (`Application::theme_animation_duration`), the
  implicit animations' default and the image fade-in. Moving them needs `anim_duration` to see
  the theme, which is a trait change of its own.
- **No widget uses the spring presets yet**: sheets, drawers and the reorder springs keep their
  own constants.
- The phone was not used (not needed for this change); Android keeps reading the setting as
  before.
