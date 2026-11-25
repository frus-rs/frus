# Milestone 846 — A terminal workbench: bare `frus`

## Objective

Issue #375. The CLI runs one command per invocation: pick a device with `frus devices`, start
`frus run -d <id>`, read its log, press `r`; build in another terminal; ask `frus doctor` in a
third. A workbench keeps that loop on one screen. Typed alone in a terminal, `frus` now opens
it: the devices, the runs on them with each one's status, their log, hot reload, a Build tab
and a Doctor tab, driven by keys and the mouse.

The issue set the shape before any code: a separate dev-only crate, a client of the protocol
the editors already use rather than a second implementation of the commands, and ratatui as
the library (decided before this milestone).

## Design

### A client of the tool's own commands

The workbench starts the commands an editor drives, as processes, and reads their JSON lines:

| panel | command | what it reads and sends |
|---|---|---|
| Devices | `frus daemon` | `device.getDevices`, then `device.enable` for `device.added` / `device.removed`; `emulator.getEmulators`; `emulator.launch` on `Enter` over an emulator; `daemon.logMessage` / `showMessage` become the notice line |
| Runs, Log | `frus run --machine -d <id>`, one process a run | `app.start`, `app.progress`, `app.started` (with `hotReload`), `app.log`, `app.restarted`, `app.stop`; `r` / `R` send `app.restart` (`fullRestart`), `s` sends `app.stop`; the build's own output (standard error) is shown dimmed and its last line is the run's status while it builds |
| Build | `frus build <target>` | its output, then its exit code |
| Doctor | `frus doctor --machine`, `frus doctor --fix` | the checks as JSON; the fix's output, after which the doctor looks again |

**No protocol was added.** One run a process gives each device its own status without a
multi-run protocol; the existing events carry everything the panels show. `frus build` has no
machine mode, and needs none here: its output is shown as text and its exit code is the result.

The tool's executable is `std::env::current_exe()`, so the workbench always drives the same
version of `frus` that opened it. Each process gets a hidden console of its own on Windows
(`CREATE_NO_WINDOW`) and its own process group elsewhere: nothing it starts can draw over the
workbench or receive the terminal's Ctrl+C, and `taskkill /T` / `kill -- -pgid` reach
everything it started.

### State, effects, drawing

- `state.rs` holds the whole state (`App`). Every key, click and process line is a `Msg`;
  `App::update` changes the state and returns the `Effect`s to carry out (start a run, send a
  command, build, quit). Nothing there touches a process or the terminal, so every behaviour is
  a unit test.
- `view.rs` computes where every panel goes in one function, `regions`, used both to draw and to
  find what a click lands on. Every key has a button in the key bar; a click on it is that key.
- `client.rs` starts and reads the processes. A run's end is reported only once its output is
  read to the end and the process is gone, so `app.stop` always comes before the exit.
- `lib.rs` is the loop: poll the terminal every 50 ms, drain the processes' messages, redraw
  when something changed.

With hot reload, which a debug build has by default, saving a file is what patches the code:
`frus run` reads `r` then as nothing to do and sends no event. The workbench does the same.
`r` on a hot run sends nothing and says that a save patches the change in. Each patch arrives as
`app.restarted` with `fullRestart: false` and is logged as "Patched in 0.4 s". On a run without
hot reload, `r` sends `app.restart`. `R` sends `app.restart` with `fullRestart: true` and can be
pressed again while a restart has not reported yet.

`r`, `R` and `s` go to the run picked in the Runs panel when that panel has the focus, and to
every run otherwise, as `frus run -d all` sends each key to all devices. `Enter` runs on every
device marked with `Space`, or on the selected one. Log lines wrap to the pane's width; a
scrolled log stays where it is as lines arrive, `End` follows again. The log keeps the last
10 000 lines.

### Leaving cleanly

`q` (or Ctrl+C, which arrives as a key in raw mode) restores the terminal, then sends
`app.stop` to every run and `daemon.shutdown` to the daemon, ends running builds with their
process trees, and waits up to 15 s; whatever is still alive then is ended with its tree.

### Where it opens, and where it does not

`main.rs` opens the workbench only for bare `frus` when standard input and output are both
terminals and `TERM` is not `dumb`. Piped, in a script or CI, bare `frus` prints the usage as
before and returns at once. If the terminal cannot be set up, the error and the usage are
printed. `NO_COLOR` gives `Theme::plain()`; the colours are a public `Theme` for the crate's
users.

### Out of the applications' graph

`frus-tui` is a new crate that only `frus-cli` depends on, through the default feature
`workbench`. No application depends on `frus-cli`, so nothing reaches an app. `cargo install
frus-cli --no-default-features` builds the tool without it; bare `frus` then prints the usage.

