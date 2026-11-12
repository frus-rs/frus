# Custom shaders and drawing on the GPU

Two ways to put your own GPU code on the screen:

- a **fragment shader**: one WGSL function that colours every pixel of a box, a path, a text
  or a whole subtree. frus compiles it once, checks it, and reports an error with its line;
- a **GPU pass**: a box you draw yourself with frus's own `wgpu` device, for 3D, a compute
  result or a renderer of your own. What you draw is composited with the widgets around it.

Both cost nothing until a screen shows one, and a still screen draws nothing: an animated
shader asks for frames only while it plays.

## The function you write

```wgsl
// Optional: the values you set from Rust, by name.
struct Uniforms {
    deep: vec4<f32>,
    bright: vec4<f32>,
    scale: f32,
};

// Required: the colour of one pixel, straight alpha, in sRGB.
fn shade(frag: Fragment) -> vec4<f32> {
    let k = 0.5 + 0.5 * sin(frag.uv.x * uniforms.scale + frag.time);
    return vec4<f32>(mix(uniforms.deep.rgb, uniforms.bright.rgb, k), 1.0);
}
```

frus gives the function:

| Name | What it is |
| --- | --- |
| `frag.position` | logical pixels from the box's top-left corner |
| `frag.uv` | `position / size`: 0 to 1 across the box |
| `frag.size` | the box, in logical pixels |
| `frag.time` | seconds: the paint's time, or the animation's clock |
| `frag.pixel_ratio` | physical pixels per logical pixel |
| `uniforms` | your `struct Uniforms`: `f32`, `i32`, `u32`, `vec2/3/4<f32>` members |
| `child(uv)` | the subtree under a shader mask, straight sRGB; transparent elsewhere |
| `image0(uv)` … `image3(uv)` | images you gave the paint, straight sRGB |

Colours go in and out as they are written everywhere in frus: sRGB, straight alpha. A mix
from black to white is 50% grey half-way, as in a CSS gradient. Declare no bindings and no
entry points: frus puts its own around your function.

## Loading it, setting it, painting with it

```rust
use frus::{AnimatedShader, Color, Shader};

// The function above; in an application, `include_str!("plasma.wgsl")`.
const PLASMA: &str = "struct Uniforms { deep: vec4<f32>, bright: vec4<f32>, scale: f32 };
fn shade(frag: Fragment) -> vec4<f32> {
    let k = 0.5 + 0.5 * sin(frag.uv.x * uniforms.scale + frag.time);
    return vec4<f32>(mix(uniforms.deep.rgb, uniforms.bright.rgb, k), 1.0);
}";

let shader = Shader::from_wgsl(PLASMA)
    // A typo never stops the application: the error is logged with its line, and the box
    // shows magenta stripes in a debug build.
    .unwrap_or_else(Shader::placeholder);

let paint = shader
    .paint()
    .color("deep", Color::rgb8(18, 24, 80))
    .color("bright", Color::rgb8(255, 140, 60))
    .float("scale", 6.0);

// A box painted by the shader, its clock running while it plays.
let background: AnimatedShader = AnimatedShader::new(paint).size(320.0, 200.0);
```

`Shader::from_wgsl` parses and validates the source and keeps the result: the same text
again costs a hash, so call it where you build the paint. An error names the line and the
column, and shows the line:

```text
shader.wgsl:4:18: invalid field accessor `speed`
  ┌─ shader.wgsl:4:18
  │
4 │     let x = frag.speed;
  │                  ^^^^^ invalid accessor
```

The setters never fail: a name the shader lacks, or a value of the wrong type, is said once
in the console. `paint.set("speed", 2.0)` answers the error instead. `float_at(index, v)`
sets floats by their place in the block, counting every component in declaration order.
`.image(0, &handle)` gives an image to `image0`; `.time(seconds)` sets the time by hand.

### In a custom painter

A scene paints with a shader directly, in a `CustomPaint` or anywhere else a scene is drawn:

