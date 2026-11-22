# Milestone 837 — The GPU stops shading what nobody sees

## Objective

After milestone 836 the demo's home page, scrolled on the test phone (Huawei STK-L21,
Mali-G51 r18p0, Vulkan, 1080 × 2340), spent about 4 to 5 ms of CPU on layout and paint, yet
its frames stayed about 45 ms apart. The shell's "render" phase grew to 37 to 45 ms, of which
encoding and submitting were 3 to 4 ms and waiting for a surface image under 1 ms. The rest is
`present`: on this driver, handing the image to the display waits for the GPU to finish it.
The frame was bound by the GPU at about 45 ms. This milestone looks for what the GPU spends
that time on and cuts it without changing a pixel.

## What a frame costs the GPU

### Not the passes

The phone has no GPU timestamp queries, so the frame's structure was checked first, from the
code and from wgpu's Vulkan backend:

- **One render pass a frame**, plus one small pass for the one layer on the page (a 55 %
  opacity strip about 820 × 140 pixels), which is cached and 0.6 MB in all. No backdrop, so no
  staging texture, no segmented frame and no blit.
- **The multisampled target is already what a tiled GPU wants** (milestone 796): created
  `TRANSIENT_ATTACHMENT` (lazily allocated memory on Mali), cleared on load, stored with
  `StoreOp::Discard`, and resolved inside the pass into the swapchain image (wgpu puts the
  resolve in the Vulkan subpass's resolve attachments). The samples never leave tile memory.
- **Pacing**: FIFO, two frames in flight; waiting for an image is 0.7 ms. Nothing is read back
  per frame and the device is only polled without waiting.

So the GPU's 40 ms is not bandwidth between passes. It is what the one pass shades.

### What the pass shades

The home page's scene at the phone's scale (a test that printed the demo's scene primitive by
primitive) shows where the pixels come from. The list of tasks is **one card 927 × 2772
pixels**, taller than the screen, inside a scroll view clipped to 1081 × 1999. The card is
three blurred rects (its shadows, each a little larger than the card) and then the card itself,
opaque white:

| Rect | Size (px) | Pixels shaded on screen |
|---|---|---|
| shadow 1 (blur 2.8) | 927 × 2772 + margin | ~2.0 M |
| shadow 2 (blur 2.8) | 932 × 2778 + margin | ~2.0 M |
| shadow 3 (blur 8.2) | 943 × 2788 + margin | ~2.1 M |
| card | 927 × 2772 | ~1.9 M |
| page background | 1081 × 2340 | 2.5 M |

The screen is 2.5 M pixels. The three shadows alone are about 6 M pixels a frame, about two
and a half screens, and a shadow pixel is the dearest thing the rect shader computes (an error
function pair and a square root at the least, four of each near a corner). **Almost all of
those shadow pixels are under the opaque card**, which paints over them a few instructions
later. And every one of the five large rects was shaded from the top of the screen to the
bottom: the clip was tested per pixel in the fragment shader, so the rows above and below the
scroll view (the app bar's 176 rows and the navigation bar's 165) were shaded and then given
no coverage.

## The change

Both parts are in frus-gpu's rect painter (`painter.rs`, `quad.wgsl`).

### The quad is held to its clip

The vertex stage now clamps each rect's quad to its clip rectangle, grown by one pixel. The
fragment stage still tests the clip at the pixel's centre, exactly as before; the pixel of
margin keeps every sample of a pixel whose centre is inside the clip covered, so the
multisampled edge is what it was. What lies beyond the clip was given no coverage before and is
not rasterised now.

Because the quad's corners now move with the clip, a pixel's position in the rect is no longer
interpolated across the quad (a position interpolated between moved corners rounds
differently from one clip to the next). The fragment stage reads the pixel's centre from
`@builtin(position)` and subtracts the rect's centre, passed flat. That also drops three
interpolated varyings (`local_px`, `uv`, `frag_px`) for one flat one, which a Mali GPU pays
for per pixel.

### Shadows are not shaded where the surface over them hides them

`cut_hidden_shadows` runs on each rect batch's instances as they are built. A shadow (any rect
with a blur) followed in the same batch, within six instances, by an **opaque** rect (one
colour or an opaque fade, alpha 1, no blur, an opaque border or none) is replaced by up to four
copies of itself, each clipped to one band of the ring around the part that rect covers fully:
the rect less its largest corner radius and a pixel all round, held to both clips, with edges
on whole pixels. Above, below, left, right.

