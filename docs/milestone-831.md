# Milestone 831 — A transformed layer is drawn around what it holds

## Objective

Reported from the test phone (Huawei STK-L21, Mali-G51, Vulkan, Android 10): on the demo's
"Wheel picker" page the wheels "scroll on and on". In the screenshot the minutes wheel had
stopped half a row off, between two values, while the hours wheel sat on a row.

Four causes were possible: a settle animation that never ends (an idle cost too), a looping wheel
settling off a row, a fling on a looping wheel that never slows down, or the two looping wheels
side by side interfering. None of them turned out to be the cause.

## What was found

### In the test harness: nothing

The demo driven through `Driver` (`/wheel`, the minutes wheel): drags of 3 to 40 px a frame,
both directions, released mid-row, released with a flick. Every gesture came to rest on a whole
row (offsets 199 680, 199 840, 199 960, 200 200, 200 320 px, all multiples of 40), and
`animating()` was false within about 1.5 s. Nothing the wheel or the scroll physics does is
wrong.

### On the phone: a quarter of a second a frame

With frame statistics on (`adb shell setprop debug.frus.frames 1`), the wheel page logged:

```
4 frames in 0.5 s (6 fps): interval 238.3/244.6/244.6 ms, ... render 241.2/242.0 ms
(waiting for an image 0.4/0.5, drawing 4.1/6.8) ... 45 layers, up to 45;
next frame asked by runtime 3 ... GPU memory 434.3 MB in 48 textures
```

Each frame took about 240 ms to render, and 48 textures held 434 MB: about 10 MB each, which is
a texture of 1080 × 2340 pixels.

The shell steps animations by the time since the last frame, **capped at 50 ms**. At 240 ms a
frame, a release that settles in about 1.5 s of animation time took about 8 s of wall time
(measured with `dumpsys SurfaceFlinger --latency`: frames every 0.2 to 0.4 s, stopping about 8 s
after a swipe). The wheel was not wrong. It was in slow motion, and a screenshot taken during
those seconds catches it between two rows. The frames do stop: there is no idle cost once the
motion has ended.

### Why every frame cost that much

