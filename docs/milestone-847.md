# Milestone 847 — A web page that works in every browser

## Objective

A frus web application or website must work in any browser: Chrome and Edge, Firefox, Safari,
the browsers of phones, with WebGPU or without it. Two things stood in the way.

1. **A desktop browser behaved like an Android phone.** In the web demo, opened in Edge on
   Windows, a list pulled past its end stretched like Android's, and lists had no scrollbar.
   On a desktop the convention is a list that stops at its ends (macOS bounces), with no
   indicator past the edge and a scrollbar.
2. **A WebGPU that fails at the canvas left the page blank** (L-008). frus chose WebGPU once it
   had a device, and a canvas that then refused its `webgpu` context, refused the configuration
   or gave no frame could not fall back to WebGL 2: the page showed an error, or nothing at all.

And web checks had only ever run in headless Edge.

## What was wrong

**The platform was read too late.** The shell read the browser's system from the navigator at
the start of `run_web` and recorded it, and `default_target_platform()` reported it from then
on. But `frus::main!(app())` evaluates `app()` first, and an application made there builds its
theme there (`Theme::default()`, a seeded theme): `Theme::platform` was filled from
`default_target_platform()` before anything had been recorded, and on `wasm32` the fallback is
the build's platform, Android. The shell's own console line said `target platform: Windows,
from the browser` while every widget followed Android: the stretch past the edge, no scrollbar,
Android's fling. The demo's Cupertino screen, whose adaptive switch prints `theme.platform`,
said `Adaptive (Android)` in Edge, Firefox and WebKit on Windows, and on an emulated iPhone.

The scroll tables themselves (milestone 616) were right: clamping physics and no indicator on
Windows and Linux, bouncing on macOS and iOS, the stretch on Android.

**The WebGPU choice ended at the device.** Milestone 745 made the device before the surface,
because a canvas that has handed out a `webgpu` context never hands out a `webgl2` one. But the
surface could still fail after that point, and by then there was no way back.

## What was done

**The platform is read when it is first asked for.** `default_target_platform()` on `wasm32`
reads `navigator.platform`, `navigator.userAgent` and `navigator.maxTouchPoints` the first time
nothing has been recorded, and keeps the answer: whatever made the theme, and whenever, gets the
browser's system. The shell still records what it reads at start-up (and logs it). `frus-core`
reads the navigator through `js-sys`, a target-specific dependency already in every web build.
`TargetPlatform::from_browser` also reads the user agent alone when `navigator.platform` is
empty (privacy settings in Firefox do that), and reads a BSD or a ChromeOS as Linux, where both
fell to the "unrecognised" Android answer.

Nothing is forced: `Theme::platform`, `Theme::with_platform`, `ScrollConfiguration` and a
scrollable's own physics still decide over the default, as on every platform.

**The page's own end-of-scroll effects are off.** The shell sets `overscroll-behavior: none` on
the page's root and body, beside the canvas's `touch-action: none` (milestone 779): a desktop
browser's elastic bounce on a touchpad, its swipe back through the history at a sideways edge
and a phone's pull to refresh no longer act over the application, which draws its platform's own
edge. A page that sets `overscroll-behavior` keeps its own.

**The WebGPU device is tried on a canvas of its own.** Once a WebGPU device is made,
`web_backend::open` draws once through it in a canvas the page never shows: a `webgpu` context,
the format chosen as the renderer chooses it (`renderer::canvas_formats`, now shared), the same
configuration, a frame cleared and presented, and the device's validation errors collected in an
error scope. Any failure — no context, a configuration refused, no frame, a validation error — is
`WebGpu::NoSurface(why)`: the device is destroyed and the page draws with WebGL 2, its canvas
never having been touched. The console says `GPU interface: WebGL 2 (this browser's WebGPU cannot
draw in a canvas: …)`.

**A page canvas that refuses WebGPU falls back too.** If the trial passed and the page's own
canvas still gives no `webgpu` context, that canvas has been given no context at all, so the
renderer takes it again through WebGL 2 instead of failing.

