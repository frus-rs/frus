# Milestone 844 — 3D in a GPU pass: a depth buffer, multisampling, a picture kept until it changes

## Objective

Issue #220 asks that a widget can draw with its own GPU code, and names the test: a spinning,
lit 3D cube in a widget inside a scrolling list, clipped correctly, with no CPU readback.
Milestone 792 gave an application frus's device, an encoder and a picture in its box (the
`GpuPass`), composited like any layer. What it did not give is what 3D needs:

- **No depth buffer.** A 3D scene draws its triangles in an order that is not the order of
  their distance; without a depth test the last one drawn wins and the far faces of a cube
  show through the near ones. An application could make one itself in `ctx.state`, but then it
  had to follow the box's size itself (the state outlives a resize, the textures must not),
  and nothing said which format or sample count worked on every backend.
- **No multisampling.** The picture had one sample; the edges of 3D geometry were stair-cased,
  and a multisampled target would again have had to follow the size by hand.
- **No contract for the pipelines.** A pipeline must be made for the exact colour format,
  depth format and sample count of the pass it draws in; the context gave only the colour
  format.
- **Run every frame.** A pass could not say its picture had not changed (L-027).

## API

```rust
use frus::{wgpu, GpuPass, GPU_PASS_DEPTH_FORMAT};

GpuPass::new(|ctx| {
    let targets = ctx.targets;            // PassTargets { format, depth, samples }
    let pipeline = ctx.state(move |device, _| make_pipeline(device, targets)).clone();
    let mut pass = ctx.begin_render_pass(Some("cube"));  // MSAA target + depth, resolved
    pass.set_pipeline(&pipeline);
    pass.draw(0..36, 0..1);
})
.depth(GPU_PASS_DEPTH_FORMAT)   // any depth/stencil format; cleared to 1.0 every frame
.depth_clear(1.0)               // 0.0 for a reversed depth range
.samples(4)                     // resolved into the picture
.version(n)                     // optional: not run again while `n` and the size stay
```

`PassTargets::multisample()`, `depth_stencil(compare)` and `color_target()` give the pipeline
states that match (each a plain wgpu value the application can change: a stencil, a bias, no
depth writes for a transparent surface). `GpuPassContext` gains `depth`, `multisampled` and
`targets`, and `color_attachment()`, `depth_stencil_attachment()` and `begin_render_pass()`
for a pass begun on them; an application that wants its own descriptor (several passes, a
stencil, other load operations) uses the views directly. The same builders exist on
`GpuCallback` (frus-gpu) and on the `GpuPass` widget. New exports: `PassTargets`,
`GPU_PASS_DEPTH_FORMAT` (from `frus-gpu`, `frus-shell` and `frus`).

## Design (`frus-gpu/src/custom.rs`)

- **The textures belong to the pass's slot**, next to its picture: the multisampled colour
  target and the depth buffer are made with the picture, at its physical size and with the
  same sample count, and made again whenever the box changes size. frus clears them in the
  same pass that clears the picture (the multisampled target resolves its transparent clear
  into the picture, so a pass that draws nothing still shows nothing), then hands them over.
- **What the device can make is decided once, from wgpu's guaranteed format features**:
  a format that is no depth format, needs a missing feature, or cannot be rendered to is said
  once in the console and the pass is drawn without a depth buffer; a sample count the colour
  or depth format cannot have falls back to 4, else 1, also said once. `Depth32Float` at 4
  samples is guaranteed on every backend frus runs on, WebGL 2 included, so that is what the
  docs recommend.
- **A change of targets drops the state** the pass keeps: its pipelines were made for the old
  format and sample count and would fail validation. A resize keeps it, as before.
- **`version`**: the slot remembers the version its picture shows; a frame that asks for the
  same version at the same size composites the kept picture without running the callback or
  submitting anything. Without a version nothing changes from milestone 792.
- **Composition is unchanged**: the resolved picture is composited through the frame shader
  like any layer, so the clip of a scrolling list, a group's opacity, a rounded clip and a
  transform apply to a 3D pass exactly as to a 2D one, and nothing is read back to the CPU.
- The memory count (milestone 796) includes the multisampled and depth textures.

### Lint fix on master

`cargo clippy -p frus-gpu --all-targets -- -D warnings` without `--features warm-shaders` failed:
`Painters::custom_mut` and the tests' `headless_device` in `custom.rs` were used only by tests
gated on that feature. The new tests of this milestone use both in every build (they drive
`CustomPainter::run_pass` on a real device), so they are no longer dead code without the
feature, and the lint passes both ways.

## Example

`crates/frus-shaders` has a new page, **3D in a list**: sixteen rows of text in a
`SingleChildScrollView`, the fifth a `GpuPass` turning a lit cube (`cube.rs`, `cube.wgsl`).
The cube's 36 vertices are made in the vertex shader from their index, with a perspective
projection and one directional light; faces are **not culled**, so what hides the back faces
is the depth test alone. It draws at 4 samples.

## How it was tested

See the PR for the commands and their results; summarised here.

- **`frus-gpu` unit tests** (`custom::tests`, on a real adapter): a pass asking for a depth
  buffer and 4 samples is given both at its box's size; a new size makes them again and keeps
  the state; new targets give a new state; a versioned pass is run once per version and size;
  an impossible sample count (3) falls back with a reason, a colour format asked as a depth
  format is refused with a reason, and `Depth32Float` at 4 samples is accepted with none.
- **Rendered** (`frus-test/tests/gpu_depth.rs`): two overlapping quads, a near red and a far
  blue: with a depth buffer the overlap is red whether red is drawn first or last; without
  one the last drawn wins (so the first test can tell). A slanted triangle at 4 samples has
  partly covered pixels along its edge, at 1 sample none. A depth-tested, multisampled pass
  half out of a clip, as a row scrolled half out of a list: depth-tested inside, what is under
  it outside. The 792 suite (`custom_shaders`) passes unchanged.
- **`frus-shell`**: the widget's builders reach the callback it paints.
- **Clippy**: as reported in the PR, including `wasm32-unknown-unknown` for the touched crates
  and `frus-gpu` with and without `warm-shaders`.

## Not done

- **The example was not run on a phone or in a browser here**, and not looked at in a window
  on screen: the depth test, the multisampling and the clip are proven by rendered tests on the
  desktop GPU; Android and the web were compiled (clippy), not run. The issue's "done when"
  names all three platforms, so #220 should stay open until someone runs the 3D page on them.
- **An extra pass per frame**: frus clears the depth buffer and the multisampled target in its
  own pass before the callback's, and stores them, which costs bandwidth on tiled phone GPUs
  that a single pass with `Clear` and a discarded multisampled target would not. A pass that
  lets the application own the clear is not done.
- **A transform that scales the box** still scales the picture as a texture: the picture is
  made at the box's untransformed physical size.
- **The callback still runs on the render thread**, in its own encoder submitted before the
  frame; there is no way to draw into frus's main pass (the "paint callback" shape of the
  issue), and no storage-texture picture for compute output.
- No golden image was added for the cube: the tests check pixels and say what each must be.
