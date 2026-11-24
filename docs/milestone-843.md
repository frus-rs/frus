# Milestone 843 — Hot reload on Linux without the system's OpenSSL

## Objective

Part of #288: run `frus run` (hot reload by default since milestone 754) end to end on Linux.
Milestone 677 got as far as `frus doctor` in WSL. Building the patch engine (frus's fork of
`dx`) linked the system's OpenSSL, through `git2`, so it needed `pkg-config` and `libssl-dev`.
Installing those takes the administrator's password. On a machine without them, `frus run`
fell back to restarts, and the run could not be checked any further.

## Design

### The engine builds OpenSSL itself

The fork's revision `b1cd923` (branch `frus/v0.7.10-linux-openssl` of
https://github.com/KalybosPro/dioxus, one commit on top of `c5438d6`, the 0.7.10 fix that
`frus run` pinned until now) gives `git2` the `vendored-openssl` feature on Linux.
`openssl-src` then compiles OpenSSL 3 from its sources and links it statically into `dx`.
The engine no longer depends on any system OpenSSL. `ENGINE_REV` in
`crates/frus-cli/src/hot.rs` names the new revision, with `ENGINE_REPO` unchanged, as
milestone 816 pinned `c5438d6`. The engine installs into `~/.frus/tools/dx-b1cd923/`, next to
any older one, which is no longer used: the first `frus run` after the update installs it
again, on every system.

OpenSSL's own build needs a C compiler, which Rust already links with, plus Perl and `make`.
Debian and Ubuntu's `perl` has every module that build loads. Fedora splits some of them
(`IPC::Cmd`, `FindBin`, …) into packages a minimal installation leaves out. So the check asks
Perl for the five modules by name instead of only asking whether `perl` runs.
`missing_build_requirement` now checks for Perl with those modules and for `make`, and names
what is missing: `sudo apt install perl make` (Fedora: `sudo dnf install perl make`). It no
longer asks for `pkg-config` or `libssl-dev`. `frus doctor` and `frus run --hot` use the same
check.

### A crash that was WSLg's compositor

The first end-to-end runs in WSL ended a few seconds after the first frame, with
`Io error: Broken pipe (os error 32)` twice, the X11 clipboard (`arboard`) losing its server,
and `Error: Exit Failure: 1`. Neither the `s` key nor `frus run`'s pipes caused it:

- WSLGd's log said `weston ... terminated with signal 11` at that moment, and WSLGd started a
  new weston. The application's Wayland connection and its X11 (clipboard) connection to
  Xwayland both go when weston goes.
- The application's binary started by hand, with no `dx` and no keys, did the same thing.
- A plain winit 0.30 + softbuffer window, with nothing from frus, did too.

The trigger was isolated with that small window, a dozen-second run each, counting compositor
crashes:

