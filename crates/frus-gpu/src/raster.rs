// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **At what scale a transformed layer is drawn**, and whether it gets a mip chain
//! (milestone 850).
//!
//! A transformed layer is drawn flat into a texture, then composited through its transform.
//! Two things went wrong with a texture drawn at the layer's own size:
//!
//! - **Shrunk**, each screen pixel covered several texels and a bilinear sample read four of
//!   them: below one half, texels were skipped, and fine text and patterns shimmered as the
//!   scale moved. A shrunk layer now gets a **mip chain**, made on the GPU when its texture is
//!   drawn, and is sampled trilinearly.
//! - **Enlarged**, the texture was magnified, and text in it blurred. An enlarged layer is
//!   now drawn **at the scale it is shown at**: its content is scaled before it is drawn, so
//!   text is laid out and rasterised at its final size.
//!
//! An enlargement that is animating would be drawn again every frame if the texture followed
//! the scale exactly. A layer kept from the frame before is drawn again only when the scale
//! has grown past what its texture holds (with some headroom, so a growing scale is not drawn
//! again every frame), has fallen well below it, or has stopped moving at another scale. A
//! layer whose content changed, or whose region moved, is drawn again anyway, but its scale
//! follows the same rule while its transform moves: a page zooming across the whole screen is
//! drawn every frame, and its text at a handful of sizes rather than at a new size each frame,
//! which would rasterise every glyph again each time.
//!
//! The arithmetic is here, as plain functions tested on the CPU.

/// The most a layer is drawn enlarged by. The memory limit ([`scale_limit`]) usually binds
/// first.
pub(crate) const MAX_SCALE: f32 = 8.0;

/// How far past the scale shown a growing layer is drawn, so that a scale animating up is
/// not drawn again every frame: the texture is drawn again each time the scale grows by a
/// quarter, and shown at most a quarter smaller than it was drawn.
pub(crate) const HEADROOM: f32 = 1.25;

/// How far below its drawn scale a layer may be shown before it is drawn again at the
/// smaller one: below that, the texture would hold more than twice the texels it shows.
pub(crate) const SHRINK: f32 = std::f32::consts::SQRT_2;

/// How the linear part of `m` (an [`Affine`](frus_core::Affine)'s `[a, b, c, d, e, f]`)
/// stretches the plane: its largest and smallest singular values. A rotation stretches by
/// one both ways; a scale by its factors, whatever the order of rotation and scale.
pub(crate) fn stretch(m: [f32; 6]) -> (f32, f32) {
    let [a, b, c, d, ..] = m;
    let sum = a * a + b * b + c * c + d * d;
    let det = a * d - b * c;
    let root = (sum * sum - 4.0 * det * det).max(0.0).sqrt();
    let most = ((sum + root) * 0.5).max(0.0).sqrt();
    let least = ((sum - root) * 0.5).max(0.0).sqrt();
    (most, least)
}

/// How many texels an enlarged layer's texture may hold, as a share of the pixels of the target
/// it is composited onto. A little over one: the region a layer is drawn in reaches a couple of
/// pixels past what is shown, for the sampler, and a layer filling the screen at four times its
/// size would otherwise be held just under four.
pub(crate) const MEMORY_SHARE: f32 = 1.5;

/// The most a `rw`×`rh` layer can be enlarged by on a `w`×`h` target whose textures are at
/// most `max_dim` texels along a side: never more texels than [`MEMORY_SHARE`] of the target's
/// pixels, never past [`MAX_SCALE`], and never less than one.
pub(crate) fn scale_limit(rw: u32, rh: u32, w: u32, h: u32, max_dim: u32) -> f32 {
    // A texel more each way: the size is rounded up to whole texels, at most one more.
    let region = (rw.max(1) as f32 + 1.0) * (rh.max(1) as f32 + 1.0);
    let target = (w.max(1) as f32) * (h.max(1) as f32) * MEMORY_SHARE;
    let memory = (target / region).sqrt();
    let side = max_dim as f32 / rw.max(rh).max(1) as f32;
    memory.min(side).clamp(1.0, MAX_SCALE)
}

/// What is known of the texture a layer had the frame before, whether or not its content changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Kept {
    /// The scale it was drawn at.
    pub(crate) scale: f32,
    /// Whether its transform is the one it had the frame before: the scale has settled.
    pub(crate) settled: bool,
}

