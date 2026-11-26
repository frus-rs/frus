# How a frame is made

This page is for contributors, and for application authors who want to know where a frame's
time goes. It follows one frame from an event to the pixels, names the crate that does each
step, and says what is kept between frames so that most frames do little. The crate-by-crate
map is [ARCHITECTURE.md](../../ARCHITECTURE.md); the reasoning behind each decision is in the
milestone notes in [`docs/`](../README.md).

## The crates, bottom up

| Crate | What it holds |
| --- | --- |
| `frus-core` | geometry, colour, paths, text styles, animation curves and controllers, semantics, and the `Scene`: the flat list of primitives a frame is drawn from |
| `frus-layout` | box layout (flexbox and grid) over `taffy` |
| `frus-text` | shaping, line breaking and fonts, over frus's copy of `cosmic-text` (`frus-cosmic-text`: variable font axes, a language per span, dictionary line breaks) |
| `frus-gpu` | the renderer: a `Scene` to draw calls, on `wgpu`; text through `frus-glyphon`, paths through `lyon` |
| `frus-widgets` | every widget, the theme, components and hooks, the router, the runtime that keeps interaction state between frames |
| `frus-shell` | the window and the event loop (`winit`), each platform's services, accessibility bridges, the `Application` trait and `FrusApp` |
| `frus` | the facade an application depends on |
| `frus-cli` | the `frus` command |

Dependencies point down: a widget never reaches the GPU, and the renderer knows nothing of
widgets. `frus-image`, `frus-svg` and `frus-lottie` are optional decoders under the widgets.

## One frame

1. **An event arrives**: a pointer, a key, a timer, the result of `async` work, a change of
   window size or insets. The shell hit-tests it against the registries the last frame left
   (where each button, field, scrollable and drop target was) and calls the handler.
2. **The handler changes state**: a hook's `set`, a `State`'s `set_state`, a notifier. That
   marks the tree dirty and asks for a frame. Nothing is drawn yet; several changes before the
   next frame cost one build.
3. **Build.** On the next frame the tree is built again from the root: each component's
   `build` runs, and finds its state and hooks where it left them, matched by place and key.
   Widgets are cheap configuration; what is expensive (layout, text shaping, painting, GPU
   resources) is cached below. A frame that only animates or scrolls does not build at all.
   Before building, the shell installs the ambient values every widget can read: the
   `MediaQuery`, the theme, the language and the framework's words.
4. **Layout.** The widget tree becomes a tree of boxes, sized by `frus-layout`; text is measured
   by `frus-text`. Boxes whose style and constraints did not change keep last frame's rectangles
   (the relayout cache).
5. **Paint.** A walk over the laid-out tree appends primitives to a `Scene` (rectangles, rounded
   boxes, paths, glyph runs, images, layers with opacity, clips, filters and shaders), and
   fills the registries the next event is hit-tested against, the focus order and the
   semantics tree. A subtree marked as a repaint boundary whose rectangles and interaction state
   did not move replays last frame's primitives instead (the paint cache).
6. **Render.** `frus-gpu` turns the `Scene` into batched draw calls on the swap chain's image:
   one pipeline per kind of primitive, glyphs from an atlas, layers in offscreen textures only
   where a group needs one. The shell then hands the accessibility tree to the platform.

The `Scene` is the only thing that crosses from the widgets to the renderer. A test can build a
frame and read the `Scene` without a GPU, which is how most of frus's tests and the
[testing guide](testing.md)'s `Tester` work; golden tests in `frus-test` render it to pixels.

## When frames are drawn

frus draws only when something asks for a frame:

- **Nothing moves**: no frame at all. An idle screen costs nothing.
- **Something moves** (an animation, a scroll, a fling, an image arriving): every frame the
  display can show, until it settles.
- **Only decoration moves** (a spinner, a shimmer): 30 frames a second, which the eye cannot tell
  from more. `FrusApp::decorative_frame_rate` changes the rate.
- **The window is hidden or minimized**: nothing.

## Where the time goes

With `FRUS_FRAME_STATS=1` in the environment (on Android, `adb shell setprop
debug.frus.frames 1` before starting), and always in a profile build, frus logs one line for
every two seconds of drawing under the target `frus::frames`: the frame rate, how many frames missed their budget, and the time spent
building, laying out and painting, and rendering (split into waiting for an image and drawing),
then the GPU memory the renderer holds. `frus run --profile` with the `P` panel shows the same
live, and [DevTools](devtools.md) graphs it. A frame over budget in **build** builds too much
(a large tree rebuilt on every keystroke, work done in `build` that belongs in `use_memo`); in
**layout and paint**, a tree too large for what
is on screen (a column where a lazy `ListView` belongs); in **render**, too much overdraw, too many
layers, or a GPU still busy with earlier frames.

## Scaled layers

A `Transform` (and every transition that scales, turns or moves a page) draws its child flat
into a texture of its own and composites that texture through the transform. The texture is
drawn for the scale it is shown at:

- **Shown smaller and at rest** (a `FittedBox` shrinking its child, a `Transform::scale`
  below one), its content is drawn at that scale, so its edges and text are as sharp as
  content drawn at that size.
- **Shrinking or growing below its own size**, it is drawn at its own size with a mip chain,
  made on the GPU and kept with it, and is read trilinearly: fine text and patterns keep their
  tone instead of shimmering as the scale moves, and the texture is not drawn again each
  frame. The chain costs a third more memory than the texture. When the scale comes to rest,
  the layer is drawn once more at its exact scale, without a chain.
- **Shown larger**, its content is drawn at that scale, so text is rasterised at the size it
  is shown at and stays sharp. While the scale grows, the texture is drawn ahead of it and
  drawn again only each time the scale passes it, not every frame. Until then it is shown at
  most a quarter smaller than it was drawn; a frame drawn with the scale at rest draws it
  once more at the exact scale. An enlarged texture never holds more texels than the screen
  has pixels.

A layer shown for the first time is drawn at its exact scale. Content that changes every
frame is drawn again every frame anyway; while it scales, it is
drawn at the same few sizes, so its glyphs are not rasterised again each frame. A frame in
which nothing changed draws no layer again.

## Backends

| Platform | Drawn with |
| --- | --- |
| Windows | Direct3D 12 (with `dxcompiler.dll` beside the program, shaders compile several times faster), Vulkan |
| macOS | Metal |
| Linux | Vulkan, or OpenGL ES where Vulkan is not usable |
| Android | Vulkan, or OpenGL ES 3 where the driver's Vulkan is not usable |
| Web | WebGPU, or WebGL 2 where the browser has no WebGPU |

The environment variable `WGPU_BACKEND` (`vulkan`, `gl`, `dx12`, `metal`, `webgpu`) chooses
instead, to reproduce a problem on a given backend. There is no software renderer: a machine
with no GPU driver at all cannot draw a frus window.

## Changing the pipeline

- A new primitive is added to `frus-core`'s `Scene`, painted by a widget in `frus-widgets`, and
  drawn by a pipeline in `frus-gpu`; the golden tests in `frus-test` show the pixels.
- A change to text, colours or rendering is checked against the goldens; see CONTRIBUTING.md.
- A colour is blended in linear space: a translucent token is converted with `to_linear`, never
  interpolated in sRGB.
- Code that only runs on Android or the web is linted only by clippy for that target
  (`--target aarch64-linux-android`, `--target wasm32-unknown-unknown`).