**A check for every browser engine.** `scripts/web-browsers-check.mjs` (Playwright, and WebDriver
BiDi for the Firefox installed on the computer) opens a built site in each engine and checks
that it starts, draws through the GPU interface expected, follows the right platform (the
demo's `Adaptive (…)` row) and scrolls with the wheel, or with a swipe on a phone. The WebGPU
failures are forced by page scripts that run before the application: `navigator.gpu` removed,
`getContext('webgpu')` answering `null` everywhere or on the page's canvas only, and
`GPUCanvasContext.configure` throwing.

## Evidence

The demo built for `wasm32` (`web-release`, with LTO off and 16 codegen units to keep the build
short; the code is the same), bound with `wasm-bindgen` 0.2.127, served locally. On Windows 11,
Intel Iris Xe.

**Before** (a build of the demo from `master` before this change, served locally; the "page's
canvas only" case was added after that server had gone, and fails the same way as the first
`getContext` case before this change, since nothing there takes the canvas again):

| run | started | GPU interface | platform | scrolls |
|---|---|---|---|---|
| Edge 158, WebGPU | yes | WebGPU | **Android** | yes |
| Edge, no `navigator.gpu` | yes | WebGL 2 | **Android** | yes |
| Edge, `getContext('webgpu')` null | **no**: "failed to initialise the renderer: canvas.getContext() returned null" | — | — | — |
| Edge, `configure` throws | **blank canvas** (the layer, nothing drawn) | WebGPU | **Android** | — |
| Firefox 160 (installed) | yes | WebGL 2 (no WebGPU adapter headless) | **Android** | yes |
| Firefox 159 (Playwright) | yes | WebGL 2 | **Android** | yes |
| WebKit 27.2 (Playwright) | yes | WebGL 2 (no `navigator.gpu`) | **Android** | yes |
| Edge as a Pixel 7 | yes | WebGPU | Android | no (see L-055 below) |
| WebKit as an iPhone 15 | yes | WebGL 2 | **Android** | yes (pointer swipe) |

**After** (`node scripts/web-browsers-check.mjs http://127.0.0.1:18847/`):

| run | started | GPU interface | platform | scrolls |
|---|---|---|---|---|
| Edge 158, WebGPU | yes | WebGPU | Windows | yes |
| Edge, no `navigator.gpu` | yes | WebGL 2 | Windows | yes |
| Edge, `getContext('webgpu')` null everywhere | yes | WebGL 2 ("cannot draw in a canvas: canvas.getContext() returned null") | Windows | yes |
| Edge, `getContext('webgpu')` null on the page's canvas only | yes | WebGPU on the trial, then WebGL 2 ("the page's canvas refused WebGPU") | Windows | yes |
| Edge, `configure` throws | yes | WebGL 2 ("no frame to draw in") | Windows | yes |
| Firefox 161 (installed, WebDriver BiDi) | yes | WebGL 2 (no WebGPU adapter headless) | Windows | yes |
| Firefox 159 (Playwright) | yes | WebGL 2 | Windows | yes |
| WebKit 27.2 (Playwright, Windows) | yes | WebGL 2 (no `navigator.gpu`) | Windows | yes |
| Edge as a Pixel 7 (touch swipe) | yes | WebGPU | Android | yes |
| WebKit as an iPhone 15 (touch swipe) | yes | WebGL 2 | iOS | yes |

Every run passes. Each canvas holds 150 to 450 colours in a sparse sample (a blank canvas gives
9: the page's header alone).

**Found on the way.**

- *The installed Firefox and the wheel.* Driven over WebDriver BiDi, a wheel turned before the
  pointer had ever moved over the page scrolled nothing: the application scrolls what is under
  the pointer, and knew of no pointer. A hand always moves the mouse first; the check now does
  too. The wheel events themselves were pixel deltas on the canvas, as in Chromium.
- *Firefox's launcher.* On Windows `firefox.exe` starts the browser again as a process of its
  own and leaves, so ending the started process's tree left the browser running; the check ends
  every process of its own profile.
- *An emulated iPhone was read as Windows.* Playwright's WebKit keeps the computer's
  `navigator.platform` (`Win32`) with an iPhone's user agent, and `platform` was asked before
  the user agent's iPhone. An iPhone or Android user agent now wins over the `platform`, which a
  desktop browser emulating a phone keeps as the computer's.
- *Chromium's phone emulation draws at the wrong scale* (L-055). With an emulated pixel ratio
  (a Pixel 7 at 2.625), Chromium reports the canvas's device-pixel box at its CSS size
  (`ResizeObserver`'s `devicePixelContentBoxSize` was 100 by 50 for a 100 by 50 CSS box, also
  with "Desktop Chrome HiDPI" at 2), so the canvas's image is 412 pixels wide while the
  application draws at 2.625: everything 2.6 times too large, and a swipe that scrolled nothing.
  WebKit's emulation and real devices report the scaled size. The check emulates the Android
  phone at a ratio of 1; the fix belongs to the windowing layer, which reads that box.

Unit tests: `frus-core` (`a_browser_reports_the_system_it_runs_on`, with an empty platform, a
BSD and a ChromeOS), `frus-gpu` (`a_device_that_cannot_draw_in_a_canvas_falls_back_to_webgl2`,
`a_canvas_trial_fails_on_either_failure`), `frus-shell`
(`the_page_has_no_overscroll_effect_of_its_own`).

## Not done

- **Real Safari** on macOS and iOS, and real phone browsers, were not run: WebKit here is
  Playwright's build on Windows, and the phones are emulated by Chromium and WebKit (their user
  agents, sizes and touch). The test phone was not used.
- **WebGPU in Firefox and WebKit** was not exercised: headless, neither gave a WebGPU adapter
  here, so both drew through WebGL 2.
- **A canvas that passes the trial and fails at configuration on the page's own canvas** still
  cannot fall back (it holds its `webgpu` context); no browser has been seen to do it. A renderer
  made again after a lost device stays on the interface its canvas holds. Both are in L-008.
- **A wheel that scrolls by pages** (Windows' "one screen at a time" setting, `deltaMode` 2) is
  dropped by the windowing layer before frus sees it. Lines and pixels are handled.
- The overscroll indicator itself is not read from the pixels: the check reads the platform the
  theme follows, and the scroll tables are the unit-tested ones of milestone 616.
