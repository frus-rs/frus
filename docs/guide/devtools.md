# DevTools

`frus run` serves DevTools for the application it runs: a page in your browser that shows the
live widget tree, what each widget measures and tells a screen reader, what every frame costs,
and the log, with switches to slow the animations down or outline every widget. The same
protocol is open to any other client: a test script, an editor, an AI agent driving the
application.

It comes with a **debug build**, which is what `frus run` builds by default. Nothing of it is
compiled into a release (`frus run --release`, `frus build`).

## Opening it

```text
$ frus run
…
devapp is running. Keys:
  …
  v  open DevTools in the browser: the widget tree, the frames, the log
DevTools: http://127.0.0.1:9100/?token=6c1f…
```

Press `v`, or open the address. In VS Code, run **frus: Open DevTools** from the command palette
while the application runs. With `frus run --machine`, the address arrives as an event:

```json
{"event":"app.devTools","params":{"appId":"frus-run-4242","uri":"http://127.0.0.1:9100/?token=6c1f…"}}
```

It works wherever `frus run` runs the application: on the desktop, on an Android device or
emulator (through `adb forward`, with no permission asked of the application), and in a browser
(`frus run -d web`). The page itself always runs in your browser, on your machine.

## What it shows

- **Inspector.** The widget tree, as laid out on the last frame: each widget's type, its key,
  the words it paints and its semantics label. Select one to see its box, its layout style
  (sizes and their bounds, padding, margin, flex factors, alignment), its semantics (role,
  label, value, state, actions), the room it was offered (its constraints), how many times it
  was built when it is a component, its ancestors and children. The selected widget is highlighted
  in the application. **Select widget** turns a tap in the application into a choice: the widget
  under the finger or the mouse is highlighted, and tapping it selects it in the tree.
- **Performance.** A bar per frame: building the tree, layout and paint, rendering (the GPU and
  the display included). Frames whose own work is over the 16.7 ms of 60 Hz, or that came late,
  are red. The frames per second, the average and the worst frame, how many components were
  built, and the components built most. Reading them does not make the application draw.
- **Settings.** The animation speed (1×, 0.5×, 0.2×, 0.1×), debug paint (an outline around every
  widget), the repaint rainbow (a border that changes colour each time the frame, or a repaint
  boundary in it, is painted again), the inspector's hover card, the performance overlay, the
  brightness, the platform the widgets behave as, the text size.
- **Logs.** The application's log, filtered as you type.

## The protocol

Version 1. One endpoint, on the address `frus run` printed:

```text
POST /api
X-Frus-DevTools: <token>
Content-Type: application/json

{"method": "find", "params": {"text": "Save"}}
```

The answer is `{"result": …}` or `{"error": "…"}`, with status 200 either way. The token is the
`token` of the printed address; it can also be given as `?token=` in the request's address. A
request without it is refused (403): another site open in your browser cannot drive the
application.

Some methods are answered by `frus run` itself; the others are relayed to the application, which
answers them against its next frame, so what they describe is what is on screen.

### Answered by `frus run`

| Method | Parameters | Result |
| --- | --- | --- |
| `status` | | `{protocol, app, device, platform}` |
| `logs` | `after?` | `{lines: [{number, text, error}]}`: the lines after line `after`, or all that are kept (5000) |
| `reload` | | `{}`: hot reload, as `r` |
| `restart` | | `{}`: restart afresh, as `R` |
| `screenshot` | | On Android: `{format: "png", data}`, the device's own screenshot, base64. Elsewhere relayed to the application. |

### Answered by the application

