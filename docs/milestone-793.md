# Milestone 793 — External textures: video, camera, maps, native views

## Objective

Pixels that widgets do not draw — a video decoder's frames, a camera preview, a map engine, a
native view — must be shown inside a frus interface, fed from any thread, at no cost while
nothing new arrives. This is the foundation that video, camera and platform-view plugins build
on.

## API

```rust
use frus::{textures, Texture, BoxFit, YuvPlane, YuvColorSpace, TextureSurface, SurfaceFormat};

// A producer, on any thread.
let texture = textures().create();               // ExternalTexture: Send + Sync, unregisters on drop
texture.push_rgba(w, h, &rgba)?;                 // also push_bgra, push_rgba_strided
texture.push_nv12(w, h, y_plane, uv_plane, YuvColorSpace::Bt709Limited)?;
texture.push_i420(w, h, y, u, v, YuvColorSpace::Bt601Limited)?;
texture.push_yuv420(w, h, y, u, v, space)?;      // any strides: Android's YUV_420_888
texture.stats();                                 // pushed / dropped / shown

// The widget.
Texture::new(texture.id()).fit(BoxFit::Contain).background(Color::BLACK);

// Android: a surface a decoder, a camera or a canvas draws into.
let surface = TextureSurface::new(1280, 720, SurfaceFormat::Yuv420)?;  // Unsupported elsewhere
surface.native_window();   // ANativeWindow* for the NDK (AMediaCodec_configure)
surface.java_surface()?;   // JNI global ref to an android.view.Surface, for Java/Kotlin
Texture::new(surface.id());
```

## Design

### The registry (`frus-core/src/texture.rs`)

A process-wide table (`textures()`) of entries, each holding the **latest** frame as an
`Arc<TextureFrame>` and up to two spare frames. A push:

1. takes a spare nobody else holds (`Arc::strong_count == 1`), or allocates one while the ring
   warms up — so once warm, a steady producer allocates nothing (tested: 50 frames, at most 3
   buffers);
2. copies the pixels into it **outside** the lock (padded rows, pixel strides and the crop are
   resolved here, into a tightly packed layout);
3. swaps it in as the latest under the lock. The frame it replaces goes back to the spares; if
   no window had taken it, it counts as **dropped**. Only the newest frame is ever kept: a
   producer faster than the display falls behind by nothing.
4. asks the shell for a frame through a wake hook (the shell installs
   `window.request_redraw`), **coalesced** by an atomic flag the shell clears when it starts a
   frame: ten pushes before a frame ask for one frame; no push, no frame. There is no polling
   anywhere and no thread of frus's own.

A texture id is drawn from the same counter as image ids, so it is never an image's.

### Drawing (`ImageData::external`, `frus-gpu/src/external.rs`)

The widget paints an ordinary `Primitive::Image` whose `ImageData` is an **external stand-in**:
no pixels, the texture's id as its id, and the frame's generation. Everything that already
handles images — clipping, layers, transforms, DPI scaling, batching — handles textures with no
new primitive variant. Two stand-ins are equal only for the same frame, so the compositor's
layer cache, which compares primitives, re-renders a layer holding a texture when a frame
arrives and reuses it otherwise.

The image painter, meeting an external image, asks the registry for the latest frame. If its
cached texture already holds that generation nothing happens; otherwise:

- the same size and layout: the planes are **written into the textures already there** (no
  texture, no bind group created);
