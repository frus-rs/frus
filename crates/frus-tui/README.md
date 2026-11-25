# frus-tui

The terminal workbench of [frus](https://github.com/frus-rs/frus): what `frus` opens when it is
typed with no command in a terminal.

```sh
cargo install frus-cli
cd my_app
frus
```

One screen for the loop of developing an application, and what surrounds it:

| tab | what it shows | keys |
|---|---|---|
| **1 Run** | the devices and the emulators that can be started; the runs, each with where it is (building, up, reloading, ended); their log, each line with its device when several run | `Enter` run on the selected device (or every device marked with `Space`; on an emulator, start it), `R` restart, `r` reload a run without hot reload (with it, saving a file patches the change in and the log says so), `s` stop, `Tab` next panel, `c` clear the log |
| **2 Build** | `frus build` for a target (desktop, web, apk, appbundle, installer), its output as it comes, its result | `Enter` build, `c` cancel |
| **3 Doctor** | `frus doctor`'s report, looked at when the tab is first opened | `Enter` look again, `f` install what is missing (`frus doctor --fix`) |

`1` `2` `3` or `←` `→` change tabs, `PgUp` `PgDn` `Home` `End` scroll the pane, `q` leaves.
`r`, `R` and `s` go to the run picked in the Runs panel when it has the focus, and to every run
otherwise. Every key has its button in the key bar at the bottom; the mouse also picks tabs,
devices and runs, marks a device by its box, and its wheel scrolls.

Leaving stops every run, and with it the applications it started. Nothing is left running.

## How it works

The workbench is a client of `frus`'s own commands, the ones an editor drives, started as
processes:

- `frus daemon` for the devices (`device.getDevices`, then `device.added` and `device.removed`
  as they come and go) and the emulators (`emulator.getEmulators`, `emulator.launch`);
- `frus run --machine -d <device>` for each run: its events are the run's status and log, and
  `r`, `R` and `s` are `app.restart` and `app.stop` on its input;
- `frus build <target>`, `frus doctor --machine` and `frus doctor --fix`, read to their end.

The protocol is described in the [CLI's README](../frus-cli/README.md#for-editors-frus-daemon-and-frus-run---machine).

It is a developer tool: `frus-cli` depends on it, behind its default `workbench` feature, and
no application built with frus does. Drawing is done by [ratatui](https://ratatui.rs) over
crossterm, which work in the Windows console, the terminals of macOS and Linux, and over SSH.

When the input or the output is not a terminal (a pipe, a script, CI) or `TERM` is `dumb`,
bare `frus` prints its usage instead, as it always did. `NO_COLOR` turns the colours off.
