# Milestone 782 — TalkBack reads a frus window on Android

## Objective

On Android, a frus window was one surface that a screen reader could not see into. TalkBack
read nothing, so a blind user could not use a frus application at all. Desktop already had
AccessKit and the Web had its DOM bridge; Android was marked "partial" and had nothing.

The goal: TalkBack reads every widget a frus application shows, with the semantics its widgets
already declare and nothing asked of the application. Swiping should move through them, a
double tap should press, and fields, sliders and scrolling areas should answer TalkBack's own
actions.

## What the application writes

Nothing. Every widget that declares `semantics()` (buttons, check boxes, switches, tabs, fields,
sliders, progress bars, headings, scrolling areas) is read on Android as it is on desktop and
the Web. A password field is now marked as such everywhere:

```rust
// What TextField declares for itself; an application's own widget can do the same.
SemanticsProperties::new(Role::TextInput)
    .value(text)
    .obscured(true); // read as a password field: one dot per character, never the text
```

## Design

### Why a dex shim

TalkBack sees inside a view through its `AccessibilityNodeProvider`, which is an abstract class
to subclass. Pure Rust over JNI cannot define a class (see "Interfaces are free, classes cost a
dex" in [platform-integration.md](platform-integration.md)). So this is the second class in the
bundled dex, beside the keyboard bridge: `FrusAccessibility.java`, compiled by
`scripts/build-input-dex.sh` into `crates/frus-shell/assets/frus_input.dex`.

### The split: policy in Rust, a pipe in Java

- **`a11y_android.rs`** (pure Rust, no JNI, built and tested on every host) turns a frame's
  semantic nodes and scrolling areas into flat rows: one row per virtual view, with its parent,
  its bounds in the window's physical pixels, its Android class name (`android.widget.Button`,
  `CheckBox`, `EditText`, `SeekBar`, `ProgressBar`, `ScrollView`, `HorizontalScrollView`, an
  `ActionBar$Tab` for a tab, and so on), its text, content description and hint, its state flags
  and its action mask. It also works out which events a change of frame calls for: focus moved,
  a box ticked, a value moved, text typed (with the UTF-16 offsets Android counts in), an area
  scrolled, or the content changed. Finally it turns TalkBack's requests back into shell actions.
- **`android_accessibility.rs`** is the JNI half. It loads the class from the dex, sends each
  publication across as one call of parallel arrays, and queues requests that arrive on the UI
  thread for the next frame.
- **`FrusAccessibility.java`** is one view, as large as the content and added to the activity.
  It swaps each published tree in whole on the UI thread, so TalkBack never reads half of one,
  and builds `AccessibilityNodeInfo`s from the rows unchanged. It checks a request against the
  published tree before handing it back, so the answer TalkBack gets is true. The one piece of
  state it keeps is TalkBack's own accessibility focus, which belongs to the screen reader and
  never reaches the application.

### Cost when no screen reader is on

The Java side reports whether any accessibility service is on. While none is, the shell builds
nothing and sends nothing. A frame publishes only when its rows or its announcement differ from
the last ones published. When a service comes back on, the shell starts again from a whole tree.

### Ids

Virtual view ids are `i32`, while frus widget ids are `u64`. `VirtualIds` gives each widget a
small id, keeps it while the widget stays on screen, and never reuses one, even when the counter
wraps. This keeps TalkBack's focus on the same widget from frame to frame, and never moves it to
a stranger.

### What TalkBack's actions do

These are the same paths a pointer or a key takes:

| TalkBack | frus |
|---|---|
| Double tap (`ACTION_CLICK`) | A field takes the focus and the keyboard follows; anything else gets its message, as a tap does. Selection bar buttons run their editing action. |
| `ACTION_FOCUS` / `ACTION_CLEAR_FOCUS` | Input focus, shown as a key would show it. |
| `ACTION_SET_TEXT` | The field's own typing message, so a field that refuses typing refuses this too (shared with desktop, milestone 710). |
| `ACTION_SCROLL_FORWARD` / `BACKWARD` | A page of the area along its axis, eased, never past its ends. |
| Slider up/down | The slider's own Right/Left key handling. |
| `ACTION_SET_PROGRESS` | The slider's drag handling, at that fraction. |
| An application's announcement | `announceForAccessibility`, spoken once per change. |

