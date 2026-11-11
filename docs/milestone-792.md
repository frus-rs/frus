# Milestone 792 — custom fragment shaders and an external GPU pass

## Objective

Two ways for an application to put its own GPU code on the screen:

1. **Fragment shaders.** An application writes one WGSL function,
   `fn shade(frag: Fragment) -> vec4<f32>`, loads it once (`Shader::from_wgsl`), sets its
   uniforms by name and its images, and paints with it: a box, a path or text in a custom
   painter, a whole subtree as a mask, or an animated box whose `time` runs only while it plays.
   An error is reported with its line and column, never a panic.
2. **A GPU pass.** `GpuPass` gives an application frus's own `wgpu` device, queue, an encoder and
   a picture of its box in physical pixels; what it draws (3D, a compute result) is composited
   like any layer: clipped, faded and transformed with the widgets around it. `frus::wgpu`
   re-exports the renderer's wgpu, so an application never pins a version of its own.

## API

```rust
// Load once (cached by the source's digest), never panic.
let shader = Shader::from_wgsl(include_str!("plasma.wgsl")).unwrap_or_else(Shader::placeholder);
let paint = shader.paint()
    .color("deep", Color::rgb8(18, 24, 80))   // vec4<f32>, straight sRGB
    .float("scale", 6.0)                      // f32 / vec2 / vec3 / vec4 / int / uint / float_at
    .image(0, &handle)                        // image0(uv) .. image3(uv)
    .blend(BlendMode::SrcIn)                  // how it meets what it paints
    .time(1.5);

scene.shader_rect(rect, &paint);              // a box
scene.shader_path(&path, &paint);             // a path, where it covers
scene.shaded(rect, &paint, |s| s.text(..));   // text or any drawing, coloured
scene.shader_masked(rect, &paint, |s| ..);    // a mask over a subtree, read as child(uv)

AnimatedShader::new(paint).playing(true).size(w, h);       // time runs, frames only while playing
ShaderMask::shader(paint).playing(true).child(widget);     // a subtree
GpuPass::new(|ctx: &mut GpuPassContext| { /* ctx.device, ctx.queue, ctx.encoder, ctx.target,
    ctx.state(|device, format| ..) */ }).playing(true).size(w, h);
```

## Design

### The function and its frame (`frus-core/src/shader.rs`, `shaders/frame.wgsl`)

The application's source is kept as written and frus's frame is **appended** after it: WGSL
declarations at module scope may come in any order, so the application uses `Fragment`,
`uniforms`, `child` and `image0`..`image3` before they are declared, and the line numbers of an
error in its source are its own. The frame holds the vertex stage (the compositor's quad and
instance, unchanged), the bindings, the helpers, the compositor's blend modes, and the fragment
entry point, which carries the target pixel back through the layer's transform, computes
`Fragment`, calls `shade`, meets the result with the layer's content by the paint's blend mode
(premultiplied, sRGB-encoded, as the compositor blends a mask), and multiplies by the clip shape,
the path clip mask and the opacity.

Colours go in and out **straight and sRGB-encoded**, as every colour in frus is written: a mix
from black to white is 50% grey half-way, as in a CSS gradient (the test checks the rendered
pixel, 128). The child and the images are converted from premultiplied linear light before
`shade` sees them.

`Shader::from_wgsl` parses and validates the whole module with **naga** (the same version and
feature wgpu already builds, so no new code is shipped) with no optional capability, the level
WebGL 2 has. An error before the frame begins is the application's: line, column, message and
naga's report with the offending line. One past it (a `shade` missing or of the wrong type, a
clash with a frame name) is explained in words instead of pointing into the frame. The struct
`Uniforms` is read from naga's module: each member's name, type and offset by WGSL's own layout
rules; the block is rounded up to 16 bytes (WebGL binds nothing else); a shader that declares none
gets an empty one. A member of a type frus cannot set (a matrix, an array) is refused at load.