- RGBA is written straight into the sampled `Rgba8UnormSrgb` texture;
- BGRA, NV12 and I420 are written into plane textures (`Rgba8UnormSrgb` for BGRA, `R8Unorm` /
  `Rg8Unorm` for YUV) and converted by one full-screen pass of `external.wgsl` into the sampled
  texture. The matrix and offsets for BT.601/BT.709, limited/full range are computed in Rust and
  tested on the CPU (black and white reach 0 and 255, the primaries land within 2 of the
  standards' values). Chroma is filtered bilinearly by hand from `textureLoad`, which works on
  every backend including WebGL2.

The frame's `Arc` is dropped right after the upload, so its buffer returns to the producer.

### Repainting only what changed (`Widget::paint_source`)

A repaint boundary replays its cached primitives when nothing it hashes has moved; a
`Texture` inside one would keep naming an old frame. The new `Widget::paint_source` hook returns
a value the paint reads from outside the tree — the `Texture` returns its frame's generation —
and the boundary fingerprint hashes it. A new frame repaints that boundary and nothing else; the
view is not rebuilt. Transparent wrappers (`forward_transparent!`, `Responsive`, `Box<dyn
Widget>`) forward it.

### Android (`frus-shell/src/texture_surface.rs`)

`TextureSurface` wraps an NDK `AImageReader` (`YUV_420_888` for decoders and cameras,
`RGBA_8888` for a canvas or GL producer), 3 images deep. Its window is handed out as the
`ANativeWindow*` and, through `ANativeWindow_toSurface`, as a JNI global reference to an
`android.view.Surface` — the shape the leading toolkit's surface producer has, so `MediaCodec`,
Camera2, ExoPlayer or `lockHardwareCanvas` target it directly, from the NDK or from Java. The
reader's listener runs on the reader's own thread: it acquires the latest image, reads its
planes (honouring the crop rectangle and pixel strides; interleaved chroma stays NV12, NV21 is
gathered to I420) and pushes them.

frus applications start at API 24 and `AImageReader_*` / `ANativeWindow_toSurface` are not all
there; every function is looked up with `dlsym` the first time a surface is made, so nothing is
linked, an application that never makes a surface pays nothing, and `java_surface` answers
`Unsupported` before Android 8. New dependency: `ndk-context` 0.1 (Android only, already in the
tree through winit) for the Java VM.

This is the path J794's platform views can use: a native view drawn into the surface's
`Canvas` (RGBA) shows as a texture.

### The demo

`crates/frus-demo/src/screens/video.rs` (`/video`, linked from the menu):

- desktop: a thread draws a 480×270 animated pattern at 60 fps and pushes RGBA;
- Android: the NDK decoder (`AMediaExtractor`/`AMediaCodec`, also via `dlsym`) plays
  `assets/texture-demo.mp4` — 320×240 H.264, 4 s, 80 KB, generated with
  `ffmpeg -f lavfi -i testsrc2` (no third-party content) — into a `TextureSurface`, looping;
- the web: one frame pushed once.

Leaving the screen stops the producer and drops the texture.

## How it was tested

### Unit tests

- `frus-core` (`texture.rs`, 12): a dropped texture leaves the registry and shows nothing; a
  frame taken before its texture went stays readable; a producer pushing five frames before a
  window takes one has four counted as dropped, and taking the same frame twice is one showing;
  ten pushes before a frame wake the shell once, an idle texture wakes it never, unregistering
  wakes it once; 50 frames go through at most 3 buffers; a frame the renderer holds is never
  written over; wrong lengths and sizes are errors, not panics; padded I420, padded NV12, NV12
  kept interleaved and NV21 gathered to I420; four producer threads never tear a frame;
  external stand-ins differ by frame.
- `frus-gpu` (`external.rs`, 3): limited-range black and white reach 0 and 255, BT.601/BT.709
  primaries within 2 of the standards, the uniform is sized for WebGL.
- `frus-widgets`: the widget draws nothing before the first frame or after the texture is gone,
  fits the frame and names its generation, moves its `paint_source` on a push only, paints its
  background; a repaint boundary around a texture is replayed while no frame arrives and
  painted again (naming the new frame) when one does.

### GPU and goldens (`frus-test/tests/external_texture.rs`)

RGBA, BGRA, NV12 and I420 frames of red and blue halves come out red and blue (within 1, and 6
for 4:2:0 chroma); a second frame of the same size replaces the first on the next render; a
texture that is gone draws nothing; `external_texture_layouts.png` shows the four layouts
side by side (generated under WSL, lavapipe). Passed on Windows (Iris Xe, D3D12) too.

### Windows, the real window

`frus-demo frusdemo://video`, Intel Iris Xe, debug build (optimised), 822×656 window:

| | value |
| --- | --- |
| frames | 60–61 fps, interval median 16.7 ms, 95th 18.6–20.5 ms, 0 over budget |
| per frame | no rebuild; layout+paint 0.4 ms (95th 0.6); render 15.5 ms of which waiting for an image 2.7, drawing 9.0, GPU 9.5 (4× MSAA) |
| frames dropped | 31 of the first 121 (while the window was starting), then 1–3 per 120 |
| CPU while playing | 66 % of one core (the producer drawing 480×270 pixels in Rust each frame included) |
| CPU with the texture shown, producer stopped (`FRUS_TEXTURE_DEMO_FRAMES=60`) | **0.0 %** over 10 s, no frame drawn |

### The phone (Huawei STK-L21, Mali-G51, Vulkan 1.1.97, Android 10), release APK

The *External texture* screen opened with `am start -d frusdemo://video`:

- the clip plays through `MediaCodec` → `TextureSurface` (log: `texture surface 320×240
  (Yuv420): CPU planes through an image reader`, `Java surface ready`, `decoding into the
  texture surface`), colours correct (screenshot checked);
- 30 fps, the clip's rate: interval 33.3 ms (95th ≈ 35 ms), no rebuild, layout+paint 0.4–0.8 ms,
  render 6.3–11 ms median (drawing 2–5 ms);
- 880 frames pushed, 47 dropped (most while the page transition ran), 832 shown;
- CPU: home screen idle 0.0 %; while playing 47–54 % of one core (decoder thread, plane copy,
  upload and the 30 fps frames together); after leaving the screen **0.0 %**;
- memory (PSS): home 129–139 MB; playing 300 MB; after leaving 283–293 MB. The jump is not the
  texture's: opening `/charts` and going back gives the same +134 MB of `Graphics` (home 67 MB,
  `/charts` 202 MB, back 199 MB). The video screen costs about 10 MB more than `/charts`
  (decoder, conversion pipeline, plane textures). Why the navigation keeps 130 MB of graphics
  memory is a separate question, noted below.
