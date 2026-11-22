# Milestone 835 — The home page's frame on a slow phone's GPU

## Objective

On the test phone (Mali-G51, r18p0 driver), the demo's home page scrolled at 11 to 16 frames
a second, with half of every frame spent waiting on the GPU. This milestone looks for where
those GPU milliseconds go and cuts them without changing a pixel.

## Where the time goes

A dump of the home page's scene (the demo's `view`, read primitive by primitive) shows what
the GPU fills each frame:

| Primitive | Count | Area, in screens |
|---|---|---|
| Rect, flat fill, no border | 22 | 1.78 |
| Rect, with a border | 5 | 0.17 |
| Rect, blurred (shadows) | 6 | 1.83 |
| Path, image, layer | 14 | 0.03 |
| Text | 22 | ~0 |

Almost every pixel the GPU shades comes from the rect shader, through one of two kinds of
rect. A temporary switch that skipped each kind of batch on the phone was used during the
investigation. It is not in the change.

- **Shadows** cover nearly two screens. Each shadow pixel summed four rows of the blurred box,
  and each row costs two error functions: eight `erf`s and four `exp`s a pixel.
- **Flat fills** (the page background and the cards) cover nearly two screens too. Each pixel
  converted the same sRGB colour to linear light, with three `pow`s, through the general fill
  path.

## The change

Both changes are in `quad.wgsl`.

### A shadow's straight band takes one row, not four

The column a shadow pixel sums runs three deviations either side of it, in four steps of one
and a half deviations. When the whole column lies between the box's corners
(`|p.y| + 3σ ≤ half.y − corner`), every row it samples is the same row: the box is at its
full width there. The four samples then add up to that one row times the sum of the four
gaussian weights. With the samples at ±0.75σ and ±2.25σ, that sum does not depend on σ: it is
`STRAIGHT_BAND_WEIGHT = 0.99863125`. On a large card most of the shadow is in this band, and
there it now costs a quarter of what it did.

The constant is not 1, and a mistake of a fraction of a percent is less than one level in the
rendered picture, so no golden would notice it.
`painter::tests::the_straight_band_weighs_what_the_four_samples_weigh` reads the constant out
of the shader and checks it against the four weights summed in f64, at deviations from 0.5 to
300. A mutation run confirmed that the test catches 1.0 and 0.9986.

### A flat fill is converted once, in the vertex shader

When a rect has a single colour and no border, the vertex shader converts that colour to
linear light once and passes it on as a flat varying. The fragment shader then returns that
colour times the coverage, skipping the general fill path. Any other rect (a gradient, a
two-colour fade, a border or a shadow) marks the varying with a negative alpha and keeps the
general path.

## Measured on the phone

Both APKs were built from the same tree, one with the master shader (base) and one with the
new shader (opt1). Each was installed in turn twice (base, opt1, base, opt1), and each run
did four swipes over the home page's list. The table below gives the shell's frame
statistics (`debug.frus.frames`) for the three half-second windows of each run after the
first one, which includes the first build. That makes six windows per build.

| | Base | Opt1 |
|---|---|---|
| Frame interval, median of windows | 82 ms | 50 ms |
| Frame interval, range | 70 – 85 ms | 40 – 54 ms |
| Frames a second | 11 – 16 | 19 – 25 |
| Render (submit to presented), median | 53 ms | 23 ms |
| Render, range of window medians | 35 – 63 ms | 16 – 32 ms |
| Layout + paint (CPU), range | 16 – 31 ms | 18 – 28 ms |
| Encoding draws (CPU), range | 3.3 – 4.9 ms | 2.6 – 4.6 ms |

The GPU part of the frame is cut by more than half, and the frame interval drops by about
40 %. The CPU numbers stay where they were, as expected for a change that only touches the
shader. In both builds the page stops drawing once nothing moves.

The screenshots taken after the swipes match pixel for pixel. The only differing pixels are
in the system status bar (rows 20 to 52: the clock and icons).

## How it was tested

- **Screenshots:** the phone screenshots above, compared pixel by pixel.
- **Shadow test:** `a_shadows_straight_middle_matches_the_four_row_sum` (frus-test,
  `shadows.rs`) renders a 200 × 200 shadow with a blur of 12. Across the shadow's edge,
  inside the band, outside it and round the corner, it compares every pixel with the four-row
  sum computed on the CPU. The largest difference is 0.79 of a level, and no two neighbouring
  pixels down the column differ by more than 3, so there is no step where the band ends.
- **Constant test:** the f64 test of the constant described above.
- **Goldens:** all frus-gpu and frus-test targets pass under WSL's software Vulkan with the
  new shader, with the existing goldens left unchanged.

## Not done

- **CPU work is now about half the frame.** Layout and paint take 20 to 30 ms a frame on
  this phone, about half of what is left, and this milestone does not touch them. That is the
  next place to look.
- **The corners still take the four-row path.** Rows near a corner, and near the ends of the
  box, still sum four rows each.
