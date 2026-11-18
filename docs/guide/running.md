# Running and hot reload

`frus run` builds the application, starts it on a device and shows its log in the terminal: the
desktop's output, Android's logcat or the browser's console, the same way on each. While it
runs, every file you save is **patched into the running application**: the screen changes in a
second or two, on the same page, with everything you typed and every state kept.

```sh
frus run                       # this computer
frus run -d android            # the connected phone, or the emulator
frus run -d web                # a browser, served on http://localhost:8080
frus run -d iphone-15          # a simulated phone, in a window
frus run -d all                # this computer and every connected phone, at once
```

`frus devices` lists what the application can run on. Whatever a run needs and the machine
lacks (a Rust target, the Android SDK, `wasm-bindgen`, the hot reload engine) is installed on
the way, and `frus doctor` says what is there and what is missing.

## Keys

While the application runs, the terminal takes single keys:

| Key | What it does |
| --- | --- |
| `r` | reload: with hot reload on, saving a file already patches it in; without, build again and start where it was, with the states that keep themselves |
| `R` | restart: build again, every state afresh |
| `q` | quit (during a build too: it stops the build) |
| `d` | detach: `frus run` ends, the application goes on |
| `s` | save a screenshot, `frus_01.png`, `frus_02.png`… |
| `w`, `f`, `S` | print the widget tree, the focus order, the semantics tree |
| `i`, `p` | the inspector, and outlines around every widget |
| `o` | make the widgets behave as another platform's (Android, iOS, Windows…) |
| `b` | light or dark |
| `T` | text size: 100%, 130%, 200%, 85% |
| `P` | the performance panel: what each frame costs |
| `l`, `K` | turn a simulated device on its side; hold its keyboard up |
| `v` | open [DevTools](devtools.md) in the browser |
| `h` | the list |

## Hot reload

A debug build on the desktop, the web or Android is patched by default. Change a `build`, a
colour, a handler, a layout; save; the change is on screen. What the application holds stays:
the page, the scroll position, the field's text, every `State` and hook.

What a patch cannot carry:

- **A state whose shape changed.** Adding a field to a `State` struct, or a hook to a
  component, changes what is kept; press `R` to start afresh.
- **On the web**, a change no patch can carry rebuilds the page, which reloads with its states
  kept.
- **A compile error** is shown in the terminal; the application keeps running the last version
  that built, and the next save that compiles is patched in.

`--no-hot` turns patching off: `r` then rebuilds and restarts the application where it was. A
`State` keeps its value across that restart when it says how:

```rust
use frus::{button, State, StateContext, StatefulWidget, Widget};

struct Counter;
struct CounterState {
    count: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> CounterState {
        CounterState { count: 0 }
    }
}

impl State for CounterState {
    type Widget = Counter;

    // Kept across a reload under `frus run`, and across the system ending the process when the
    // state also has a `restoration_id`.
    fn save(&self) -> Option<String> {
        Some(self.count.to_string())
    }
    fn restore(&mut self, saved: &str) {
        self.count = saved.parse().unwrap_or(0);
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        Box::new(button(format!("{}", self.count), cx.callback(|s: &mut Self| s.count += 1)))
    }
}
# let _ = Counter.into_widget();
```

### What it costs

- **The first run installs the engine** that builds patches: about twenty minutes, once. Linux
  needs the OpenSSL headers for it.
- **On Android**, the first hot build is slow and its library large (a debug library of a few
  hundred megabytes, on the device only); patches then take a few seconds. Switching between a
  hot run and `--no-hot` (or `frus build apk`) reinstalls the application, and its data on the
  device goes with it.
- **A release build** restarts rather than patches; `--hot` asks for patches anyway. `--profile`
  never patches: it measures, and a patched build would measure the patching.
- There is no hot reload on an iOS simulator.

## Builds, flavors and values

`frus run` takes the same options as `frus build`: `--release` and `--profile`, `--flavor`,
`--define KEY=VALUE` and `--define-from-file`, `--build-name` and `--build-number`. They are in
[Build modes, flavors and releases](build-variants.md).

## In an editor

The VS Code extension and the Android Studio plugin start `frus run` for you, with its keys as
buttons, and patch on save. `frus daemon` and `frus run --machine` speak JSON lines, for other
editors. `frus mcp` lets a coding agent run and drive the application: see [AI agents](mcp.md).

## Tests and checks

```sh
frus test                      # every test, failures shown where they happened
frus test login                # the tests whose name contains "login"
frus analyze                   # clippy and rustfmt; --fix applies what can be applied
```

See [Testing](testing.md).