Why the picture does not change:

- Inside that part the opaque rect's coverage is exactly 1 and its alpha is 1, so the blend
  writes its colour whatever was under it. Instances of one draw are drawn in order and
  nothing of another batch comes between two instances of the same batch, so the opaque rect is
  the last word on those pixels.
- The bands' shared edges lie on whole pixels and pixel centres on half pixels, so every pixel
  centre the shadow could shade is in exactly one band, and the clip test there gives it the
  colour it had. The outer edges are the shadow's own clip; a band whose far edge does not come
  back exactly from its width in floating point (the shader tests `x <= clip.x + clip.width`)
  makes the shadow stay whole.
- With the quad held to its clip, the hidden part is never rasterised.

A translucent surface, a blurred one, a gradient, or a shadow with nothing over it is left as
it was. On the home page the card's three shadows each become a ring 30 to 60 pixels wide, and
about 5.5 M shaded shadow pixels a frame go.

## How it was tested

- `painter::tests::a_hidden_shadow_is_cut_to_a_ring`: a shadow under an opaque card, clipped on
  a fractional edge, becomes three bands (nothing is left above the card, which runs past the
  clip's top); every pixel centre of a 400 × 400 sweep is in exactly one band, except those the
  card covers fully, which are in none. Fails without the change (one instance, not three).
- `painter::tests::a_shadow_that_shows_is_left_whole`: a translucent card, a blurred one, a card
  before the shadow, a card too far after it, and a lone shadow leave the instances alone.
- `painter::tests::cutting_hidden_shadows_changes_no_pixel`: two cards with two shadows each,
  under a clip with fractional edges, one card cut by the clip, rendered on the GPU with and
  without the cut: all four shadows are cut, and not one pixel differs.
- All 97 frus-gpu tests pass.
- **Goldens**: all frus-test and frus-gpu targets pass under WSL's software Vulkan
  (`CARGO_INCREMENTAL=0`, fresh build), with no golden regenerated: the position read from
  `@builtin(position)` and the clamped quads leave every image within the usual tolerance.

## Measured

**Not measured on the phone.** The test phone was disconnected when the first run began, came
back once for long enough to install the APK, and went away again for the rest of the
milestone (more than 30 minutes of waiting). No property was left set on it. The release APKs
for the comparison are built (base = master with milestone 836; new = this change, built from
separate target directories, with different contents) and the interleaved script is ready, but
it has not been run. The elimination runs planned with a temporary switch (no shadows, MSAA 1×,
no rects, no text) did not run either. That switch is not in the change.

What is known without the phone:

- The arithmetic above: of the about 10.5 M pixels the home page's pass shaded per frame, about
  6 M were shadow pixels and about 1.3 M were outside their rect's clip. After the change about
  0.5 M shadow pixels are left, in the card's ring.
- The frame structure: one pass, no memory round trip (see above). So no pass, load or store is
  left to remove, and what the GPU does is what it shades.

Without the phone, the share of the 45 ms these pixels cost is an estimate, not a measurement.
Milestone 835 halved the GPU part of the frame by making the same shadow pixels cheaper, which
suggests that shadow pixels are the larger part of it.

## Not done

- **Other hidden pixels are still shaded.** Only a blurred rect under an opaque rect of the same
  batch is cut. A flat rect under an opaque one (the page background under the card, about
  1.9 M pixels a frame here) is cheap per pixel and left alone; so is anything hidden by a rect
  of another batch, or by an image or a path.
- **Opaque rects are still drawn with blending.** A Mali GPU discards the hidden pixels of
  earlier draws by itself (forward pixel kill) only under a draw that does not blend, and every
  rect blends because its edge is anti-aliased in the shader. Drawing the inside of a large
  opaque rect without blending, and only its edge with it, would let the GPU do the cut above
  for everything. Not tried.
- **`present` still waits for the GPU** on this driver, so the CPU and the GPU of a frame do not
  overlap. That is the driver's, and not changed here.
