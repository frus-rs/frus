# Accessibility

A frus application draws its own pixels, so a screen reader cannot read them. frus describes the
screen to the platform instead: every frame, each widget says what it is (a button, a heading, a
checked box, a field and what is in it), and the platform's screen reader reads that, moves
between the parts and activates them.

| Platform | Screen readers | How |
| --- | --- | --- |
| Windows | Narrator, NVDA, JAWS | UI Automation |
| macOS | VoiceOver | the macOS accessibility API |
| Linux | Orca | AT-SPI |
| Android | TalkBack | a virtual view tree, built only while a service is on |
| Web | NVDA, JAWS, VoiceOver, ChromeVox | one HTML element per node, inside the canvas, with ARIA roles |

On every platform a reader can activate a button, tick a box, type into a field, move a slider
and reach a row's named actions. On the web, a phone's reader cannot explore by touch, because
the elements have no position on the page. There is no iOS application yet; see LIMITATIONS.md.

## What you get without asking

The widgets describe themselves. A `Button` is a button named by its label, disabled when it has
no `on_press`. A `Checkbox`, `Switch` and `Radio` are checked or not; a `Slider` has its range;
a `TextField` is named by its label (or, without one, by its placeholder) and reads its text, or
says nothing of it when it is obscured. Lists, tabs, dialogs, progress bars and trees have their
roles. A screen made of frus widgets is readable as it is.

What the widgets cannot know is what **your** picture or icon means:

```rust
use frus::{Icon, IconButton, Icons, Image, Row, Text, Widget};

fn header() -> Box<dyn Widget> {
    Box::new(
        Row::new()
            // An icon button has no words of its own: name it.
            .child(IconButton::new(Icons::CLOSE).label("Close").on_press(|| {}))
            // A title that readers can jump to, as on a web page.
            .child(Text::new("Inbox").heading())
            // A picture that says something...
            .child(Image::network("https://example.com/map.png").semantic_label("Map of the route"))
            // ...and one that is only decoration.
            .child(Image::network("https://example.com/swirl.png").exclude_from_semantics(true))
            .child(Icon::new(Icons::SETTINGS)),
    )
}
```

## Saying it yourself: `Semantics`

`Semantics` describes what is under it. Use it for a widget of your own, a gesture area that
acts as a button, or a group that should read as one thing:

```rust
use frus::{
    row, Checkbox, Container, ExcludeSemantics, Icon, Icons, Role, Semantics,
    SemanticsProperties, Text,
};

// A box drawn by hand that is, for a reader, a button.
let star: Semantics = Semantics::new(
    SemanticsProperties::new(Role::Button).label("Add to favourites").clickable(),
    Container::new().width(40.0).height(40.0).child(Icon::new(Icons::STAR)),
);

// A checkbox and its words read as one control: "Notifications, checked".
let setting: Semantics = Semantics::merge(row![Checkbox::new(true), Text::new("Notifications")]);

// A chevron the row's words already cover.
let chevron: ExcludeSemantics = ExcludeSemantics::new(Icon::new(Icons::CHEVRON_RIGHT));
# let _ = (star, setting, chevron);
```

`SemanticsProperties` carries everything a node can say: its `role`, `label` and `value`,
`toggled`, `clickable`, `disabled`, a `range(min, value, max)`, `obscured`, `expanded`, `live`,
and named actions:

```rust
use frus::{Role, SemanticsProperties};

// An error that is read out as soon as it appears, wherever the reader is.
let error = SemanticsProperties::new(Role::Label).label("The email is not valid").live(true);
// A row with actions a reader picks by name, rather than by swiping.
let row = SemanticsProperties::new(Role::ListItem)
    .label("Invoice 4417")
    .custom_action("Archive")
    .custom_action("Delete");
// A disclosure button that says whether its panel is open.
let more = SemanticsProperties::new(Role::Button).label("More").expanded(false);
# let _ = (error, row, more);
```

The widgets that need them already use these: `Field` reads its error as a live region,
and `SwipeActions` offers its actions by name. A field
can speak after a submit with `TextField::announce(..)`.

The tree a reader sees is flat: every node is a child of the window, in reading order. Merging
joins the words of a subtree; a merged row keeps one clickable child activatable.

## The keyboard

Everything a pointer can do, the keyboard can do: Tab and Shift+Tab move between controls,
Space and Enter press them, arrows move inside lists, menus, tabs and sliders, and Escape closes
what is open. The focused control has a visible highlight once the keyboard is used. When the
order should not be the order on screen:

```rust
use frus::{column, Button, ExcludeFocusTraversal, FocusTraversalGroup, FocusTraversalOrder, Text, Variant};

let form: FocusTraversalGroup = FocusTraversalGroup::new(column![
    FocusTraversalOrder::new(2.0, Text::new("second")),
    FocusTraversalOrder::new(1.0, Text::new("first")),
    // Reached by a click, skipped by Tab.
    ExcludeFocusTraversal::new(Button::new("Help").variant(Variant::Text).on_press(|| {})),
]);
# let _ = form;
```

To move focus from code, put the target in `keyed(key, widget)` and call
`frus::host::focus(key)` (or return `Command::focus(key)`); `TextField::autofocus(true)` focuses
a field when its page opens. [Taking input](input.md) has an example that moves focus to the
first field in error.

## The reader's settings

`MediaQuery::of()` carries what the reader set on the device:

- `text_scaler`: their text size. frus scales all text by it; see
  [Text and fonts](text.md#the-readers-text-size).
- `accessibility.disable_animations` (or `MediaQuery::reduce_motion()`): frus shortens its own
  animations to nothing; do the same with yours. See [Animation](animation.md).
- `accessibility.bold_text`, `high_contrast`, `invert_colors`, `accessible_navigation`: reported
  for the application to act on, for example with a high-contrast theme.

```rust
use frus::{component, FontWeight, Text, Widget};

fn hint() -> Box<dyn Widget> {
    Box::new(component(|_cx| {
        let media = frus::MediaQuery::of();
        let text = Text::new("Swipe left to archive");
        Box::new(if media.accessibility.bold_text { text.weight(FontWeight::Bold) } else { text })
    }))
}
```

Android reports all of these. The desktop and the web report reduced motion and the light or
dark preference; the others are off there.

An application written as an `Application` can give a high-contrast theme through
`high_contrast_theme` and `high_contrast_dark_theme`; frus picks it when the device asks for
more contrast.

## Checking it

- `frus run`, then `S`: prints the tree a screen reader sees, node by node.
- `T` steps the text size up, to see the screen as a reader with large text does.
- [DevTools](devtools.md) shows each widget's semantics beside it.
- In tests, `Tester::tap(label)` and `Tester::enter_text(label, text)` find a control by the
  name a screen reader hears, and act on it as a reader does, so a test that passes also
  proves the control is named. See [Testing](testing.md).
- On a device: turn on TalkBack, Narrator or VoiceOver and go through each screen once with the
  screen covered.
