# Milestone 800 — Android frames without a copy, shaders compiled ahead

## Objective

Two costs milestones 792 and 793 left on the GPU side:

1. **A video frame was copied twice.** A `TextureSurface` on Android read every frame of a
   decoder or a camera from the image reader's CPU planes, packed it, and uploaded it. The
   phone's GPU can read the decoder's buffer where it lies; frus should let it.
2. **A shader's first frame hitched.** The GPU pipeline of a custom shader was compiled when
   a frame first drew it, on the render thread, and that frame waited for the driver.

## API

An application that shows a `TextureSurface` turns on the `hardware-buffers` feature of
`frus` (off by default: an application with no video pays no byte for it); nothing else
changes. One that loads a `Shader` turns on `warm-shaders` and gets the warm-up, nothing else
changing. Two additions, for a producer of its own:

```rust
use frus::{HardwareBuffer, YuvColorSpace};

// Does the running renderer import hardware buffers? (Vulkan on Android, with the extensions.)
if frus::textures().hardware_buffers() {
    // `ptr` is an AHardwareBuffer* the producer keeps unwritten until `release` runs.
    let frame = unsafe { HardwareBuffer::new(ptr, width, height, move || release(image)) }
        .unwrap()
        .with_crop(0, 0, 1280, 720);
    texture.push_hardware_buffer(frame, YuvColorSpace::Bt709Limited)?;
} else {
    texture.push_nv12(/* ... copied planes ... */)?;
}
```

## Design

### Opening the device (`frus-gpu/src/hardware.rs`, `renderer.rs`)

Importing an `AHardwareBuffer` needs `VK_ANDROID_external_memory_android_hardware_buffer`
(and what it builds on: external memory, dedicated allocation, the foreign queue family) and
the **sampler Y'CbCr conversion** feature. wgpu asks for none of them. On Android, when the
adapter is Vulkan 1.1 and offers the extension and the feature, frus opens the device itself:
wgpu-hal's `required_device_extensions` and `physical_device_features` give what wgpu needs,
the import extensions are added, `vkCreateDevice` is called, and the device is handed to wgpu
with `device_from_raw` / `create_device_from_hal`. wgpu chains the Y'CbCr feature structure
on a 1.1 device but leaves it off; frus switches it on in the chain rather than chaining a
second one, which Vulkan forbids (`open_with_callback` cannot do this: its callback sees the
chain before wgpu adds its structures). Any failure on the way opens the plain device instead,
and the frames are copied. The renderer then says what it opened:
`textures().set_hardware_buffers(true | false)`.

### The surface (`frus-shell/src/texture_surface.rs`)

When the renderer imports hardware buffers, a `TextureSurface` is an
`AImageReader_newWithUsage` with `AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE`, in the
**producer's own format** (`AIMAGE_FORMAT_PRIVATE`) for YUV surfaces — the decoder or the
camera writes what suits it, possibly a compressed layout no CPU could read — and RGBA for a
canvas. Its listener does no work on the pixels: it takes the newest image, its
`AHardwareBuffer` (`AImage_getHardwareBuffer`), the buffer's real size
(`AHardwareBuffer_describe`) and the crop, and pushes a `HardwareBuffer` whose release deletes
the image. All of these are looked up with `dlsym` (Android 8), as milestone 793's functions
are. Otherwise the surface is what it was: YUV_420_888 planes copied.

The reader holds three images, as the copying one does: the newest (waiting for a frame),
the one the GPU may still be reading, and one free for `acquireLatestImage`. Not more: a
decoder sizes its pool of output buffers by adding the reader's count to what it needs, and the
test phone's decoder refuses a pool larger than 25 buffers. With five images it did not start
(`setting nBufferCountActual to 30 failed`), found on the phone; with three it asks for 25.
The reader itself is deleted only when the surface is gone **and** the last image it handed out
is back (`ReaderHandle`, reference-counted by every image held), so a frame the GPU reads after
the surface was dropped is never one of a reader already deleted.