/// The scale to draw a layer at: `most`, its largest stretch, held between one and `limit`,
/// and changed from the `kept` texture's scale only when that one no longer serves (see the
/// module notes). One when the layer is not enlarged.
pub(crate) fn chosen_scale(most: f32, limit: f32, kept: Option<Kept>) -> f32 {
    // A thousandth of slack: a scale of one that rounding moved a hair is one.
    const SLACK: f32 = 1e-3;
    let wanted = most.min(limit).max(1.0);
    let wanted = if wanted < 1.0 + SLACK { 1.0 } else { wanted };
    let Some(kept) = kept else {
        return wanted;
    };
    // Held to the limit: the region it was drawn for may have been smaller.
    let k = kept.scale.min(limit.max(1.0));
    if wanted > k * (1.0 + SLACK) {
        // Grown past it: drawn again, ahead of the scale.
        let ahead = (wanted * HEADROOM).min(limit);
        return if ahead < 1.0 + SLACK { 1.0 } else { ahead };
    }
    if k > wanted * SHRINK {
        return wanted;
    }
    if kept.settled && (k - wanted).abs() > wanted * SLACK {
        // At rest at another scale: drawn once more, at the exact one.
        return wanted;
    }
    k
}

/// Whether a layer drawn at `scale` and shown with `least` as its smallest stretch needs a
/// mip chain: whether a screen pixel covers more than one texel along some axis.
///
/// A layer drawn at its own size gets one as soon as it is shown smaller, as an image does:
/// between one and one half the sampler already blends in the second level, and a chain made
/// only later would change the picture the frame it appeared. A layer drawn enlarged is shown
/// at most [`SHRINK`] smaller than it was drawn and reads well enough from its first level;
/// it gets a chain only past one half, which only a stretch far from uniform reaches.
pub(crate) fn needs_chain(least: f32, scale: f32) -> bool {
    if least <= 0.0 {
        return false;
    }
    let texels_per_pixel = scale / least;
    if scale > 1.0 {
        texels_per_pixel > 2.0
    } else {
        texels_per_pixel > 1.0 + 1e-3
    }
}

/// The size of a `rw`×`rh` layer's texture drawn at `scale`: whole texels, at least one,
/// at most `max_dim`.
pub(crate) fn scaled_size(rw: u32, rh: u32, scale: f32, max_dim: u32) -> (u32, u32) {
    let side = |v: u32| {
        // A thousandth of slack, so that 100 × 1.5 is 150 and not 151.
        ((v as f32 * scale - 1e-3).ceil() as u32).clamp(1, max_dim.max(1))
    };
    if scale == 1.0 {
        return (rw.max(1), rh.max(1));
    }
    (side(rw), side(rh))
}

