# Milestone 775: open an address, and share through the system's share sheet

## What

Two capabilities an application reaches for constantly, as one function each, re-exported from
`frus`:

```rust
// The browser, the mail application, the dialer, the messages application.
frus::open_url("https://frus.dev")?;
frus::open_url("tel:+15551234567")?;

// The system's share sheet.
frus::share("Meet at 6")?;
frus::Share::text("Here is the report")
    .subject("Weekly report")
    .url("https://example.com/reports/42")
    .file("/path/to/report.pdf")
    .show()?;
```

`open_url` returns `Result<(), OpenUrlError>` (`InvalidUrl`, `NoHandler`, `Unsupported`,
`Failed`). `Share::show` returns `Result<ShareOutcome, ShareError>`: `ShareOutcome::SheetShown`,
or `ShareOutcome::CopiedToClipboard` where a desktop had no share window to show and copied the
text and link instead, for the application to say so. `ShareError` is `Empty`, `FileNotFound`,
`Unsupported` or `Failed`.

## Why

Until now an application that wanted to open a web page, start a call or share a result wrote
JNI by hand on Android, a `ShellExecute` call on Windows, and its own `navigator.share` glue on
the web. These are among the first things a real application needs.

## Design

### Opening an address

The address is checked first, the same on every platform:

- it must start with a scheme of **two letters or more**: `C:\Windows\…` is a path, and handing a
  path to `ShellExecute` would run the program it names;
- no control characters, something after the colon;
- `javascript:` is refused (it runs code rather than opening anything).

Then:

| Platform | How |
|---|---|
| Windows | `ShellExecuteW("open", url)`; `SE_ERR_NOASSOC` is `NoHandler` |
| macOS | `/usr/bin/open url`, waited for (it returns at once); a failure is `NoHandler` |
| Linux | `xdg-open url`, **not** waited for (on some desktops it lasts as long as the browser); a missing `xdg-open` is `Unsupported` |
| Android | `FrusShare.openUrl`: `Intent.ACTION_VIEW` with `FLAG_ACTIVITY_NEW_TASK`; `ActivityNotFoundException` is `NoHandler` |
| Web | `http`/`https`: `window.open(url, "_blank")`, with the new tab's `opener` cleared; a blocked tab is `Failed`. Other schemes: `location.assign`, which the browser hands to the system without leaving the page |
| iOS | `Unsupported` |

On Android the intent carries `NEW_TASK` and **not** `SINGLE_TOP`. For an address another
application claims, `SINGLE_TOP` changes nothing. For an address the application claims itself,
`SINGLE_TOP` would deliver it to the running activity as `onNewIntent`, which a native activity
does not forward, and the address would be lost; without it a second activity starts and hands the
address to the running one (milestone 763).

### Sharing

| Platform | How |
|---|---|
| Android | `FrusShare.share`: `ACTION_SEND` (`ACTION_SEND_MULTIPLE` for several files) in `Intent.createChooser`. Text and link go as `EXTRA_TEXT`, the subject as `EXTRA_SUBJECT` |
| Windows | `IDataTransferManagerInterop::ShowShareUIForWindow` for the application's window, with a `DataRequested` handler that fills the `DataPackage` (title, text, web link, storage items). If it cannot be shown, the text goes to the clipboard: `CopiedToClipboard` |
| macOS | `NSSharingServicePicker` over the key window's content view, from the main thread; the clipboard otherwise |
| Web | `navigator.share({title, text, url})` where it exists; `Unsupported` otherwise and for files |
| Linux, iOS | `Unsupported` |

**Files on Android.** Another application cannot read this one's private files, and the usual
answer is a `FileProvider` declared in the manifest. A frus application is a `NativeActivity`
APK with no `classes.dex`; the framework's Java is loaded at run time from an in-memory dex, and a
content provider must be a class of the APK that the system instantiates when the process starts.
So no provider can be declared. What needs no manifest and no permission from Android 10 is the
**shared Downloads collection** of `MediaStore`: each file is copied to
`Download/<application label>/`, the copy's `content://` address is shared with
`FLAG_GRANT_READ_URI_PERMISSION` and in the intent's `ClipData`, and a later share of the same name
first deletes the earlier copy (the application owns it). Before Android 10 a share with files is
`Unsupported`; text and links still go. The copies remain visible in Downloads, which the rustdoc
says.

**The window on Windows.** `share` is a free function, so it finds the window itself: the
foreground window if it belongs to this process, else the process's first visible unowned
top-level window. The `DataRequested` registration is kept per thread and replaced by the next
share.

### Dependencies

`windows` (already in the tree through the tray and the notifications) becomes a regular Windows
dependency rather than one only the `taskbar` feature pulls in, with the WinRT features the share
window needs, together with `windows-collections` and `windows-future` (both from the same
`windows-rs` project, MIT or Apache-2.0, already in the lock file) for the list of files and for
waiting for `StorageFile.GetFileFromPathAsync`. `objc2-foundation` gains its `NSArray`,
`NSGeometry` and `NSURL` features. The dex gains `FrusShare.java`, rebuilt with
`scripts/build-input-dex.sh`.

## How it was tested

- Unit tests: the address check (schemes, case, Windows paths, `javascript:`, control
  characters), which schemes the browser opens itself, the builder, an empty share and a missing
  file as errors, how the link joins the text.
- Windows, for real: a small program calling `frus::open_url("https://frus.dev")` got `Ok(())`
  from `ShellExecute` and the default browser (Edge) started new processes. The screen was locked
  during the run, so the page itself was not seen. The same program refused
  `C:\Windows\System32\calc.exe` with `InvalidUrl`. Run from a console with no window,
  `Share::text(..).url(..).show()` went down the fallback path (no window for the share window)
  and answered `Failed`, because a locked session's clipboard belongs to the secure desktop.
- Android: compiled and linted for `aarch64-linux-android`, and the dex rebuilt with `FrusShare`;
  **not run on a device** (see below).
- Clippy for Windows, Android (`aarch64-linux-android`), the web (`wasm32-unknown-unknown`) and
  macOS (`x86_64-apple-darwin`, check only).

## Not done

- **Nothing was run on Android.** The test phone was not connected for the whole milestone, and
  the APK build for a test application could not finish (WSL could not fetch or read the crates in
  time). `open_url` with `https://` and `tel:`, and the share chooser, still need to be seen on a
  phone. The Java side compiled against API 34, but nothing has run it.
- **Not run on macOS, Linux or iOS.** The macOS share menu and `open` were type-checked against
  `x86_64-apple-darwin`, never run on a Mac. Linux's `xdg-open` was not run.
- **The web was compiled, not run in a browser.**
- **The Windows share window was never seen.** It needs a window and an unlocked session; the
  only run was from a console with the session locked, where the fallback was taken.
- **Sharing files on Android** was not exercised on the phone (only text); the Downloads copy is a
  visible side effect, and Android 9 and older refuse files.
- **No `can_open_url`.** From Android 11 an application must declare `<queries>` to learn whether
  another application handles an address, which the build does not write yet; rather than answer
  wrongly on some platforms, it is left out. `open_url` reports `NoHandler` instead.
- iOS returns `Unsupported` for both.
- What the person picks in the share sheet is not reported.
