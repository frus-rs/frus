# Animation

Most movement in an application is a value that changes — a box that grows, a hint that fades,
a number that counts up — and should get there smoothly rather than at once. In frus that is an
**implicit** animation: you build the widget with the new value, as you would without
animation, and it moves there over the time you give it.

## A box that changes: `AnimatedContainer`

`AnimatedContainer` is a `Container` whose size, colour, corners and padding move to their new
values. The duration is in seconds; the curve says how it accelerates.

```rust
use frus::{component, AnimatedContainer, Button, Column, Curve, Text, Widget};

fn growing_card() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let open = cx.use_state(|| false);
        let on = open.get();
        let scheme = &cx.theme().scheme;
        let toggle = open.clone();
        Box::new(
            Column::new()
                .spacing(16.0)
                .child(
                    Button::new(if on { "Shrink" } else { "Grow" })
                        .on_press(move || toggle.update(|on| *on = !*on)),
                )
                .child(
                    AnimatedContainer::new(0.4, Curve::ease_in_out())
                        .color(if on { scheme.primary } else { scheme.tertiary })
                        .size(if on { 260.0 } else { 140.0 }, if on { 120.0 } else { 72.0 })
                        .radius(if on { 32.0 } else { 8.0 })
                        .padding(16.0)
                        .child(Text::new(if on { "Expanded" } else { "Compact" })),
                ),
        )
    }))
}
```

## Fading, sliding, switching

The same shape works for one property at a time — `AnimatedOpacity`, `AnimatedPadding`,
`AnimatedAlign`, `AnimatedPositioned`, `AnimatedSize` (a box that follows the size of its
child), `AnimatedDefaultTextStyle`:

```rust
use frus::{component, AnimatedOpacity, Button, Column, Curve, Text, Widget};

fn hint() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let shown = cx.use_state(|| true);
        let toggle = shown.clone();
        Box::new(
            Column::new()
                .spacing(12.0)
                .child(Button::new("Toggle the hint").on_press(move || toggle.update(|s| *s = !*s)))
                .child(AnimatedOpacity::new(
                    if shown.get() { 1.0 } else { 0.0 },
                    0.3,
                    Curve::ease_in_out(),
                    Text::new("Swipe left to archive"),
                )),
        )
    }))
}
```

When what changes is **which** widget is shown — a number that goes up, an icon that becomes
another — `AnimatedSwitcher` fades the old one out and the new one in. It is given a value and
a function that builds the widget for a value; a new value starts the transition.

```rust
use frus::{component, AnimatedSwitcher, Button, Column, Text, Widget};

fn counter() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let count = cx.use_state(|| 0_i32);
        let add = count.clone();
        Box::new(
            Column::new()
                .spacing(12.0)
                .child(AnimatedSwitcher::new(0.3, count.get(), |n: &i32| {
                    Text::new(n.to_string()).size(40.0)
                }))
                .child(Button::new("+1").on_press(move || add.update(|n| *n += 1))),
        )
    }))
}
```

`AnimatedCrossFade` does the same between exactly two children, and resizes between them.

## Curves

`Curve::Linear`, `Curve::ease_in_out()`, `Curve::ease_out()`, `Curve::ease_in()` and the
emphasised curves of the design system are all there; `Curve::Cubic { x1, y1, x2, y2 }` is any
other, with the control points of CSS's `cubic-bezier()`.
A short duration and an easing curve — 150 to 300 ms, `ease_out` for something arriving — is
what feels responsive.

## Timing from the theme

`theme.motion` holds the design system's timing, so a widget does not pick its own numbers:
durations from `short1` (50 ms) through `medium2` (300 ms) to `extra_long4` (one second), the
easings `standard`, `emphasized` and their `_decelerate` (entering) and `_accelerate` (leaving)
forms, and three springs, `spring_fast`, `spring_default` and `spring_slow`:

```rust
use frus::{component, AnimationController, Button, Column, Opacity, Text, Widget};

fn reveal() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let motion = cx.theme().motion.clone();
        let fade = cx.use_animation_controller(AnimationController::unit);
        let start = fade.clone();
        Box::new(
            Column::new()
                .child(Button::new("Show").on_press(move || {
                    start.forward(motion.medium2, motion.emphasized_decelerate.clone())
                }))
                .child(Opacity::new(fade.value(), Text::new("Hello"))),
        )
    }))
}
```

`Theme::with_motion(MotionTheme { .. })` retimes an application in one place.

## Motion patterns: fade through, shared axis, fade scale

Four named transitions cover most of what moves between two states of a screen:

- **Fade through** (`MotionPattern::FadeThrough`) between things that are not related: the
  old one fades out, then the new one fades in as it grows from 92 %.
- **Shared axis** (`MotionPattern::SharedAxis(SharedAxis::X | Y | Z)`) between steps of one
  flow: both slide 30 px along the axis (or scale, along Z) as one fades out and the other in.
  Along X, "next" always comes from the end edge, also in right-to-left layouts.
- **Fade scale** (`MotionPattern::FadeScale`) for something appearing over the page — a menu,
  a dialog: it fades in as it grows from 80 %, and simply fades out.
- **Container transform** (`OpenContainer`): a card that grows into a page.

A switcher plays a pattern between its children, for as long as the theme's motion tokens say:

```rust
use frus::{text, AnimatedSwitcher, MotionPattern, SharedAxis};

fn step_view(step: u32, forward: bool) -> AnimatedSwitcher {
    AnimatedSwitcher::patterned(MotionPattern::SharedAxis(SharedAxis::X), step, |n: &u32| {
        text(format!("Step {n}"))
    })
    // Going back moves the other way.
    .reverse(!forward)
}
```