- APK: 8.1 MB (release, arm64); the clip is 80 KB of it.

The phone's Vulkan driver offers `VK_ANDROID_external_memory_android_hardware_buffer` and
`VK_KHR_sampler_ycbcr_conversion` (`cmd gpu vkjson`), so the zero-copy import is possible on
this device; it is not done (below). The path that ran is the CPU-plane copy.

Checks: `cargo clippy --workspace --all-targets -D warnings` (Windows and Linux),
`aarch64-linux-android` clippy of frus-shell and frus-demo, `wasm32-unknown-unknown` clippy of
the crates touched, `cargo check --target aarch64-apple-ios` of frus-shell and frus-widgets, the
guide's examples compiled as doctests, the workspace's tests under Linux.


## Not done

- **Zero-copy on Android.** Frames are copied from the image reader's CPU planes. Importing the
  `AHardwareBuffer` into Vulkan (the phone supports the extension) needs the extension enabled
  at device creation (`wgpu-hal` `open_with_callback`) and, for decoder and camera buffers, a
  `VkSamplerYcbcrConversion` WGSL cannot express; neither is done. No GLES fallback import
  either.
- **Windows D3D11/DXGI shared handles**, **macOS IOSurface**: not done; desktop producers push
  CPU frames.
- **The web's `<video>` / `ImageBitmap` through `copyExternalImageToTexture`**: not done. A web
  producer pushes pixels like anyone else; the demo pushes one frame there. Not run in a browser.
- **A GPU frame from the producer** (`push_frame` with a texture the producer rendered): not
  done; the only frames are CPU ones and the Android surface's.
- **Upload staging**: frames are uploaded with `queue.write_texture`, which stages through wgpu's
  own buffers; the ring of reused buffers is on the producer's side.
- **Freeze, and showing a frame sized differently from the last before layout catches up**: the
  widget draws the frame size it read at paint; a size change between paint and render
  stretches one frame.
- **The navigation's graphics memory**: going to any demo screen and back leaves about 130 MB
  of `Graphics` PSS on the phone (seen with `/charts` too); not investigated here.
- **iOS**: `TextureSurface` answers `Unsupported`; pushing frames works.

