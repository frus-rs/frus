# Milestone 785 — The Material motion system: patterns, presence and presets

## Objective

frus had the pieces of motion — explicit transitions, an `AnimatedSwitcher`, platform page
transitions, the motion tokens of milestone 778 — but none of the named transitions a Material
application is designed with, no way for a child to animate **out** of a tree that is rebuilt
from state, and no named timings. Three gaps, each one an application ends up working around:

1. **The motion patterns**: fade through, shared axis (X, Y, Z), fade scale and the container
   transform, usable as a page transition (per route and as the theme's default), as a switch
   between two children, and — for the container transform — on its own.
2. **Presence**: a child removed from the tree plays an exit before it disappears, without
   leaving state behind or keeping taps.
3. **Presets**: named timings built on `theme.motion`, so an application writes
   `Transition::preset(MotionPreset::EmphasizedEnter)` rather than numbers; and a staggered
   entrance for a list of children.

## What changed

### The patterns, as pure functions (`motionpatterns.rs`)

`MotionPattern::{FadeThrough, SharedAxis(SharedAxis::X | Y | Z), FadeScale}` and
`pattern.pose(progress, entering, forward) -> MotionPose { opacity, scale, dx, dy }`. One
function poses the child arriving and the child leaving at any moment; the page transitions,
the switcher and the container all call it, so they cannot drift apart. The numbers are the
specification's:

| pattern | arriving | leaving |
|---|---|---|
| fade through | 0 until 30 %, then fades in and scales 92 → 100 % on `cubic(0, 0, 0.2, 1)` | fades out over 0–30 % on `cubic(0.4, 0, 1, 1)` |
| shared axis X / Y | slides from ±30 px on `cubic(0.4, 0, 0.2, 1)`, fades in over 30–100 % (decelerating) | slides ∓30 px, fades out over 0–30 % (accelerating) |
| shared axis Z | scales from 80 % (from 110 % going back) | scales to 110 % (to 80 % going back) |
| fade scale | fades in over 0–30 % linearly, scales 80 → 100 % (decelerating) | fades out linearly |

Durations come from the motion tokens: `medium2` (300 ms) for fade through, shared axis and the
container transform; `short3` (150 ms) for a fade scale arriving and half of it (75 ms) leaving.
A theme with slower tokens slows the patterns. The shared X axis turns round in right-to-left
layouts, so "next" always comes from the end edge. Fade through and shared axis stand their
children on a fill (the scheme's surface by default).

`container_transform(value, opening, kind, colors) -> ContainerFrame` is the container
transform: bounds on "fast out, slow in" (its mirror while closing), the scrim at full strength
by a fifth of the way while opening and leaving with the movement while closing; for
`ContainerTransitionType::Fade` the open content fades in over 20–40 % while the colour crosses
over, for `FadeThrough` the closed content fades out over 0–20 % through a middle colour and
the open one in over 20–100 %. Closing replays the same sequence in time order.

### Page transitions

`PageTransitionsBuilder` gains `FadeThrough`, `SharedAxis(SharedAxis)` and `FadeScale`
(`From<MotionPattern>`). `PagePose` gains `shift_x` and `shift_y` in logical pixels (the 30 px),
which the navigator applies with the page's other offsets and mirrors with them.
`transition_duration_in(&MotionTheme, forward)` reads the tokens for the patterns; the platform
transitions keep their platform numbers. `PageTransitionsTheme::all(builder)` makes one
transition the default on every platform.

- **Per route**: `GoRoute::transition(builder)`. A push plays the arriving page's transition,
  a pop the leaving page's, a back gesture the page on top's; the router hands it to the
  navigator through the new `Navigator::transition` (and `Widget::navigator_transition`,
  forwarded by the transparent wrappers and `Responsive`).
- **Between tabs** (milestone 781's shells had none): `GoRoute::shell(..)
  .branch_transition(PageTransitionsBuilder::FadeThrough)` plays a transition in the shell's
  own navigator when the branch on show changes — forwards to a later tab, backwards to an
  earlier one, so a shared X axis moves towards the tab chosen. Without it tabs still switch at
  once, as before: a default change would have changed every existing application.
- **Reduced motion**: the router reads `Runtime::still` / `MediaQuery::reduce_motion()` where it
  is built, and a move then shows its page at once.

### A switch between two children

`AnimatedSwitcher::patterned(pattern, value, build)` plays a pattern for the theme's durations;
`.pattern(pattern)` plays one over durations of the caller's; `.reverse(true)` steps backwards;
`.fill_color(..)` changes the fill (`Color::TRANSPARENT` for none). The transition a switcher
applies is now told whether its child is leaving, the layout direction and the fill — the
existing `.transition(|child, t| ..)` is unchanged.

### Presence (`presence.rs`)

The view is a function of state, so a removed child is not in the next tree. What can be kept is
the value it was made from, as `AnimatedSwitcher` keeps its values. The switcher's runtime
store (`Switch`) learnt three things for this, and every presence widget lives in it, so the
shell, the test driver and `frus-test`'s `Stage` already step them and forget them when their
widget leaves:

- **turning round**: a value coming back while it leaves resumes from where its exit had got
  to, with its identity (what its subtree keeps), instead of a second copy arriving;
- **keyed lists**: entries carry a key; an item no longer listed stays where it was — after the
  item that was before it — while it leaves;
- **delays**: an entry can wait before it arrives, which is a staggered entrance.

Widgets:

- `AnimatedPresence::new(visible, child)` and `AnimatedPresence::of(option, build)`, with
  `.enter(Transition)`, `.exit(Transition)` and `.appear(bool)`. A leaving child is wrapped in
  `IgnorePointer` and `ExcludeSemantics`; once its exit ends it is gone from the tree.
- `PresenceList::new(items, key, build)` with `.enter`, `.exit`, `.stagger(seconds)`,
  `.appear(bool)`, `.horizontal()`. By default items fade and unfold in, fade and fold away.
- `Staggered::column()/row()` with `.child`, `.children`, `.interval` (50 ms), `.enter`
  (a fade and 16 px rise on `EmphasizedEnter`) and `.gap`: an entrance played once.

### Presets (`MotionPreset`, `Transition`)

`MotionPreset::{Emphasized, EmphasizedEnter, EmphasizedExit, Standard, StandardEnter,
StandardExit}`: 500 / 400 / 200 / 300 / 250 / 200 ms with `emphasized`,
`emphasized_decelerate`, `emphasized_accelerate`, `standard`, `standard_decelerate`,
`standard_accelerate`, read from `theme.motion` where they play. `Transition` is a value — the
opacity, scale, offset and unfolding something has when it is not there, and a timing (a preset
or numbers) — with `preset`, `fade`, `none`, `scale`, `slide`, `size`, `size_along`,
`from_opacity`, `timing`, `duration`, `curve`, `resolve` and `pose`. An exit's curve is read in
time (an accelerating exit starts slowly), which the switcher's reversed clock otherwise turns
inside out.

### The container transform on its own (`opencontainer.rs`)

`OpenContainer::builder(|open| closed, |close| opened)` keeps whether it is open itself and
hands each builder the callback that moves it; `OpenContainer::new(open, closed, opened)
.on_open(..).on_close(..)` is the controlled form. A tap on the closed card opens it (unless
`.tappable(false)`); Escape and the system's Back close it. The closed card records the box it
was painted in; opened, it leaves a placeholder of its size in the layout and floats as a modal
overlay over the window, the container's rectangle, corners, elevation and colour interpolated
and each content laid out at its own size (the card's, the window's) and scaled to the
container's width from its top-left corner. Nothing in it takes
taps while it moves; the page behind takes none while it is over it. Styling:
`OpenContainerTheme` on `theme.widgets.open_container` (closed and open colours, middle colour,
elevations 1 and 4, corner radii 12 and 0, scrim at 54 %, the transition type), every one
overridable per container.

### Demo

`crates/frus-demo/examples/motion.rs`: three tabs that fade through, a shared X axis between
steps, a fade through, a fade-scale presence, a page that arrives along Z, a staggered entrance,
a presence list, and three container transforms. `FRUS_MOTION_SPEED` slows it down,
`FRUS_MOTION_AUTO=1` plays every change by itself, `FRUS_MOTION_TAB` picks the first tab.

## How it was tested

- **Unit tests** (`cargo test -p frus-widgets --lib`: 2265 passed, 0 failed):
  - `motionpatterns.rs`: every pattern begins with the old child at rest and ends with the new
    one at rest, both ways; fade through gone by 30 % and growing from 92 % on its curve; shared
    axis X/Y 30 px each way and turned round going back; shared axis Z 80 % / 110 %; fade scale
    in by 30 % from 80 %, out linearly, 150 / 75 ms; durations follow the tokens; the six presets
    pair the specification's durations and easings; a `Transition` poses from its values to
    rest; the container transform's fade (20–40 %), its close replayed in time, its fade through
    a middle colour.
  - `pagetransitions.rs`: the patterns pose the two pages (push and pop), a fade scale leaves
    the page under it alone, `PageTransitionsTheme::all`.
  - `navigator.rs`: a navigator told the shared X axis draws the arriving page the posed number
    of pixels towards the end edge, and towards the start edge in right-to-left, the leaving page
    gone.
  - `router.rs`: a route's own transition is played on its push (300 ms, 30 frames of 10 ms
    under a theme whose platform zooms) and on its pop, a route without one plays the theme's; a
    shell's branch transition plays in the shell's navigator with both tabs drawn, forwards and
    backwards; without one tabs switch at once; under reduced motion a push has no transition.
    The existing back-gesture tests caught a double borrow in the first version of the iOS check.
  - `presence.rs`: **a removed child plays its exit, takes no tap during it, and is then gone**
    from the picture, from the hits and from the runtime; shown again mid-exit it turns round
    from where it was (one child, same opacity); `of` keeps the value for the exit; reduced motion
    removes at once; a list item folds away where it was, the item under it moving up, then takes
    its place and its taps; items arriving together are staggered; `Staggered` plays child by
    child.
  - `opencontainer.rs`: the card opens on a tap, grows to the posed width a third of the way,
    takes no tap while moving, is the window's and takes taps once open, and shrinks back; the
    self-kept form opens from its callback, and under reduced motion at once.
- **Goldens** (`frus-test` under WSL, `CARGO_INCREMENTAL=0`, lavapipe): six new pictures in
  `tests/material_motion.rs` — a shared X axis and a fade through a third of the way and at 60 %,
  two pages half way through a shared-Z push and a fade-scale push, a container a third of the way
  open, a list item half way out. The existing `goldens` (106), `motion` (16) and `switcher` (2)
  pass unchanged: the navigator's new pixel offsets and the switcher's new plumbing change no
  picture.
- **A real Windows window**: `cargo run -p frus-demo --example motion` with
  `FRUS_MOTION_AUTO=1 FRUS_MOTION_SPEED=0.2`, captured with `PrintWindow` every 250–350 ms on each
  tab: the shared X axis and the fade through between steps (the arriving step offset and fading
  in), a list item arriving between two others while the staggered panels replay, and the
  container growing from the card over a scrim. The first run showed the closed card's text
  through the open page, whose own background was transparent; the open content now stands on the
  open colour, and the second run shows it clean.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`.

## Not done

- **The phone and the browser were not used.** Everything here is drawn by the shared walk and
  unit-tested, but the example was run on Windows only.
- **The container's start is the box the card was painted in on the frame before**, in window
  coordinates. A card inside a scaled layer (mid page transition) or one that moves while the
  container opens starts from where it was, and a transform above the card is not taken into account.
- **The closed content is rebuilt** while the container is open (it moves from the layout to the
  overlay), so state it keeps in components is not carried over; nor is the open content's when it
  settles. A container is not a route: the router's location does not change while it is open.
- **An interrupted transition reverses from where it was** for presence and the container, but a
  pattern switcher interrupted half way starts the leaving child's exit from its progress, which
  for fade through can make it vanish at once.
- **The fill behind fade through and shared axis** is the scheme's surface, like the canvas colour
  it stands for; on a page whose background is not the surface it shows as a box while the change
  runs (`fill_color(Color::TRANSPARENT)` turns it off).
- **Tabs do not fade through by default**: `branch_transition` has to be asked for, so existing
  applications keep switching at once.
- No predictive back for the patterns, and no `ModalConfiguration` (a fade-scale dialog helper):
  `Dialog` keeps its own entrance.