ratatui is used without its default features (`crossterm` only: no calendar widget, macros or
layout cache). `unicode-width`, for wrapping, was already in ratatui's tree.

## Tests

- `cargo test -p frus-tui`: 31 tests.
  - The reducer: running on the selected device, on marked devices, never twice on one device,
    not on an unavailable one; `r` only once the application is up; `r`/`R`/`s` to all runs or
    to the picked one; a run that ends, and one that dies without `app.stop`; the build status;
    a scrolled log staying put; devices coming and going; `Enter` on an emulator; `q` from every
    tab; a build to its result; the doctor on first visit and after a fix; on a hot run, `r`
    sending nothing and each patch logged; the cursor staying on its device as the list changes
    (the bug the real terminal found); escape sequences and carriage returns stripped.
  - The protocol: daemon answers and events, run events, JSON escapes and broken lines.
  - Screens, through ratatui's `TestBackend`, compared with text snapshots in
    `crates/frus-tui/snapshots/` (`FRUS_TUI_BLESS=1` rewrites them): looking for devices, a run
    with its log, the Build tab, the Doctor tab, a 40×12 terminal (and 3×2 and 0×0 without a
    panic).
  - Clicks: on a tab, a key button, a device, a device's box, an emulator, a run, the log, each
    found on the rendered screen and checked to do what its key does.
  - Key events: a key release (which Windows reports) is ignored.
- `cargo test -p frus-cli`: `opens_workbench` only in a terminal, not with `TERM=dumb`, not
  when built without the feature.

### In a real terminal

The workbench was driven in a Windows pseudo-console (ConPTY, through `pywinpty`, the screen read
back with `pyte`), 120×34, on an application made by `frus create`, run on this computer:

| step | seen |
|---|---|
| bare `frus` | the workbench, the devices listed by the daemon within a second |
| `Enter` | "Running on Windows"; the build's output in the log, the run's status following it; up with hot reload |
| a saved change to `src/counter.rs` | "Patched in 4.3 s", then the application's own "hot reload: patch applied" |
| `r` | "Hot reload is on: save a file and the change is patched in, its state kept; R restarts" |
| a click on "2 Build" (SGR mouse input) | the Build tab |
| `R` | "Restarting: building…", "Restarted in 20.2 s" |
| `q` | the workbench ended with exit code 0 within a second; the 7 processes it had started (2 × `frus`, 2 × `dx`, the application, 2 consoles) were all gone |

The run used a private `FRUS_HOME` holding the installed patch engine of revision `c5438d6`
under the name of `b1cd923`, the one `frus run` now pins. That revision only adds OpenSSL built
from source on Linux, so it changes nothing on Windows. Installing it takes about twenty minutes,
and the machine's build slot was shared.

The first runs in the real terminal found three bugs that the unit tests had passed:

- **The device list reordered under the cursor.** On its first look, the daemon's watcher reports
  every device as `device.added`. The list moved each device to its end, so `Enter` could have run
  on another device than the one shown selected. Known devices now keep their place, and the
  cursor follows its device's id through every change. A test covers it.
- **`r` on a hot run hung in "reloading…"**, and `R` was refused while it did (the section above).
- **Command answers and `app.devTools` were printed raw** in the log. Answers are now ignored,
  and DevTools' address is shown as "DevTools: <address>".

Piped (`frus < /dev/null`, `echo | frus`) and with `TERM=dumb`, bare `frus` printed the usage and
returned at once.

## Not done

- **No setup wizard.** The issue mentions a guided `frus setup`, but the CLI has none. Its
  equivalent, `frus doctor --fix`, runs from the Doctor tab with its output shown. A fix that
  asks a question cannot be answered there: its input is closed.
- **`frus build` has no machine mode.** The Build tab shows its text output and its exit code,
  with no progress bar or artifact path parsed out of it.
- **A save made while the application is still starting** (before its first frame) was not
  patched in one run: no patch and no error, either in the workbench or in `frus run`'s own
  events. Saved 15 s after the start, the same change was patched in 4.3 s, in the workbench and
  with `frus run --machine` alone. This is the CLI's and the engine's timing, not the workbench's,
  and it was not investigated further.
- **Only Windows was driven for real**, and only the desktop target. The Unix paths (process
  groups, `kill -- -pgid`) compile but were not run, and the web target was not run from the
  workbench. The phone was not used: it appears in the device list (the daemon lists it), and
  nothing was run on it.
- **The Doctor and Build tabs were not run in the real terminal.** They are covered by the
  reducer and snapshot tests, and `frus doctor --machine`'s output format is the one parsed.
- **No in-terminal DevTools or plugin dialogs.** The issue leaves them out of the first cut.
- **Wrapping is by character**, not by word, and the log keeps its last 10 000 lines.
