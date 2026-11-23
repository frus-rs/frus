# Milestone 840 — Memory given back when the system is short of it

## Objective

Part of #221. Decode to size (687), decoding off the interface's thread (688) and mip chains
for images drawn small (743) are done. Two things were left:

- **Memory pressure.** Android's `onTrimMemory`, the low-memory callbacks and a hidden page
  should shrink what the process holds, instead of leaving the system to end it. Until now
  only the low-memory callback was handled, and only the fetched pictures nobody showed went.
  The GPU's textures, often the larger share, stayed.
- **Evidence** that a grid of 200 large photos stays within a stated memory figure, and that
  no large picture is decoded on the interface's thread.

The mipmap item of #221 needed nothing new: milestone 743 gives an image drawn smaller than
its pixels a mip chain and a trilinear sampler (the default `FilterQuality::Medium`).

## Design

### One call, three levels

`frus_widgets::trim_caches(level: TrimLevel)` is the one entry point. frus calls it, and an
application can too: on leaving a screen full of photographs, or from its own signal on a
desktop, where the system gives none. `TrimLevel` lives in frus-core and is re-exported.

| Level | Decoded images | GPU |
|---|---|---|
| `Moderate` | the ones nobody shows | nothing |
| `Critical` | the ones nobody shows | the glyph atlas back to its first size, the rasterised glyphs, the triangles kept for paths |
| `Hidden` | the ones nobody shows | every texture the next frame can make again: kept layers, image textures and mip chains, multisampled scratch targets, the staging texture, every window's surface targets |

A picture on screen is never given back, since the next frame would only decode it again.
Between frames the renderer holds what the last frame drew, so `Moderate` leaves the GPU
alone: what a frame no longer uses is already evicted at its end. The glyph atlas is the
exception, since it grows to the most text a frame ever drew and never shrinks; that is why
`Critical` replaces it. `Hidden` is for an application nobody can see. Its next frame is far
off, and the first frame back renders and uploads what it draws again. An external texture
(milestone 793) is kept: its producer may not write its frame again.

### Threads

The decoded images are behind locks, so they go at once, from whatever thread asks. The GPU
belongs to the renderer on the loop's thread. `trim_caches` records the request in an atomic
(several requests before the loop turns become one, at the highest level) and asks for a frame.
The shell takes it at the start of the loop's next turn (`new_events`) and hands it to the
renderer (`Renderer::trim_caches`) and to every window's surface (`Renderer::trim_surface`). A
request made while nothing renders, such as an Android application in the background whose
renderer is already gone, is simply taken.

### Where the signals come from

- **Android `onTrimMemory`.** The native activity does not forward it to native code. A small
  Java class in the bundled dex, `FrusMemory`, registers itself as a `ComponentCallbacks2`
  on the application's context, once a process, and hands the level to a native method
  registered through the input bridge's class loader. The levels map as the platform defines
  them: `RUNNING_MODERATE` and `RUNNING_LOW` to `Moderate`, `RUNNING_CRITICAL` to `Critical`,
  `UI_HIDDEN` and every background level to `Hidden`.
- **Android `onLowMemory` and the iOS memory warning** reach winit's `memory_warning`, which
  now trims at `Critical` instead of dropping only the fetched pictures.
- **The Web and the desktops.** A hidden browser tab (winit listens to `visibilitychange`), a
  canvas scrolled out of view and a minimised window arrive as `WindowEvent::Occluded`. When
  no window of the application can be seen, the shell trims at `Hidden`. Browsers give a page
  no memory signal of its own.
- **`frus run`'s `trim-memory` command** now takes a level: `moderate`, `critical` (the
  default) or `hidden`.

### Seeing it

- Each trim logs what it gave back: the decoded images in KB, and the GPU textures in MB.
- Each frame statistics line (`debug.frus.frames`, `FRUS_FRAMES`) now also says what the
  decoded images hold, and how many pictures were decoded since the line before, in place (on
  the thread that asked, the interface's for a widget) and on the decoding threads
  (`image_decodes()`, `decoded_image_bytes()`, new and public). That answers "no decode on the
  UI thread" with a number on every line.

## How it was tested

- `frus-gpu` `compositor::tests::a_trim_gives_back_gpu_memory_and_the_next_frame_rebuilds_it`:
  a frame with a photograph drawn small (a mip chain), a kept layer, a path and more text than
  the first glyph atlas holds is rendered on the GPU. `Moderate` changes nothing. `Critical`
  brings the atlas back to its first size, drops the path triangles and keeps the layer.
  `Hidden` leaves the painters holding exactly what they held when made. The same frame drawn
  again holds what it held before.
- `frus-widgets` `memory::tests::trimming_gives_back_unshown_pictures_and_asks_the_gpu_from_critical_up`:
  the picture held is kept, the one nobody holds is decoded again, `Moderate` leaves no GPU
  request, and two requests before the loop turns are taken once, at the higher level.
- `frus-shell` `memory_pressure::tests::android_levels_map_to_how_much_to_give_back`: every
  level the platform defines.
- `frus-core` `image::tests::decodes_are_counted_by_where_they_happen`, and
  `frame_stats::tests::a_line_says_the_decoded_images`.
- The workspace's check, clippy (`-D warnings`), the tests of frus-core, frus-widgets,
  frus-shell and frus, all 98 frus-gpu tests, the core exports guard, clippy for
  `aarch64-linux-android` and `wasm32-unknown-unknown`, `cargo check` for `aarch64-apple-ios`,
  and rustdoc with `-D warnings`.

## Measured

Not yet. The measurement on the test phone is owed: a release build of a grid of 200
synthetic 4000x3000 JPEG photographs, served from the laptop through `adb reverse`, scrolled to
the bottom and back, then sent `RUNNING_CRITICAL` and hidden, with `dumpsys meminfo` read at
each step and the frame statistics lines read from the log. The phone was locked when the
build was ready. The figures, and the stated memory bound they support, will be added to this
note when that run is made.

## Not done

- **The phone measurement** above, and with it the evidence item of #221: a stated memory
  figure for the grid of 200 photographs, and zero decodes in place while it scrolls.
- **A desktop memory signal.** Windows, macOS and Linux give a running application no
  low-memory callback that winit forwards; only an occluded or minimised window trims (at
  `Hidden`). An application can call `trim_caches` from its own signal.
- **A Web memory signal.** Browsers give a page none; only a hidden page trims.
