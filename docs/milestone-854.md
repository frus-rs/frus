# Milestone 854 — Overlays see the page's scope, and `frus mcp` serves requests side by side

Two limits of LIMITATIONS.md: L-043 (shared state, whose "Not done" left overlays unchecked) and
L-053 (`frus mcp` answered one request at a time and ignored cancellations).

## Part 1: what an overlay sees of the values in scope

### What was wrong

Milestone 825 put values in scope for a subtree (`Scoped`), held by every walk that builds
components as it goes down. An overlay is not walked where it stands: the walk sets it aside
(the content, its anchor's box, its theme) and lays it out and paints it **after** the page,
above everything, when the walk has left the place it was declared and nothing is in scope any
more. A component inside it that read a `Model` found the application's (`provide`, or a
`Scoped` at the root) instead of the page's, or, with none, panicked:

```
no Model<Cart> above this component: put one in scope with `Scoped::new(model, child)` above it
```

A probe through the shell's frame, run on the code before the change, panicked for every
placement of the portal: below the anchor, centred (a dialog), beside it (a submenu), the
drawers on both sides, the bottom sheet, a panel covering its anchor, at the pointer, and a
notification. Only the components the walk before the layout reached got the right scope, by
chance of which walk built them first; a list built on demand inside an overlay builds its rows
while the overlay is painted, and never did.

Going through the kinds one by one found a second fault, which had nothing to do with scope. A
`Dialog`, an `AlertDialog`, a `CupertinoAlertDialog`, a `BottomSheet` and a `Drawer` keep their
panel outside their children (the children are the page behind; the panel is only what
`overlay()` returns), so no walk built the components inside the panel before the overlay was
laid out. The layout cache takes a fingerprint of the content first, and that walk read the
children of a component that had never been built: in a debug build, the assertion "a
component's children were read before it was built" panicked; in a release build the
fingerprint saw nothing below the component. Any component in a dialog's panel did this, a
reader of a model or not. The portal, the menus and the other overlays hold their content as a
child, which the walk before the layout reaches, so they did not.

### The decision

