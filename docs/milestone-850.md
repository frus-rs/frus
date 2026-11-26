# Milestone 850 — A scaled layer is drawn for the scale it is shown at

Part of #221 (its "mipmaps" item) and #224 (L-026).

## Objective

A `Transform`, a page transition that zooms, a picker wheel's rows and every widget that scales
its child draw that child flat into a texture of its own and composite the texture through the
transform. The texture was always drawn at the child's own size, and two things went wrong:

1. **Shrunk, it shimmered.** Below one half, a bilinear sample skips texels: a one-pixel
   checkerboard at one eighth came out as whichever texels the samples landed on, black or
   white, and the tone swung as the layer moved. Small text lost strokes the same way. Images
   have had mip chains since milestone 743; layers had none (milestone 732's note).
2. **Enlarged, it blurred.** The texture was magnified: text in a layer scaled four times was
   ten-pixel glyphs stretched over forty pixels, with each edge spread over four.

And either fix had to keep an idle frame free and an animating one cheap on a phone's GPU.

## What was done

**Where a layer is drawn and how** (`crates/frus-gpu/src/raster.rs`, plain functions tested on
the CPU, and `compositor.rs`):

- A layer's transform is measured by its **stretch**: the singular values of its linear part,
  so a rotation changes nothing and a scale about any axis is read whatever the order.
- **Shown smaller and at rest** (the transform the same as the frame before), or shown for
  the first time, its content is scaled down (`Primitive::scaled`) and drawn into a texture of
  the size shown, with no chain: its edges and text are those of content drawn at that size.
  It is drawn at no less than a sixteenth (`MIN_SCALE`); shown smaller still, it gets a chain.
  The first version kept a shrunk layer at its own size with a chain even at rest, and a box
  shrunk to 0.6 had its edge spread over two pixels instead of one (the goldens
  `boxes_that_fit`, `turned_and_rotated`, `explicit_transitions`); a `FittedBox` or a
  `Transform::scale` below one is usually still, so that softness was in every frame.
- **Shrinking or growing below its own size** (the smallest stretch below one, the scale
  moving), it is drawn at its own size with a full mip chain, rendered on the GPU from what was drawn, and the compositor's sampler is trilinear.
  The chain uses milestone 743's generator, given the surface's format and a premultiplied
  entry point (a layer is painted over a transparent target, so its texels are already
  premultiplied; weighting them by alpha again would darken every edge). As for images, any
  shrinking counts: between one and one half the sampler already blends in the second level,
  and a chain made later would change the picture the frame it appeared. The chain is kept
  with the layer's texture in the layer cache: a page zooming out with its content still makes
  one chain for the whole animation, and none on idle frames. A texture drawn shrunk at rest
  is kept while the layer is shown no larger than it was drawn (copied into a texture with room
  for a chain if it shrinks further), and drawn again at its own size once it grows past it.
  Coming to rest costs one draw at the exact scale.
- **Shown larger** (the largest stretch above one), its content is scaled
  (`Primitive::scaled`, the same path a DPI change takes) and drawn into a texture that many
  times larger, so text is laid out and rasterised at the size it is shown at. The composite
  draw's inverse and region are carried over to the texture's texels; where it lands on the
  screen does not move.
- **While the scale moves**, a kept texture is drawn again only when it no longer serves: when
  the scale grows past it, it is drawn a quarter ahead of the scale (so a growing scale is
  drawn again each time it grows by a quarter, not every frame, and is shown at most a quarter
  smaller than it was drawn, from its first level); when the scale falls more than √2 below
  it; and when the transform is the same as the frame before at another scale (at rest): drawn
  once more, exactly. A layer whose content changed is drawn again anyway, at its exact scale.
- **Memory cap**: an enlarged texture never holds more texels than the target it is
  composited onto has pixels, never more than the GPU's largest texture side, and never more
  than eight times the layer's size.
- **A layer with the whole target as its texture** (a backdrop, a path clip, a shader, a
  transformed layer inside it) is drawn at its own size, enlarged or shrunk, with a chain when
  shown smaller: what it draws is placed in the target's own pixels. A plain transformed layer
  whose region would cover the whole target (a full-screen page, or a line of text, whose
  footprint is its clip's whole width) used to be given no region at all, like those; the
  composite then read its texture as the target's size, and a texture drawn at another scale
  showed at the wrong size. It now keeps its region. Found by the test of a layer at rest at
  0.6 holding text, whose box first showed at its flat size, blurred.
- A layer with an image filter is drawn at its own size and gets no chain: its blur radius is
  in the content's units.
- Nested transformed layers (inside another layer's texture) follow the same rules, at their
  exact scale, since they are not kept from a frame to the next.

## Measured

`layer_bench` (`crates/frus-gpu/src/layer_bench.rs`, an ignored test): a phone-sized page
(1080 × 2340, forty lines of 34 px text and as many boxes), 4× multisampling, on the Windows
laptop's Intel Iris Xe (Vulkan). Each frame is timed from the start of its render to the
end of its GPU work. Three runs before and three after, alternated in one job, while other
builds shared the machine: the plain page's floor alone moved between 9 and 14 ms from run to
run, so differences under a few milliseconds are noise. Medians per frame, lowest to highest
of the three runs:

| Scenario | Before | After | Layer draws (before / after) | GPU memory (before / after) |
|---|---|---|---|---|
| Plain page, idle (the floor) | 9.3–12.3 ms | 8.8–14.4 ms | 0 / 0 | 38.9 / 38.9 MB |
| Zoom out 1 → 0.85 over 30 frames, content still | 3.2–3.8 ms | 3.2–4.1 ms | 1 / 1 | 48.5 / 51.7 MB |
| At 0.5, content changing every frame | 9.6–13.8 ms | 10.9–18.1 ms | 30 / 30 | 48.5 / 51.7 MB |
| At 0.5, idle (20 frames) | 2.7–3.0 ms | 3.1–3.4 ms | 1 / 1 | 48.5 / 51.7 MB |
| Zoom in 1 → 2.5 over 30 frames, then 5 at rest | 8.9–10.4 ms | 17.3–23.1 ms | 30 / 31 | 40.4 / 49.6 MB |
| At 2.5, idle (20 frames) | 2.7–3.2 ms | 3.4–3.9 ms | 1 / 1 | 40.4 / 48.8 MB |

With a shrunk layer drawn at its exact scale at rest, three more runs (the machine busier
still: the plain page's floor was 14.8–15.6 ms), against the "After" column above:

| Scenario | Median | Layer draws | GPU memory |
|---|---|---|---|
| Plain page, idle | 14.8–15.6 ms | 0 | 38.9 MB |
| Zoom out 1 → 0.85 over 30 frames, content still | 3.9–4.4 ms | 1 | 51.7 MB |
| At 0.5, content changing every frame | 14.2–15.3 ms | 30 | 50.9 MB |
| At 0.5, idle (20 frames) | 3.0–3.1 ms | 1 | 41.3 MB |
| Zoom in 1 → 2.5, then 5 at rest | 22.8–24.5 ms | 31 | 49.6 MB |
| At 2.5, idle | 3.9–4.6 ms | 1 | 48.8 MB |

The zoom out still draws its page once and makes one chain while it moves. At rest at 0.5,
the page is drawn once at its exact scale into a texture a quarter of its size, with no
chain: 41.3 MB against 48.5 MB on master and 51.7 MB with the chain kept. Content changing
every frame at 0.5 is drawn at 0.5, into that smaller texture. The idle frames draw nothing.

- **Idle frames do no extra work**: the same layer draws as before (the first frame's only),
  no chain made again. What an idle frame of a shrunk or enlarged layer costs is its composite
  draw, which now reads a trilinear or a larger texture.
- **A shrinking page** costs its chain once while it moves: 3.2 MB, a third of its 9.7 MB
  texture. At rest it is drawn once more, smaller and with no chain (see the second table). The zoom
  out does not draw the page again for it (the picture it had is copied into a texture with
  room for the chain). Content changing every frame while shrunk makes the chain every frame:
  a few milliseconds here at most, within the noise.
- **An enlarging page** is the cost of sharp text: a page zooming across the whole screen is
  drawn every frame before and after (its region moves), but now into a texture the screen's
  size rather than the region's, and with its glyphs rasterised at the sizes shown, so about
  twice the time per frame while it zooms. At rest its texture is the screen's size: 9.2 MB
  more than the 1.6 MB texture of the region it magnified. The one more layer draw is the
  exact scale at rest.

## How it was tested

- `raster.rs` (CPU): the stretch of rotations, scales and mirrors; the memory limit and the
  texture's side; a scale animating from one to three drawn again at most six times in sixty
  frames, never shown magnified, and exactly at three once at rest; a shrinking scale drawn
  again only past √2; a scale shrinking from one to 0.6 never drawn again while it moves,
  once at 0.6 at rest, then kept, drawn again at its own size when it grows back; when a chain
  is needed; the texel offset and the size it adds.
- `compositor.rs` (GPU): a shrinking layer gets its chain once and keeps it through a
  shrinking animation, is drawn once at its exact scale at rest, with no chain, and not again
  on idle frames (the chain of a layer kept as it is is
  made from a copy of its picture, which is checked texel by texel); the chain holds the
  picture at every level with and without multisampling, and a one-texel checkerboard image's
  third level and the screen are the grey of linear 0.5 (sRGB 187); an enlarging layer is drawn
  again a handful of times over thirty frames, at rest at its exact scale, its texture three
  times its region, and not again on idle frames.
- `frus-test/tests/layer_raster.rs` (GPU, the same file run before and after the change):

  | Measure | Before | After |
  |---|---|---|
  | One-texel checkerboard in a layer at 1/8: mean (grey is 187.5) | 255.0 | 187.0 |
  | Same: pixels black or white, of 576 | 576 | 0 |
  | Same, moved a texel across at a time over eight steps: range of the mean | 0.0..255.0 | 187.0..187.0 |
  | Text ×4 in a layer: edge contrast (text drawn at ×4 size: 245.0) | 86.9 | 245.0 |
  | Same: lit pixels (drawn at size: 809) | 1372 | 809 |
  | Text ×3, turned: edge contrast | 112.1 | 196.6 |

  Before, every sample of the shrunk checkerboard lands on a white texel; text magnified
  four times spreads each edge over four pixels and lights 70 % more of them. The
  checkerboard is shown while its scale moves (a frame at its own size, then one at an
  eighth, `frus_test::render_scenes`), which is when the chain reads it; master keeps
  nothing between frames that would change its picture.

  At rest, against content drawn at the size shown:

  | Measure | Chain at rest (first version) | Exact scale at rest |
  |---|---|---|
  | Text and a box in a layer at 0.6: edge contrast (drawn at size: 255.0) | 182.9 | 255.0 |
  | Same: pixels off by more than 24 | 263 | 0 |
  | A box in a layer at 0.6 moved to (5, 4.5), region off the corner: pixels off | | 0 (163 without the texel offset) |
- Goldens: run under WSL on lavapipe (mesa 25.2.8, CI's version); every frus-test target
  passes. A golden is one frame, with nothing kept, so every scaled layer in one is drawn at
  its exact scale. Each moved golden was read against master's and the first version's:
  - `boxes_that_fit` (a `FittedBox` at 0.6) is master's again, pixel for pixel;
    `turned_and_rotated` (a `Transform::scale(0.6)`) differs from master in 12 pixels of its
    rounded corners, by at most 9 levels; `explicit_transitions` (a `ScaleTransition`) has
    edges as sharp as master's, its corners drawn at the scale (88 pixels, at most 24). The
    first version, a chain at rest, spread their edges over two pixels; drawn at 0.6 with
    texels off the screen's grid, they were softer still, which the texel offset fixed.
  - enlarged, text drawn at the size shown: `sliver_app_bar_expanded` (the large title),
    `open_container_third` (the opening card's title), `animated_switcher_halfway` (the
    digits growing in), all sharper than master's magnified texture.
  - shrunk pages and rows, text drawn at the size shown: `picker_wheel` (the rows above and
    below the selection; the selected row, not scaled, is unchanged), `navigator_mid_push_zoom`,
    `navigator_mid_push_shared_z`, `navigator_mid_push_fade_scale`, `motion_switches_third`,
    `motion_switches_sixty`. Their text is thinner than master's, which drew it at full size
    and resampled it down; card outlines and positions are unchanged (an edge pixel's
    coverage differs by a few levels).
  None lost content, changed scale or moved. `navigator_mid_push_fade_scale` and
  `motion_switches_sixty` are the second golden of a test whose first had failed, so they
  showed only once the first was updated.

## Not done

- **Content laid out off the screen.** A transformed layer's texture holds what of its content
  lies flat on the target (since milestone 831, and the whole target before it): content laid
  out past the screen's edge and shrunk into view is not drawn. Found while writing the
  checkerboard test, whose picture first lay partly off its canvas; recorded in L-026. A shrunk
  layer at rest is now drawn at its scale, so its region could be what of its content lands on
  the screen rather than what lies there flat, with a texture no larger than the screen; not
  done here.
- **Layers drawn at their own size whatever the scale**: a layer with an image filter (its blur
  radius is in its content's units), and a layer that may draw anywhere and so has the whole
  target as its texture: one clipped to a path, over a backdrop, coloured by a shader, or
  holding another transformed layer. The inner layers of such a layer are drawn at their own
  scale.
- **A shrinking layer is softer while it moves**: read through its chain, a hard edge spreads
  over about two pixels until the scale comes to rest, which costs one more draw. A layer moved
  by a fraction of a pixel at rest is drawn once more too, so that its texels fall on the
  screen's pixels; while it moves, its texture serves as it is.
- **A growing scale runs ahead**: up to a quarter, read from the first level, so text is a
  little softer than drawn at its size while it grows; the exact scale needs a frame drawn
  with the scale at rest. A frame that never comes (nothing else moves) leaves the last texture.
- **Enlarging a whole page costs more**: its texture is the screen's size instead of the
  region's, and its text is rasterised at the sizes it is shown at (see the measurements).
- **The chain's pipeline is made the first time a layer is shown smaller**, as the images'
  is: a shader compiled during that frame, once per run.
- **No bicubic filter**: `FilterQuality::High` enlarges an image as `Low` does.
- **Not run on a phone, on WebGL 2 or on OpenGL ES**: the device is not to be used for this
  milestone. The chain is rendered with the same render passes, `textureLoad` and sRGB targets
  as the images' chain (milestone 743). Measured and tested on the Windows laptop's GPU (Vulkan).
