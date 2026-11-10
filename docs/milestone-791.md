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

Unit tests (`cargo test -p frus-cli`, 159 pass):

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

On this machine (Windows 11, the Huawei STK-L21 on Android 10, an x86_64 emulator), with
projects made by the new `frus create` against this checkout:

- **`frus create`**: four projects — every platform; `--platforms android,web --org com.acme
  --deeplink myapp --description "Tides & currents"`; `--platforms windows --deeplink
  https://tides.example`; `--platforms web` — and the refusals of `ios` and of an `http` link.
  Each wrote what its platforms need and nothing else.
- **Builds**: the Windows-only project `frus build desktop --debug`; the web-only project and the
  Android + web one `frus build web`; the Android + web one `frus build apk --debug`, the one with
  every platform `frus build apk --debug` and `frus size desktop` (release). `frus build web` in
  the Windows-only project and `frus build desktop` in the Android + web one stop with the
  command that adds the platform.
- **Adding a platform**: `frus create --platforms android --deeplink https://tides.example .` in
  the web-only project wrote `res/`, appended the Android section and the intent filter, rewrote
  the list; asked again, it changed nothing.
- **A new application's first web build failed**, before this milestone too: with no lockfile
  yet, the `wasm-bindgen` on the PATH (0.2.129) was taken, and refused the module the build then
  resolved (0.2.130). The program is now chosen once the build has written the lockfile; the
  first `frus build web` of the web-only project installed and used 0.2.130.
