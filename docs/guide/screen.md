# The screen: size, insets, keyboard and orientation

A frus screen runs on a phone held either way, a tablet, a resizable window and a browser tab.
This page covers what frus tells a screen about where it is drawn (its size, its pixel density,
the parts the system covers, the keyboard) and the widgets that use it. Laying widgets out is in
[Laying out a screen](layout.md).

## What the screen is: `MediaQuery`

`MediaQuery::of()` describes the surface being built for. Any widget reads it while it builds,
and frus builds again when it changes: a rotation, a resized window, the keyboard opening.

| Field | What it is |
| --- | --- |
| `size` | the surface, in logical pixels |
| `device_pixel_ratio` | physical pixels per logical pixel (2.625 on many phones) |
| `padding` | what the system covers: status bar, notch, gesture bar (the safe area) |
| `view_insets` | what the keyboard covers, at the bottom |
| `view_padding` | `padding` as it is with the keyboard closed |
| `system_gesture_insets` | the edges where the system's back and home gestures start |
| `text_scaler` | the reader's text size, see [Text and fonts](text.md#the-readers-text-size) |
| `platform_brightness` | light or dark, as the system is set |
| `accessibility` | reduced motion, bold text, high contrast, see [Accessibility](accessibility.md) |

and `orientation()`, `size_class()` and `safe()` (the larger of `padding` and `view_insets` on
each side) are worked out from them.

Every size in frus is in **logical pixels**: a 48 by 48 button is about the same size under a
finger on every screen, whatever its density. frus scales to physical pixels when it draws.

## Adapting to the width

Three size classes divide the widths the way most layouts change: `Compact` below 600 (a phone
held upright), `Medium` up to 840 (a phone on its side, a small tablet), `Expanded` above.
`responsive` picks one widget per class; a class you leave out takes the nearest one you gave:

```rust
use frus::{component, responsive, Column, MediaQuery, Row, Text, Widget};

fn inbox() -> Box<dyn Widget> {
    Box::new(component(|_cx| {
        let width = MediaQuery::of().size.width;
        Box::new(
            responsive(width)
                .compact(Column::new().child(Text::new("List")))
                .expanded(
                    Row::new().spacing(24.0).child(Text::new("List")).child(Text::new("Detail")),
                ),
        )
    }))
}
```

The width of the **window** is what a page layout should follow. A widget that must adapt to the
room its **parent** gives it, a card in a grid say, asks for that with `LayoutBuilder` or
`OrientationBuilder`:

```rust
use frus::{column, row, text, LayoutBuilder, Orientation, OrientationBuilder, Text};

let card: LayoutBuilder = LayoutBuilder::new(|size| {
    if size.width > 320.0 {
        Text::new("Wide card")
    } else {
        Text::new("Narrow")
    }
});
let gallery: LayoutBuilder = OrientationBuilder::new(|orientation| match orientation {
    Orientation::Landscape => row![text("a"), text("b")],
    Orientation::Portrait => column![text("a"), text("b")],
});
# let _ = (card, gallery);
```

## What the system covers: the safe area

On a phone the status bar, a notch or camera hole and the gesture bar sit over the screen.
`SafeArea` pads its child clear of them, and only on the sides that need it:

```rust
use frus::{Edges, Insets, SafeArea, Text};

// Clear of everything.
let page: SafeArea = SafeArea::new(Text::new("Content"));
// A list that scrolls under the gesture bar, but starts below the status bar.
let list: SafeArea = SafeArea::new(Text::new("Rows")).edges(Edges::ALL.without_bottom());
// At least 16 pixels on every side, even where the system covers nothing.
let padded: SafeArea = SafeArea::new(Text::new("Card")).minimum(Insets::new(16.0, 16.0, 16.0, 16.0));
# let _ = (page, list, padded);
```

A `Scaffold`'s bars keep clear of the system on their own: the app bar below the status bar, the
bottom bar above the gesture bar. Its **body** reaches the edges, so that a picture or a list can
run under the system's bars, and is only told about them; wrap the body in `SafeArea` when its
content must stay clear. A `SafeArea` consumes the room it pads, so one inside another does not
pad twice. On the desktop the safe area is empty.

## The keyboard

When the on-screen keyboard opens, `view_insets.bottom` is its height. A `Scaffold` shortens its
body by it, so the field being typed in and the button under it stay visible; a screen that
should not move says `.resize_to_avoid_bottom_inset(false)`. Outside a scaffold,
`SafeArea::new(..).avoid_keyboard()` does the same.

The keyboard closes when the reader taps outside the field (`TextField::tap_outside` changes
that), or, if you ask, when they scroll:

```rust
use frus::{Column, KeyboardDismissBehavior, SingleChildScrollView, Text, TextField};

let form: SingleChildScrollView = SingleChildScrollView::new()
    .keyboard_dismiss_behavior(KeyboardDismissBehavior::OnDrag)
    .child(Column::new().child(Text::new("Name")).child(TextField::new("")));
# let _ = form;
```

The kind of keyboard (numbers, email, phone) and its action key are the field's to choose:
see [Taking input](input.md).

## The system bars

The colour behind the status and navigation bars, and whether their icons are light or dark,
follow what the screen shows behind them. By default that is the theme's background. A page with
a dark picture at the top says so for the part of the screen it covers:

```rust
use frus::{AnnotatedRegion, Color, Container, SystemUiOverlayStyle};

let photo = Container::new().height(240.0).color(Color::rgb(0.08, 0.08, 0.1));
// Light icons over the dark picture.
let header: AnnotatedRegion = AnnotatedRegion::new(
    SystemUiOverlayStyle::for_background(Color::rgb(0.08, 0.08, 0.1)),
    photo,
);
# let _ = header;
```

This applies on Android; elsewhere the system draws its own bars.

## Orientation

A screen follows the device as it turns, and builds again for the new size. To keep an Android
application upright, set the activity's orientation in `Cargo.toml`:

```toml
[package.metadata.android.application.activity]
orientation = "portrait"
```

Keep the `config_changes` list that `frus create` wrote there: it is what tells Android to hand
the rotation to the running application rather than starting it again.

## Seeing other screens without the devices

`frus run -d iphone-15` (or `iphone-se`, `iphone-15-pro-max`, `ipad`, `android-phone`) runs a
debug build in a window shaped like that device: its size and density, its notch or island, its
safe area and its platform's look. `l` turns it on its side, `K` opens a keyboard, `T` changes
the text size and `b` switches light and dark. `--web` shows it in a browser instead.
`frus devices` lists the profiles; describe one of your own in `Cargo.toml`:

```toml
[[package.metadata.frus.devices]]
id = "pixel-9"
name = "Pixel 9"
platform = "android"            # or "ios"
width = 412                     # logical, held upright
height = 924
ratio = 2.625
safe-area = [52, 0, 24, 0]      # top, right, bottom, left
cutout = "hole"                 # none, notch, island or hole
cutout-size = [34, 34]
```

It is a preview: the layout, the insets and the platform's conventions are the device's; the
fonts, the GPU, the native services and the speed are your computer's. Check on a real device
before shipping.