### The registry (`frus-core/src/texture.rs`)

A `TextureFrame` may now hold a `HardwareBuffer` instead of bytes: no planes, its size the
crop's. Such a frame never goes back to the spares (that would hold the producer's buffer for
ever): the one it replaces is dropped, so released, as soon as nothing else holds it, and its
release runs outside the registry's lock. A push answers `Unsupported` (and releases the
buffer) when no renderer imports buffers.

### Drawing it (`hardware.rs`, `external.rs`)

The decoder's buffer is YUV in a layout only the driver knows (an *external format*); the only
way to read it is through a combined image sampler carrying a `VkSamplerYcbcrConversion`,
fixed in the descriptor set layout. WGSL has no way to name such a sampler and wgpu has no way
to make one. Two ways were weighed:

- **Sample the buffer in the image painter's own pipeline**: a hand-made Vulkan copy of that
  pipeline, its batching, clipping and blending, per buffer format. Large, and a second
  implementation of the most used pipeline frus has.
- **Convert it in a small Vulkan pass into an RGBA texture wgpu owns** (chosen). The pass is
  one triangle the size of the video, reading the buffer once per *new* frame (nothing on
  frames that bring none), and everything after it — clipping, layers, transforms, filtering,
  the layer cache — is the unchanged path of milestone 793's frames.