The result is kept by the digest of the source: the same text again costs a hash, so a build
function can call it every frame. `Shader::placeholder(err)` logs the error once and draws
magenta stripes in a debug build, nothing in a release build.

The setters on `ShaderPaint` never fail: an unknown name or a wrong type is said once per shader
and message (so a paint rebuilt every frame does not flood the console) and ignored;
`ShaderPaint::set` answers the `UniformError`. `float_at(i, v)` indexes the floats of the block
in declaration order, for shaders whose uniforms are set by position.

### In the scene

A shader is a field of `LayerFilter` (`shader: Option<Box<ShaderPaint>>`), and a GPU pass
another (`external: Option<Box<ExternalPass>>`), boxed so that every other primitive of a scene
stays the size it was (unboxed, a layer grew to 456 bytes and every primitive with it). A layer already is the place an effect over a subtree is
applied, so a mask, a coloured text and a filled box are the same thing with a different blend
mode default (`Modulate`, `SrcIn`, `Src`) and with or without content. `LayerFilter` stops being
`Copy` (the paint holds the shader and the images); every use in the workspace is a clone of a
few reference counts. Both share a layer with nothing but a clip (`merge` refuses the rest, like
a blend mode). A layer's footprint and bounds include the shader's box, so a box with no content
is ordered among the rest like anything else; `take_owned` keeps such a layer while its owner is
shown.

The paint carries its box in the scene's pixels and in logical pixels: scaling the scene for the
display grows the box and leaves the logical size, so `frag.position` is in logical pixels and
`frag.pixel_ratio` the ratio of the two.

### On the GPU (`frus-gpu/src/custom.rs`)

The custom draw is a composite draw with its own pipeline: the same quad, the same instance
buffer (clip, shape, inverse transform, opacity, region), the same texture bind group for the
child, and a third bind group of its own: the frame's numbers, the application's uniforms, four
images and a sampler. Every slot of every custom draw of a frame goes into one buffer, aligned to
the device's offset alignment. The compositor's draw loop switches pipeline for those draws and
back.

- **A box with no content costs one draw**: no layer pass, no texture, no slot in the layer
  cache. A mask or a shaded path renders its content into a layer first, as any group does.
- **Pipelines are made the first time a shader is drawn**, through the device's pipeline cache
  (Vulkan), and the cache file is written again when one is added, so the next launch finds it.
  On native backends the pipeline is made inside a validation error scope: a shader the driver
  refuses is logged and skipped, never a panic.
- **OpenGL ES and WebGL 2**: wgpu translates the WGSL to GLSL there. frus first runs naga's GLSL
  ES 3.0 writer on the module, and a shader it cannot express (a storage buffer, say) is refused
  with naga's reason in the console and its box left empty. A test translates the frame and an
  application shader that uses everything (uniforms, child, image, maths) to GLSL ES 3.0 for
  WebGL, and checks that a storage buffer is refused with a reason.
- **Images** go through the image painter's texture cache (marked used, so they survive the
  frame), sampled linearly with their edges clamped.

### GPU passes

`GpuCallback` (frus-gpu) wraps the application's function; its identity is the closure's type, so
the same pass keeps its picture and its state from frame to frame, and `.key(n)` tells apart
passes drawn by one closure. The core carries it as an opaque `Arc<dyn Any + Send + Sync>`
(`ExternalPass`), so the scene stays free of wgpu. Each frame the pass is drawn the renderer
keeps (or remakes, when the size changed) an `Rgba8UnormSrgb` texture of the box's physical size,
clears it, hands the callback the device, the queue, an encoder (submitted after it), the target,
the texture, the size, the pixel ratio and the time, and composites the picture through the frame
with the external flag: premultiplied linear, clipped, faded and transformed like any layer.
`GpuPassContext::state` keeps what the pass makes once (pipelines, buffers), typed, `Send + Sync`
natively and not on the Web, where wgpu's objects belong to the page's thread. Pictures no frame
drew are dropped at the end of the frame.

`GpuPass` is a widget of `frus-shell` (the crate that knows both the widgets and the GPU),
re-exported by `frus` with `GpuCallback`, `GpuPassContext`, `PassState`, `GPU_PASS_FORMAT` and
`wgpu`.

