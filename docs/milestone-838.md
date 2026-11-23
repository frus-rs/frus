# Milestone 838 — What an opaque rect hides is not shaded, and its inside does not blend

## Objective

After milestone 837 the demo's home page, scrolled on the test phone (Huawei STK-L21,
Mali-G51 r18p0, Vulkan, 1080 × 2340, MSAA 4×), still ran at about 25 to 30 ms a frame, bound
by the GPU: the shell's `render` phase, most of which is `present` waiting for the GPU, was
14 to 24 ms of it. Milestone 837 left two leads in its "Not done": the page background under
the task card was still shaded (about 1.9 M pixels a frame), and every rect was drawn with
blending, which keeps a tiled GPU from dropping hidden pixels by itself. This milestone tries
both, measures each on the phone, and keeps what the numbers support. The picture must not
change by one pixel.

## What was still shaded for nothing

The page's scene (see milestone 837 for how it was dumped) is, in its first rect batch, the
page background (1081 × 2340, opaque), four fully transparent ink circles in the app bar, the
card's three shadows and then the card (927 × 2772, opaque, clipped to 1081 × 1999), then the
card's contents: tiles, chips and rows, many of them opaque. After milestone 837:

- **The background under the card**: the card covers about 1.74 M of the background's 2.5 M
  pixels fully. Milestone 837 only cut *shadows* under an opaque rect, and only a rect within
  six instances of the shadow; the background is nine instances before the card.
- **Opaque rects under opaque rects**: the card under its tiles, and so on.
- **Rects that paint nothing**: the app bar's ink circles have no colour at all and are drawn
  anyway (four quads of 110 × 110 to 275 × 110).
- **Every rect blends.** A Mali GPU kills the fragments of earlier draws still queued under a
  fragment that does not blend (forward pixel kill). A rect's edge is anti-aliased in the
  shader, so the rect pipeline always blends, and the GPU can never do this.

## The change

All of it is in frus-gpu's rect painter (`painter.rs`, `quad.wgsl`). `cut_hidden` replaces
milestone 837's `cut_hidden_shadows` and runs on each rect batch's instances as they are built.

### Any rect is cut where a later opaque rect hides it

A batch's **covers** are its opaque rects (one colour or an opaque fade, alpha 1, no blur, an
opaque border or none) whose fully covered part is at least 16 pixels each way. That part is
the rect less its largest corner radius, or its border width when wider, and a pixel more all
round, held to its clip, with edges on whole pixels.

Each instance, blurred or not, is weighed against the next eight covers after it in the batch,
however far: the one that hides most of what the instance shades (its quad, grown by a shadow's
margin, within its clip) is chosen, and the instance is replaced by up to four copies of
itself, each clipped to one band of the ring around the hidden part. This is milestone 837's
cut, unchanged, for every rect rather than for shadows only. On the home page the background is
now drawn as its four bands around the card (the app bar, the navigation bar and the two
margins), and the card as its bands around the largest tile on it.

### The inside of an opaque rect is drawn without blending

An opaque cover that no later cover cuts, with a fully covered part of at least 32 pixels each
way, is replaced by **its inside** (the fully covered part, as its clip) and its ring (up to
four bands, as above). The inside goes to a second pipeline, `frus.pipeline.opaque`: the same
shaders, no blending, and a vertex entry point (`vs_opaque`) that holds the quad to the clip
exactly instead of the clip grown by a pixel. Grown, it would write the pixel round the inside,
which the clip gives no coverage, as nothing; held exactly, its edges lie on whole pixels and
cover only the pixels inside.

`Painter::draw` draws a batch's range as its blended runs and its opaque runs, in order,
switching pipelines between them; the opaque instances are gathered into runs once a frame.

### A rect that paints nothing is dropped

A flat rect whose two colours have alpha nought and whose border is absent or has alpha nought
is left out: blended at alpha nought, it leaves every pixel as it was.

### A latent hole in "opaque"

A rect with a border is shaded as `mix(color, color2, t)` even when its fade has no direction
(then at `t = 0.5`); only a flat rect with no border and no direction is shaded from its first
colour alone. Milestone 837 counted a bordered rect with a translucent second colour as
opaque. `is_opaque` now asks for an opaque second colour unless the rect takes the flat path.
The scene's helpers always give a solid rect the same two colours, so no picture changed.

## Why the picture does not change

- Within one draw, instances are drawn in order, and nothing of another batch comes between
  two instances of the same batch. Inside its fully covered part an opaque rect's coverage is
  exactly one and its alpha one, so it writes its own colour whatever was under it: it is the
  last word there. The last rect that covers a pixel fully is never cut at that pixel, which is
  why a cover may itself be cut by a later one.
- The bands' shared edges lie on whole pixels and pixel centres on half pixels, so every pixel
  centre is in exactly one band or in the hole; a band whose far edge does not come back
  exactly from its width in floating point leaves the rect whole (as in milestone 837).
- Inside the fully covered part the shader outputs alpha one. Blending `src × 1 + dst × 0`
  gives `src`, which is what the opaque pipeline writes. The inset by the border width as well
  keeps the border's mix at exactly nought there.