Widgets are named by `id`, 16 hexadecimal digits. A widget can also be **found** by any of
`text` (the words it paints), `label` (its semantics label), `type` (its type's name, `Button`),
`key` (its key as `tree` lists it: hexadecimal, a hash of the value given to `Keyed`) or `id`. Several criteria must all hold. `text`
and `label` match whole words unless `contains: true` is given, which matches a part, ignoring
case. When several widgets match, the one painted last, on top, is acted on.

Coordinates are logical pixels from the top-left corner of the application's surface.

| Method | Parameters | Result |
| --- | --- | --- |
| `hello` | | `{protocol, frus, platform, size: {width, height}, devicePixelRatio}` |
| `tree` | | `{size, selected, nodes: [node…]}`, in paint order |
| `node` | `id` | `node` with `style`, `constraints`, `ancestors` and `children` |
| `find` | a finder | `{nodes: [summary…]}` |
| `at` | `x`, `y` | the `summary` of the deepest widget at the point, or `null` |
| `select` | `id` or `null` | `{selected}`: the widget highlighted in the application |
| `tap` | a finder, or `x`, `y` | `{x, y, id}`: where it tapped |
| `scroll` | a finder, or `x`, `y`; `dx`, `dy` | `{x, y}`: the scroll area at that point moved by `dx`, `dy` logical pixels; `dy > 0` brings what is further down into view |
| `type` | `text`; `target?`, a finder or a point to tap first | `{typed, focused}`: the characters typed into the text field with the focus, through its editing as an input method commits text (on every platform, Android included); with no field focused, pressed as keys |
| `setText` | `target`, a finder; `text` | `{id}`: the field's whole value replaced, as an assistive technology does |
| `key` | `key`: `"Enter"`, `"Tab"`, `"Escape"`, `"Backspace"`, `"ArrowDown"`, `"F5"`, `"a"`, `"ctrl+a"`… | `{}` |
| `back` | | `{wentBack}`: the system's Back, short of leaving the application |
| `screenshot` | | `{format: "png", width, height, data}`: the frame, base64 (desktop and web) |
| `frames` | `after?` | `{budget, frames: [{number, interval, build, layoutPaint, render, acquire, draw, gpu, why, rebuilds}]}`, in milliseconds (`rebuilds`: components built for the frame); recording starts with the first call, and the last 600 frames are kept |
| `rebuilds` | `limit?` (20) | `{components: [{id, rebuilds}]}`: the components built most since the application started |
| `state` | | the switches below, and `size` |
| `set` | any of the switches | the switches, after |

A `node`, as `tree` lists it:

```json
{
  "id": "4f9a0c21d3b2e810",
  "name": "Button",
  "depth": 7,
  "parent": 12,
  "rect": {"x": 24, "y": 310, "width": 120, "height": 40},
  "key": "7",
  "text": "Save",
  "semantics": {"role": "Button", "label": "Save", "actions": ["tap"]}
}
```

`parent` is the index, in `nodes`, of the widget it sits in. `key`, `text`, `semantics` and
`rebuilds` (how many times a component was built) are there only when the widget has them. A
`summary` is `{id, name, rect, center, text, label}`.

`constraints`, in `node`, is the room the widget was offered, as the flexbox layout offers it:
`availableWidth` and `availableHeight`, its parent's content box (the parent's box less its
padding), `null` along an axis the parent scrolls; and `minWidth`, `maxWidth`, `minHeight`,
`maxHeight`, the widget's own size, minimum and maximum resolved against that room (`null` for
unbounded).

`frames`, `state` and `rebuilds` are answered from what the last frame left, without asking
the application for a frame: polling them does not keep an idle application drawing. (`state`
is answered that way once the application has been asked anything else; before that, it waits
for a frame like the rest.)

The switches of `state` and `set`: `animationSpeed` (a number, 1 is normal), `debugPaint`,
`inspector` and `performanceOverlay` (booleans), `brightness` (`"light"`, `"dark"`, or `null`
for the platform's), `platform` (`"android"`, `"ios"`, `"windows"`, `"macos"`, `"linux"`,
`"fuchsia"`, or `null`), `textScale` (a number, or `null`), `selectMode` and
`repaintRainbow` (booleans) and `selected` (an id, read only: use `select`).

Version 1 only grows: a field or a method is added, never changed or taken away.

### An example

Tap the button that says "Add", then check the count went up:

```sh
TOKEN=6c1f…
api() { curl -s -H "X-Frus-DevTools: $TOKEN" -d "$1" http://127.0.0.1:9100/api; }
api '{"method":"tap","params":{"text":"Add"}}'
api '{"method":"find","params":{"text":"Count: 1"}}'
```

### How a request reaches the application

`frus run` relays it as one line of JSON, `{"id":7,"method":"tree","params":{}}`, and the
application answers with one line, `{"id":7,"result":{…}}`:

- a desktop application on the socket it reports to `frus run` on (`FRUS_DEV`), the lines
  marked `devtools `;
- a page through the script `frus run` adds to it, which posts the answer back;
- an Android application on a local socket in the abstract namespace,
  `frus_devtools_<package>`, which `frus run` reaches with `adb forward`. Any process on the
  device can reach such a socket: it is opened by debug builds started by `frus run` only.