- **Several devices**: `frus run -d all --no-hot` with the phone connected ran on Windows and the
  phone; `s` wrote one screenshot per device (`frus_01.png`, `frus_02.png`, one after the other),
  an edit to `src/counter.rs` then `r` reloaded both ("Reloaded in 72.6 s", "Reloaded in
  137.6 s") and both screens showed the edit, `q` ended both runs. The same with
  `-d windows -d iphone-15` (two desktop runs) and `-d windows -d emulator-5556`. The first
  screenshot attempt had shown two runs writing `frus_01.png` at once, hence the screenshots one
  after the other, each waiting for the previous run to say where its file went.
- **For an editor**: `frus run --machine -d windows -d iphone-15 --no-hot`: two `app.start` with
  their own `appId`, `app.key p` without an `appId` answered once and sent to both,
  `app.stop` with one `appId` stopped that run only, an unknown `appId` answered
  `no run has that appId`, `app.stop` without one stopped the other.
- **Deep link on the phone**: the package `frus build apk` made for the `--deeplink myapp`
  project, installed; `dumpsys package` lists the `myapp` scheme with `VIEW`, `DEFAULT` and
  `BROWSABLE` for the activity; `am start -a android.intent.action.VIEW -d myapp://home` cold
  started it, and it was the focused window.
- **Size**: `frus size desktop` on the every-platform counter: 14.90 MB as shipped; naga 1.46 MB,
  core 1.29 MB, frus_widgets 610 kB, wgpu_hal 606 kB…; the seven bundled faces found inside
  (3.3 MB); `[unattributed]` 2.04 MB. Its first version put the compiler's anonymous constants
  (`anon.<hash>.<n>`, where the fonts live) and the jump tables and unwinding data of Rust
  functions (`switch.table._ZN…`, `$ip2state$_ZN…`) under `[C/C++]`, 5.5 MB of it, and counted
  the fonts twice; `frus size <exe> --compare` against that first analysis shows the move.
  `frus size apk` on the same counter linked the arm64 `.so` with its symbols and then stopped at
  the signing, for want of a release key on this machine; the analysis now carries on with the
  library when only the package fails. Read with `frus size <.so>`: 13.51 MB as shipped (20.00 MB
  with the symbols), naga 958 kB, core 915 kB, wgpu_core 574 kB, frus_widgets 479 kB, the seven
  faces found inside, `[unattributed]` 2.14 MB.

## Second part (after the first pull request)

What the first part left, taken up again:

- **`q` while building stops the build.** The keys are heard from the start of a run
  (`keys::start`), not from the moment the application is up. `q` (or an editor's `app.stop`)
  sets a flag that every build waits on: cargo, cargo-apk and wasm-bindgen are run through
  `cargo::wait_or_cancel`, which ends the process tree (`taskkill /T` on Windows, an interrupt
  elsewhere) and returns `CANCELLED`; the hot-reload loops stop their engine. The run then ends as
  asked, not as a failure ("Stopped before the application was up."). The error check that
  follows a failed patch (`cargo check` of the shadow package, minutes long) is waited on the
  same way: it was the reason a stopped web run did not end.
- **An editor's session with `deviceId: "all"`** keeps the `appId`s that started and ends with
  the last `app.stop`, not the first.
- **Verified https links in the APK.** cargo-apk 0.10 cannot write `android:autoVerify`, so once
  it has made the package, frus makes it again around the same manifest with the attribute on
  every intent filter of `https` links (`app_links.rs`), with the SDK's own tools as cargo-apk
  does: `aapt package` (the manifest, the resources, and the libraries cargo-apk left, stored and
  not compressed as cargo-apk stores them), `zipalign -p`, `apksigner` with the key cargo-apk
  signed with. A package with no `https` filter is left exactly as cargo-apk made it; a failure
  leaves cargo-apk's package and says so.
- **`--template package` and `--template plugin`.** A package: a library with a `Tag` widget
  (themed defaults, every visual overridable), its tests and a gallery example. A plugin: one
  function (`platform_version()`), its Android side in Java under `android/java/<org path>/…`,
  compiled by `build.rs` with `javac` and `d8` into a dex the library carries and loads with an
  `InMemoryDexClassLoader` into the activity (as frus-shell's bridges do), reached through
  `ndk-context` and JNI, every exception cleared; the desktops, the web and iOS return
  `PluginError::Unsupported`. The templates are built in like the application's
  (`template-package/`, `template-plugin/`), and exempt from the SPDX header like it.
- **Mach-O sizes.** `parse_macho` reads the sections and the symbol table of a 64-bit Mach-O and
  sizes each symbol up to the next one of its section (Mach-O has no symbol sizes); the
  assembler's `ltmp` labels and the stabs are left out. Tested on a real object, cross-compiled
  here with `rustc --target x86_64-apple-darwin --emit=obj` (`src/fixtures/`).
- **The web's size after wasm-bindgen.** The analysis runs wasm-bindgen as `frus build web` does
  (which runs no wasm-opt) and measures its `_bg.wasm`; the `name` section it keeps is read and
  not counted as shipped.
- **Hot reload of a new application on the web.** Found on the way: the engine refuses what
  wasm-bindgen 0.2.130 writes (an imported exception tag: "exceptions proposal not enabled"),
  and a new application resolves 0.2.130, so its first hot web build failed whatever the number
  of devices. The shadow package now pins the versions the engine reads (`ENGINE_WASM_CRATES`:
  wasm-bindgen 0.2.127, js-sys and web-sys 0.3.104, wasm-bindgen-futures 0.4.77, the ones frus's
  own lockfile has) when the application asks for the template's `wasm-bindgen = "0.2"`.
- **Desktop hot engine on its own port**, as before, and the shadow per device.

How it was tested, second part (`cargo test -p frus-cli`: 172 pass with master merged in; the VS Code extension's
tests: 30 pass, the new one included):

- `q` during the hot builds of `-d windows -d edge` stopped both runs ("Stopped before the
  application was up", twice); `a_build_is_stopped_when_asked` ends a 60-second process within a
  moment of the flag.
- The APK of an application made with `--deeplink https://tides.example`: `aapt2 dump xmltree`
  shows `android:autoVerify(0x010104ee)=true` on the `https` filter; `aapt list -v` shows the
  library stored, as cargo-apk stored it; installed on the phone, `dumpsys package` reads
  `AutoVerify=true` for `tides.example`.
- The plugin, made with `--template plugin --org com.acme`: its tests and its example on Windows
  (`battery_level does not work on Windows`), its Android build (javac and d8 in `build.rs`, a
  1204-byte dex), and an application depending on it, installed on the phone: it shows
  "Android 10 (API 29)", read from the Java side. The package: its tests and its example built.
- On the phone, that debug build idles at 0.0 % CPU (no CPU time used over six seconds of
  `top`), 137 MB PSS, of which 66 MB graphics.

## Not done

- **Hot reload on several devices, end to end.** On Windows the hot run came up ("Windows is
  running, with hot reload") beside a web run; the web run with the pinned wasm-bindgen then
  failed its build ("cargo build finished with errors") on a machine loaded by other builds, and
  the errors could not be read before the check of them was itself cut off. So no save was seen
  patched into two devices at once; the reload with `r` on two devices (with `--no-hot`) is what
  was shown, in the first part. The pin is unverified beyond the shadow's lockfile taking the
  versions.
- **The web size after wasm-bindgen** was built into `frus size web` and not run on a real
  application in this part.
- **The phone numbers for a release build** (RAM, idle CPU) were not measured: the numbers above
  are a debug build's; the guide's APK sizes per font feature are the release numbers measured
  before.
- **Windows and Linux scheme registration** belongs to the installers (#448) and is left to them.
- **iOS**: `--platforms ios` refused, plugin side `Unsupported`.
- **`-d all` leaves out the browsers** on purpose; `-d all -d edge` adds one.
