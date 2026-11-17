# Milestone 812 — Apple-style widgets

## Objective

frus had the Apple-style colours (`CupertinoColors`, adaptive colours) and theme
(`CupertinoTheme`, milestone 628) and, apart from the platform-dependent touches of
milestones 614 to 621, no Apple-style widgets to use them. An application written for iOS
could follow the platform's scrolling, page transitions and back gesture, and then had to
draw its own switch. This milestone adds the seven widgets an iOS screen is made of:

- **`CupertinoButton`**: plain, tinted and filled; small, medium and large; dims while pressed;
  disabled without a message.
- **`CupertinoSwitch`**: the 51 × 31 track and the 28-pixel white thumb that stretches while
  held; tap and drag; animated.
- **`CupertinoNavigationBar`**: leading, a centred middle and trailing; a back chevron with the
  previous page's title; a translucent background that blurs what is under it; a large-title
  variant.
- **`CupertinoTabBar`**: icons over small labels, the selected tab in the primary colour, a
  hairline along the top.
- **`CupertinoAlertDialog`** and **`CupertinoDialogAction`**: title, message, buttons side by side
  or stacked, default and destructive actions, the grow-and-fade entrance.
- **`CupertinoActionSheet`**: title, message, actions, a cancel button apart, sliding up from
  the bottom.
- **`CupertinoActivityIndicator`**: eight ticks whose brightness turns once a second; still;
  partly revealed; any radius.

and the adaptive constructors that pick between the two families by the theme's platform:
**`Switch::adaptive`** and **`CircularProgressIndicator::adaptive`**.

## Design

### Numbers

Every dimension, timing and colour is the leading toolkit's (its `cupertino/` sources, cited
by file and line in the code): the button's paddings 6/10/16 × 12/15/20, its least sizes
28/32/44, its corners 40/40/12, the tint at 12 % in the light and 26 % in the dark, the press
dimming to 0.4; the switch's 59 × 39 box, 51 × 31 track, 14-pixel thumb radius, 7-pixel stretch,
200 ms flip on `Curves.ease` (flipped on the way off), half opacity disabled, its two thumb
shadows; the bar's 44 pixels, 52 more for a large title, 16 at either end, 6 between the title
and the controls, 50 for the back button, twelve characters before the previous title becomes
"Back", the 0x4D000000 hairline, the 10-sigma blur; the tab bar's 50 pixels, 30-pixel icons,
4 under each label, the adaptive hairline; the alert's 270 width, 14 corners, 20 of padding,
45-pixel buttons, its translucent grey and pressed grey, 250 ms and the 1.3 scale; the sheet's
8-pixel edges, 12 corners, 57.17-pixel buttons, 13.5 / 16 content padding, its five adaptive
colours, 335 ms; the spinner's eight alphas 47 47 47 47 72 97 122 147, its one-second turn and
its tick shape (a bar from a third of the radius to the radius, a fifth of a tenth of it wide).

Colours come from the existing `CupertinoTheme`: the primary colour (the main theme's primary
unless the Apple-style override says otherwise), the colour on it, the bar background, the
text theme (action, small action, navigation title, large title, tab label). Adaptive system
colours resolve against the theme's brightness, so every widget is right in the dark with no
code of its own for it.

### Everything can be overridden

Each widget takes its colours, sizes and paddings from the theme and lets the caller replace
each: a button's fill, label colour, disabled colour, focus colour, pressed opacity, corners,
padding and least size; a switch's four colours, focus colour and whether it takes the
theme's primary colour; a bar's background, hairline (or none), end padding and blur; a tab
bar's active and inactive colours, background, icon size, height and hairline; an alert's
panel colour and scrim; an action's text style; a spinner's colour and radius. Slots take any
widget: a button's content, the bar's leading, middle, trailing and large title, an alert's
title and message.

### Two framework hooks

Two behaviours of the leading toolkit could not be expressed with what the widget trait offered.

**A press that dims a whole subtree.** The button's fill and its content fade together, as
one layer, while it is held. `Widget::pressed_opacity` names the opacity; the walk reads the
widget's own press progress (the runtime's 200 ms press animation, eased so most of the
dimming happens early, as the leading toolkit's 120 ms fade-out does) and drains the subtree into a
layer at that opacity. At rest no layer is made, so a screen of buttons costs nothing more.
The hook is forwarded by `Box<dyn Widget>`, the transparent-wrapper macro and every wrapper
the macro's tests check.

**An overlay that grows into place.** A centred overlay showed its content whole while only
the scrim faded. `Widget::overlay_presentation` returns an `OverlayPresentation` — fade, and a
scale at progress 0 — which the overlay pass applies to the content as one layer, scaled about
its centre; the hit regions are mapped through the same matrix. The alert answers
`fade_and_scale(1.3)` while it opens and `fade()` while it closes, which is the leading toolkit's
entrance and its fade-only exit, on the alert's own 250 ms.

### Layout

The navigation bar's three slots are laid out by the rule of the leading toolkit's toolbar, as a
`CustomMultiChildLayout` delegate (`toolbar_layout`): the leading at the start and the
trailing at the end, the bar's height; the middle **centred across the whole bar** and moved
aside only where it would come within 6 pixels of a control. Right to left, everything is
mirrored, the back chevron included (it is drawn as two round-capped strokes in the text
colour, not a font glyph, so it needs no icon font).

The alert decides between buttons side by side and stacked by measuring the labels: two
buttons whose labels each fit half the width less their padding sit side by side; anything
else is stacked. The action sheet is the screen's width less 8 either side (the screen's
height in landscape), and keeps clear of the system's bars at the bottom.

### Semantics

