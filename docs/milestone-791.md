# Milestone 791 — `frus create` platforms and links, several devices at once, the size report

## Objective

Three pieces of developer tooling, each driven from the commands a developer already types:

1. **`frus create` decides what an application is made for.** `--platforms android,web` writes
   only what those platforms need and records the choice; `--deeplink myapp` or
   `--deeplink https://example.com` declares the links that open the application; `--description`
   says what it is. `frus create --platforms web .` adds a platform to an application later,
   without touching what is there.
2. **One run, several devices.** `frus run -d all`, or `-d windows -d android -d edge`: one
   build per device's platform, the application started on each, every log in one terminal with
   its device in front, and every key (`r`, `R`, `q`, `p`…) for all of them. An editor gets each
   device's run as its own `appId`.
3. **What the shipped binary is made of.** `frus build <target> --analyze-size` (and
   `frus size`): the release binary broken down by crate, the fonts and assets found inside it, a
   short table, the whole as JSON, and `--compare old.json` to see what grew.

## Design

### Platforms (`platforms.rs`, `create.rs`)

The choice is recorded where the rest of an application's configuration is, in its manifest:

```toml
[package.metadata.frus]
platforms = ["android", "web"]
deeplink = "myapp"
```

A manifest without `platforms` (every application made before this milestone) is made for all of
them, so nothing that worked stops working.

**The template is written in regions.** Each platform's part of `Cargo.toml.liquid` starts at a
`# --- <title> ---` comment, which the template already used for its sections: `# --- Web ---`
holds the `wasm-bindgen` dependency and `[profile.web-release]` (moved there from the bottom),
`# --- Android packaging (cargo-apk) ---` every `[package.metadata.android…]` table and the
signing note. Cutting a platform is cutting its region, up to the next heading. The regions are
plain TOML comments, so the template stays a valid `cargo generate` template with no new syntax
(`frus create` refuses template syntax it does not know, and `cargo generate` would have asked a
question per platform for Liquid conditions). The same cut applies to the README (`## Android`,
`## Web`, `## Desktop`, and their rows of the table) and to the GitHub workflows: the `android`
and `web` jobs, each desktop's matrix entries (`windows-*`, `macos-*`, `ubuntu-*`), a job left
with no entry, and the `needs:` lists. Files go with their platform: `res/` with Android, `web/`
with the web, `src/bin/` with the desktops.

**Adding a platform to an application** reuses the same regions: the template's region for each
platform the manifest did not list is appended to the application's `Cargo.toml`, unless one of
its tables is already there (then nothing is appended and the report says to compare by hand),
the `platforms` line is rewritten, and the platform's missing files are written. Nothing else in
the manifest is touched. `frus create .` restores only the files of the application's own
platforms: a web-only application does not get the desktop program back.

`frus run` and `frus build` read the list: a device or target of a platform the application is
not made for stops with `this application is not made for web: … frus create --platforms web .
adds it, with the files it needs`. `ios` is refused by `--platforms` with the reason (there is
no iOS shell yet).

### Links (`Deeplink`)

`--deeplink` takes a scheme of the application's own (`myapp`, `myapp://`) or an `https` host
(`https://example.com`, `example.com`); `http`, system schemes (`file`, `content`, `intent`…)
and malformed ones are refused before anything is written. What is written:

- **Android:** a `[[package.metadata.android.application.activity.intent_filter]]` with the
  `VIEW` action, the `DEFAULT` and `BROWSABLE` categories and the scheme (and the host), which
  cargo-apk turns into the manifest's `<intent-filter>`. The hot-reload engine's Android manifest,
  which frus writes itself, gets the same filter, and there an `https` one carries
  `android:autoVerify="true"`.
- **macOS:** `CFBundleURLTypes` in the `Info.plist` of the `.app` frus makes, for a scheme.
- **The web:** nothing: since milestone 560 the address is the router's location.
- For an `https` host, `frus create` prints the `assetlinks.json` the host must serve, with the
  package name filled in and how to read the release key's fingerprint.

`frus create --deeplink myapp .` adds the links to an application that has none (once: asked
again, nothing changes).

### Several devices (`sessions.rs`)

Each device gets a run of its own: this program again, `frus run --machine -d <id>` followed by
the rest of the command line as it was typed (`forwarded`: everything but the devices and
`--machine`, so options added later reach the device runs without this code knowing them). Each
child builds, installs or serves, starts and follows exactly as a run on one device does, so a
device behaves the same alone and among others. The parent:

