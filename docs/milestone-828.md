# Milestone 828 — Rust code that runs with no window

Issue #215. Every line of Rust in a frus application ran inside the windowed process, while
it was on screen. Nothing could sync on a schedule, finish an upload after the person left,
or do anything at all with the application closed.

frus has no runtime to boot: the application is a native library, and a background entry
point is a Rust function in it. The work here is the shape of that function, what it is
handed, who calls it on each platform, and how what it writes reaches the window.

## The API, discussed

### Where an entry point is declared

Three shapes were weighed.

1. **An argument of `main!`**, after the application:

   ```rust
   frus::main!(App::default(), headless: { "sync" => sync, "upload" => upload });
   ```

2. **An attribute on the function** (`#[frus::headless] fn sync(..)`), collected at link
   time (`inventory`, `linkme`).
3. **A second macro** (`frus::headless!{ .. }`) beside `main!`.

The first is chosen. `main!` is already where the application says how it is entered, on
every platform, and a headless entry point is one more way in: on Android it is a second
native symbol the system calls, on the desktop a second branch of `run()`. Generating both
in the macro that already generates `android_main` and `run()` keeps them together and
lets the macro register the table before either path runs. Link-time collection (2) works
by sections the linker keeps or drops; it has failed on some targets and would be a new
dependency for one table. A second macro (3) would have to coordinate with `main!` over
who registers what, for no gain.

**Names are strings, chosen by the application.** A job scheduled today runs after an update
of the application, and Android keeps a persisted job across restarts: the name is what the
scheduler stores. A function's path (`stringify!(jobs::sync)`) would change with a refactor
and orphan every scheduled job.

**Cost.** Without `headless:`, `main!` expands to what it expanded to before: no table, no
native symbol, no argument parsing. The new functions in `frus-shell` are only reached
through those, so a release build of an application that does not use them links none of
it (the size budget's application, `frus-hello`, is unchanged by construction). On Android
the job service's code (7 KB of dex) and its manifest entry are only added to a package
whose library exports the native method.

### What an entry point is

```rust
fn sync(cx: &frus::HeadlessContext) -> frus::anyhow::Result<()>
```

A plain function, run on a thread of its own, allowed to block. `Ok` is a job done; an
error (or a panic, which is caught) is logged and the job is tried again later — Android
reschedules it with its own growing delay, the desktop retries five times from 30 s,
doubling.

### What the context may do

- **Name and input**: `name()`, and `input()`, the text the job was scheduled with.
- **Storage shared with the window**: `prefs()`, the application's preferences, the very
  store `use_pref` reads; `data_dir()`; `write_file(name, bytes)`, written whole or not at
  all, so the window never reads half a file.
- **Post a notification**: `notify(title, body)` returns at once; a tap opens the
  application. One notification per entry point, replaced by its next.
- **Know it was asked to stop**: `is_stopped()`, set when Android's `onStopJob` runs (its
  constraints stopped holding, or it ran too long) or the desktop's schedule is cancelled.
- **Schedule or cancel jobs**, the network (any blocking client, or `pollster::block_on`),
  the file system, anything a Rust function can do.

### What it may not do

- **Show anything, open a window, use widgets or a theme.** There may be no window, and on
  Android no activity in the process at all.
- **Send the application a message.** There may be no application. What the job leaves for
  the window goes through storage, which the window reads when it is next built — and at
  once when the window is open in the same process, since a preference change rebuilds it.
- **Ask for a permission.** Only a window can show the question. `notify` answers an error
  when notifications are not allowed; the application asks from its window beforehand.
- **Wait for the person**, or run long. Android stops a job after about ten minutes.

### Scheduling

```rust
frus::schedule_job(
    BackgroundJob::periodic("sync", Duration::from_secs(3600))
        .requires_network()
        .requires_charging()
        .persisted(),
)?;
frus::schedule_job(BackgroundJob::once("upload").input(id).delay(Duration::from_secs(30)))?;
frus::cancel_job("sync")?;
frus::run_headless("sync", None)?; // now, on this thread
```

Constraints: `requires_network`, `requires_unmetered_network`, `requires_charging`,
`requires_idle`, `requires_battery_not_low`; `persisted` keeps it across a restart of the
device. Scheduling a name again replaces its job. A name `main!` did not declare answers
`JobError::NoSuchEntry` at once rather than failing later in the background.