Every overlay of frus is declared in the tree, by the page that opens it: there is no route
stack to push a dialog onto (`Dialog::new(open)`, `BottomSheet::new(open)`,
`MenuAnchor::new(anchor, open, ..)`). So it sees what is in scope **where it is declared**, the
nearest value first, as it already saw that place's theme: an overlay portal's content is a
child of the place that declares it, for everything inherited. Routes are the other case. A
route pushed on a navigator is a sibling of the page that pushed it, so it sees what is above
the navigator and nothing between the navigator and the opener; frus has no route for an
overlay, so that rule applies where frus does have routes: **a page pushed on the router sees
what is above the router**, not what the page that pushed it put in scope. A router
inside a page (a shell's branch) is inside that page, and its pages see that page's scope.

Rejected: making overlays see only the root's scope, to mirror a dialog route literally. A frus
dialog is declared inside the page that owns its state; cutting it off from that page's models
would force every dialog's content to be handed its models by hand, the problem `Scoped` was
made to solve, and would disagree with the theme, which overlays already take from the place
they are declared.

### The change

The walk records, with each overlay it sets aside, what is in scope where it was declared
(`Scopes::capture`, nothing allocated where nothing is in scope), and `process_overlays` makes
that the whole scope for the overlay's layout and painting (`Scopes::restore`), putting back
what was there after it. An overlay declared inside an overlay is recorded while its parent is
walked, so it takes its parent's scope with its own. The selection bars record theirs the same
way.

Then, before anything is asked of the content, `process_overlays` runs the deferred builds over
it (`build_deferred_at`, under the overlay's theme and scope), the preparation a list's row
already gets (milestone 748). A component the walk before the layout already built this frame is
not built again; one in a dialog's panel is built there, once.

### How it was tested

`crates/frus-shell/src/overlay_scope_tests.rs`, through the shell's frame (`Driver`). Each test
opens one overlay kind on a page that puts its own cart in scope (0 items), under an
application that puts another cart in scope at the root (100 items). The overlay must show the
page's cart; a change to that cart (from outside the tree) must build each reader inside the
overlay once (a list's row is built on every frame the list paints, so only what it shows is
checked); and, where the overlay holds a button, a press inside it must change the page's cart,
seen by the page.

| Overlay | How it is opened | Readers inside | Press inside |
|---|---|---|---|
| `OverlayPortal`, every placement: `Below`, `Center`, `Beside`, `Left`, `Right`, `Bottom`, `Covering`, `Pointer`, `Notification` | always open | content, a list's row | yes |
| `Dialog` | `Dialog::new(true).child(..)` | content, row | yes |
| `AlertDialog` | `content_widget` | content, row | yes |
| `CupertinoAlertDialog` | `content_widget` | content, row | yes |
| `BottomSheet` | `sheet` | content, row | yes |
| `Drawer` | `panel` | content, row | yes |
| `MenuAnchor` (popover) | `content` | content, row | yes |
| `HoverCard` | held open (`open(true)`) | content, row | yes |
| `PopupMenuButton` | `item_widget` | a row's widget | |
| `ContextMenu` | `MenuItem::widget`, open at a point | a row's widget | |
| `MenuBar` | a menu open, `MenuItem::widget` | a row's widget | |
| `DropdownButton` | `DropdownOption::widget`, open | an option's widget | |
| `SearchAnchor` (search view) | `suggestion` | content, row | yes |
| `OpenContainer` | open | content, row | yes |

A dialog whose panel holds a component that reads nothing shows it (the second fault, alone).

Before the change: the probe of the portal's placements panicked at every one ("no
Model<Cart> above this component"); with the scope captured but before the deferred builds were
run over the content, the dialogs, the sheet and the drawer panicked on "a component's children
were read before it was built" (so did the router test, whose pushed page holds a dialog); the
menus, the dropdown and the search view saw the page's cart. Every test passes with both.

And a router: a page pushed from a page with its own cart sees the cart put around the router
(`FrusApp::builder`), not the pushing page's; a dialog on the pushed page sees that page's.

The overlays whose content is words only have no component to find a scope: `Tooltip` (its
message), `RichTooltip` (title, text, an action), the snack bar (`host::show_snack_bar`, shown
by the application's root), `FabMenu`, `CommandPalette`, `Autocomplete`, `DropdownMenu` and
`CupertinoActionSheet` (labels and messages). Their actions are closures or messages, which
capture the model they change. The bar over a text selection holds the application's items
when a `SelectionArea` says what it holds; it records its scope like every other overlay, but no
test opens one with a reader in it.

The guide (`docs/guide/managing-state.md`, "Dialogs, sheets, menus and pages") says all of
this, with a compiled example of a dialog reading the page's model, and how to carry a model
where it is not in scope: `Scoped::new(model.clone(), ..)` around what is shown, or a closure
that captures it. Nothing new was needed for that: a `Model` is a handle, every clone the same
model.

## Part 2: `frus mcp` serves requests side by side

### What was wrong

The server read a line, served it, and read the next: while `launch_app` waited for a build (up
to `wait_seconds`, five minutes by default), a `ping` waited too, and `notifications/cancelled`
was read only once the request it named had been answered.

### The change

- **A tool's call runs on a thread of its own.** The loop that reads standard input goes on
  reading; everything else (`initialize`, `ping`, the lists) is answered at once, in order.
  Every answer is written whole, one line, under the one lock that already guarded standard
  output (`report::line`).
- **The sessions are shared**: `Mutex<Vec<Arc<Session>>>`, and in a session, the child process
  and its input each behind their own lock and the command counter atomic. No lock is held
  while a request waits: a run being looked for, a build, a call of the application's DevTools.
  `launch_app` puts its runs in the list at once, so `app_status` and `get_logs` see them
  building.
- **Cancellation.** Each request in flight has a flag, found by its id when
  `notifications/cancelled` names it. Every wait of the server asks it at least ten times a
  second: the wait for a run's events, the retries of a call that finds the application
  restarting, the call of the DevTools itself (read in slices of 100 ms, so a cancelled call
  hangs up instead of waiting for its minute), and `analyze` / `run_tests`, whose `frus`
  process is ended. A cancelled request is not answered, as the protocol asks. A cancelled
  `launch_app` stops the runs it started rather than leaving them building behind the agent's
  back.
- **A run still building is ended at once** when stopped (by `stop_app`, a cancelled launch, or
  the client leaving): `frus run` reads no command until its application is up, so asking it to
  stop meant twenty seconds of waiting before it was killed anyway.
- **Ending a run ends what it started, on Linux and macOS too.** Stopping a run (or a cancelled
  `analyze` / `run_tests`) killed only the `frus` process outside Windows: cargo, its build
  scripts and compilers, or the hot-reload engine and its application, went on running. CI's
  Linux run of the launch test caught it (the build script outlived the cancelled launch), and
  it was reproduced under WSL before the fix. `frus run` and the commands are now started as
  the leaders of process groups of their own, and ending one freezes its tree from the top
  (`SIGSTOP`, so nothing in it starts anything more while it is walked with `pgrep -P`), then
  kills every process found and the group. The tree is followed and not only the group,
  because the hot-reload engine leads a group of its own. A run that is up is still asked to
  stop first (`app.stop`), which `frus run` answers by stopping its engine and application
  itself.
- When the client closes standard input, the requests still in flight are cancelled and waited
  for, then every run launched is stopped, as before.

### How it was tested

`crates/frus-cli/tests/mcp_stdio.rs` starts the `frus` program built by cargo, writes JSON-RPC to
its standard input and reads its standard output, a message a line:

Both fail on the server before the change (`mcp.rs` of master, the same test file): the
second request is not answered in twenty seconds.

- **A call that waits holds nothing up.** A fake application's DevTools (a TCP server in the
  test) answers `status` and `hello` at once and holds `tree` unanswered. After `attach_app`,
  `get_widget_tree` waits; meanwhile `app_status` (which itself calls the application) and a
  `ping` are answered. Then `notifications/cancelled` for the tree: the server hangs up on the
  application (the fake sees its connection closed), a later `ping` is answered, and the tree
  never is. Closing standard input ends the server cleanly.
- **`app_status` answers while `launch_app` builds, and a cancelled launch ends its build.** A
  throwaway application whose build script writes its process id and sleeps five minutes;
  `launch_app` on the desktop with `--no-hot` waits for it. `app_status` answers in well under
  ten seconds and says the run is building; after `notifications/cancelled` for the launch, the
  build script's process is gone, `app_status` says there is no app, and the launch is never
  answered.

The unit tests of `mcp.rs` (the handshake, every tool handled, an attached run driven through
its DevTools, the events of a run) pass as before.

## Checks

`cargo clippy --workspace --all-targets -D warnings`; the tests of frus-widgets, frus-shell
(689) and frus-cli (257 unit tests, the 2 stdio tests); the guide's examples
(`GuideManagingState`); the goldens under WSL (`cargo test -p frus-test --tests`), all
unchanged.

## Not done

- **Resources and prompts** are still not offered (the lists answer empty). They were optional
  here and nothing in the tools needs them.
- **Progress notifications** (`notifications/progress` while `launch_app` builds) are not sent.
- **A cancelled `hot_reload`** stops waiting, but the rebuild it asked `frus run` for goes on
  (that is `frus run`'s work, which it was told to do); the next `hot_reload` waits for it.
- **Overlays: no device run.** Everything is checked through the shell's frame without a
  window; the change has no platform code.
- **The selection bar with the application's own items** records its scope like every overlay,
  but no test opens one with a reader inside.
- **macOS**: the stdio tests ran on Windows, under WSL (Linux) and on CI's Linux; macOS only
  on CI. Ending a tree relies on `pgrep`, present on both.
