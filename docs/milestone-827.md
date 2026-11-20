# Milestone 827 — More windows than one

Issue #218: a frus application had exactly one window. The shell held a single
`window: Option<Arc<Window>>` and `window_event` ignored the `WindowId` it was given.
Desktop applications need more: a document per window, a detached panel, an inspector, a
settings window, a picture-in-picture player.

The issue asked for the API's shape to be discussed before any shell work. That discussion is
the first part of this note; with nobody else to hold it with, it ends in a choice, and the
rest of the note is that choice built.

## The API: three shapes weighed

Whatever the shape, two things were fixed by how frus already works. A window is **a value
the state describes**, not a handle kept by the application (the tray, milestone 706, the
taskbar button, 708, and the global shortcuts, 709, are declared the same way and the shell
diffs them). And the main window stays special: it is the one `view` builds, the one the
router, restoration, the title bar line and the window memory belong to, and closing it ends
the application.

### A. One view returning several windows

`fn view(&self, theme) -> Windows<Msg>`, a list of `(id, attributes, tree)`.

- For: one function says everything on screen; a window cannot be listed without content.
- Against: it changes the signature every application implements, so every existing
  application changes — the issue requires that single-window applications do not. A default
  method wrapping the old `view` would keep them compiling, but then there are two ways to
  write the main window.
- Against: every window's tree is built whenever any window is. The shell rebuilds a window
  when its frame comes; one view building all of them would build the inspector's tree for a
  frame of the editor, and throw it away.
- Against: the attributes (title, size) and the tree are produced together, so finding out
  which windows exist means building them all.

### B. One view per window, the windows listed apart (chosen)

`fn windows(&self) -> Vec<Window<Msg>>` lists the windows besides the main one, as values:
an id, a title, an opening size, a minimum size and the message its close button sends.
`fn window_view(&self, id, theme) -> Box<dyn Widget<Msg>>` builds one window's interface.
`view` keeps building the main window.

- For: nothing changes for an existing application: both methods have defaults (no windows,
  an empty box).
- For: listing windows is cheap and separate from building them: the shell reads the list
  after every turn of the loop, compares it with the windows it has, and opens, closes and
  retitles; each window is built only for its own frames.
- For: it is the same shape as `tray`, `taskbar` and `global_shortcuts`, which the shell
  already diffs from the state.
- Against: the id ties the two methods together, and a typo makes an empty window. The id
  is a string the application chooses; `window_view` is handed it back, so a `match` over
  the ids the application lists is the usual body.

### C. Imperative handles

`Command::open_window(attributes) -> WindowHandle`, `Command::close_window(handle)`.

- For: familiar from most desktop toolkits.
- Against: the window is then state the shell holds and the application must mirror; a
  restart (hot reload, restoration) loses the mirror. The issue asks for windows "opened,
  closed and titled from the application's state, not through imperative calls scattered
  around". Rejected.

### The close button

A window's close button does not close it. It sends the message the window was listed with,
and the window goes when the state stops listing it. That is what "the state owns the window"
means, and it lets an application ask before closing (unsaved changes) by not taking the
window out. The message is required (`Window::new(id, on_close)`), because a window whose
close button did nothing would be a trap.

### Component applications

`FrusApp` gets the same two pieces as builders: `.windows(|| ...)` and
`.window_view(|cx, id| ...)`. `window_view` is a function of the build context, as
`FrusApp::from_fn`'s root is, so hooks can be called in it; its components keep their state
for as long as the window stays open. The demo opens its second window this way.

## What the shell does

### Per-window state

The shell (`crates/frus-shell/src/app.rs`, 20 000 lines) was written against one window. Its
127 fields were sorted into two:

- **The shell's** (56): the application, the renderer, the clipboard, the modifiers, the
  tray, the taskbar button, the global shortcuts, the subscriptions, the platform settings,
  the lifecycle, the development channels.
- **Each window's** (71), now `WindowState`: the winit window and its GPU surface, the
  retained tree and the laid-out interface, the `Runtime` (focus, scroll positions, overlays,
  component states, animations), the theme fade, the AccessKit adapter and the announcements,
  the gestures and drags under way, the caret, the pending focus and scroll requests, the
  insets, the window memory and the title bar line.

`App` holds the window in front of it as `win: Box<WindowState>` and the others in
`parked`. Every input and frame method kept its body and reads `self.win.<field>` where it
read `self.<field>` (a mechanical rewrite, which is most of the diff). An event for another
window swaps that window's state in, which moves two pointers; nothing is copied. Work every
window has at each turn of the loop — its timers, its accessibility actions, its decoration
frames, the wait it asks the loop for — is run for each in turn (`each_window`), and the loop
waits for whichever window needs it first. An application with one window never swaps.

### Events routed by `WindowId`

`window_event` starts by bringing the window it is for in front; an event for a window
already closed is dropped. `CloseRequested` on another window sends that window's message.
On the main window it does what it did: it ends the application (or hides the window, with a
tray that keeps running).

### One device, a surface per window

`frus-gpu` gains `WindowSurface`. `Renderer::add_surface(window, w, h)` makes one from the
renderer's own wgpu instance and adapter; `Renderer::render_to(&mut surface, scene)` draws a
frame into it; `resize_surface` and `reconfigure_surface` follow the window. The device, the
queue, the pipelines, the glyph atlas and the image textures are the renderer's, shared:
opening a second window builds no pipeline and uploads no image twice.