## Platforms

### Android

`JobScheduler` and a `JobService` of frus's own, `dev.frus.FrusJobService`
(`crates/frus-cli/java`). `WorkManager` would bring AndroidX and its database into every
package for what the platform's scheduler already does from API 21; frus's minimum is 24.

- **The service** reads the job's name and input from its extras, loads the application's
  library (`System.loadLibrary`, the name taken from the service's `dev.frus.lib_name`
  meta-data) and calls the native `nativeRun` on a thread of its own, then `jobFinished`
  with "retry" when the entry point failed. `onStopJob` calls `nativeStop`, which sets
  `is_stopped()`.
- **The process.** The service runs in the application's process: the one the window runs
  in when it is open, or one the system starts for the job, with no activity. In the second
  case the native side records what the activity would have (the `files` folder, the Java
  VM), installs the preferences' storage and the logger, and runs the entry point. If the
  person opens the application while that process lives, the activity starts in it and
  finds the same store already loaded.
- **The window's process ends with its activity** (milestone 755). A job running in it is
  given two seconds to finish first; one cut short is run again by the scheduler.
- **The packaging.** `frus build` and `frus run` look for the `nativeRun` symbol in the
  packaged libraries. Found, they declare the service (bound only by
  `android.permission.BIND_JOB_SERVICE`, not exported), ask for `RECEIVE_BOOT_COMPLETED`
  (which `persisted()` needs), and add the service's dex: as `classes2.dex` beside frus's
  activity or the Billing Library, or as `classes.dex` in a package with no other code. The
  split packages and the bundle (`android_pack`) carry the same files. The dex is built by
  `scripts/build-jobs-dex.sh` and checked in, like the activity's.
- **Scheduling from the window** loads the service's class through the activity's class
  loader. A package built by hand with `cargo apk` has no service, and `schedule_job` says
  so.
- **Periods** are at least 15 minutes (`MIN_JOB_PERIOD`); the system picks the moment
  within each period, and Doze defers jobs further.

### The desktop

A desktop application is a process the person starts; there is no system service to wake a
library. Two things are offered:

- `schedule_job` runs the job on a thread of the application's process, at its period, for
  as long as the process runs. An application that must keep working with its window closed
  keeps running in the tray (`Tray`, milestone 706). Constraints are not checked.
- **`app --frus-job <name>`** (and `--frus-job-input <text>`) runs one entry point with no
  window and ends, its exit status the job's result. That is what the system's own
  scheduler — Task Scheduler, cron, launchd — is given. frus does not register it there:
  how and when is the application's choice, and an installer's job.

