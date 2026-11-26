# Milestone 848 — The accessibility tree has a shape

## Objective

[L-040](../LIMITATIONS.md#l-040-the-semantics-tree-is-flat): every annotated widget was a child
of the window. A screen reader could not step into a card as a unit or over a list as a whole, a
list's items did not know they were items, let alone "3 of 12", a tree view's rows had no level,
a heading had no level, and on the web a dialog's content was not inside the dialog
([L-041](../LIMITATIONS.md#l-041-web-accessibility-has-not-been-heard-through-a-screen-reader)).
This milestone gives the tree its shape on every platform, and feeds it to the three bridges:
AccessKit on the desktop, the virtual view hierarchy on Android, the element layer on the web.

## Design

### A span per node, not a tree of nodes

The walk already collected the frame's nodes in reading order, and everything the walk does to
that list stays as it was: a barrier cuts a subtree off at its start, a cached subtree is copied
out and spliced back elsewhere, a merging `Semantics` drains its subtree into one node, a tappable
area takes its words into its name. A tree of boxed nodes would have had to re-learn every one of
those.

So the list stays, and each node gains **its span**: how many of the nodes after it are its
descendants. A pre-order list with a span per node is a tree, and it stays one under every
operation above:

- a cut at a subtree's start takes whole subtrees with it;
- a cached subtree's spans only count inside it, so it can be replayed anywhere;
- a merged subtree leaves a node with no span;
- a span is set when a container **closes**, once the walk has left its subtree and the number
  is known, so nothing is patched while the subtree is still being walked.

`Ui::semantics()` is unchanged (every test and tool that reads it still does), and
`Ui::semantics_spans()` / `Ui::semantics_parents()` give the shape. The buffer is
`ui/semtree.rs`: one `Vec<u32>` beside the nodes, filled as they are pushed, and no allocation per
node.

### Who holds what

A node holds its subtree when its **role** is one that holds others (`Role::is_container`): a
list, a list item, a group, a tab list, a toolbar, a navigation landmark, a scroll view, a dialog,
a tree. A button, a checkbox, a text do not: what is inside them is part of them, and on the web a
button's content is not even read as elements of its own.

New roles: `List`, `Group`, `TabList`, `Toolbar`, `Navigation`, `ScrollView`. New properties on
`SemanticsProperties`: `level` (a heading's, or a tree item's depth), `position`
(`(position, count)`, "3 of 5"), and `scroll` (`SemanticsScroll`: offset and largest offset across
and down).

| Widget | Node |
|---|---|
| `ListView` | `List`, with where it rests; every row an item holding what the row shows, numbered among **all** rows, built or not ("1 of 5000"). `ListView::semantic_list(false)` makes it only a scrolling area. |
| `ReorderableList` | `List`, its rows `ListItem`s, numbered when the list closes |
| `SingleChildScrollView`, `PageView`, `CustomScrollView` | `ScrollView` holding its content, with where it rests, **when it scrolls**; one whose content fits adds nothing |
| `Card` | `Group` holding its words and controls; `Card::semantic_container(false)` adds nothing |
| `Dialog` / `AlertDialog` surface, `CupertinoAlertDialog`, the command palette | `Dialog` holding its content |
| `AppBar` | `Toolbar` holding its title (a heading) and actions |
| `TabBar`'s strip, `CupertinoTabBar` | `TabList`, its tabs numbered |
| `NavigationRail`, the bottom navigation bar, `NavigationDrawer` | `Navigation` |
| `FileTree` | `Tree` holding its rows, each with its level and its place among its siblings |
| `Text::heading_level(n)`, `Semantics::level(n)` | a heading of level `n` |
| `Semantics::new(..)` (not merging) with a role that holds, or no role and a name | the parent of its subtree's nodes, a section a reader steps into |

A `Semantics` that names **one control** (a button, a heading) stands beside what it annotates, as
it always did: making every annotation a parent would have changed the nodes of every widget built
on one.

### The rules kept

- **An empty, silent container goes.** A group, a list item, a scroll view, a toolbar, a
  navigation or a tab list that holds nothing and says nothing is dropped when it closes: an empty
  card is nothing a reader should stop on.
- **A tappable area is still named by its words.** A card inside a `GestureDetector` used to read
  as one button named after the card's text. With cards as groups it would have become an unnamed
  button beside a group, so an unnamed group inside a tappable area is lifted out of the way first.
  A list or a named group inside one keeps what it holds.
- **Merging speaks for everything inside**, containers included: their words and states join the
  merged node, their role does not.
- **Focus order, actions, live regions** read the same flat list as before and are unchanged.

### Desktop: AccessKit

Each node's AccessKit node lists its children; the window lists the top-level ones. A level becomes
`level`, a place `position_in_set` / `size_of_set`, a scroll position `scroll_x`/`scroll_y` with
their bounds, and a list or a scroll view that can move offers `ScrollDown`/`ScrollUp` (or
`ScrollRight`/`ScrollLeft`), which the shell carries out as a page of scrolling, the move Android's
reader already had. `SemanticsScroll::paged` is now the one place that move is computed.

### Android: the virtual view hierarchy

The Java half already built children from each row's parent, so the hierarchy needed no Java:
a row's parent is its semantic parent's row, and the scrolling areas the bridge inserted from
geometry are inserted **between** a node and its parent only when they are not already around the
parent. A list or a scroll view that says where it rests is now a scrolling row itself (class,
flag, scroll offsets, forward/backward actions). New: each row carries `[rows, index]`, and
`FrusAccessibility` sets `CollectionInfo` on a list and `CollectionItemInfo` on an item, which is
what TalkBack reads as "in list, 12 items" and "3 of 12". That is a Java change: the dex was rebuilt
with `scripts/build-input-dex.sh`.

### Web: nested elements

Each element is inside its parent's element: `role="list"` holds `role="listitem"`s, each holding
its checkbox; a `role="dialog" aria-modal="true"` holds its content; a `role="tablist"` its tabs.
`aria-level` on headings and tree items, `aria-posinset` / `aria-setsize` on anything numbered. An
element is placed relative to its parent's box, and one that holds others lets them show past its
edges (a scroll view clips; its rows may not fit inside its box). A node that can be pressed and
holds others is a `div` with the same click listener, not a `<button>`, whose content a browser
does not expose.

The update stays diff-based: the order and parent of every node is compared with last frame's,
and the elements are moved only when one changed, walked backwards so each element's next sibling
is already in place. A frame that changes nothing writes nothing (checked: zero mutations over 30
hover frames).

## How it was tested

- `frus-core`: the container roles and the items they number, `over` keeping a level and a place,
  `SemanticsScroll` (axis, forward/backward, `paged`, doctest).
- `frus-widgets/src/ui/semtree.rs`: spans to parents, a container closing over what follows it and
  numbering its items, an empty group dropped, an item wrapped once, unnamed groups lifted out.
- `frus-widgets/src/semantics.rs`, through `build_ui`: a named section holds its content; a
  `ListView` of 50 rows is a list holding numbered items (each holding its checkbox and caption),
  with its scroll extent, and with `semantic_list(false)` a scroll view of labels; a card groups a
  text and a button, adds nothing when told not to, and pressed as a whole is still one button
  named by its words; heading levels from `Text` and from `Semantics`; a scroll view holds its
  content only when it scrolls. The existing merge tests are unchanged and pass.
- `frus-shell/src/a11y.rs` (AccessKit, desktop): the root holds a heading, a list and a button;
  the list holds two items, an item its checkbox; places, a level, the scroll position and the
  scroll actions; the scroll requests forwarded.
- `frus-shell/src/a11y_android.rs`: the existing tree tests, now with parents.
- `frus-shell/src/a11y_web.rs`: levels, places, container roles, a pressable card that holds
  others is not a `<button>`, placement relative to the parent.
- `scripts/web-accessibility-check.py` (headless Edge, Chromium's computed accessibility tree),
  new checks: the new task's checkbox is inside a list item inside a list, the item says its
  place, the row's delete button is in the same item; the `Tasks` destination is inside the
  navigation landmark; the settings tabs are in a tablist; the journal's `Row 1` is in an item
  that is "1 of 5000"; the Cupertino screen's section heading is level 2; its alert is a modal
  dialog holding its buttons.
- `scripts/web-accessibility-firefox-check.py`: the tabs' elements are inside the tablist's.

## Results

- **Unit tests** (Windows, `--lib`): `frus-core` 287 passed; `frus-shell` 663 passed, 2 ignored;
  `frus-widgets` 2596 passed. The last was built at opt-level 0, where
  `a_deep_tree_builds_on_a_platform_s_smallest_stack` overflows its 1 MB thread: its frames are
  sized for the workspace's opt-level 1, which CI builds. That test was skipped locally and is
  left to CI; an opt-level 1 build of this branch before the last two fixes passed it, and both
  fixes keep their work out of the recursive walk's frame (`#[inline(never)]`).
- **Clippy** `-D warnings`: native (`--all-targets`, core, widgets, shell, demo) and
  `wasm32-unknown-unknown` clean. `aarch64-linux-android --no-deps` clean apart from
  `clippy::missing_const_for_thread_local` on `frus-core/src/text_style.rs:776`, a file this
  milestone does not touch (the initializer is already `const`), so it was allowed for the run.
- **Headless Edge** (`scripts/web-accessibility-check.py`, demo built for the web): every check
  passes. The computed tree has the new task's checkbox under `listitem` > `list`, the item
  saying `1 of 1` with its delete button in the same item; `Tasks` inside the navigation
  landmark; the settings tabs in a `tablist`; the journal's `Row 1` in an item that is `1 of
  5000`; 0 mutations of the layer over 30 hover frames.
- **Two bugs the browser found**, both from nesting, both fixed:
  - Enter on a focused checkbox no longer ticked it. The key bubbled to its list item's own
    listener, which sent it to the shell and cancelled the browser's click. A key is now routed
    only by the focused element's own listener.
  - A click on a button inside a pressable card would have bubbled into a second activation of
    the card. The innermost pressable answers a click and marks it handled.
  - The script's reading-order check compared places in `getFullAXTree`'s list, which is not
    reading order once elements nest; it now walks the tree depth-first.
- **The Cupertino checks were skipped, not passed**: on the web the demo's Cupertino screen lays
  out its scrolling body with no height (the canvas paints none of it either), so its section
  headings and its alert are not on the page. No layout code changed in this milestone. The
  script says `SKIP` when the body is absent.
- **Firefox** (`scripts/web-accessibility-firefox-check.py`) did not start the application in
  this session (no layer at all), so it says nothing either way.

## Not done

- **Android was compiled and unit-tested only.** No phone or emulator was used: the hierarchy,
  the scrolling rows and the collection info have not been heard through TalkBack, and the
  rebuilt dex's `CollectionInfo` calls were not run on a device.
- **No screen reader was listened to** on any platform (L-041 stays partial).
- **The Cupertino screen's body has no height on the web** (above), so a heading's
  `aria-level` and a modal dialog holding its buttons were not seen in a browser; the unit
  tests cover both.
- **Firefox was not checked** (above).
- **Not every container is one yet.** `BottomNavigationBar`, `NavigationBar`, `Drawer`, the
  bottom and side sheets, `ExpansionPanel`, `DataTable` (a table, with rows and cells) and
  `GridView` (a grid: `CollectionInfo` with columns) say nothing of their shape. A `Column` of
  rows marked `Role::ListItem` by the caller becomes a list only inside a `Semantics` with
  `Role::List`.
- **Tabs and radio groups say their place on the web through `aria-posinset`**; AccessKit is
  given the same numbers, but Android reads only a list's and its items' (`CollectionInfo`),
  not a tab bar's.
- **A scroll position on the web** has no ARIA attribute; the browser's own scrolling of the
  layer is clipped off on purpose, so a reader's "scroll" on the web is still the shell's.