The same patterns are page transitions. A route can have its own, the theme can make one the
default everywhere, and a shell can move between its tabs with one:

```rust
use frus::{text, GoRoute, PageTransitionsBuilder, PageTransitionsTheme, SharedAxis, Theme};

let details = GoRoute::new("/album/:id", |_, _| text("Album"))
    .transition(PageTransitionsBuilder::SharedAxis(SharedAxis::Z));

let mut theme = Theme::light();
theme.page_transitions = PageTransitionsTheme::default().all(PageTransitionsBuilder::FadeThrough);
# let _ = (details, theme);
```

On a shell route, `.branch_transition(PageTransitionsBuilder::FadeThrough)` fades through
between the tabs of a navigation bar (they switch at once otherwise).

```rust
use frus::{text, Button, OpenContainer};

// Keeps whether it is open itself; each builder is handed what opens or closes it.
let album = OpenContainer::builder(
    |_open| text("Album"),
    |close| Button::new("Back to the albums").on_press(close),
);
# let _ = album;
```

`OpenContainer::new(open, closed, opened).on_open(..).on_close(..)` is the same container for
an application that keeps the answer in its own state. Its colours, elevations, corners and
scrim come from `theme.widgets.open_container`.

## Things that leave: presence

A child the state no longer has is not in the next tree, so nothing is left to animate. An
`AnimatedPresence` keeps it — or the value it was built from — for as long as its exit lasts,
then lets it go with everything it held. While it leaves it takes no taps and is not announced.

```rust
use frus::{text, AnimatedPresence, MotionPreset, Transition};

fn saved_note(saved: bool) -> AnimatedPresence {
    AnimatedPresence::new(saved, text("Saved"))
        .enter(Transition::preset(MotionPreset::EmphasizedEnter).slide(0.0, 16.0))
        .exit(Transition::preset(MotionPreset::EmphasizedExit).scale(0.9))
}
```

`AnimatedPresence::of(option, build)` is for a value that is gone the moment it is removed. A
`PresenceList` does the same for each item of a keyed list — a removed item leaves where it
was, the items under it closing up — and `Staggered` brings children in one after another:

```rust
use frus::{text, PresenceList, Staggered};

let tasks = vec![(1, "Write"), (2, "Ship")];
let list: PresenceList = PresenceList::new(tasks, |task| task.0, |task| text(task.1)).stagger(0.05);
let intro: Staggered = Staggered::column().child(text("Welcome")).child(text("Let's start"));
# let _ = (list, intro);
```

## Presets instead of numbers

`MotionPreset` names the specification's pairs of a duration and an easing, read from
`theme.motion` where they play: `Emphasized` (500 ms), `EmphasizedEnter` (400 ms, decelerating),
`EmphasizedExit` (200 ms, accelerating), `Standard` (300 ms), `StandardEnter` (250 ms) and
`StandardExit` (200 ms). A `Transition` is a value — what something fades, grows and slides
from, and on which preset — so an application names its own once and reuses them. Every pattern
and preset respects reduced motion: with it on, things are simply there, or simply gone.

## What frus animates for you

Several movements need no code: the page transition when the router pushes or pops a screen
(and the back gesture that follows the finger), a button's ripple, a switch's thumb, a text
field's floating label, a scroll's fling and its overscroll. They follow the platform, and the
theme's `page_transitions` changes them.

`Hero` flies a widget from one screen to the next — the photo of a list row that becomes the
header of its page — when both screens wrap it in a `Hero` of the same tag.

## Animations you drive

When the movement is not a value reaching another — a pulse that repeats, a sequence a button
starts, a progress you set — `cx.use_animation_controller(..)` gives a controller that holds a
progress, usually from 0 to 1. Start it with `forward`, `reverse`, `repeat` or `animate_to`,
and the frames move it, rebuilding what reads `value()`, until it stops:

```rust
use frus::{component, AnimationController, Button, Column, Curve, Opacity, Text, Widget};

fn pulse() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let glow = cx.use_animation_controller(AnimationController::unit);
        let (start, stop) = (glow.clone(), glow.clone());
        Box::new(
            Column::new()
                .spacing(12.0)
                .child(Opacity::new(0.3 + 0.7 * glow.value(), Text::new("Recording").size(24.0)))
                .child(Button::new("Start").on_press(move || start.repeat(1.2, true, Curve::ease_in_out())))
                .child(Button::new("Stop").on_press(move || stop.stop())),
        )
    }))
}
```

A `StatefulWidget` keeps one in its state with `AnimationHandle::new(controller)`. The
`*Transition` widgets (`FadeTransition`, `SlideTransition`, `ScaleTransition`,
`SizeTransition`, `DecoratedBoxTransition`) draw straight from a controller's progress.

People who prefer the system's reduced motion setting get it, on Android, Windows, macOS,
Linux (GTK's setting), iOS and the Web: `MediaQuery::reduce_motion()` reports it, frus's own
transitions complete at once, and its decorations (a placeholder's shimmer, a chart's pulsing
halo) hold still. A decoration of your own should ask too.

## Slow motion, for debugging

`frus::set_animation_speed(0.1)` makes every animation ten times slower, to look at one that is
over too fast to see; `4.0` makes them four times faster. It reaches everything the frames
move (transitions, controllers, springs, flings, ripples, the theme's fade), not input or
timers. The speed is clamped to `0.01..=100`:

```rust
frus::set_animation_speed(0.1);
// ... look at it ...
frus::set_animation_speed(1.0);
```