- reads every child's JSON lines and says them for a person with the device in front
  (`[Windows]`, `[STK L21]`, same width): `app.log`, `app.progress`, `app.started` ("is running,
  with hot reload"), `app.restarted` ("Reloaded in 3.1 s"), `app.stop` — and the children's
  standard error (cargo, cargo-apk) the same way;
- sends every key to every child as the editor command it stands for (`app.restart`,
  `app.stop`, `app.key`). `s` goes to one child after the other, two and a half seconds apart,
  because each takes the next free `frus_NN.png` and only knows it once the one before has
  written its file;
- with `--machine`, passes the children's events through untouched — each with its own `appId`
  — answers the editor's commands itself, and sends a command to the child its `params.appId`
  names, or to all of them;
- ends when every child has ended, with an error if one did.

`-d all` is this computer and every connected, authorised Android device, for the platforms the
application is made for. Browsers are left out, as are emulators that are not running, iOS
simulators and simulated devices: there are usually several browsers, and one is a `-d web`
away; `-d all -d edge` adds it. When `all` comes down to one device, it is an ordinary run.

**Build once per target.** Two devices of one platform build the same thing in the same target
directory; cargo lets one build at a time, and the second finds the work done. Runs that patch the
code are kept apart: a child of several gets `FRUS_RUN_SESSION=<device>`, and the hot-reload
shadow package, its build and the desktop's run copies go to a directory of that name below the
usual one. Two engines watching one shadow package each patched a saved file twice, as each
mirror wrote it; now each engine has its own copy of the sources, and a save is patched into
every device. The desktop's hot-reload engine is given a free port of its own (`--port`), as the
Android and web ones already were: its default, 8080, is the web run's.

`app.started` now says `hotReload`, for the parent to tell a person that `r` has nothing to do on
a device that patches each save already.

### Size (`size.rs`)

The shipped build has no symbols (`strip = true`), so the analysis builds the release again with
them kept, in `target/frus-size/`: the build that ships is not touched, and the first analysis
compiles everything once. The tool has no dependencies, so the three formats are read by hand, as
far as sizes go:

- **ELF** (the Android `.so`, a Linux program): the section headers and the symbol table; every
  `FUNC` and `OBJECT` with a size in the code or the read-only data, an alias at the same address
  counted once. What ships is the file without its non-loaded sections.
- **WebAssembly:** the code section's function bodies, named by the `name` section (the index
  space starts after the imported functions); the data section is read-only data no name covers;
  custom sections do not ship.
- **Windows:** the program's symbols are in the PDB, whose format is far from small, so the
  analysis asks the linker for its map instead (`cargo rustc … -- -Clink-arg=/MAP:…`, which
  reaches the final link only): every public and static symbol, sized up to the next one in its
  section.

A symbol's crate is the first segment of its demangled path (legacy `_ZN…E` and v0 `_R…`, or a
name the `name` section holds demangled already); for `<A as B>::f` it is A's crate, or B's when
A is a primitive. A name with no Rust path is `[C/C++]`; the code and read-only data no symbol
covers is `[unattributed]`. A font is found by its bytes: every file of the `frus-text` crate's
`assets/` (found through `cargo metadata`) and of the application's `assets/` of 4 kB or more
that is inside the binary whole is listed with its size, and taken out of `[unattributed]`.

The JSON goes to `target/frus-size/<platform>-NN.json`, numbered so that earlier ones stay for
`--compare`, which lists every crate and file that changed by a kilobyte or more.

## How it was tested

Unit tests (`cargo test -p frus-cli`, NNN pass):

- `platforms`: the list in any order and spelling, `desktop`, `ios` and unknown words refused;
  deep links parsed and refused; the intent filter cargo-apk reads; regions cut; keys set in their
  table, in the file's line endings; the README and both workflows trimmed — including a test
  that the template's own files still have the regions and jobs the code cuts.
- `create`: a new application with only Android (no `web/`, no program, no web tables, README and
  CI without them, the list read back by `Project`); the web added to it (the file written, the
  region appended, the list rewritten, everything else byte for byte as it was, nothing on a
  second run, `frus create .` restoring only the web's files); links declared, and added once to
  an application that had none; the description escaped for TOML and for HTML.
- `sessions`: what `all` means, devices resolved once each, tags and directory names, each
  device's platform, the letter of an app key, a run's events read.
- `size`: a hand-made ELF (two functions and an alias), a hand-made module (an import, two
  bodies, their names, data), a map, the crate of every kind of symbol, a font found by its bytes
  and a file that is not, JSON there and back, the comparison, numbered files.
- `bundle`: `CFBundleURLTypes` for a scheme, none for an `https` host; `hot`: the engine's
  Android manifest has the `autoVerify` filter.

On this machine — EVIDENCE

## Not done

- **`autoVerify` in the APK cargo-apk builds.** cargo-apk 0.10's intent filter has no field for
  it, so an `https` filter in a `frus build apk` package is not verified: Android before 12 asks
  which application opens the link, Android 12 and later opens it in the browser until the person
  turns the link on in the application's settings ("Open by default"). The hot-reload engine's
  manifest, which frus writes, has it.
- **Windows and Linux scheme registration.** A scheme is registered when an application is
  installed (a registry key, a `.desktop` file); frus records the scheme in the manifest for an
  installer to read, and does not write to the registry itself.
- **`--template package|plugin`.** frus has no plugin system for a template to start from, and a
  package template would be a library crate with no platform files; left for when there is a
  need.
- **iOS** in `--platforms`: refused with the reason until the iOS shell exists.
- **The size of a macOS program** is not broken down: Mach-O's symbol table has no sizes, and no
  Mac was at hand to check a reader against; `frus size` says the format is not read.
- **The web's size is the module before wasm-bindgen**, which trims a little more.
- **The editor extensions** start one run per debug session; a launch with `deviceId: "all"`
  works through the parent run, but the session ends when the first device's run ends.
