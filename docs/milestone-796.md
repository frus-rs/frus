# Milestone 796 — GPU memory a page leaves behind, and passes a phone pays for twice

## Objective

frus apps must be fluid, fast and light on a phone. Measured on the test phone (Huawei
STK-L21, Mali-G51, Vulkan, Android 10), three things were not:

1. **GPU memory grew by about 130 MB with the first page visited and never came back.** The
   demo opened at 128 MB PSS (66 MB of it the GPU driver's, `GL mtrack`); one visit to `/charts`
   and back left it at 267 MB (194 MB of GPU).
2. **The shader gallery ran at 22 to 26 frames a second**: a plasma shader in a rounded card, a
   shimmering title and a GPU pass.
3. **A page with native views scrolled slower** than a plain page.

And, after each change: nothing animating must cost no frame and no CPU.

## What was wrong

### Memory

Four things kept GPU memory after the page that needed it was gone.

- **The device allocator's blocks.** wgpu was asked for `MemoryHints::Performance`, under
  which its Vulkan allocator takes memory from the driver 128 to 256 MB at a time and gives a
  block back only when nothing at all lives in it. A page transition draws both pages into
  screen-sized layers, multisampled; those landed in a new block, and one small texture made
  during the visit (a glyph atlas grown, an image) kept the whole block alive. The 130 MB step
  in `GL mtrack` is that block.
- **The glyph atlas never reused its room.** The text painter never called the atlas's `trim`,
  so every glyph ever drawn stayed marked "in use": the atlas could never evict, only double, and
  it kept its largest size for good.
- **Multisampled scratch targets were kept.** One per size drawn at, up to sixteen, each four
  times the size of the layer it was drawn for, and kept after the layer itself was cached or
  gone. The staging copy of the screen made for a backdrop blur was kept too.
- **Multisampled targets were real memory at all.** A tiled GPU never needs the samples outside
  its tile memory when a pass resolves and does not store them. frus stored them (`StoreOp::Store`)
  every pass, and allocated them as ordinary textures: a full-screen 4× target is 40 MB, and
  writing it back costs four times the screen's bytes of bandwidth every pass.

### The gallery

The plasma sits in a `ClipRRect`. The clip is a layer whose only content is the shader, and
the shader changes every frame it plays, so the clip's layer was rendered again every frame into
a multisampled texture the size of the card — only to be composited with rounded corners. That,
on top of the multisampled stores above, is where the frames went. (The shimmering title was
already cheap: the mask's time is the compositing draw's, and the text's texture is kept; a test
now says so.)

### Native views

frus moved the native views (`setTranslationX/Y`) on every frame a page scrolled. Their
pictures are drawn by frus, so the native place only matters to the screen reader and the
keyboard's popups; but Android draws a view again when it moves in its window, and every such
draw was an image read back, copied and uploaded. `dumpsys gfxinfo` counted 44 view draws during
six swipes of `/native`. A view is now moved once it stops (and resized at once): 25.

That was not where the page's time went, though; see "Measurements" and "Not done".

### A layer as large as its clip

Measuring the gallery after the changes above showed no change at all, which is what found the
real cost: `Scene::shaded` (what `ShaderMask::shader` paints) makes its layer with the current
clip — the screen — and a layer's texture, its multisampled pass and its compositing draw were
the size of its clip. The shimmering title was a full-screen custom shader every frame. A
layer's region is now its clip held to what it holds: the batch planner's footprint of its
content (the one the draw order already relies on), plus its shader's box, grown by 16 px for an
antialiased edge; a line of text counts for the clip's whole width and its own height again
above and below, since a title can draw past the box it was laid out in. Any group faded or masked with the screen as its clip
benefits.

## The change

- `frus-gpu/renderer.rs`: `MemoryHints::MemoryUsage` on Android and iOS (blocks of 8 to 64 MB);
  desktops keep `Performance`. Where the adapter says `transient_saves_memory`, the painters'
  multisampled scratch targets are made `TRANSIENT_ATTACHMENT`.
- `frus-gpu/compositor.rs`:
  - a pass discards its multisampled samples (`StoreOp::Discard`) unless a later segment of the
    same frame loads them again (a frame cut for a backdrop or a blended layer); the resolved
    picture is all that is written;
  - a scratch target is transient only for a draw done in one pass that clears first, which is
    what a transient attachment allows; a frame cut into segments gets a kept one;
  - at the end of a frame, the scratch targets and the staging texture the frame did not use are
    let go;
  - `folded_clip`: a layer that only clips (rectangle, rounded rectangle or oval; no transform,
    no filter) a single shader filling a box or a single GPU pass is composited as that shader,
    through the outer clip's shape, with the two opacities multiplied. Only where the result is
    the same: the inner layer's clip holds the outer one, or the outer one is a plain rectangle.
  - `layer_region` holds a layer's region to its content's footprint (plus 16 px);
  - a layer whose content changed is drawn again **into its own texture** when the size is the
    same, instead of a new one every frame;
  - `Painters::memory()`: the textures kept from frame to frame and their size.
- `frus-shell/platform_view.rs`: a native view that moves is placed on the first frame it
  stays put (the shell asks for that one frame); a new size is sent at once.
- `frus-gpu/text.rs` + `frus-glyphon`: `TextAtlas::trim` once a frame, before the frame's first
  prepare (never between two, so a later batch cannot evict what an earlier one drew with);
  `TextAtlas::sizes` for the count.
- **The guard**: `Renderer::gpu_memory() -> GpuMemory { textures, bytes }`,
  and every frame-statistics line (`FRUS_FRAME_STATS=1`, `debug.frus.frames`) ends with
  `GPU memory X MB in N textures`.
  It is computed only when a line is written.

## How it was tested

Unit tests in `frus-gpu` (real GPU where there is one, skipped otherwise):

- `a_page_left_leaves_no_memory_behind`: a page fading and growing over a backdrop for twelve
  frames, then gone: the painters hold exactly what they held before it (textures and bytes).
- `the_glyph_atlas_reuses_its_room`: 26 capitals at 40 sizes, one size a frame: the atlas stays at
  what one frame needs.
- `a_rounded_clip_around_a_playing_shader_renders_no_layer`: three frames of a playing shader in
  a rounded clip render no layer.
- `a_clip_is_folded_only_where_it_draws_the_same`: no fold for a shader smaller than the rounded
  clip, a filtered clip, or a clip around anything else; a plain rectangle folds to the
  intersection.
- `a_playing_shader_mask_keeps_its_text`: the shimmering title is rendered once.
- `a_layer_is_drawn_around_what_it_holds`: a 20 px square in a screen-clipped layer gets a
  46 px region; a shader's box counts.
- `a_layer_drawn_again_reuses_its_texture`: three frames of changing content, one texture.
- `platform_view::a_moving_view_is_placed_once_it_stops`: nothing sent while it moves, one
  `Place` when it stops, a resize at once, and the rectangle touches use follows the frame.
- `frame_stats::a_line_ends_with_the_gpu_memory`: the line's end, and the memory asked once.

With `end_frame`, the atlas trim and the fold disabled, four of these fail (the mask test
passes both ways: it documents behaviour that was already right). All 88 `frus-gpu` tests pass.

## Measurements

Test phone (Huawei STK-L21, Mali-G51, Vulkan, Android 10), release builds of `frus-demo` and
`frus-shaders` from `master` (before) and this branch (after), installed in turn and measured
back to back, battery at 40 to 42 °C. Memory from `dumpsys meminfo` (PSS total; `Graphics` and
`GL mtrack` are the same figure on this driver); frames from the frame statistics line.

### Memory, home → `/charts` → back

| | Home, first | On `/charts` | Home again (1st, 3rd, 5th return) |
| --- | --- | --- | --- |
| Before: PSS / GPU | 136.8 / 66.0 MB | 284.0 / 196.0 MB | 278.8 / 194.3 MB, every time |
| After: PSS / GPU | **75.7 / 5.9 MB** | **102.2 / 17.9 MB** | **101.0–101.4 / 17.0 MB** |

The first page visited took 130 MB of GPU memory before and kept it; now it takes 12 MB, and
returning home gives back what `/charts` alone needed. Five round trips leave it where the
first left it. The frame statistics now end with, at home after the visits: `GPU memory
1.2 MB in 3 textures` (the measuring build also printed wgpu's allocator report: 41.1 MB in
use of 51.0 MB taken from the driver; that report was left out of the shipped line, see Not
done).

### Shader gallery (`frus-shaders`)

| Battery | | Frames | Interval median / 95th | Render median | PSS / GPU |
| --- | --- | --- | --- | --- | --- |
| 44 °C | Before | 18 fps | 56.3 / 59.0 ms | 53.6 ms | 274 / 202 MB |
| 44 °C | **After (this branch)** | **32–33 fps** | **30.9–31.3 / 32.6–35.3 ms** | **27.4–27.7 ms** | **84 / 16 MB** |
| 40 °C | Before | 22 fps | 47.1 / 51.7 ms | 43.5 ms | 278 / 202 MB |
| 40 °C | An earlier build | 41–42 fps | 24.0–24.8 / 26.6–28.3 ms | 21 ms | 89 / 16 MB |

The phone warmed up between the two sessions, and the before build was measured again each
time, back to back with the after one. The earlier build held the shimmering title's layer to
the text's own box; a golden then showed a title drawing past its box (see "A layer as large as
its clip"), and the branch now gives text the clip's whole width, which costs the gallery some of
that gain: 1.8 times the frames instead of 1.9, a third of the memory either way. The plasma
alone filling the screen runs at 39 fps at 44 °C (47 at 40 °C, 55 when the phone was cool,
milestone 792): the gallery is now close to what its pixels of plasma cost, and the 55 fps
target is this GPU's fill rate for the shader, not frus's overhead (see Not done).

### Native views (`/native`), scrolled with six swipes beside the views

| | Interval median | Render median (of it, drawing) | View redraws (`gfxinfo`) |
| --- | --- | --- | --- |
| Before | 51–61 ms | 45–54 ms (6–7 ms) | 44 |
| After | 51–65 ms | 45–58 ms (6–8 ms) | 25 |
| `/licenses`, after | 47–50 ms | 37–39 ms (33–35 ms) | — |

frus's own drawing is 6 to 8 ms a frame on `/native`; the rest of the render (about 40 ms) is
`present` waiting for the GPU, which the views' own rendering in the application's process keeps
busy. Fewer view redraws did not move the frame interval: what is left is not frus's drawing
(see Not done).

### Idle

Nothing moving costs nothing, before and after: home, `/native` after a scroll, and the gallery
paused each logged **0 frames and 0 CPU ticks in 10 s** (`/proc/<pid>/stat`).

### Transitions

A page push still takes about 112 ms a frame for its 300 ms (before: 113 ms): two
screen-sized transformed layers and the arriving page laid out (33 ms) every frame. Unchanged.

### Size

`frus-hello`'s Android library (release, aarch64, the CI size job's build, local toolchain):
14,407,944 bytes on `master`, 14,410,944 with this branch, **+3.0 KB**. A first version was
+9.3 KB; leaving out wgpu's allocator report (about 1 KB) and the texture format table used to
size textures (about 3 KB) brought it down.