### Passwords

`SemanticsProperties::obscured` is new in `frus-core`, and `TextField` sets it from its own
`obscure`. `readable_value()` gives one bullet per character. Android marks the node as a
password and reads only the dots; AccessKit now gives such a field the `PasswordInput` role and
the dots as its value. Before this, desktop handed the real password to the screen reader.

### Smaller changes

- The keyboard bridge view (`FrusTextBridge`), focusable for the IME's sake, is now
  `IMPORTANT_FOR_ACCESSIBILITY_NO`. Without that it would have been one more unnamed stop for
  TalkBack.
- `App::announce`, `activate_for_assistive_tech` and `set_value_for_assistive_tech` now include
  Android. iOS still falls into the no-op branch.

## How it was tested

**Unit tests** in `a11y_android.rs` (run on Windows; the module is compiled in tests on every
host) cover the following:

- class names per role;
- where labels, values and hints go;
- states and actions;
- password dots;
- slider actions;
- physical-pixel bounds;
- the focused node;
- id stability and wrap-around;
- nesting under scrolling areas, and a field that scrolls on its own;
- no events for an unchanged tree;
- each kind of change raising its event;
- UTF-16 offsets for text changes;
- request-to-action mapping, including a vanished node and an action not offered;
- page scrolls staying inside the area.

`frus-core` tests the new `obscured`/`readable_value`, including over a merge. `a11y.rs` tests
that AccessKit gets `PasswordInput` and dots.

**Android clippy**: `cargo clippy -p frus-shell --no-deps --target aarch64-linux-android -- -D
warnings` passes.

**On a phone** (Huawei STK-L21, Android 10, TalkBack 8.1, release APK of `frus-demo`):

- `uiautomator dump` shows the demo's tree as TalkBack receives it. Each node has its Android
  class (`Button`, `CheckBox`, `EditText`, `ProgressBar`, `HorizontalScrollView`,
  `ActionBar$Tab` "Tasks, Tab 1 of 3", selected), its label as content description ("Menu",
  "Light", "Add", "Delete task"), its states (checkable/checked on the segmented filter,
  selected on the current tab) and its bounds. The bounds match the pixels on a screenshot
  taken at the same time: "Add" is `[765,816][959,938]`, exactly the green button.
- With the keyboard up, the tree follows the shorter window. Nodes are clipped at the keyboard,
  and a `ScrollView` node appears because the content now scrolls.
- With TalkBack on, TalkBack's own service dump records its first focus as frus's "Menu" button
  (`FIRST_FOCUSABLE_NODE`, package `com.frus.demo`). It then moved its focus to the check box
  that held frus's input focus (`FOCUS_SYNCHRONIZATION`). So TalkBack reads the provider's
  tree, and the input-focus flag reaches it.

## Not done

- **TalkBack gestures were not driven on the phone.** On this phone, touches injected from adb
  (`input swipe`, `monkey` scripts) bypass TalkBack's touch exploration and reach the app as
  plain touches: in the Settings app as a control, a "swipe" opened a row instead of moving
  focus. `sendevent` is refused by SELinux. So swipe navigation, double-tap activation, and
  TalkBack's set-text, scroll and slider actions are proven by unit tests only, not by a hand
  on a device. That needs a person holding the phone.
- **No green focus box.** TalkBack's accessibility focus is held in the Java view, but nothing
  draws it. Under `NativeActivity` the view tree is never drawn, because the native side owns the
  surface. So the outline TalkBack draws for ordinary views does not appear over a frus window.
  frus would need to be told the accessibility-focused node and paint the outline itself.
- **No explore by touch.** Dragging a finger over the screen to hear what is under it is not
  answered (no hover hit-testing to virtual views). Swiping and the reading controls are the
  only ways to move.
- **Gaps in the demo's tree, seen on the phone.** The "Tip" banner's text does not appear in the
  tree, and the "Mark active"/"Mark done" actions read as plain text, not buttons. These come
  from those widgets' declared semantics, not from the bridge, and were not fixed here.
- **iOS** still has no accessibility bridge.