/// `inverse` (screen → flat, `[a, b, c, d, e, f]`) followed by an enlargement by `scale`:
/// screen → the texels of a texture drawn `scale` times larger.
pub(crate) fn scaled_inverse(inverse: [f32; 6], scale: f32) -> [f32; 6] {
    inverse.map(|v| v * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_core::{Affine, Point};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn stretch_is_the_scale_whatever_the_rotation() {
        let (most, least) = stretch(Affine::IDENTITY.m);
        assert!(close(most, 1.0) && close(least, 1.0));
        let (most, least) = stretch(Affine::rotation(0.7).m);
        assert!(close(most, 1.0) && close(least, 1.0), "{most} {least}");
        let (most, least) = stretch(Affine::scale(3.0, 0.5).m);
        assert!(close(most, 3.0) && close(least, 0.5));
        // Rotated, then scaled, then moved: the same stretch.
        let m = Affine::rotation(1.1)
            .then(Affine::scale(0.25, 2.0))
            .about(Point::new(40.0, 7.0));
        let (most, least) = stretch(m.m);
        assert!(close(most, 2.0) && close(least, 0.25), "{most} {least}");
        // A mirror stretches as much as the scale it mirrors.
        let (most, least) = stretch(Affine::scale(-2.0, 2.0).m);
        assert!(close(most, 2.0) && close(least, 2.0));
        // Degenerate: flattened to a line.
        let (most, least) = stretch(Affine::scale(2.0, 0.0).m);
        assert!(close(most, 2.0) && close(least, 0.0));
    }

    #[test]
    fn an_enlarged_layer_holds_at_most_a_share_of_the_target_s_pixels() {
        // A 100×50 layer on a 1000×1000 target: held to the maximum.
        assert_eq!(scale_limit(100, 50, 1000, 1000, 8192), MAX_SCALE);
        // A layer half the target's area: √3, less a hair for the rounding.
        let k = scale_limit(500, 1000, 1000, 1000, 8192);
        assert!((k - 3f32.sqrt()).abs() < 4e-3, "{k}");
        let (tw, th) = scaled_size(500, 1000, k, 8192);
        assert!(u64::from(tw) * u64::from(th) <= 1_500_000, "{tw}x{th}");
        // Larger than the target (a rotated layer's box): one, never below.
        assert_eq!(scale_limit(2000, 2000, 1000, 1000, 8192), 1.0);
        // The texture's side binds: 1000 texels wide, at most 2048.
        let k = scale_limit(1000, 10, 10_000, 10_000, 2048);
        assert!(close(k, 2.048), "{k}");
        let (tw, _) = scaled_size(1000, 10, k, 2048);
        assert!(tw <= 2048);
    }

    #[test]
    fn a_layer_with_nothing_kept_is_drawn_at_its_exact_scale() {
        assert_eq!(chosen_scale(1.0, 8.0, None), 1.0);
        assert_eq!(chosen_scale(0.3, 8.0, None), 1.0, "shrunk: its own size");
        assert_eq!(
            chosen_scale(1.0004, 8.0, None),
            1.0,
            "a hair over one is one"
        );
        assert!(close(chosen_scale(2.5, 8.0, None), 2.5));
        assert!(
            close(chosen_scale(20.0, 3.0, None), 3.0),
            "held to the limit"
        );
    }

    /// A scale animating from one to three, the content unchanged: drawn again a handful of
    /// times, never every frame, and never shown magnified by more than a hair.
    #[test]
    fn a_growing_scale_is_drawn_again_a_handful_of_times() {
        let mut kept = Kept {
            scale: 1.0,
            settled: false,
        };
        let mut drawn = 0;
        let frames = 60;
        for frame in 1..=frames {
            let s = 1.0 + 2.0 * frame as f32 / frames as f32;
            let k = chosen_scale(s, 8.0, Some(kept));
            if k != kept.scale {
                drawn += 1;
            }
            assert!(
                k >= s * (1.0 - 1e-3),
                "frame {frame}: drawn at {k}, shown at {s}"
            );
            assert!(k <= s * SHRINK, "frame {frame}: drawn at {k}, shown at {s}");
            kept.scale = k;
        }
        assert!(drawn <= 6, "drawn again {drawn} times");
        // At rest at three: drawn once more, at three exactly, then kept.
        kept.settled = true;
        let k = chosen_scale(3.0, 8.0, Some(kept));
        assert!(close(k, 3.0), "{k}");
        kept.scale = k;
        assert_eq!(chosen_scale(3.0, 8.0, Some(kept)), k);
    }

    #[test]
    fn a_shrinking_scale_is_drawn_again_only_well_below_its_texture() {
        let kept = |scale| {
            Some(Kept {
                scale,
                settled: false,
            })
        };
        // Drawn at three, shown at 2.5: kept.
        assert_eq!(chosen_scale(2.5, 8.0, kept(3.0)), 3.0);
        // Shown at two: more than √2 smaller, drawn again at two.
        assert!(close(chosen_scale(2.0, 8.0, kept(3.0)), 2.0));
        // Back at its own size or smaller: drawn again at one.
        assert_eq!(chosen_scale(0.5, 8.0, kept(3.0)), 1.0);
        // Never drawn ahead past the limit.
        assert!(close(chosen_scale(2.9, 3.0, kept(2.0)), 3.0));
        // A layer at its own size, still: kept.
        let still = Some(Kept {
            scale: 1.0,
            settled: true,
        });
        assert_eq!(chosen_scale(1.0, 8.0, still), 1.0);
        assert_eq!(chosen_scale(0.5, 8.0, still), 1.0);
    }

    #[test]
    fn only_a_layer_shown_smaller_than_its_texels_needs_a_chain() {
        assert!(!needs_chain(1.0, 1.0), "its own size");
        assert!(!needs_chain(0.9995, 1.0), "a hair small, from rounding");
        assert!(needs_chain(0.9, 1.0));
        assert!(needs_chain(0.125, 1.0));
        assert!(!needs_chain(2.0, 1.0), "enlarged");
        // Drawn ahead of a growing scale, shown up to √2 smaller: its first level serves.
        assert!(!needs_chain(2.0, 2.5));
        assert!(!needs_chain(2.0, 2.0 * SHRINK));
        // Stretched far from uniform: three wide, a quarter high.
        assert!(needs_chain(0.25, 3.0));
        assert!(!needs_chain(0.0, 1.0), "flattened: nothing is shown");
    }

    #[test]
    fn a_scaled_texture_covers_the_scaled_region() {
        assert_eq!(scaled_size(100, 40, 1.0, 4096), (100, 40));
        assert_eq!(scaled_size(100, 40, 1.5, 4096), (150, 60));
        assert_eq!(scaled_size(3, 3, 2.4, 4096), (8, 8));
        assert_eq!(scaled_size(3000, 3, 2.0, 4096), (4096, 6));
        let inv = Affine::scale(0.5, 0.5).inverse().m;
        let scaled = scaled_inverse(inv, 2.0);
        // A screen point maps to twice its flat position in the texture's texels.
        let flat = Affine { m: inv }.apply(Point::new(10.0, 4.0));
        let texel = Affine { m: scaled }.apply(Point::new(10.0, 4.0));
        assert!(close(texel.x, flat.x * 2.0) && close(texel.y, flat.y * 2.0));
    }
}