### Animation and idle cost

`AnimatedShader` and `ShaderMask::playing` are `continuous` while they play, which is what keeps
frames coming; `frag.time` is the frame clock (`FilterContext` now carries it to a mask). Once
stopped they ask for no frame: nothing is drawn while the screen is still. `AnimatedShader::decorative`
puts a decoration at the decoration frame rate (milestone 783).

### Hot reload

`Shader::from_wgsl` caches by the digest of the text, so a new text is a new shader: with
`include_str!` and a hot patch that rebuilds the function, the edited shader is compiled and drawn.
A test checks the cache answers the same program for the same text and another for an edited one.

## Dependencies

- `naga` 30 with `wgsl-in` in `frus-core`: the version and the feature `wgpu-core` already builds,
  so it adds no code to a binary; it parses, validates and reflects a shader at load.
- `naga` 30 with `glsl-out` in `frus-gpu`, except on Apple's systems: the GLSL ES check. wgpu's
  OpenGL ES backend builds it already on those targets.

Both MIT OR Apache-2.0.

## How it was tested

**Unit tests.** `frus-core` (`shader::tests`, 11): the uniform layout read from the source (offsets
0/16/32/40/48, a `vec2` after an `f32` on the next 8 bytes, the block rounded to 64), values
packed at their offsets and nowhere else (padding untouched), `float_at` counting float
components only, a wrong name, type, float index or image slot refused and changing nothing,
16 bytes for a shader with no block, `struct Uniforms` in a comment not counting, the same
source compiled once and an edited one anew, a parse error and a type error naming the
application's line and column with the line shown, a missing or mistyped `shade` explained in
words, a matrix uniform refused at load, the placeholder compiling, and a paint following the
scene's scaling and moves. `frus-gpu` (`custom::tests`, 4): the frame and a shader using
uniforms, `child`, an image and maths translated to GLSL ES 3.0 for WebGL by naga's writer, a
storage buffer refused with a reason, the frame uniform's 64 bytes, a callback being the same
pass every frame and `.key` telling passes apart. `frus-widgets`: `AnimatedShader` animating
only while playing (its clock from the frame's, the paint's own time when stopped) and filling
when unsized; the `filters` tests (15) after `LayerFilter` lost `Copy`. `frus-shell`: `GpuPass`
painting its box with the callback and the frame's time, continuous only while playing.

**Rendered, on a real GPU** (Windows, Intel Iris Xe, Direct3D 12; `frus-test/tests/custom_shaders.rs`,
10 tests): a black-to-white shader is 128 grey half-way (sRGB as written, not linear light),
a shader stays inside its box and its clip, uniforms arrive (pad, `vec2`, colour → exactly
51/102/153), an image's two texels are sampled where they are, a path is filled only where it
covers, a shader mask fades its child (orange at the left, the background at the right), a
mask reading `child` inverts it where it is and is transparent where it is not, a GPU pass
draws a green triangle with its own pipeline in its box (the callback run once, its pipeline
made once), and a GPU pass inside a half-opacity group is faded with it. These tests found a
bug before the phone did: `GpuPassContext::state` downcast the `Box` rather than what it holds,
and panicked on the first frame.

**Goldens** (WSL, lavapipe, `CARGO_INCREMENTAL=0`): the whole `frus-test` suite passes unchanged;
three new goldens: `shader_gradient_noise`, `shader_mask_fade`, `gpu_pass_triangle`.

**Clippy**: the workspace with `-D warnings`, `wasm32-unknown-unknown` (core, gpu, shell, the
example) and `aarch64-linux-android --no-deps` (core, gpu, shell).

**Windows window** (`crates/frus-shaders`, debug build, Iris Xe): the gallery draws the
shimmering title, the turning triangle of the GPU pass and the plasma; full screen (1920 × 1000)
the plasma runs at 56 to 60 frames a second, 11 to 13 ms a frame on the GPU, and stops drawing
when tapped.