The button is a `Button` with its label; the switch a `Switch` with its state, announced as
unavailable when disabled; each tab a `Tab` with "label, Tab i of n" and whether it is
selected; the back button a `Button` labelled with the localised "Back"; the alert and the
sheet `Dialog`s labelled with a new localised entry, `alert_dialog_label` ("Alert",
"Alerte"); each of their buttons a `Button`, disabled when it has no message; the spinner a
`ProgressBar` with an optional label.

### Adaptive constructors

`Switch::adaptive(on)` returns an `AdaptiveSwitch` that takes what both switches share (the
message, enabled, the two track colours, the thumb colour) and builds the Apple-style switch
under a theme whose platform is iOS or macOS, the other one everywhere else.
`CircularProgressIndicator::adaptive(value)` does the same for the spinner, a determinate
value becoming a partly revealed spinner as in the leading toolkit. That toolkit has more
adaptive constructors (checkbox, radio, slider, alert dialog); those widgets either have no
Apple-style counterpart in frus yet or take content the counterpart cannot take, and are left
for later.

## How it was tested

Unit tests in each new module (`cargo test -p frus-widgets --lib cupertino`), through
`build_ui` and the scene it paints, so that the numbers checked are the ones on screen:

- **Button**: the box is the padding round the content at least the size's least (50 × 44 for
  a 10-pixel child in a large button, 34 × 28 in a small one); plain has no fill, filled is
  the primary colour, tinted 12 % light and 26 % dark, a caller's colour at the tint's
  opacity; the label's colour per style and disabled, icons at 1.2 × the text; a held button is
  one layer at 0.4 and one told `pressed_opacity(1.0)` makes none; the easing; a disabled
  button sends nothing, takes no focus, does not dim, has the disabled fill and is announced
  disabled; the focus ring's lightness and opacity.
- **Switch**: the 51 × 31 track centred in 59 × 39, the thumb 1.5 pixels from either end;
  mirrored right to left; green / secondary fill in both brightnesses, the theme's primary
  when asked, the caller's over both, a white thumb; a held thumb 35 wide; the flip's curve
  and 200 ms; tap, drag (mirrored), release past half way, disabled inert; half opacity and
  still announced when disabled; the adaptive switch on iOS and macOS versus Android and
  Windows.
- **Activity indicator**: 2 × radius square; eight ticks 2 pixels wide at the native alphas;
  the brightest tick turning a full circle per second; still when not animating; a partly
  revealed one draws that share, alike; the dark grey, a caller's colour, semantics; the
  adaptive indicator's size per platform.
- **Navigation bar**: the toolbar rule (title centred across the bar despite a wide leading,
  moved aside only where needed, mirrored right to left); 44 under a 20-pixel status bar with
  the hairline under it, 52 more with a large title, the dark bar colour, a caller's
  background with no hairline; the back button announced "Back", at the start, at least 50
  wide, sending its message, showing a short previous title and "Back" for a long one, the
  chevron two strokes in the primary colour; a translucent bar adds a backdrop layer that an
  opaque one, or one with the blur turned off, does not.
- **Tab bar**: 50 above a 34-pixel gesture bar with the hairline on top; the selected label in
  the primary colour and the other in the inactive grey; each tab announced with its place and
  state, half the width each; a tap sending its index, mirrored right to left.
- **Alert and sheet**: closed shows nothing; open, a 270-pixel dialog centred, announced
  "Alert", over Apple's scrim; two short buttons side by side, three or a long label stacked;
  the default semibold, the destructive red (and dark red in the dark), each sending its
  message; the presentation (1.3 and fade while opening, fade while closing, 250 ms), and half
  way in a layer at half opacity scaled about the panel's centre, none once settled; the sheet
  8 pixels from the edges and the bottom, its cancel 8 under the actions on its own
  background, the title semibold in the sheet's grey; right to left, the alert's buttons swap.

The demo has a new screen, **Apple-style widgets** (`/cupertino`, linked from the task list),
with every widget, both adaptive ones, the alert and the sheet.

## Not done

- **The switch's drag follows the finger** and flips when let go past half way; the leading toolkit's
  switch instead jumps to the other end once the drag passes 70 % (20 % back) and flips on
  release. The frus drag machinery hands a widget a value, not the drag's total; changing that
  was out of scope.
- **No haptic tick on a flip.** The leading toolkit vibrates only on iOS, and frus's haptics answer
  unsupported on iOS for now (milestone 777); there is no widget-side hook to the shell's
  haptics either.
- **The large title does not collapse on scroll**: the large variant is the static bar with its
  title under it. The collapse needs a sliver header, which the bar is not.
- **No transitions between two pages' bars** (the title sliding into the back button).
- **The alert does not scroll** content taller than the screen, and does not switch to the
  accessibility layout at large text sizes; the action sheet does not scroll its actions
  either.
- **The press is the runtime's 200 ms both ways**, eased; the leading toolkit fades out in 120 ms and
  back in 180 ms on two different curves. The button also does not cancel the dimming when a
  finger slides 70 pixels off it.
- **The switch's on/off labels** (an accessibility setting on iOS) are not drawn.
- **No other adaptive constructors**: checkbox, radio, slider and alert dialog wait for their
  Apple-style counterparts.
- **A padding straight under a `ThemeBuilder` is applied twice.** The builder takes its
  child's style as its own, padding and margin included, and the child then applies them again
  inside it. The action sheet's edges came out at 16 pixels instead of 8 for that reason; the
  sheet now puts its padding inside a plain column. The builder itself is unchanged here and
  will be fixed in a milestone of its own. Likewise `Expanded` replaces its child's least
  height with zero, so the alert's side-by-side buttons sit in a plain column inside it to
  keep their 45 pixels.
- Verified by unit tests and by building the demo; **not looked at on an iPhone or an iOS
  simulator**, and not compared pixel by pixel with the leading toolkit's rendering.