## How it was tested

- `painter::tests::a_hidden_shadow_is_cut_to_a_ring` (updated): the shadow's three bands
  partition what the shadow can shade, less what the card hides; the card becomes its inside,
  marked opaque, and three bands, which together partition the card's clip exactly.
- `painter::tests::a_shadow_that_shows_is_left_whole` (updated): a translucent card, a blurred
  one, a bordered one whose second colour is translucent, a card before the shadow and a lone
  shadow leave the shadow whole.
- `painter::tests::a_page_under_a_card_is_cut` (new): a page with ten translucent rects
  between it and a card is cut to four bands around the card; the card's inside is opaque.
- `painter::tests::a_rect_that_paints_nothing_is_dropped` (new).
- `painter::tests::cutting_hidden_rects_changes_no_pixel` (replaces
  `cutting_hidden_shadows_changes_no_pixel`): a page, two cards with two shadows each, tiles on
  a card (flat, a border wider than its corners, an opaque fade), a translucent rect over them
  and a transparent one, under a clip with fractional edges, rendered on the GPU (with the
  adapter's MSAA) with and without `cut_hidden`: at least five cuts and four opaque insides,
  and not one pixel differs. It passes on the desktop GPU and under WSL's software Vulkan.
  **Mutation check**: with `vs_opaque` growing its quad by a pixel, as the blended one does,
  this test fails ("pixels changed by the cut").
- All 99 frus-gpu tests pass; clippy for the workspace, for Android and for wasm; strict docs.
- **Goldens**: every frus-test and frus-gpu target passes under WSL (`CARGO_INCREMENTAL=0`,
  fresh build), with no golden regenerated.

## Measured

**What was measured** is an experiment build: this change with a temporary switch (a system
property read once at start, not in the change) choosing what `cut_hidden` does, all four
choices from one release APK, interleaved, two rounds, the same swipes on the home page. Mode 0
is close to milestone 837 (only shadows cut, by an opaque rect within six instances; no opaque
insides); mode 1 adds the general cut and the dropping of empty rects; mode 2 adds the opaque
insides only; mode 3 is both, which is this change. Frame stats medians, from the shell's
log windows during a slow swipe (400 ms) and a fast one (150 ms):

| mode | interval, slow swipe | interval, fast swipe | render, slow | render, fast | fps, fast |
|---|---|---|---|---|---|
| 0 (≈ 837) | 31.6 / 30.4 ms | 26.1 / 24.3 ms | 24.3 / 20.7 ms | 19.3 / 14.0 ms | 39 / 42 |
| 1 (cut) | 25.6 / 26.4 ms | 20.7 / 21.8 ms | 17.5 / 19.3 ms | 11.5 / 13.6 ms | 48 / 47 |
| 2 (opaque insides) | 25.1 / 25.2 ms | 21.6 / 21.3 ms | 17.7 / 17.0 ms | 13.7 / 11.9 ms | 45 / 49 |
| 3 (both, this change) | 25.2 / 24.8 ms | 21.3 / 21.1 ms | 17.8 / 17.0 ms | 13.9 / 12.6 ms | 48 / 49 |

(Round 1 / round 2.) The app was idle before and after the swipes in every run.

So, against mode 0, the frame interval drops by about 5 ms on slow swipes and 3 to 4 ms on fast
ones, and the render phase (mostly waiting for the GPU) by 3 to 6 ms; the two halves each give
about the same gain and do not add up on this page.

**Not measured: the final build against master.** Release APKs of master (`f715a3de`) and of
this change without the switch were built from separate target directories (different
checksums), and the three interleaved rounds were scripted, but the phone was locked with its
screen off when the run began and stayed so for the 30 minutes waited; no numbers were faked.
No property was left set on the phone (`debug.frus.j838` was restored to empty,
`debug.frus.frames` to its earlier `1`). The SurfaceFlinger frame-interval column of the
script printed 0 ms medians in these runs (why was not looked into), so the frame
stats' own intervals are the ones reported.

## Not done

- **The two halves overlap on this page.** Measured apart (a temporary switch, not in the
  change), the general cut alone and the opaque insides alone each brought the frame interval
  from about 30 to 25 ms on slow swipes and from about 25 to 21 ms on fast ones, and both
  together did not measurably beat either. On this page they remove the same hidden pixels,
  the background's under the card. Both are kept: the cut needs no help from the GPU (a
  desktop GPU or another mobile GPU may not kill hidden fragments), and the opaque insides also
  cover what the cut does not: pixels hidden by a rect of another batch drawn later, or under
  text, images and paths drawn before an opaque rect in a later batch, when those are still
  queued.
- **Only one cover cuts a rect.** A card under several tiles is cut around the largest only.
- **Opaque insides still run the full rect shader.** A cheaper fragment entry point for them
  (a flat colour, no distance field) was not tried, because the full shader is what keeps the
  output identical; on a flat fill it already returns early.
- **`present` still waits for the GPU** on this driver; not changed here.