The preferences are one file shared by both processes. A change is made under an exclusive
lock on `prefs.json.lock` (`std::fs::File::lock`; on Android, whose standard library does
not lock files, the C library's `flock`), after reading the file again if another
process wrote it since: a job's write is never overwritten by the window's stale copy. A read
looks at the file's stamp (time, length, and inode on Unix) at most every 250 ms and reads it
again when it changed. On Android the job and the window share one process and one store,
behind its mutex.

### The web

`schedule_job` answers `JobError::Unsupported`. A page's code stops with the page. The way
to run with the page closed is a service worker, and a frus application is a module that
expects a document and a canvas: running an entry point there would take a second wasm
build for the worker's scope (no `window`, no WebGPU surface), its own glue, and a storage
both sides reach (IndexedDB, as `localStorage` is not available in a worker). The Periodic
Background Sync API that would wake it exists in Chromium only, for an installed
application, at a period the browser picks. That is a milestone of its own; until then the
limit is stated in the API's documentation. `run_headless` runs an entry point in the page.

### iOS

`schedule_job` answers `JobError::Unsupported`: `BGTaskScheduler` needs identifiers declared
in the bundle's `Info.plist` and a registration before the application finishes launching,
which the iOS bootstrap does not reach yet.

## How it was tested

- `crates/frus-shell/src/headless.rs`: an entry point runs by name and reports its result,
  an error and a panic are caught and the next run is unaffected, an unknown name is an
  error; `--frus-job` parsing; a job's constraints; the desktop's schedule runs periodically
  until cancelled and nothing runs after; a stop reaches the running job.
- `crates/frus-widgets/src/prefs.rs`: two stores over one file, as two processes: the
  window reads the job's value, and its own next change keeps the job's rather than writing
  its stale copy over it.
- `crates/frus-shell/src/storage.rs`: two `FileStorage`s over one path see each other's
  writes, and the lock is exclusive.
- `crates/frus-cli/src/android_jobs.rs`: the service is declared once, with its library,
  permission and code; only a library with the native method is named; the dex files are
  laid out after the primary one; the checked-in dex holds the service.
- A desktop run of the example (below).
- A phone run (below).

### The example

`crates/frus-background-example`: `sync` counts its runs in the preferences, writes
`sync-result.txt` in the data folder and posts a notification; the window shows the count
and the file, with buttons to schedule it every 15 minutes, run it once, or cancel. The
application schedules the job when it starts, unless it already is, before its window is
built: on a locked phone the activity starts but its window is not built, and the job must
be scheduled all the same. What it logs while it starts reaches the console (logcat on
Android), as the logs are now set up before the application is made.

### The desktop run

`frus-background-example --frus-job sync` with no window, twice: each run logged
`background job "sync": running`, `sync: run N`, `done`, and exited 0; the second counted
run 2 and rewrote `sync-result.txt`. `--frus-job nope` answered
`no headless entry point named "nope"` and exited 1.

### The phone run

A Huawei phone (EMUI, Android 10), a release package built by `frus build apk`.

- **Scheduled at startup.** Opening the application logged
  `opened: the job ran 0 times; last: nothing yet`, then
  `background job "sync": scheduled`. `dumpsys jobscheduler` showed the job
  `#u0a186/-2133700292 com.frus.background/dev.frus.FrusJobService`,
  `PERIODIC: interval=+15m0s0ms`, `PERSISTED`, extras `{frus.entry=sync}`. As for any new
  periodic job, the scheduler runs it once at once.
- **Run 1, in the window's process** (pid 12862): `background job "sync": running`,
  `sync: run 1`, `done`.
- **Run 2, in a fresh process** (pid 13515, the first one ended with `am crash`), forced
  with `cmd jobscheduler run -f`: `sync: run 2`. The count run 1 left in the preferences
  file was read by the new process, so the store is shared between processes.
- **The notification** run 1 posted is in `dumpsys notification` (tag `frus.background`,
  channel `frus.background`, the application's icon).

**The bug the phone found.** The first runs on the phone all failed:
`the preferences could not be kept: …/files/frus-prefs.json.lock could not be locked:
lock() not supported; it will be tried again`. Android's standard library answers
`Unsupported` to `File::lock`, so every change to the preferences from a job, and from the
window, failed there. The lock now calls the C library's `flock(fd, LOCK_EX)` on Android
(retried when interrupted) and `File::lock` elsewhere; the desktop tests could not see it.

**A testing tip.** `am force-stop` and `pm clear` cancel the application's jobs, and on this
phone the cancellation lands seconds later (`CANCEL … reason:app force stopped`), after the
application has scheduled the job again: the job seems to vanish. To end the process and
keep its job, use `am crash <package>`.

## Not done

- **The cold start on the test phone.** Forced with the application's process gone, the job
  did not run: `Service starting has been prevented by iaware or trustsbase` then
  `FrusJobService unavailable`. Huawei's iAware stops a third-party application from being
  started in the background unless the person allows it (Settings, Battery, App launch: the
  application set to manage manually, with auto-launch, secondary launch and run in
  background on). The phone was locked and the setting was not changed, so the run with the
  application closed, the issue's "done when", is not shown on a device; on stock Android
  the system starts the process for the job.

- **iOS** (`BGTaskScheduler`) and **the web** (a service worker running the wasm module)
  answer `Unsupported`; the reasons are above.
- **Push notifications' data payloads, home-screen widgets and share intents** handled
  without the window: the entry points are the mechanism they would call, but each needs its
  own platform receiver.
- **Desktop constraints** (network, charging) are not checked, and the desktop does not
  register `--frus-job` with the system's scheduler.
- **`frus run`'s hot-reload path on Android** builds its own package and does not add the
  job service; use `--no-hot` or `frus build` to try a job on a device.
- **A change another process made is seen when the window is next built**, not pushed to
  it: on the desktop, a job run by the system's scheduler while the window is open shows on
  the window's next rebuild.
- A preference read-then-written by a job (a counter) is not one atomic step across
  processes: the lock covers each change, not a read followed by a change.