```rust
use frus::{Color, CustomPaint, Path, Point, Rect, ResolvedTextStyle, Shader};

let rainbow = Shader::from_wgsl(
    "fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(f.uv.x, 0.4, 1.0 - f.uv.x, 1.0); }",
)
.unwrap_or_else(Shader::placeholder);
let paint = rainbow.paint();

let canvas: CustomPaint = CustomPaint::new(240.0, 120.0, move |scene, bounds, _theme| {
    // A rectangle filled with the shader.
    scene.shader_rect(Rect::new(bounds.x, bounds.y, 120.0, 40.0), &paint);
    // A path filled with it: the shader spans the path's bounds.
    let star = Path::new()
        .move_to(Point::new(bounds.x + 180.0, bounds.y))
        .line_to(Point::new(bounds.x + 240.0, bounds.y + 60.0))
        .line_to(Point::new(bounds.x + 120.0, bounds.y + 60.0))
        .close();
    scene.shader_path(&star, &paint);
    // Text in the shader's colours.
    scene.shaded(Rect::new(bounds.x, bounds.y + 70.0, 240.0, 40.0), &paint, |s| {
        s.text(
            Point::new(bounds.x, bounds.y + 70.0),
            "Shaded",
            &ResolvedTextStyle::exact(32.0),
            Color::WHITE,
        );
    });
});
```

### Over a subtree

`ShaderMask::shader` runs a shader over its child, which the shader reads as `child(uv)`.
The shader's colour meets the child by the blend mode: `Modulate` by default, so its alpha
becomes the child's transparency; `SrcIn` colours the child where it is painted.

```rust
use frus::{BlendMode, Color, Shader, ShaderMask, Text};

let shimmer = Shader::from_wgsl(
    "struct Uniforms { base: vec4<f32> };
     fn shade(frag: Fragment) -> vec4<f32> {
         let band = 1.0 - smoothstep(0.0, 0.15, abs(frag.uv.x - fract(frag.time * 0.4)));
         return vec4<f32>(mix(uniforms.base.rgb, vec3<f32>(1.0), band), 1.0);
     }",
)
.unwrap_or_else(Shader::placeholder);
let title: ShaderMask = ShaderMask::shader(shimmer.paint().color("base", Color::rgb8(150, 160, 190)))
    .blend(BlendMode::SrcIn)
    .playing(true) // `frag.time` follows the frames
    .child(Text::new("Custom shaders").size(36.0));
```

## Drawing on the GPU yourself

A `GpuPass` is given frus's device and queue, an encoder frus submits afterwards, and a
picture the size of its box in physical pixels, cleared to transparent. What it draws is
composited like an image: clipped, faded, rounded and transformed with the widgets around it.
Use `frus::wgpu`, frus's own, so the versions never disagree; keep pipelines in
`ctx.state`, made once.

```rust,no_run
use frus::wgpu;
use frus::GpuPass;

let pass: GpuPass = GpuPass::new(|ctx| {
    // Made the first time, kept after.
    let _state = ctx.state(|device, _format| device.features());
    let _pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("my pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: ctx.target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                // Premultiplied linear colours: frus's picture format is sRGB-encoded.
                load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.2, b: 0.5, a: 1.0 }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
})
.playing(true) // frames keep coming and `ctx.time` runs
.size(320.0, 200.0);
```

The pass runs every frame its box is drawn. Several from the same closure (the rows of a
list) tell themselves apart with `.key(n)`, each with its own picture and state.
`crates/frus-shaders` draws a turning triangle with its own pipeline this way.

## On every platform

- **Windows, macOS, Linux, Android**: through Vulkan, Metal, Direct3D 12, or OpenGL ES where
  a device has no Vulkan.
- **The Web**: through WebGPU, or WebGL 2 where the browser has none. On WebGL 2 and OpenGL
  ES, wgpu translates the WGSL to GLSL ES 3.0; a shader that cannot be translated (one that
  reads a storage buffer, say) is refused with its reason in the console, and its box is left
  empty, rather than stopping the page.
- **Hot reload**: an edited shader in `include_str!` is new text, and `Shader::from_wgsl`
  compiles new text anew, so a hot patch that rebuilds the function picks it up.

## Costs

A shader filling a box is one draw, with no texture in between. A mask, a shaded path or a
text renders its content into a layer first, as any group does.

With the `warm-shaders` feature of `frus` (off by default, so that an application with no
shader pays no byte for it), the GPU pipeline is **compiled ahead**: the moment
`Shader::from_wgsl` loads a new text, the renderer starts compiling it on a thread of its own, and the first frame that shows the shader
finds it ready (or waits only for what is left). Load your shaders early, before the screen
that shows them (in `main`, or in the `init_state` of a page that comes before), and the
first frame pays nothing; a shader first loaded in the `build` of the frame that shows it
still overlaps its compilation with the rest of that frame. Without the feature the pipeline
is compiled at the first frame that draws the shader. The pipeline is kept, in the
device's pipeline cache where there is one, so the next launch finds it ready. On the Web,
where there is no such thread, it is compiled at its first frame. A full-screen shader is
measured in `docs/milestone-792.md`, the warm-up in `docs/milestone-800.md`.