### Windows

`frus-shaders` run on Windows (Intel Iris Xe, Direct3D 12): the rounded plasma, the shimmering
title and the GPU pass draw as before (screenshot in the PR), and the frame line ends with
`GPU memory 9.8 MB in 5 textures`.

## Not done

- **The gallery is at 32–33 fps on a warm phone (18 before), not 55.** Most of what remains is
  the plasma's own fill rate on a Mali-G51 (39 fps for the plasma alone at the same
  temperature), and the shimmer's layer now spanning the screen's width. Drawing a playing shader at
  a lower resolution, or without multisampling where it has no edges, would be the next step.
- **`/native` still scrolls at about 18 fps on a warm phone.** frus's drawing is 6 to 8 ms; the
  frame waits about 40 ms in `present`. The likely cost is the web view's own GPU work in the
  process (Chromium renders on its own threads), which frus does not control; importing the
  views' `HardwareBuffer`s (milestone 793's external textures) instead of copying them would
  remove the copies but not that. Not proven with a GPU profiler: the phone has no GPU clock.
- **Page transitions are about 112 ms a frame**, before and after. Two screen-sized layers per
  frame, plus the arriving page's layout (33 ms). Drawing a transformed page straight onto the
  frame when its opacity is 1 would remove one of the passes; not done here.
- The layer region follows the batch planner's footprint of its content, plus 16 px, and a
  line of text counts for the clip's whole width and three times its height: a golden (the
  large app bar's title, laid out narrower than it draws) caught the first version, which
  trusted the text's box sideways. Content drawn further than that outside its footprint, inside
  a layer, would now be cut.
- `MemoryHints::MemoryUsage` is used on Android and iOS only; the iOS path was not run.
- Web: the memory count works there; not run in a browser.
- **The allocator's own figure is not in the line.** wgpu can report what its allocator has
  taken from the driver (which is what showed the pinned 128 MB block), and the measuring build
  printed it; it added about 7 KB to the smallest application's library, where the Android size
  budget has no room, so the shipped line counts frus's own textures only.