**Phone** (Huawei STK-L21, Mali-G51, Android 10, release APK, 6.8 MB):

| Page | Frames | Interval (median / 95th) | Render (median / 95th) | CPU drawing |
| --- | --- | --- | --- | --- |
| Full-screen plasma, 1080 × 2340 | 55 to 56 fps | 17.5–18.0 / 21.7–22.8 ms | 15.5–16.1 / 19.9–20.7 ms | 1.4–2.0 ms |
| Gallery (mask + GPU pass + rounded plasma, 3 layers) | 26 fps | 39 / 43 ms | 36 / 39 ms | 2.4–3.0 ms |
| Stopped (after a tap) | none | — | — | 0 CPU ticks in 20 s (`/proc/<pid>/stat`), `top` 0.0% |

The full-screen shader is one draw with no layer, and holds the display's rate with about
1 frame in 100 late. The gallery is GPU-bound at 26 fps: three layers, two of them (the mask's
text and the plasma under its rounded clip) rendered into textures first, each at 4× MSAA, on
a weak GPU; see Not done. PSS of the app: 267 MB (most of it the GPU driver's mappings).

**Web**: `frus build web` (11.5 MB of wasm, release), served locally, loaded in
headless Edge over CDP at 420 × 800 CSS pixels: through WebGPU (`GPU interface: WebGPU`) and,
with `navigator.gpu` removed, through WebGL 2 (`GPU interface: WebGL 2 (this browser has no
WebGPU)`). Both draw the shimmering title, the GPU pass's triangle (its own pipeline, built by
the application through `frus::wgpu`) and the rounded plasma, with no error in the console.

## Not done

- **The gallery page runs at 26 fps on the test phone.** A shader inside a layer (here the
  plasma under a rounded clip) changes every frame, so the enclosing layer's cache misses and
  the whole layer is rendered again, at 4× MSAA, before being composited; the shimmer mask
  does the same for its text. A full-screen shader alone holds 55–56 fps. Drawing a shader
  layer straight into its parent's pass when only its clip shape needs a layer, or keeping the
  child texture of a mask whose child did not change, is not done.
- **Hot reload end to end.** A new source text is a new shader (tested), but editing a `.wgsl`
  file under `frus run` and watching the patched build pick it up was not tried: whether the
  patcher rebuilds on a change to a file read by `include_str!` is the patcher's, and unverified.
- **The WebGL 2 fallback in a browser.** The translation is checked by naga's GLSL ES 3.0 writer
  in a unit test (and a storage buffer refused with its reason), and the web page was loaded
  in headless Edge as reported above; a pixel comparison between the WebGPU and the WebGL
  pictures of the same shader was not made, and no phone browser was tried.
- **Pipelines are compiled at a shader's first draw**, not in the renderer's warm-up: the first
  frame that shows a new shader pays its compilation (a few ms on desktop; not measured on the
  phone). The pipeline cache keeps it for the next launch on Vulkan.
- **Uniforms**: `f32`, `i32`, `u32` and `vec2`/`vec3`/`vec4<f32>` only; matrices and arrays are
  refused at load. Four images at most, sampled linearly with clamped edges (no repeat, no
  nearest, no mipmaps).
- **GPU pass**: one colour picture, no depth buffer, no MSAA, and not a storage texture (an
  `Rgba8UnormSrgb` picture cannot be one): compute results are copied or drawn into it. The
  callback runs on the render thread every frame the box is drawn; a pass that only changes on
  demand has no way to say "unchanged" yet.
- **On the web, a pipeline the browser refuses** after naga accepted the shader goes to wgpu's
  uncaptured-error handler: there is no error scope there, since popping one is asynchronous.
- **iOS** was not compiled (`cargo check --target aarch64-apple-ios` was not run for this
  change); the code has no iOS-specific path (Metal, no GLSL check).
- `AnimatedShader` and `GpuPass` declare no semantics: they are pictures, and an application
  that means something by one wraps it in `Semantics` with a label.