The pass is GLSL compiled to SPIR-V (`shaders/hardware.vert`, `hardware.frag`, built with the
NDK's `glslc`; the `.spv` files are checked in), sampling through the immutable sampler and
writing through an `Rgba8Unorm` view of the `Rgba8UnormSrgb` texture: the conversion answers
non-linear R'G'B', so the bytes stored are what an sRGB texture holds, as the copying path
stores. The conversion's matrix and range follow the surface's colour space
(`set_color_space`), as the copying path's do; the component mapping and chroma siting come
from the buffer (`vkGetAndroidHardwareBufferPropertiesANDROID`); chroma is filtered linearly
where the format allows it. An RGBA buffer is read through a plain sampler by the same pass.

Per buffer, once: a `VkImage` with the external format, its memory imported from the buffer
(dedicated), a view through the conversion, a descriptor set. An image reader cycles through a
handful of buffers, so the imports are kept, by buffer, and one unseen for 30 frames is let
go. Vulkan's import holds its own reference to the buffer while the memory lives, so a cached
pointer cannot be reused by another buffer.

**Recording and synchronisation.** The pass is recorded into a wgpu command encoder through
`as_hal_mut` and submitted with `queue.submit`, so it takes its place in wgpu's ordering on the
one queue, before the frame that samples the texture.

- The texture's state is wgpu's to track. A new texture is first **sampled once through wgpu**
  (a 1×1 pass of the copying path's converter): wgpu makes it, clears it and records it ready
  for sampling. From then on the pass takes it from *undefined* to a colour attachment (its
  whole area is redrawn) and leaves it ready for sampling, which is what wgpu believes; its
  barriers order it after the earlier frames' sampling and before the later ones, by submission
  order.
- The buffer's image is taken from the foreign queue family (`VK_QUEUE_FAMILY_FOREIGN_EXT`)
  each time it is read.
- **The producer gets its buffer back only once the GPU has read it**: the frame is held by
  `queue.on_submitted_work_done`, which wgpu calls when that submission is done (the renderer
  polls every frame), and dropping it deletes the image. Until then the producer cannot write
  into it.
- What a texture leaves behind (imports, view, framebuffer) is destroyed the same way, after
  the GPU is done; it holds the device, not the importer, so the queue holding it never holds
  what holds the queue.

### Shaders compiled ahead (`frus-gpu/src/custom.rs`, `frus-core/src/shader.rs`)

`Shader::from_wgsl` keeps every shader it loads in a list, and calls the hooks renderers
registered. Each renderer's custom painter now makes its pipelines through a **warmer**: a map
from shader to a `OnceLock` of its pipeline. When a shader is loaded the warmer puts an empty
slot in the map and fills it on a thread of its own (one per batch, gone when it is done; on
the Web, which has no such thread, nothing is done ahead). A frame that draws the shader takes
the slot: ready, it costs nothing; still being filled, the frame waits for that compilation
rather than starting another. The renderer starts the shaders loaded before it existed while
it warms its own pipelines, and registers its hook for the rest. A pipeline made ahead is
counted like one made at a draw, so the pipeline cache file is written again. wgpu's error
scopes are per thread, so the worker's validation scope catches what the driver refuses there
as it did on the render thread.

The GPU pass's compositing shader (frus's own) is loaded with the first `GpuCallback` made, in
the `build` of the frame that shows it, so it too compiles while that frame is laid out.

### Zero cost when unused: two features

Both halves are behind cargo features of `frus`, off by default, and forwarded through
`frus-shell` and `frus-gpu` (and `frus-core` for the second):

- `hardware-buffers`: the Vulkan device opening, the import pass, `ash`, and the surface's
  buffer path. Without it the surface code takes the copying path at compile time, and no
  extension is asked of the device.
- `warm-shaders`: the list of loaded shaders, the load hooks, the warm-up thread. Without it a
  pipeline is made at its first draw, as in milestone 792.

`frus-demo` turns on the first, `crates/frus-shaders` the second. The smallest application
(`frus-hello`, release, aarch64 Android, built as CI's size job builds it, NDK r26d) measured
14,411,400 bytes on the base commit and **14,412,168** bytes here (+768 bytes: the frame's
optional buffer and the registry's flag). Before the features existed the same build was
14,425,176 bytes (+13.8 KB); the budget is not raised.

## How it was tested

### Unit tests

- `frus-core` (`texture.rs`, 4 new): a hardware buffer pushed with no importing renderer is
  refused as `Unsupported` and released at once; a frame held by the renderer is released only
  when the holder lets go, one replaced before any window took it at once (and counted as
  dropped); a hardware frame never becomes a spare (five pushes, four released, no spare); a
  frame too large is refused and released. The `HardwareBuffer`, `push_hardware_buffer`,
  `hardware_buffers` and `Shader::on_load` / `loaded_since` doctests.
- `frus-gpu` (`custom.rs`, with `warm-shaders`, on a real GPU): a shader loaded before any frame
  is not compiled until the warmer runs, is compiled by its thread, raises the pipeline cache's
  flag, and is the pipeline the first draw takes; once a renderer warms shaders, one loaded
  later compiles with no frame asking. `hardware.rs` (Android only): the crop in texture
  coordinates, Y'CbCr formats told apart, colour spaces mapped to the conversion's model and
  range.
- The whole `frus-core`, `frus-gpu` and `frus-shell` libraries, and `frus-test`'s
  `external_texture` and `custom_shaders` suites (Windows, Intel Iris Xe, D3D12): unchanged.

### The phone (Huawei STK-L21, Mali-G51 r18p0, Vulkan 1.1, Android 10), release APKs

The log says what was opened: `hardware buffers: imported by Vulkan, no copy`, then
`texture surface 1280×720 (Yuv420): hardware buffers, imported by the GPU without a copy`.
The demo's *External texture* screen plays the 320×240 clip of milestone 793 or a 1280×720
one (`adb shell setprop debug.frus.texture_clip 720`); `debug.frus.texture_copy 1` makes it
copy frames even where they could be imported, to compare on the same build. CPU is the
process's share of one core over 10 s while playing (`/proc/<pid>/stat`), the decoder's
thread and the demo's polling decode loop included; PSS from `dumpsys meminfo`.

| Clip | Path | CPU playing | PSS (Graphics) | Frames | Render, median/95th |
| --- | --- | --- | --- | --- | --- |
| 320×240 | imported | 52.7 % | 140.2 MB (68.3) | 30 fps | 6.1–6.8 / 9.5–24.5 ms |
| 320×240 | copied | 54.4 % | 136.0 MB (68.1) | 30 fps | 3.9–9.8 / 14.4–26.1 ms |
| 1280×720 | imported | **50.5 %** | **136.4 MB** (67.8) | 30–31 fps | 4.2–9.4 / 9.2–19.8 ms |
| 1280×720 | copied | 55.5 % | 142.4 MB (68.3) | 30–31 fps | 4.4–9.2 / 9.7–27.0 ms |

After leaving the screen: 0.0 % CPU in all four. At 720p the import saves about 5 % of a core
and 6 MB; at 320×240 the copy was already small. Most of the CPU in every row is the demo's
decode loop and the decoder, which the import does not change. Colours: the bars of the
720p screenshots of both paths are the same pixels (red 254,0,0; yellow 255,255,0; blue
0,0,255; cyan 1,255,255), and the picture is the right way up and cropped.

**A bug only the phone showed**: with five images in the reader the decoder did not start at
all (`setting nBufferCountActual to 30 failed`, then `Codec reported err`), every unit test
being green; three images fixed it (see the surface's design above).

### Shaders compiled ahead, on the phone

`crates/frus-shaders` loads its two shaders in `main!` before the window opens (the pattern
the guide recommends). Three cold launches each (`pm clear`, so no pipeline cache), the
startup line's "presented in" being the first frame's cost from layout to the image handed to
the display:

| Build | First frame presented in | Shader pipelines |
| --- | --- | --- |
| base commit (compiled at the first draw) | 554 / 456 / 384 ms | on the render thread, in that frame |
| this milestone, `warm-shaders` | **347 / 304 / 248 ms** | 140–240 ms each, ahead; the frame waited 45–52 ms for the last one still compiling |

About 150 ms less on the first frame. The remaining wait is the GPU pass's compositing shader,
loaded in the first `build`; the application's own triangle pipeline, made in its callback,
is in both rows.

### Checks

`cargo clippy --workspace --all-targets -D warnings`; `frus-gpu` with `warm-shaders`;
`aarch64-linux-android --no-deps` for `frus-gpu`, `frus-shell` (with and without
`hardware-buffers`), `frus-demo` and `frus-shaders`; `wasm32-unknown-unknown` for `frus-core`,
`frus-gpu` and `frus-shell`; `cargo doc` with `-D warnings`; the workspace tests as CI runs
them. CI gains `cargo test -p frus-gpu --features warm-shaders --lib custom::`.

## Not done

- **CPU saved is modest on this phone.** The import removes the plane copy and the upload,
  about 5 % of a core at 720p; the demo's decode loop dominates the rest. A 1080p or 4K source
  would save more; none was measured.
- **The import pass is a second draw of the frame's size** per new video frame (one triangle);
  sampling the buffer straight in the image painter would save it, at the cost of a hand-made
  copy of that pipeline per buffer format.
- **Only Vulkan on Android.** OpenGL ES (`EGLImage` from the buffer), Windows (D3D11/DXGI
  shared handles), macOS and iOS (`IOSurface`/`CVPixelBuffer`) and the web
  (`copyExternalImageToTexture` of a `<video>`) still copy, or push CPU frames.
- **The surface chooses its path when it is made**: one made before the renderer exists
  copies its frames for its whole life.
- **No Vulkan validation layer was run** on the phone (none is installed there); the barriers
  and the layouts were reasoned from the specification and checked by the pictures.
- **A decoder's colour space** is the surface's (`set_color_space`), as on the copying path;
  the buffer's own suggestion (`suggestedYcbcrModel`) is not used.
- **Warm-up on the Web**: nothing is compiled ahead there (no thread may hold wgpu's objects).
- **Pipeline warm-up measured on one application** (`frus-shaders`, cold starts); a shader
  loaded in the very `build` of its first frame gains only the overlap with that frame.
- **The 720p clip adds 318 KB to the demo's APK** (the demo only; `frus-hello` is unchanged).