What depends on a surface's size stays with that surface: the multisampled scratch targets,
the staging texture and the cached layers (`SurfaceTargets`) are swapped into the painters for
that window's frame. Without that, drawing two windows of different sizes alternately would
let each other's targets go at the end of every frame and make them again at the next.

The other windows' surfaces are opaque even when the main one is see-through (milestone 643,
the title bar line on Windows); the title bar line is the main window's alone. A lost device
makes a new renderer on the main window, then new surfaces on the others.

### Messages, rebuilds and timers across windows

- A message from any window goes to `update`; every window is then built again (or, for a
  component application, every window is drawn again and builds what the message marked).
- A rebuild asked of the framework (`request_rebuild`, a component's `set_state`, a task
  that finished, an image that landed) is taken by whichever window draws first; that window
  now marks the others, so a component in the second window that a callback in the first
  changed is built again.
- A component's `use_interval` registered its timer in a list each build starts afresh: a
  build of the second window dropped the first window's timers. Each window now keeps the
  timers of its latest build, and the subscription reads every window's
  (`frus_widgets::__set_intervals`).
- The application's own animations (`Application::tick`, a router's transition) are advanced
  by the time since any window last advanced them: with two windows drawing they would
  otherwise run twice as fast.

### What stays the main window's

Restoration keeps the main window's interface. The tray, the taskbar button, the global
shortcuts, a notification's raise and the single-instance handoff act on the main window
wherever the shell's attention is. The window memory (milestone 783) remembers the main
window. On exit, the main window is brought in front before its placement is kept.

## Android and the web

**Not supported, said so.** `Application::windows` is not read for opening there: an
Android activity has one surface, and a browser tab is a page of its own (a second tab would
be a second copy of the application, with its own state, which is not what a window listed
from this state means). The first time an application lists a window on these platforms, the
log says once that it is not opened. The rustdoc of `Application::windows` and
`FrusApp::windows` says the same.

Android's own multi-window mode (split screen, freeform) resizes the one activity, which
frus already follows; a second activity for a second window is left for later.

## Cost

- **A single-window application** pays one call to `Application::windows` per turn of the
  loop on the desktops (an empty `Vec`, no allocation), one `Box` indirection for the window
  state, and a check that `parked` is empty in the few places that look at other windows. No
  frame does more work. Nothing is read on a turn the loop does not take, so the idle cost is
  unchanged.
- **Binary size**: `ci/size-budget.toml` is unchanged. On Android (what the budget measures)
  the new code is the window list's warning and the per-window state's `Box`; the window
  opening code is desktop-only. The CI size job measures it.

## How it was tested

- `window.rs`: the diff between the windows open and the windows listed — a window newly
  listed opens, one no longer listed closes, a changed title is set, a window without a title
  takes the application's, a window listed twice opens once.
- `app/windows.rs`: the loop waits for whichever window needs it first.
- `frus-gpu`: its tests pass; the surface path is exercised by the real run below. The
  goldens were not run: they render through `render_offscreen` and `Painters::render`, which
  this change does not touch (the only compositor change is `swap_targets`, used by
  `render_to` alone).
- Clippy with `-D warnings` for the workspace, for `wasm32-unknown-unknown` and for
  `aarch64-linux-android`; rustdoc with `-D warnings`; the eight `frus-examples` projects and
  the gallery's wasm build.
- The whole frus-shell suite, which drives the single-window shell through `testing::Driver`,
  passes unchanged: the per-window refactor kept every behaviour.
- **On Windows, for real** (Intel Iris Xe, DirectX 12): the demo, with its data folder
  pointed at a scratch copy. Its windows were found by process id and title, and every click
  was real pointer input at the centre of an element UI Automation found inside the window
  meant, refused unless that point was on that window. In order:
  1. Stats, then "Open the summary in a window": a second window, "frus — Task summary",
     opened beside the first (log: `window "summary" opened`).
  2. A click in the second window's field and typed text with Enter: "Bought milk" was added;
     the second window listed three tasks, and the main window's Tasks badge went from 2 to 3.
  3. The checkbox of "Write the report" clicked in the second window: ticked in both windows
     (their accessibility trees read `On` in each), the badge down to 2. Clicked again in the
     main window: unticked in both.
  4. "Close this window" in the second window: it closed (`window "summary" closed`), and
     the main window went on taking clicks.
  5. Opened again, then closed by the system's close (UI Automation's `WindowPattern.Close`,
     which is the title bar's button): closed, the main window still running and responding.
  Each window answered the clicks meant for it with its own focus: the field focused in the
  second window took the typing while the main one kept its own state.

## Not done

- **Cross-window drag and drop**: dragging a tab out into a new window, dropping between
  windows. A drag lives in the window it started in.
- **The single-instance link handoff opening a window** (milestone 563): a link from a second
  process still goes to the main window's location.
- **macOS and Linux were not run**: the code is the same winit path as Windows and compiles
  for them, but the issue's "done when" names all three desktops; only Windows was exercised.
- **Overlays opened by the component host** (`host::show_dialog`, snack bars, toasts) belong
  to the main window: a dialog asked for from the second window's component shows in the main
  one.
- **Wakes from other threads** (`set_local_wake`, an external texture's new frame) ask the
  main window for a frame; the main window's frame runs the woken tasks and marks the other
  windows, but a video playing in a second window is not redrawn by its own frames alone.
- **The title bar line** (milestone 640) and **window memory** (783) are the main window's;
  other windows have the system's caption and open where the system puts them.
- **Android multi-window as several activities**, and **web popups**: not supported.
