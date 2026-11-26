# Known limitations

Every limitation of frus that we know about, in one place: what you will run into, on which
platforms, whether there is a way around it, and where the evidence lives. It is meant to be
read **before** you choose frus for a project, not discovered halfway through one.

What is listed here is true on the current `master`. Each entry links to the issue, the
milestone note or the code that shows it, so you can check it for yourself. The limits shared
by every toolkit that draws its own pixels are tracked as a group in
[#224](https://github.com/frus-rs/frus/issues/224); the ones that still hold are listed below
with the rest.

## How an entry works

- **Id.** `L-001`, `L-002`, … An id is stable: it is never renumbered and never reused, so it
  is safe to cite in an issue, a commit or your own documentation. A new entry takes the
  number after the highest one in this file, wherever its area puts it.
- **An entry is never deleted.** When a limitation is lifted, its status becomes
  `fixed in X.Y.Z` (the first release that has the fix) and the entry stays, so an old link
  still says what happened.
- **Area** is the part of frus it belongs to; the entries are grouped by it.
- **Platforms** says where you meet it. "All" means every platform frus runs on.
- **Status** is one of `open` (nothing done yet), `partial` (part of it is answered, the rest
  is described) or `fixed in X.Y.Z`.
- **Workaround** says what you can do today, or "None".
- **Evidence** is the issue, the milestone note (`docs/milestone-NNN.md`), the guide page or
  the code that shows the limit.

A test, `cargo test -p frus --test limitations`, checks that every id is well-formed and
unique, that none is missing, that every entry has its fields, and that every file it cites
exists.

## Reporting a limitation

Found a limit that is not here? [Open an issue](https://github.com/frus-rs/frus/issues/new)
that says what you tried, what happened, on which platform, and what you expected. If you
know the evidence already, a pull request that adds the entry with the next free id is just
as welcome. A real use case is the best argument for moving an entry up.

## Contents

- [Platform: iOS](#platform-ios)
- [Platform: Android](#platform-android)
- [Platform: web](#platform-web)
- [Platform: desktop (Windows, macOS, Linux)](#platform-desktop-windows-macos-linux)
- [Platform services](#platform-services)
- [Native views and web views](#native-views-and-web-views)
- [Rendering](#rendering)
- [Text](#text)
- [Input](#input)
- [Accessibility](#accessibility)
- [Widgets and application structure](#widgets-and-application-structure)
- [Tooling and hot reload](#tooling-and-hot-reload)
- [Packaging and size](#packaging-and-size)

---

## Platform: iOS

### L-001 An iOS application compiles but cannot run

- **Area:** platform: ios
- **Platforms:** iOS
- **Status:** open
- **What you hit:** CI builds frus for `aarch64-apple-ios` and the simulator, but there is no
  UIKit host behind it: no lifecycle, no safe-area insets, no soft keyboard or text input, no
  accessibility bridge, no state restoration and no `.ipa` packaging. `frus run` and
  `frus build` do not target iOS. The services that already have an iOS branch (haptics,
  sharing, purchases, video, the camera, web views) answer `Unsupported`, and connectivity
  answers `Unknown`.
- **Workaround:** None. Ship on Android, the desktops and the web.
- **Evidence:** [#19](https://github.com/frus-rs/frus/issues/19),
  [docs/milestone-276.md](docs/milestone-276.md),
  [docs/milestone-775.md](docs/milestone-775.md),
  [docs/milestone-777.md](docs/milestone-777.md),
  [docs/platform-integration.md](docs/platform-integration.md)

## Platform: Android

### L-002 Saved state survives the process being killed only from Android 10

- **Area:** platform: android
- **Platforms:** Android 9 and older
- **Status:** partial
- **What you hit:** From Android 10, what an application marks restorable (the route, scroll
  offsets, fields, `use_restorable_state`) comes back after the system ends a background
  process. Before Android 10 nothing is kept, because the moment the save needs does not exist
  there; the reader comes back to the first screen. A popped page's `extra` object is never
  restored, and list rows are restored by index, not by key.
- **Workaround:** On Android 9 and older, keep what matters yourself with `frus::prefs()` or a
  database, and read it back at start.
- **Evidence:** [#57](https://github.com/frus-rs/frus/issues/57),
  [docs/milestone-753.md](docs/milestone-753.md),
  [docs/milestone-756.md](docs/milestone-756.md),
  [crates/frus-shell/src/restoration.rs](crates/frus-shell/src/restoration.rs)

### L-003 A configuration change the manifest does not declare closes the application

- **Area:** platform: android
- **Platforms:** Android (12 and later for the remaining case)
- **Status:** partial
- **What you hit:** A frus application runs one event loop per process, so it cannot survive
  Android destroying its activity and creating another in the same process. The manifest that
  `frus build` and `frus run` write declares every configuration change that can be declared,
  and links, shares and sign-ins come back to the running activity. What is left: the
  wallpaper palette's asset-paths change (Android 12 and later) still recreates the activity,
  and the application closes. An application created before milestone 755 must add the
  `config_changes` line itself, and an intent that opens the application must keep
  `FLAG_ACTIVITY_SINGLE_TOP | FLAG_ACTIVITY_NEW_TASK`.
- **Workaround:** Build with `frus build` / `frus run`, which write the full list; open the
  application with the flags above.
- **Evidence:** [docs/milestone-755.md](docs/milestone-755.md),
  [docs/milestone-763.md](docs/milestone-763.md),
  [docs/milestone-801.md](docs/milestone-801.md)

### L-004 A sign-in in progress is lost if Android ends the process

- **Area:** platform: android
- **Platforms:** Android
- **Status:** open
- **What you hit:** `frus::auth_session` does not write down the PKCE verifier, on purpose. If
  the system ends the application while the browser is in front, the redirect starts it afresh
  at the redirect's location and the code cannot be exchanged. A package made by hand with
  `cargo apk` (rather than `frus build`) runs in the system's native activity, where a
  custom-scheme redirect answers `AuthError::Unsupported`.
- **Workaround:** Treat the redirect location as "sign in again" (a router fallback page does);
  with a hand-made package, use a loopback redirect.
- **Evidence:** [docs/milestone-795.md](docs/milestone-795.md),
  [docs/milestone-801.md](docs/milestone-801.md),
  [docs/guide/data.md](docs/guide/data.md)

### L-005 The system's predictive back gesture is not received

- **Area:** platform: android
- **Platforms:** Android
- **Status:** open
- **What you hit:** The Back key and the back gesture pop a page, but the page does not follow
  the finger during Android's predictive back gesture: frus does not receive that gesture yet.
  The system Back is played as a swipe let go at once.
- **Workaround:** None.
- **Evidence:** [docs/milestone-620.md](docs/milestone-620.md),
  [docs/milestone-621.md](docs/milestone-621.md)

### L-006 In landscape with the keyboard up, the page has very little room

- **Area:** platform: android
- **Platforms:** Android, web on phones
- **Status:** open
- **What you hit:** With a phone in landscape and the soft keyboard up, the page keeps only what
  the keyboard leaves (about 80 px on the test phone, 38 CSS px in Chrome). The app bar is not
  collapsed for the keyboard, so the focused field may not be visible, and the floating action
  button can cover the end of the app bar.
- **Workaround:** Lock the orientation to portrait on screens that type, or give such screens a
  layout without an app bar in landscape.
- **Evidence:** [docs/milestone-752.md](docs/milestone-752.md),
  [docs/milestone-765.md](docs/milestone-765.md)

## Platform: web

### L-007 The page has no text for the browser: find in page, indexing and printing fail

- **Area:** platform: web
- **Platforms:** Web
- **Status:** open
- **What you hit:** Text is painted into a canvas. The semantics tree is projected into the DOM
  for screen readers, but as words a find does not search, so Ctrl+F finds nothing, search
  engines and link previews see an empty page, printing gives at best a picture of the
  viewport, and the browser's translation and reader mode have nothing to work on.
- **Workaround:** Keep pages that must be indexed or printed as ordinary HTML beside the
  application.
- **Evidence:** [#212](https://github.com/frus-rs/frus/issues/212),
  [docs/milestone-564.md](docs/milestone-564.md)

### L-008 A WebGPU device that fails at the surface cannot fall back to WebGL 2

- **Area:** platform: web
- **Platforms:** Web
- **Status:** partial
- **What you hit:** frus picks WebGPU where the browser offers it and WebGL 2 otherwise. Since
  milestone 847 a WebGPU device is first tried on a canvas of its own that the page never shows
  (a `webgpu` context, configured as the renderer configures it, one frame drawn), and the page
  draws with WebGL 2 when that fails; a page canvas that refuses its `webgpu` context also falls
  back. What is left: a canvas that passes the trial and then fails at configuration on the
  page's own canvas keeps its WebGPU context and cannot fall back (no browser has been seen to do
  it), and a renderer made again after a lost device stays on the interface the canvas already
  holds. WebGL 2 also draws without the GPU's clock and without a pipeline cache.
- **Workaround:** None in the application.
- **Evidence:** [docs/milestone-745.md](docs/milestone-745.md),
  [docs/milestone-847.md](docs/milestone-847.md)

### L-009 SQLite and the secure store answer `Unsupported` on the web

- **Area:** platform: web
- **Platforms:** Web
- **Status:** open
- **What you hit:** `frus::database` answers `DatabaseError::Unsupported` (SQLite in a page
  needs a separate build and storage of its own), and `frus::secure_store()` answers
  `SecureStoreError::Unsupported`, since any script of the page could read what the page keeps.
- **Workaround:** Keep data and secrets on a server; `frus::prefs()` keeps small settings in
  the browser.
- **Evidence:** [docs/guide/data.md](docs/guide/data.md),
  [docs/guide/platform.md](docs/guide/platform.md),
  [docs/milestone-795.md](docs/milestone-795.md)

### L-055 Chromium's phone emulation draws the page at the wrong scale

- **Area:** platform: web
- **Platforms:** Web
- **Status:** open
- **What you hit:** In Chromium's device emulation with a pixel ratio other than the screen's
  (a Pixel 7 at 2.625 in headless Edge, through the DevTools protocol's device metrics), the
  browser reports the canvas's device-pixel size at its CSS size. The windowing layer sizes the
  canvas's image from that report and the application draws at the emulated ratio, so
  everything is about 2.6 times too large, cut off at the right and bottom, and touches land in
  the wrong place. Real phones, desktop browsers and WebKit's emulation report the right size.
- **Workaround:** Emulate the phone at a pixel ratio of 1 (its size and user agent are enough
  to try the layout and the platform's conventions), or try it in WebKit's emulation or on a
  real phone.
- **Evidence:** [docs/milestone-847.md](docs/milestone-847.md),
  [scripts/web-browsers-check.mjs](scripts/web-browsers-check.mjs)

## Platform: desktop (Windows, macOS, Linux)

### L-010 An application has exactly one window

- **Area:** platform: desktop
- **Platforms:** Windows, macOS, Linux
- **Status:** open
- **What you hit:** The shell holds a single window and ignores the window id of every event.
  A document per window, a detached panel, a settings window or a second view of the same data
  cannot be opened. The remembered window placement is per program for the same reason.
- **Workaround:** Draw secondary panels as overlays, dialogs or side sheets inside the window.
- **Evidence:** [#218](https://github.com/frus-rs/frus/issues/218),
  [docs/milestone-783.md](docs/milestone-783.md),
  [crates/frus-shell/src/app.rs](crates/frus-shell/src/app.rs)

### L-011 No native macOS menu bar or Dock conventions

- **Area:** platform: macos
- **Platforms:** macOS
- **Status:** partial
- **What you hit:** Cmd is read as the shortcut key on a Mac, but the application's menus are not
  in the system menu bar (there is no `NSMenu`), and there is no standard Edit menu, services
  menu, About panel or Dock menu.
- **Workaround:** Use the in-window `MenuBar` widget.
- **Evidence:** [#60](https://github.com/frus-rs/frus/issues/60),
  [docs/milestone-736.md](docs/milestone-736.md)

### L-012 Video, the camera and native views have no macOS or Linux backend

- **Area:** platform: desktop
- **Platforms:** macOS, Linux (and iOS, see L-001)
- **Status:** open
- **What you hit:** `VideoPlayerController` answers `VideoError::Unsupported`, the camera
  answers `CameraError::Unsupported`, and `PlatformView` / `WebViewWidget` show their stand-in
  (a labelled box in a debug build, nothing in a release build). They work on Android, Windows
  and the web.
- **Workaround:** None on those platforms; check the error and offer something else.
- **Evidence:** [docs/guide/video.md](docs/guide/video.md),
  [docs/guide/camera.md](docs/guide/camera.md),
  [docs/guide/native-views.md](docs/guide/native-views.md),
  [docs/milestone-797.md](docs/milestone-797.md),
  [docs/milestone-798.md](docs/milestone-798.md),
  [docs/milestone-802.md](docs/milestone-802.md)

### L-013 Wayland: no global shortcuts and no window position

- **Area:** platform: linux
- **Platforms:** Linux under Wayland
- **Status:** open
- **What you hit:** Global shortcuts work on Windows, macOS and X11 only: Wayland lets no
  application grab a combination. The window comes back at its size and maximized state but
  not at its position, which Wayland does not give. Files dropped on a window with several drop
  targets reach the target under the last known pointer position, on macOS too, because the
  windowing layer reports no position during a drag.
- **Workaround:** Run under X11 (XWayland) where global shortcuts matter.
- **Evidence:** [docs/milestone-709.md](docs/milestone-709.md),
  [docs/milestone-783.md](docs/milestone-783.md),
  [docs/milestone-695.md](docs/milestone-695.md),
  [crates/frus-shell/src/shortcuts.rs](crates/frus-shell/src/shortcuts.rs)

### L-014 A Windows notification clicked later, from the notification centre, is not delivered

- **Area:** platform: windows
- **Platforms:** Windows
- **Status:** open
- **What you hit:** A click on a toast while it is on screen comes back to `show_async`. Once
  the toast has gone to the notification centre, a click there does nothing for the
  application: frus registers no activator to start the program again.
- **Workaround:** None.
- **Evidence:** [docs/milestone-701.md](docs/milestone-701.md),
  [crates/frus-shell/src/notification.rs](crates/frus-shell/src/notification.rs)

### L-015 macOS builds are compiled by CI but have rarely run on a Mac

- **Area:** platform: macos
- **Platforms:** macOS
- **Status:** open
- **What you hit:** The macOS paths of the permission questions, notifications, the tray, the
  taskbar and the disk image are type-checked and built in CI, but most were never run on a
  Mac: the system's permission question has never been seen, and notifications do not report
  the person's answer there. The bundle carries no `.icns` icon, and the disk image is not
  signed or notarized, so Gatekeeper warns.
- **Workaround:** Test on a Mac before shipping; sign and notarize the disk image yourself.
- **Evidence:** [docs/milestone-733.md](docs/milestone-733.md),
  [docs/milestone-744.md](docs/milestone-744.md),
  [docs/milestone-790.md](docs/milestone-790.md),
  [docs/guide/build-variants.md](docs/guide/build-variants.md)

## Platform services

### L-016 In-app purchases work only through Google Play

- **Area:** platform: services
- **Platforms:** iOS, macOS, Windows, Linux, web
- **Status:** open
- **What you hit:** `Purchases` talks to Google Play Billing on Android. Everywhere else every
  call answers `PurchaseError::Unsupported`: StoreKit and the Microsoft Store are not wired.
- **Workaround:** `Purchases::fake(FakeStore)` for tests and development on every platform.
- **Evidence:** [docs/guide/purchases.md](docs/guide/purchases.md),
  [docs/milestone-807.md](docs/milestone-807.md)

### L-017 No system file picker on Android, iOS or the web

- **Area:** platform: services
- **Platforms:** Android, iOS, web
- **Status:** open
- **What you hit:** `FileDialog` shows the system's dialogs on Windows, macOS and Linux only.
  Android's picker (the Storage Access Framework) gives content URIs and the browser gives a
  name and bytes, not paths, so the dialog's `PathBuf` answer does not fit them.
- **Workaround:** On Android, call the picker through JNI; on the web, use an `<input
  type="file">` through `web-sys`.
- **Evidence:** [docs/milestone-698.md](docs/milestone-698.md),
  [crates/frus-shell/src/file_dialog.rs](crates/frus-shell/src/file_dialog.rs)

### L-018 The camera's flash, zoom and focus point work only on Android

- **Area:** platform: services
- **Platforms:** Windows, web
- **Status:** open
- **What you hit:** Preview, pictures, frames and code scanning work on Android, Windows webcams
  and in the browser, but flash, torch, zoom and the focus point answer
  `CameraError::Unsupported` on webcams and in browsers.
- **Workaround:** None.
- **Evidence:** [docs/guide/camera.md](docs/guide/camera.md),
  [docs/milestone-798.md](docs/milestone-798.md)

## Native views and web views

### L-019 Native views on Android are copied through the CPU

- **Area:** native views
- **Platforms:** Android
- **Status:** open
- **What you hit:** An embedded Android view draws into an image that frus copies on the CPU and
  uploads each time the view draws something new. It costs nothing at rest and is fine for a
  page, a form or a map, but a full-screen video or a constantly animating view is expensive,
  and a page of web views scrolls slowly on a low-end phone.
- **Workaround:** For video, use `VideoPlayer` or a `TextureSurface`, which are drawn without
  that copy.
- **Evidence:** [docs/guide/native-views.md](docs/guide/native-views.md),
  [docs/milestone-794.md](docs/milestone-794.md),
  [docs/milestone-796.md](docs/milestone-796.md)

### L-020 A native view ignores rotating and scaling transforms for input

- **Area:** native views
- **Platforms:** Android, Windows, web
- **Status:** open
- **What you hit:** On Android a view under a rotating or scaling frus transform is drawn
  transformed, but its touches and its place for the screen reader follow its untransformed
  box. On Windows and the web it keeps its untransformed box altogether.
- **Workaround:** Do not animate native views with rotations or scales; move them instead.
- **Evidence:** [docs/guide/native-views.md](docs/guide/native-views.md),
  [docs/milestone-794.md](docs/milestone-794.md),
  [docs/milestone-802.md](docs/milestone-802.md)

### L-021 On Windows and the web a native view sits above frus and takes the whole gesture

- **Area:** native views
- **Platforms:** Windows, web
- **Status:** open
- **What you hit:** The view is a child window or an element over the canvas. frus content drawn
  over it is hidden unless it is an overlay or a press target; under a dialog the view is a
  still picture (Windows) or hidden (web). A wheel or a drag over a web view scrolls the page,
  never the frus list around it, and Tab does not move between frus's focus and the view.
- **Workaround:** Keep frus content beside native views rather than over them.
- **Evidence:** [docs/guide/native-views.md](docs/guide/native-views.md),
  [docs/milestone-802.md](docs/milestone-802.md)

### L-022 A web view on the web is an iframe

- **Area:** native views
- **Platforms:** Web
- **Status:** open
- **What you hit:** Sites that refuse framing show nothing and report nothing. A page from another
  origin gives no script results, no title, no history beyond what frus loaded and no
  navigation decisions. Settings such as the user agent, zoom, cache and local storage answer
  `WebViewError::Unsupported`.
- **Workaround:** Open such pages with `frus::open_url`, in the browser itself.
- **Evidence:** [docs/guide/webview.md](docs/guide/webview.md),
  [docs/milestone-802.md](docs/milestone-802.md),
  [docs/milestone-804.md](docs/milestone-804.md)

## Rendering

### L-023 No automatic fallback when a present Vulkan driver misbehaves

- **Area:** rendering
- **Platforms:** Android, Linux
- **Status:** partial
- **What you hit:** Where there is no Vulkan driver, or only the Android emulator's, frus draws
  through OpenGL ES by itself. A device whose Vulkan driver exists but draws wrongly or crashes
  is still drawn through Vulkan: there is no list of known-bad drivers. A device lost while the
  application runs is recovered (milestone 686); a driver that draws wrongly without losing the
  device is not detected.
- **Workaround:** Set `WGPU_BACKEND=gl` for such a device.
- **Evidence:** [#217](https://github.com/frus-rs/frus/issues/217),
  [docs/milestone-686.md](docs/milestone-686.md), [docs/milestone-697.md](docs/milestone-697.md),
  [crates/frus-gpu/src/renderer.rs](crates/frus-gpu/src/renderer.rs)

### L-024 Pipelines are compiled at every launch outside Vulkan

- **Area:** rendering
- **Platforms:** Windows, macOS, iOS, web
- **Status:** partial
- **What you hit:** On Vulkan the compiled pipelines are kept on disk between launches. DirectX
  12, Metal and the browser keep none, so their pipelines are compiled at every start (on
  Windows the DXC compiler frus places beside the program makes this short). glyphon's text
  pipelines are not cached anywhere, and a custom shader is compiled at the first frame that
  draws it.
- **Workaround:** Load custom shaders before the screen that shows them.
- **Evidence:** [#217](https://github.com/frus-rs/frus/issues/217),
  [docs/milestone-684.md](docs/milestone-684.md),
  [docs/milestone-686.md](docs/milestone-686.md), [docs/milestone-697.md](docs/milestone-697.md),
  [docs/milestone-792.md](docs/milestone-792.md)

### L-025 No OpenGL ES 2 rendering

- **Area:** rendering
- **Platforms:** Android
- **Status:** open
- **What you hit:** A device or emulator image with OpenGL ES 2 only cannot run a frus
  application. It fails at start and says why and what to change.
- **Workaround:** Use an emulator image with Vulkan or OpenGL ES 3.
- **Evidence:** [docs/milestone-769.md](docs/milestone-769.md)

### L-026 A transformed layer has no mipmaps, and scaled-up text blurs

- **Area:** rendering
- **Platforms:** All
- **Status:** partial
- **What you hit:** Images have mipmaps, but a layer scaled below one half by a `Transform` still
  skips texels, so small text in it looks uneven. A layer scaled up is enlarged as a texture,
  so its text is not laid out again at the final size and blurs. There is no bicubic filter:
  `FilterQuality::High` enlarges as `Low` does.
- **Workaround:** Animate a font size rather than scaling text with a transform.
- **Evidence:** [docs/milestone-732.md](docs/milestone-732.md),
  [docs/milestone-743.md](docs/milestone-743.md)

### L-027 Custom shaders and GPU passes have a fixed shape

- **Area:** rendering
- **Platforms:** All
- **Status:** open
- **What you hit:** Shader uniforms are `f32`, `i32`, `u32` and `vec2`/`vec3`/`vec4<f32>`
  only (matrices and arrays are refused), with at most four images, sampled linearly with
  clamped edges. A `GpuPass` (which has a depth buffer and multisampling since milestone 844,
  and can say it did not change with `.version`) draws in its own encoder before the frame,
  never inside frus's own pass, and its picture cannot be a storage texture.
- **Workaround:** Pack matrices into vectors; draw into your own textures and show them with a
  `Texture`.
- **Evidence:** [docs/guide/shaders.md](docs/guide/shaders.md),
  [docs/milestone-792.md](docs/milestone-792.md),
  [docs/milestone-844.md](docs/milestone-844.md)

### L-028 External frames are shown without a copy only on Android with Vulkan

- **Area:** rendering
- **Platforms:** Android with OpenGL ES, Windows, macOS, Linux, web
- **Status:** partial
- **What you hit:** A `TextureSurface` on a Vulkan phone shows a decoder's or the camera's frames
  where they were written. Everywhere else frames are copied through the CPU and uploaded: video
  on Windows and the web, and every pushed frame. There is no way for a producer to hand over a
  GPU texture it rendered.
- **Workaround:** None; size the frames to what is shown.
- **Evidence:** [docs/guide/textures.md](docs/guide/textures.md),
  [docs/milestone-793.md](docs/milestone-793.md),
  [docs/milestone-800.md](docs/milestone-800.md)

### L-029 Layer-heavy pages and page transitions are slow on low-end GPUs

- **Area:** rendering
- **Platforms:** Android (low-end GPUs)
- **Status:** open
- **What you hit:** A page transition renders two screen-sized layers per frame, about 112 ms a
  frame on the Mali-G51 test phone. A playing shader inside a clipped layer or a mask re-renders
  the whole layer each frame; the shader gallery holds about 32 fps there.
- **Workaround:** Avoid playing shaders under clips and masks on low-end phones.
- **Evidence:** [docs/milestone-792.md](docs/milestone-792.md),
  [docs/milestone-796.md](docs/milestone-796.md)

### L-030 Graphics memory stays high after leaving a page

- **Area:** rendering
- **Platforms:** Android
- **Status:** open
- **What you hit:** Going to a page and back leaves graphics memory allocated: 14 to 63 MB after
  the camera page on the test phone, and up to about 130 MB seen elsewhere. The allocator's
  block policy is the lever and has not been changed.
- **Workaround:** None.
- **Evidence:** [docs/milestone-793.md](docs/milestone-793.md),
  [docs/milestone-806.md](docs/milestone-806.md)

## Text

### L-031 A span of rich text cannot name its own font family

- **Area:** text
- **Platforms:** All
- **Status:** open
- **What you hit:** A `TextRun` carries a size, weight, colour, decoration, letter spacing and
  shaping (features, axes, language), but no family. A rich text is drawn in the default family
  (or the Arabic face for Arabic), and an OpenType feature on a span works only if that family
  has it.
- **Workaround:** Put the differently set words in their own `Text` inside a `Row` or `Wrap`.
- **Evidence:** [docs/milestone-784.md](docs/milestone-784.md),
  [crates/frus-core/src/text_style.rs](crates/frus-core/src/text_style.rs)

### L-032 The web has no platform fonts

- **Area:** text
- **Platforms:** Web
- **Status:** open
- **What you hit:** On the desktops and Android, a script the bundled faces lack is drawn with
  the system's fonts. A page cannot reach them, so on the web such a script is drawn as boxes.
- **Workaround:** Register a face that covers the script in `frus::main!({ .. })`.
- **Evidence:** [docs/milestone-784.md](docs/milestone-784.md)

### L-033 A missing character is drawn as a box, and nothing says which

- **Area:** text
- **Platforms:** All
- **Status:** open
- **What you hit:** When no face has a glyph for a character, the face's `.notdef` box is drawn.
  Nothing in the log names the character, which matters most with subset fonts.
- **Workaround:** Keep `--notdef-outline` when subsetting so the box shows while you test.
- **Evidence:** [#20](https://github.com/frus-rs/frus/issues/20),
  [docs/milestone-567.md](docs/milestone-567.md),
  [docs/getting-started.md](docs/getting-started.md)

### L-034 The desktops read every system font at start-up

- **Area:** text
- **Platforms:** Windows, macOS, Linux
- **Status:** open
- **What you hit:** Android loads the platform's fonts lazily; the desktops scan every installed
  font when the application starts, which is part of the time before the first frame. macOS and
  Linux use static per-script fallback lists rather than the platform's own fallback service.
- **Workaround:** None.
- **Evidence:** [docs/milestone-683.md](docs/milestone-683.md),
  [docs/milestone-784.md](docs/milestone-784.md)

### L-035 Editing at a boundary between two directions follows logical order

- **Area:** text
- **Platforms:** All
- **Status:** partial
- **What you hit:** Mixed left-to-right and right-to-left lines are highlighted and caret-placed
  correctly, but a letter of the other direction typed at a boundary is inserted at the logical
  offset (it can appear at the far side of the run), Ctrl and an arrow move by word in logical
  order, the IME paths set the caret downstream, and a single-line field can scroll an upstream
  caret out of view.
- **Workaround:** None.
- **Evidence:** [#56](https://github.com/frus-rs/frus/issues/56),
  [docs/milestone-747.md](docs/milestone-747.md),
  [docs/milestone-764.md](docs/milestone-764.md)

### L-036 A text field gets no spell checking and no process-text items

- **Area:** text
- **Platforms:** Android, Windows, macOS, Linux
- **Status:** open
- **What you hit:** A misspelled word gets no red underline and no suggestions, and the Android
  selection menu does not offer the entries other applications add (Translate, Search). On
  Android the soft keyboard's own suggestions still work.
- **Workaround:** None.
- **Evidence:** [#214](https://github.com/frus-rs/frus/issues/214)

## Input

### L-037 A shortcut has no "Cmd on a Mac, Ctrl elsewhere" key

- **Area:** input
- **Platforms:** All (matters on macOS)
- **Status:** open
- **What you hit:** The framework's own editing keys follow the platform, but a `KeyStroke`
  names Ctrl or the logo key explicitly. An application shortcut meant for Cmd on a Mac and
  Ctrl elsewhere must be bound twice.
- **Workaround:** Bind both strokes to the same action.
- **Evidence:** [docs/milestone-736.md](docs/milestone-736.md),
  [crates/frus-widgets/src/shortcuts.rs](crates/frus-widgets/src/shortcuts.rs)

### L-038 No key deletes a word in a frus text field

- **Area:** input
- **Platforms:** Windows, macOS, Linux, Android with a keyboard
- **Status:** open
- **What you hit:** Ctrl+Backspace (Option+Backspace on a Mac) deletes one character, as
  Backspace does: the field's editing has no word deletion. On the web the browser's own input
  element does it.
- **Workaround:** None.
- **Evidence:** [docs/milestone-736.md](docs/milestone-736.md),
  [crates/frus-widgets/src/interaction.rs](crates/frus-widgets/src/interaction.rs)

## Accessibility

### L-039 TalkBack has no explore by touch and draws no focus outline

- **Area:** a11y
- **Platforms:** Android
- **Status:** open
- **What you hit:** TalkBack reads a frus window node by node: swiping moves through it, a double
  tap presses, fields, sliders and scrolling areas answer. Dragging a finger over the screen to
  hear what is under it does nothing, and the outline TalkBack draws around the node it is on
  does not appear, because the activity's view tree is never drawn.
- **Workaround:** Navigate by swiping.
- **Evidence:** [docs/milestone-782.md](docs/milestone-782.md),
  [docs/platform-integration.md](docs/platform-integration.md)

### L-040 The semantics tree is flat

- **Area:** a11y
- **Platforms:** All
- **Status:** partial
- **What you hit:** Since milestone 848 the tree has a shape on every platform: lists hold
  numbered items, cards are groups, dialogs hold their content, app bars, tab bars, navigation
  and scroll views are containers, headings and tree rows carry their level. What is left: a
  bottom navigation bar, sheets, expansion panels, a data table (rows and cells) and a grid
  (columns) still say nothing of their shape; Android reads a place only for a list's items, not
  for tabs; and the Android side was not run on a device.
- **Workaround:** Give a container its role with `Semantics::new(SemanticsProperties::new(Role::List), ..)`
  and its items `Role::ListItem`; the list numbers them.
- **Evidence:** [docs/milestone-848.md](docs/milestone-848.md),
  [docs/milestone-700.md](docs/milestone-700.md),
  [docs/milestone-811.md](docs/milestone-811.md),
  [crates/frus-widgets/src/ui/semtree.rs](crates/frus-widgets/src/ui/semtree.rs)

### L-041 Web accessibility has not been heard through a screen reader

- **Area:** a11y
- **Platforms:** Web
- **Status:** partial
- **What you hit:** The semantics tree reaches the browser as a layer of elements placed over
  the canvas, checked against Chromium's computed accessibility tree: roles, names, states,
  values, boxes, focus, actions and the live region; Firefox was driven at the DOM level. It
  has not been listened to with a screen reader, Safari was not driven, and no phone was used.
  Since milestone 848 the elements nest (a list's items in the list, a dialog's content in the
  dialog: [L-040](#l-040-the-semantics-tree-is-flat)). A custom action (a row's Archive) has no
  web equivalent and is not offered.
- **Workaround:** None.
- **Evidence:** [#18](https://github.com/frus-rs/frus/issues/18),
  [docs/milestone-564.md](docs/milestone-564.md),
  [docs/milestone-845.md](docs/milestone-845.md),
  [docs/milestone-848.md](docs/milestone-848.md),
  [docs/milestone-711.md](docs/milestone-711.md)

## Widgets and application structure

### L-042 Every rebuild walks the whole widget tree

- **Area:** widgets
- **Platforms:** All
- **Status:** open
- **What you hit:** There is no retained widget tree: a rebuild builds the whole tree from `view`
  or from the root component, and nothing is memoized. Text measurement is cached, so this is
  rarely the bottleneck, but a large tree with one animating widget pays for all of it.
- **Workaround:** Keep animations inside components and lists virtualised (`ListView::builder`).
- **Evidence:** [#53](https://github.com/frus-rs/frus/issues/53),
  [ARCHITECTURE.md](ARCHITECTURE.md)

### L-043 Shared state can only be global

- **Area:** widgets
- **Platforms:** All
- **Status:** open
- **What you hit:** `provide` / `service` keep one value per type for the whole application. Two
  instances of a screen cannot each give their own value to their subtree (the second
  `provide` replaces the first), and a value that changes cannot rebuild only the widgets that
  read it.
- **Workaround:** Pass the value down, or key the global value by the screen's identity.
- **Evidence:** [#222](https://github.com/frus-rs/frus/issues/222),
  [crates/frus-widgets/src/component.rs](crates/frus-widgets/src/component.rs)

### L-044 Routes and translation keys are strings checked at run time

- **Area:** widgets
- **Platforms:** All
- **Status:** open
- **What you hit:** `GoRouter` matches string patterns, so a misspelled location compiles and
  fails when it is opened, and parameters come back as strings to parse. Fluent messages are
  looked up by string key: a typo or a missing translation shows up on screen, in one locale.
- **Workaround:** Keep locations and keys in constants, and test each locale's screens.
- **Evidence:** [#223](https://github.com/frus-rs/frus/issues/223)

### L-045 A router shell inside another shell's branch panics

- **Area:** widgets
- **Platforms:** All
- **Status:** open
- **What you hit:** `GoRoute::shell` gives tabs their own stacks, but a shell inside a branch of
  another shell is refused by an assertion when the router is made, so the application panics
  at start. Branches not on show also restart from their first page after process
  restoration.
- **Workaround:** Keep one level of shell.
- **Evidence:** [docs/milestone-781.md](docs/milestone-781.md),
  [crates/frus-widgets/src/router.rs](crates/frus-widgets/src/router.rs)

### L-046 Only a few widgets have an Apple-style look

- **Area:** widgets
- **Platforms:** iOS, macOS
- **Status:** partial
- **What you hit:** There are Apple-style colours, a theme, and seven widgets (button, switch,
  navigation bar, tab bar, alert, action sheet, activity indicator), and scrolling, page
  transitions and the back swipe follow the platform. The other controls (checkbox, radio,
  slider, pickers…) have no Apple-style counterpart and no adaptive constructor; the large
  title does not collapse on scroll.
- **Workaround:** Style the controls through their themes.
- **Evidence:** [#50](https://github.com/frus-rs/frus/issues/50),
  [docs/milestone-812.md](docs/milestone-812.md)

### L-047 A data table's cell label stays on one line

- **Area:** widgets
- **Platforms:** All
- **Status:** open
- **What you hit:** A table too narrow for its columns runs past its box rather than wrapping the
  labels. Flexible columns placed where the width is unbounded (a row beside other children, a
  horizontal scroll) come out at zero width, and in a virtualised table a data label wider
  than its heading can run into the next column.
- **Workaround:** Give the table a bounded width and its columns fixed or minimum widths.
- **Evidence:** [docs/milestone-758.md](docs/milestone-758.md),
  [docs/milestone-767.md](docs/milestone-767.md)

### L-048 `FileTree` builds every visible row and has no arrow-key navigation between levels

- **Area:** widgets
- **Platforms:** All
- **Status:** open
- **What you hit:** Every row under an expanded node is built, scrolled into view or not,
  which suits hundreds of rows, not tens of thousands. Left on a closed node and Right on an open one do nothing; there is no type-ahead,
  no Home/End, no multi-selection and no drag and drop.
- **Workaround:** Keep large trees collapsed, or show deep levels in a `ListView::builder`.
- **Evidence:** [docs/milestone-811.md](docs/milestone-811.md)

## Tooling and hot reload

### L-049 Hot reload has run end to end on Windows only

- **Area:** tooling
- **Platforms:** Linux, macOS
- **Status:** partial
- **What you hit:** `frus run` patches changes into the running application on Windows, Android
  and the web. On Linux the engine needs OpenSSL's headers and the full run was not seen; on
  macOS it was never tried.
- **Workaround:** `frus run --no-hot` restarts on every change instead; please report what you
  see on #288.
- **Evidence:** [#288](https://github.com/frus-rs/frus/issues/288),
  [docs/milestone-677.md](docs/milestone-677.md)

### L-050 A hot patch cannot change the shape of live state

- **Area:** tooling
- **Platforms:** All
- **Status:** open
- **What you hit:** A patch replaces code, not data. Adding a field to a state that is alive, or
  otherwise changing its layout in memory, needs a restart.
- **Workaround:** Press `R` under `frus run` to restart afresh.
- **Evidence:** [docs/milestone-669.md](docs/milestone-669.md)

### L-051 Hot reload on Android builds a different package

- **Area:** tooling
- **Platforms:** Android
- **Status:** open
- **What you hit:** Under hot reload the engine builds its own Android project. Its manifest
  declares only INTERNET, VIBRATE and ACCESS_NETWORK_STATE, so a permission the application
  declares (the camera, notifications) is missing there; purchases answer `Unsupported`; and a
  flavor that changes the package name restarts instead of patching.
- **Workaround:** `frus run --no-hot` for the features above.
- **Evidence:** [docs/milestone-763.md](docs/milestone-763.md),
  [docs/milestone-790.md](docs/milestone-790.md),
  [docs/milestone-807.md](docs/milestone-807.md),
  [crates/frus-cli/src/hot.rs](crates/frus-cli/src/hot.rs)

### L-052 Web hot patches take seconds

- **Area:** tooling
- **Platforms:** Web
- **Status:** open
- **What you hit:** A patch on the web takes 7 to 9 s, and the first about 40 s, mostly spent by
  the engine processing the debug wasm (about 137 MB).
- **Workaround:** None.
- **Evidence:** [docs/milestone-816.md](docs/milestone-816.md)

### L-053 `frus mcp` answers one request at a time

- **Area:** tooling
- **Platforms:** All
- **Status:** open
- **What you hit:** While `launch_app` waits for a build, every other request waits too, and a
  cancellation is not acted on. Only tools are offered, not resources or prompts.
- **Workaround:** Pass a smaller `wait_seconds` and poll `app_status`.
- **Evidence:** [docs/milestone-789.md](docs/milestone-789.md)

## Packaging and size

### L-054 The bundled fonts are a large share of a small application

- **Area:** packaging
- **Platforms:** All
- **Status:** partial
- **What you hit:** The bundled faces are about 3.4 MB, roughly 40% of a minimal application.
  A release `frus build` now cuts them, and the application's own fonts embedded with
  `include_font!`, to the characters the application can show: Latin, the scripts of its
  languages, the widgets' symbols and the text of its sources. What is left: a character
  outside those is drawn as a box (the log names it), until its language or range is declared;
  a build made with `cargo` by hand, or `frus run`, embeds the fonts whole; a font embedded
  with `include_bytes!` is not cut; the sizes on macOS were not measured.
- **Workaround:** Declare the languages and ranges the application shows in
  `[package.metadata.frus.fonts]`; turn off the faces you do not need.
- **Evidence:** [#20](https://github.com/frus-rs/frus/issues/20),
  [docs/milestone-292.md](docs/milestone-292.md),
  [docs/milestone-567.md](docs/milestone-567.md),
  [docs/milestone-823.md](docs/milestone-823.md),
  [docs/guide/text.md](docs/guide/text.md)