A wheel draws each row flat and composites it onto the cylinder as **one transformed layer**
(a scale about the row's centre, then a move along the wheel). The compositor gave a layer a
texture of its own size only when it was not transformed (milestones 610 and 796). A transformed
layer got a texture the size of the whole target and a composite quad covering the whole target.

The page shows five wheels with about nine rows each: 45 full-screen multisampled passes and
45 full-screen composite draws a frame, for rows 40 px tall. That is the 434 MB and the quarter
of a second.

## The change

`layer_region` (frus-gpu) now bounds a transformed layer too.

- **Where its texture lies.** The texture holds the layer's content flat, where it was laid out.
  The only part of that flat content that can reach the screen is what the transform sends into
  the clip. So the clip, held to the target, is taken back through the inverse transform (all
  four corners, so a rotation is covered as well as a scale), grown by two pixels so that a
  sample at the clip's edge blends with real content and not the texture's clamped border, and
  then narrowed to what the layer holds (the footprint rule of milestone 796). It is still held
  to the target's own rectangle, as the full-target texture always was, so nothing is drawn that
  was not drawn before.
- **Where its quad goes.** For an untransformed layer the quad is the region, as before. For a
  transformed one it is the box the transform sends the region to, held to the clip and the
  target (`layer_quad`). The composite shader takes it as a new instance attribute,
  `@location(7) quad`: the vertex stage places the quad with it, and the fragment stage still
  finds the texel with the inverse transform and the region. A layer moved wholly off the
  target gets a quad of no size and draws nothing.
- **What keeps the whole target.** A transformed layer coloured by an application's shader or
  GPU pass: their own pipelines read the region as the box they paint. Anything that already
  kept it (an image filter, a backdrop, a path clip, or such a layer inside) still does, and so
  does a transformed layer inside another layer (`boundable` is unchanged).

This changes every transformed layer, not only the wheel's. The others: `Transform`,
`RotatedBox`, `FittedBox`'s scale, the page transitions, an overlay's scaled entrance, a hero
in flight, Android's overscroll stretch. Each is drawn from the same pixels as before, from a
smaller texture. A page transition's sliding page, for instance, now gets a texture of only the
part of it that is on screen.

## How it was tested

- `compositor::tests::a_transformed_layer_is_drawn_around_what_it_holds`: a wheel-like row
  (scale 0.9 × 0.7 about its centre, moved up 30 px) gets a texture of the row plus its margin,
  332 × 132, not the target, and a quad where the transform puts it; a clip that cuts the row
  keeps only what reaches the clip; a quarter turn still finds the row and turns its quad on its
  side; a layer moved off the target draws nothing. **It fails without the change** (checked by
  putting `transform.is_some()` back in the full-target condition: the test panics on "a
  region, not the whole target").
- `offscreen::tests::a_transformed_layer_lands_where_it_is_sent`: rendered and read back on the
  GPU. A square stretched and moved lands where it should, nothing is left where it was laid
  out, and a screen clip cuts it at the right row.
- The existing full-target test now asks it of a shader layer under a transform. All 93
  frus-gpu tests pass.
- frus-gpu (93), frus-widgets (2570, plus doctests) and frus-demo (98) tests pass; `cargo
  clippy --workspace --all-targets -D warnings` is clean.
- The goldens (frus-test, under WSL, fresh build): see "Measurements".
- On the phone: see "Measurements".

## Measurements

On the test phone, wheel page, release builds of the demo before and after the change. Each
row is a slow drag (200 px in 600 ms) and two flicks on the minutes wheel. Frame intervals come
from `dumpsys SurfaceFlinger --latency`, memory from frus's frame statistics and `dumpsys
meminfo`, and "frames stop" is when SurfaceFlinger last received a frame.

| | before | after |
|---|---|---|
| frame interval, median, while the wheel moves | 300 ms | 20 ms |
| render, median (frame statistics) | 241 ms | 14 to 17 ms |
| GPU memory in layer and target textures | 434 MB in 48 textures | 9.4 MB in 48 textures |
| `GL mtrack` (the driver's GPU memory) | 511 MB | 56 MB |
| total PSS | 693 MB | 187 MB |
| frames stop after the swipe (drag, flick, flick) | 9.0, 8.4, 10.1 s | 1.7, 1.8, 1.7 s |

After the change each wheel rests on a row, the picture matches what it was, and no frames are
drawn once the motion has ended.

**Goldens** (frus-test, all targets, under WSL, fresh build): every image matches except
`constraint_boxes`, where 90 pixels differ: 87 in one column, and three by one step in one channel. That picture has a
vertical overflow label: a white plate turned a quarter turn, whose edge lands at x = 22.5,
exactly on the centre of pixel column 22. The composite shader decides whether a pixel is inside
the layer's texture from the pixel position interpolated across the quad. At an exact tie, the
last bit of that interpolation decides, and it changes with the quad's size. With the
full-target quad the column fell outside; with the small quad it falls inside. Bisected: the
smaller texture with the old full quad matches the golden byte for byte, so the texture holds
the same pixels and only the tie moved. The golden was regenerated for that one image.

## Not done

- **Animation time is capped at 50 ms a frame** (`App` frame: `dt.min(0.05)`), so any page
  that renders slower than 20 frames a second plays its animations in slow motion rather than
  skipping ahead. That is what turned a slow page into a wheel that "never stops". The cap
  exists so that a stall (a debugger break, a window dragged) does not make an animation jump.
  Whether it should be raised, or replaced by a cap that only applies to a single long gap, is a
  separate decision. It is left for a follow-up and not changed here.
- The home page also renders slowly on this phone (13 frames a second while its list scrolls,
  render about 52 ms, one or two layers). That is not a transformed layer and is not addressed
  here.
- The composite shader's "inside the texture" test is a hard edge on an interpolated position,
  so a pixel centre exactly on a transformed layer's edge can go either way (see the golden
  above). Reading the pixel centre from `@builtin(position)` would make it independent of the
  quad. That would move every such tie at once, so it is left as its own change.
- The `RotatedBox` comment about painting at the origin "because a layer is rendered into a
  window-sized texture" still holds: content laid out off the target before its transform is
  still not drawn, as before.
