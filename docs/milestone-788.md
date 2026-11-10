# Milestone 788 — DevTools, launched from `frus run`

## Objective

A developer looking at a running application wants to see what is on screen as the framework
sees it: which widget drew this, how big it is and why, what it tells a screen reader, which
frames are slow, and the same motion ten times slower. `frus run` had keys for parts of this
(milestone 668: `w` prints the tree, `i` a hover card, `P` an overlay), each printing to the
terminal or painting over the application. This milestone adds **DevTools**: one place, in the
browser, with the live widget tree, a widget's details, a frame chart, the switches, and the
log; opened with `v`, or **frus: Open DevTools** in VS Code.

It is also the base of the next milestone, an MCP server through which an AI agent finds
widgets, taps, scrolls, types, takes screenshots and reads the log. So the work is first a
**debugging service in the application and a protocol**, and the page is one client of it.

## Design

### Where DevTools runs: a page served by `frus run`

The page is a single HTML file compiled into `frus` (`crates/frus-cli/src/devtools.html`),
served by `frus run` on `127.0.0.1` (port 9100 when free, any other otherwise). The alternative
was a DevTools application written with frus and started as a second process. It was weighed
and set aside:

- **It works the same for every target.** The page runs on the developer's machine whatever
  the application runs on: a desktop window, an Android phone, a browser tab. A frus DevTools
  application would need a desktop to run on and a GPU, and could not follow a phone's
  application any better than a page does.
