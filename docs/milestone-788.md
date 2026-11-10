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

## Not done

- **The constraints** a widget was given are not shown: the layout does not record them. The
  inspector shows the style that asks for them and the box that resulted.
- **Rebuild counts** per widget are not counted.
- **Hot reload runs** (`frus run` without `--no-hot`, the default of a debug build) use the
  same relays (the desktop's socket, the device's socket, the page's queue) but were not run
  here: the engine's first build takes too long for this milestone's machine time.
- **The log on the desktop without hot reload** goes straight to the terminal: the run does not
  read it, so the Logs view is empty there. With hot reload (the default), on Android and on
  the web, the log is there.
- **The performance view draws frames itself**: each poll asks the application for its frames,
  which wakes a frame. The chart shows them; an idle application shows about two frames a
  second.
- **Repaint rainbow and layer views**: frus paints one scene a frame, without layers to colour.
- **iOS**: no shell yet.
- **Android Studio**: the plugin has no DevTools command yet.