| Window | weston crashed |
|---|---|
| Wayland, client-side decorations (winit's default) | 2 of 3 |
| Wayland, decorations + minimum size (what frus asks) | 1 of 3 |
| Wayland, decorations + `app_id` | 0 of 3 |
| Wayland, decorations + `app_id` + minimum size | 2 of 3 |
| Wayland, no decorations (+ minimum size) | 0 of 9 |
| X11 (Xwayland), minimum size | 0 of 6 |
| The frus application itself, Wayland | 4 of 4 |
| The frus application itself, X11 | 0 of 4 |

WSLg's weston 9 has no server-side decorations for Wayland clients, so winit draws its own
title bar and borders (sctk-adwaita, as subsurfaces). weston crashes while handling those
windows, typically 2 to 5 seconds after they appear. The same windows over X11 are framed by
weston's own window manager, and none crashed.

Turning decorations off would leave a frus window with no title bar under WSLg. So frus opens
its window through X11 when it runs under WSLg (`window_system::x11_preferred`): the kernel says
`microsoft`, `/mnt/wslg` is there, and `DISPLAY` names an X server. `FRUS_WAYLAND=1` keeps
Wayland, to reproduce the crash or to check a fixed WSLg. A Linux machine of its own is
unchanged: winit still chooses.

A loop ended by a lost window system now says so. winit reports it only as
`Exit Failure: 1`. The desktop `run` turns it into "the window system ended the connection
(exit failure 1): the display server closed or restarted, and the window went with it".

### Three things the end-to-end runs showed

- **`q` left the application running on Linux.** `frus run` ended, and so did `dx`, but the
  application went on with its window open. `hot::stop` ends `dx`'s process group with
  `kill -TERM -<pid>`, and procps-ng's `kill` (4.0.4, Ubuntu 24.04) reads that as nothing to
  signal: it returns 0 and the group gets no signal. `dx.kill()` then ended `dx` alone, and
  the application was left behind. The same path serves `R` (restart afresh), so every `R`
  on Linux left the old application running next to the new one. It is now
  `kill -s TERM -- -<pid>`: POSIX asks for the `--` before a process group, since an initial
  negative number can otherwise be read as a signal. macOS was not run.
  `stopping_the_engine_stops_its_process_group` starts a group leader with a child and checks
  that the child goes too.

- **A screenshot taken during the start showed the background alone.** `s` was pressed once
  `frus run` said the application was running, and the application drew the picture from the
  frame it was building. That frame came before the first one presented, and its scene was
  empty. A screenshot now waits for the first frame that reached the screen
  (`startup::pending()` false) and asks for another frame meanwhile, so it is never lost.
- **Each screenshot logged two `EGL 'eglInitialize' code 0x3001: DRI2: failed to load driver`
  errors.** They were frus's own: the offscreen device the picture is drawn with asked wgpu for
  every backend, OpenGL included, and wgpu's EGL probe logs at error level where Mesa has no
  hardware driver, as under WSLg. The offscreen device now asks the primary backends (Vulkan,
  Metal, DirectX 12, WebGPU) first, and every backend only where none of them has an adapter.

WSLg also restarts weston on its own, often just as a new `wsl` session starts. The
end-to-end script waits until the compositor has been up for 30 seconds, watches its socket,
and says so by name if it restarts.

## How it was tested

- `cargo test -p frus-cli`: `the_engine_needs_no_system_openssl` (the message names Perl and
  `make`, never `libssl`). `cargo test -p frus-shell window_system`: WSLg with an X server
  takes X11, a plain Linux machine and WSL without WSLg or an X server let winit choose, and
  `FRUS_WAYLAND=1` keeps Wayland.
- The engine was built on WSL (Ubuntu 24.04) with no OpenSSL headers installed: 28 minutes,
  with `dx` linked statically against the OpenSSL it built.
- End to end on WSL (Ubuntu 24.04, WSLg, llvmpipe Vulkan), a `frus create` counter counting by
  itself. `frus run -d linux`, then: change the label and save; save a change that does not
  compile, then fix it; a screenshot (`s`) after each; `q`. The script fails within seconds
  when `frus run` ends, never blocks on its input, and kills what it started.

  The run that passed (2026-11-24, with every build directory deleted beforehand, so the
  engine's first build of the application took 14 minutes in the run before it and 38
  seconds here):

  | Step | What happened |
  |---|---|
  | start | first frame 1.8 s after launch, the window through X11; `s`: the counter at 4 |
  | label changed | "Patched src/counter.rs in 6.0 s", same process (pid 1767); `s`: "Count: …" |
  | change that does not compile | `counter.rs:30:43: error[E0308]` said, the application untouched |
  | the fix | "Patched src/counter.rs in 3.0 s", still pid 1767; `s`: "Count = 40", counting on |
  | `q` | `frus run` exit 0; `dx` and the application gone within 1 s |

  Two earlier runs had the same two patches into one process (the counter went from 19 to 36
  across the second, so the state was kept), and one of them found the `q` bug above: the
  application was still running after `frus run` and `dx` had ended.
- `cargo test -p frus-cli --bin frus hot::` on WSL: 12 passed, with
  `stopping_the_engine_stops_its_process_group`. Clippy with `-D warnings` on `frus-shell`,
  `frus-cli` (Linux, all targets) and `frus-gpu` (Linux, all targets with `warm-shaders`;
  wasm32).

## Not done

- macOS, the other half of #288.
- `R` (restart afresh) was not part of the scripted run.
- The weston crash itself belongs to WSLg. With the small window above it reproduces in about
  half the runs, and that window is what a report there would need.