- **No build.** A second frus program would have to be compiled the first time it is opened
  (minutes) or shipped as a binary for every host; the page is 30 KB inside `frus` itself, and
  `frus` has no dependencies (milestone 652's rule for the CLI holds).
- **Editors embed pages.** VS Code opens it with one call; a later milestone can show it in a
  webview without changing it.

Dogfooding is the cost: the page is plain HTML and JavaScript, not frus widgets.

### One service in the application, one protocol

The application answers requests (`crates/frus-shell/src/devtools.rs` and
`crates/frus-shell/src/app/devtools.rs`), compiled only with `debug_assertions`: a release
build has none of it, not even the socket. A request is one line of JSON,
`{"id":7,"method":"tree","params":{}}`, answered by one line, `{"id":7,"result":…}`. It is
queued, the window is woken, and the frame answers it **after its layout**, so the tree it
describes is the one on screen; a frame with a request waiting collects the inspector's nodes,
the same collection F12 uses.

`frus run` is the hub (`crates/frus-cli/src/devtools.rs`). It serves the page and `POST /api`,
answers what it knows itself (the log, a reload, the Android device's screenshot) and relays
the rest to the application:

- **desktop**: the socket the application already reports to `frus run` on (milestone 668),
  with lines marked `devtools ` both ways;
- **web**: the page's command queue, which the script `frus run` adds already polls; the page
  posts the answer to `/__frus/devtools`;
- **Android**: a local socket in the abstract namespace, `frus_devtools_<package>`, opened by
  the application and reached with `adb forward tcp:0 localabstract:…`. The existing channel —
  commands written with `run-as` into a file the application polls, reports read from logcat —
  cannot carry a tree of a few hundred kilobytes or a screenshot. A TCP socket would need the
  `INTERNET` permission, which an application must not have to declare to be debugged; a Unix
  socket needs none. The forward is removed when the run ends.

`frus run` keeps the same address across reloads and restarts: the page goes on working while
the application is rebuilt.

The protocol is written down in `docs/guide/devtools.md`. Its methods: `hello`, `tree`,
`node`, `find`, `at`, `select`, `tap`, `scroll`, `type`, `setText`, `key`, `back`,
`screenshot`, `frames`, `state`, `set` (the application's) and `status`, `logs`, `reload`,
`restart` (`frus run`'s). The automation methods reuse the paths a test's `Driver` drives:
`tap` is a pointer down and up through `pointer_event`; `type` presses each character through
the shell's key handling (`press_stroke`, moved out of the driver so both share it); `setText`
is the assistive technology's set-value; `scroll` is the wheel's scroll at a point
(`wheel_scroll`, moved out of the window event); `find` matches the words a widget painted
(the scene's text primitives, by owner), its semantics label, its type, its key or its id.

### Security

The page and the API answer only a request carrying the run's token — the `token` of the
printed address — in an `X-Frus-DevTools` header or the query. A page from another site open in
the developer's browser could otherwise post to `127.0.0.1:9100` and tap through the
application; it cannot read the token, and a custom header is not sent across sites without a
preflight the server never grants. On Android the abstract socket can be reached by any process
on the device; it exists only in a debug build started by `frus run`.

### Inspector data

`InspectorNode` now carries the widget's `key` and its layout `style` (sizes and their bounds,
padding, margin, flex factors, alignment). The parent of each node is recovered from the paint
order and the depths. The constraints a widget was given are not recorded by the layout and are
not shown (see "Not done").

### Highlight and select mode

The selected widget is painted over the application with `paint_highlight`, the hover card of
the inspector made a function of its own. In select mode the window's pointer events stop at
DevTools: the widget under the pointer is highlighted as it moves, and a press selects it; the
page follows the selection.

### Frames

The frame statistics of milestone 609 already timed each stage when measuring. DevTools turns
measuring on with its first `frames` call and keeps the last 600 frames (number, interval,
build, layout and paint, render with its acquire and draw parts, GPU, and what asked for the
next frame). The page polls them twice a second and draws a bar per frame against the 60 Hz
budget. A frame is marked janky when its own work (build, layout and paint, drawing) is over
the budget, or when it came more than one and a half frames after the one before within a run
of frames. The first chart counted the whole render, which includes waiting for the display,
and marked every frame of a debug build red; the wait is drawn faint instead. The frames per
second are counted over runs of frames, not across the pauses of an idle application.

### A window behind another one

The first real run answered nothing: the application's window was behind the terminal, and an
occluded window draws no frame, so the requests waited for a frame that never came. An
occluded window now runs the frame when a request of DevTools' is waiting, and a minimised one
answers from the last frame laid out. An agent driving the application must not need its
window in front.

## Second round: what the first one left

The first version shipped with a list of what it did not do. This round does it.

### Constraints

A box-protocol layout hands each child a minimum and maximum width and height; frus lays out
with flexbox (taffy), where a child is not handed bounds but is laid out in the room its
container's content box gives it, then sized by its own size, minimum and maximum style. That
room is what DevTools shows as the widget's **constraints**: `availableWidth`/`availableHeight`
(the parent's box less its padding, unbounded along an axis the parent scrolls) and the
`min`/`max` its style resolves to in it. They are worked out from what the inspector already
collects (each node's box and style, and the frame's scroll areas), at the moment `node` is
asked, so a frame pays nothing for them.

Recording the bounds inside the layout itself was weighed and left: the layout runs taffy's
high-level tree, whose recursion over the nodes is internal and only calls back for measured
leaves (text, images). Recording what each node received would mean driving taffy through its
low-level traits — replacing the layout's core for a debugging aid — and for every flex child
the value recorded would be the same content box shown here. What this does not show is the
min-content and max-content probes taffy makes while sizing; it shows the final pass.

### Rebuild counts

`frus-widgets` counts each component's builds (debug builds only; `note` is empty in a
release): `rebuild_count(id)`, `rebuild_counts(limit)` and `take_frame_rebuilds()`. The
inspector shows the count beside a component and in its details; each frame record carries
how many components were built for it; the performance view lists the components built most.
The root `view` of an Elm-style application is not a component and is not counted: the frame
record's `build` (its time, or `null` when the frame reused the tree) says when it ran.

### An idle application stays idle

The first version answered every request on the next frame, so the performance view's polling
woke two frames a second. `frames`, `state` and `rebuilds` are now answered on the thread the
request arrives on, from what the last frame left (a frame publishes the switches once
DevTools has spoken to the application). An idle application draws nothing while DevTools
polls it, and the chart shows only frames the application drew for itself. Nothing of
DevTools runs in an application nobody has opened DevTools on, beyond the socket waiting to be
connected to (Android) — no thread polls, no frame is drawn.

### Repaint rainbow

frus draws a whole frame each time it draws; there are no compositor layers to colour. What it
does skip is painting: a repaint boundary (`Widget::repaint_boundary`) replays its last output
when nothing in it changed. The rainbow outlines the frame in a colour that moves on with each
frame drawn, and each repaint boundary in a colour that moves on each time it is painted
rather than replayed: a boundary whose colour stays put was reused. `PaintCache` records the
boundaries of each frame for it (`last_frame_boundaries`). An application without repaint
boundaries shows the frame's border only, which is the truth: everything was painted again.

### The log without hot reload

A desktop debug build's output is now read by `frus run` (it was handed the terminal), printed
as before and kept for DevTools. Its colours survive: the run asks for them with
`RUST_LOG_STYLE=always` when its own error output is a terminal, turns on escape sequences in
the Windows console, and DevTools gets the lines without them. The shell's logger now reads
`RUST_LOG_STYLE`.

### Several devices

With `frus run -d all`, each device has a run of its own, each with its DevTools on a port of
its own (the first gets 9100, the others whatever port is free). Each run's address is printed
under its device's tag, and `v` opens every one, named by its device.

### Android Studio

**Tools > frus > Open DevTools** opens the address of the run, which the plugin reads from the
run's `app.devTools` event; the console says the address too.

### Typing on Android

`type` pressed each character as a key, through the shell's handling of a key. On Android,
with the input bridge installed, that path is closed to typing on purpose — editing goes
through the input connection alone, or each keystroke would arrive twice — so the characters
were dropped while `setText` worked (found by the MCP milestone, on the phone). `type` now
writes into the focused text field through the field's own editing, as an input method commits
text, on every platform; with no field focused it still presses keys.

### Hot reload

`frus run` with hot reload (the default of a debug build) was run on Windows with DevTools:
the engine starts the application with `FRUS_DEV` inherited, so it reports on the same socket
and DevTools answers as without hot reload; a patch keeps the process and DevTools sees the
patched widgets; a restart (`R`, or DevTools' `restart`) reconnects the new process on the
same address. Nothing needed fixing there.

## How it was tested

- **Unit tests.** `frus-shell`: requests read and answered, ids round-trip, each node's parent
  recovered from the paint order, the tree's words, keys and boxes, finding by words (whole and
  contained), type, key and id, a widget's details (style, ancestors), key names, base64, the
  frame records (kept, numbered, listed after a number), the request queue's order.
  `frus-cli`: a call relayed with the hub's id and answered without it, an error relayed, the
  hub's own `status`, an application that never answers, the log kept and listed after a
  number, the token (page and API refused without it, served with it), tokens differ; the key
  map (`v` is DevTools). VS Code: the adapter keeps `app.devTools`'s address, says it in the
  console, sends `frus.devTools` and answers the `devTools` request with it.
- **Windows, for real.** A project made by `frus create` with `--frus-path` on this branch,
  `frus run --no-hot`: the address printed with the keys; over `curl`, `status`, `hello`,
  `tree` (17 widgets), `find` by label and by words, `tap` on "+" twice (the count read 2 through
  `find`), `node`, `select`, `set` (animation speed 0.2, debug paint), `key` (Tab moved the
  focus), `back`, `at`, an unknown method refused, `screenshot` (the frame with the selected
  button highlighted and its card). Select mode with a real mouse click on the window: the
  widget under the pointer was selected and highlighted, and the counter did not move. The
  page in headless Edge: the inspector with a widget's details, the performance chart while the
  counter animated, the settings.
- **Android, on the test phone** (Huawei STK-L21, Android 10): `frus run -d XMJNW19B23011768
  --no-hot`; the run made the forward `tcp:… localabstract:frus_devtools_com.example.devapp`;
  `status`, `hello` (2.55 pixels a point), `tree`, two `tap`s on "+" (the count read 2),
  `set` (animation speed 0.1, select mode), then a real touch (`adb shell input tap`) on "Start
  auto": the text under the finger selected and highlighted with its card, the button not
  pressed; the device's screenshot through `screenshot`; the page showing the phone's tree. The
  same checks ran first on the `frus_api28` emulator (Android 9), which also showed that an
  emulator started without `-feature GLESDynamicVersion` cannot draw frus at all (an existing
  limit, said in the log).
- **Web**: `frus run -d web-server --no-hot`, the page open in headless Edge with WebGPU:
  `hello` (platform `web`), `tap` on "+" (the count read 1), `logs` (the browser's console),
  `screenshot` (the canvas, read in the task that drew it).
- Clippy for Android (`aarch64-linux-android`) and the web (`wasm32-unknown-unknown`) on
  `frus-shell`.

- **Second round, Windows** (`frus run`, hot reload, the counter made by `frus create`):
  `hello`, `tap`, `logs` (the log came through the engine); a hot patch changed "Start auto"
  to "Begin auto" in 68.9 s and `find` saw the new label; `rebuilds` (two components); `node`
  of the button gave its constraints (125 by 40, tight, in a 140 by 88 content box);
  eighteen polls of `frames`, `state` and `rebuilds` over seven seconds left the frame count
  at 2 — an idle application stayed idle; the repaint rainbow's border changed colour from one
  frame to the next (pixel read from two screenshots); the performance view with the frames of
  "Start auto" and the "built most" list (headless Edge); `restart` came back on the same
  address in 351 s (a machine shared with other builds).
- **Second round, the phone** (Huawei STK-L21, `frus run -d XMJNW19B23011768 --no-hot`, the
  counter with a "Name" field added): `type` with `{"target":{"label":"Name"},"text":"Ada
  Lovelace"}` tapped the field, typed twelve characters, and the page read "Hello Ada
  Lovelace"; the screenshot shows the text in the field, and the keyboard's suggestion strip
  following it ("Lovelace"), so the input method saw the text too.
- Unit tests for the second round: the room offered (content box, scrolled axis, root), the
  constraints in `node`, frames carrying rebuilds, polling that queues nothing, the rebuild
  counts, the paint cache's boundaries, the log's colours taken out, a run's `app.devTools`
  kept per device, `type` reaching a focused field, the Android Studio event.

## Not done

- **Constraints recorded by the layout itself**: shown as the room the flexbox layout offers
  (see "Constraints" above), not recorded inside taffy; the min-content and max-content probes
  are not shown.
- **Rebuilds of an Elm-style `view`**: not a component, not counted; the frame's `build` time
  says when it ran.
- **iOS**: no shell yet.
