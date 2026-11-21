# Changelog

## Unreleased

## 0.3.0

Released with frus 0.6.0.

- **The extension's links lead to the frus organisation**: the Marketplace page, the repository,
  the issues and the requests it makes to crates.io now point at
  [github.com/frus-rs/frus](https://github.com/frus-rs/frus) and [frus.dev](https://frus.dev),
  where frus moved. 0.2.0 still named the old repository.
- **Hot reload on Android and the web too**: a save is patched into the running application on
  the desktop, Android and the web, every state kept.
- **frus: Open DevTools** opens the running application's DevTools in the browser: the live
  widget tree, the frames and the log.
- **Several devices at once**: with `"deviceId": "all"` in a launch configuration, one F5 runs on
  every device, and the session lasts until the last of them has stopped.
- **Run Frus Update updates every install, and says when there is nothing to do**: it compares
  `frus --version` with the newest on crates.io and, when that one is newer, runs
  `cargo install frus-cli --locked --force`, so a `frus` installed from a checkout is replaced too
  (cargo refused that without `--force`, and the update silently did nothing).
- **Hot reload by default, with every frus**: a debug run passes `--hot`, so that a save is
  patched into the running application with frus 0.5.0 too, whose own default is off.
  `"hot": false` restarts to reload instead (`--no-hot`, left out for a frus that does not know
  it); `"hot": "frus"` passes nothing and lets frus decide; `true` patches a release build too.
- **Every run ends with a line in the Debug Console**: its exit code and, when it failed, the
  last error it printed. A run used to end with nothing said.
- **The device F5 runs on, until one is picked**: a phone plugged in, else a running emulator,
  else this computer, and the status bar shows that one. It used to show the first device by
  name (a simulated "Android phone") while F5 let frus choose this computer; simulated devices
  are now listed last and used only when picked.

## 0.2.0

- **Simulated phones and tablets** (frus 0.5): the picker has a **Simulated** section — an
  iPhone, an iPad, an Android phone, and the devices a project describes in its `Cargo.toml` —
  and F5 runs the application in a window shaped like the one chosen.
- **frus: Create New Project**: a folder, a name and an organisation, then `frus create`, and
  the new project opened. Also a **Create frus Project** button in an empty window.
- **frus: Run Frus Update**: `cargo install frus-cli` in a terminal, the newest `frus`.
- **frus: Run Doctor** is now **frus: Run Frus Doctor**.

## 0.1.1

Not published on its own: it reached the Marketplace with 0.2.0.

- A clearer Marketplace page: how to start in three steps, what the extension does, its keys
  and settings, and links to frus.

## 0.1.0

The first release on the Marketplace.

- A device picker in the status bar: this computer, phones, emulators and browsers, followed as
  they come and go; emulators started and created from it.
- F5 runs the application on the selected device, with its log in the Debug Console.
- Reload on save, with each component's state kept; restart, stop, and the keys of `frus run`
  as commands (debug paint, inspector, platform, brightness, performance overlay, the trees,
  screenshots).
- **frus: Run Doctor**.
- Installs `frus` (`cargo install frus-cli`) when it is missing.
