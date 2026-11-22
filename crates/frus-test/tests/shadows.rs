// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Shadows, **rendered** (milestones 606 and 746).
//!
//! A shadow's rectangle reaches its blur beyond the shape it is the shadow of. The renderer
//! used to fade across the rectangle's edge, so the outer half was never drawn and every
//! shadow stopped short, at half its strength, in a hard grey line: the grey box around the
//! text selection bar that a phone showed (606). It then faded inside the rectangle, ending
//! at its edge, which still cut the widest shadows short of where a blur ends (746): now a
//! shadow is its shape blurred by a gaussian, drawn as far out as the gaussian reaches.

use frus_core::{paint_elevation, BorderRadius, BoxShadow, Color, Rect, Scene};
use frus_test::render_scene;

/// The darkest a channel of a white page gets, going out along a row from `from` to `to`.
fn darkest(shot: &frus_test::Snapshot, y: u32, from: u32, to: u32) -> Vec<u8> {
    (from..to).map(|x| shot.pixel(x, y)[0]).collect()
}

/// **A shadow fades out to nothing, with no step anywhere**: going out from the box, the
/// page gets lighter and lighter, past the shadow's rectangle (a blur's gaussian reaches
/// further than the blur), until it is the page's own white three deviations out.
#[test]
fn a_shadow_fades_to_nothing_with_no_step_at_its_edge() {
    let rect = Rect::new(60.0, 40.0, 60.0, 40.0);
    let mut scene = Scene::new();
    scene.fill_rect(Rect::new(0.0, 0.0, 200.0, 140.0), Color::WHITE);
    let shadow = BoxShadow::new(0.0, 0.0, 12.0, Color::rgba(0.0, 0.0, 0.0, 0.8));
    scene.shadow(
        shadow.bounds(rect),
        shadow.color,
        BorderRadius::ZERO.inflate(12.0),
        12.0,
    );
    let Some(shot) = render_scene(&scene, 200, 140, Color::WHITE) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    // From the box's right edge out past where the shadow's rectangle ends (at 132), and
    // past where its gaussian (deviation 7.4) does, about 22 pixels out.
    let row = darkest(&shot, 60, 120, 150);
    for pair in row.windows(2) {
        assert!(pair[1] >= pair[0], "lighter and lighter going out: {row:?}");
    }
    let last_inside = row[(131 - 120) as usize];
    assert!(
        (240..255).contains(&last_inside),
        "faint, and still fading, at the rectangle's edge: {row:?}"
    );
    assert_eq!(row[29], 255, "nothing left three deviations out: {row:?}");
    // And at the box's own edge, the shadow is at about half its strength: 0.8 of black
    // halved is 0.4 over white, blended in linear light, so 0.6 of white there — about 203
    // once written back in sRGB.
    let at_edge = row[0];
    assert!(
        (195..=215).contains(&at_edge),
        "half strength at the box's edge: {row:?}"
    );
}

/// **A surface one high casts a faint, tight shadow**: under a 60×40 box on white, the
/// darkest pixel just below it is light grey, and ten pixels further out there is nothing.
#[test]
fn a_surface_one_high_casts_a_faint_tight_shadow() {
    let rect = Rect::new(60.0, 40.0, 60.0, 40.0);
    let mut scene = Scene::new();
    scene.fill_rect(Rect::new(0.0, 0.0, 200.0, 140.0), Color::WHITE);
    paint_elevation(
        &mut scene,
        rect,
        BorderRadius::uniform(8.0),
        1.0,
        Color::BLACK,
    );
    scene.fill_rect(rect, Color::WHITE);
    let Some(shot) = render_scene(&scene, 200, 140, Color::WHITE) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    let under = shot.pixel(90, 81)[0];
    assert!(
        (150..250).contains(&under),
        "a faint shadow just under: {under}"
    );
    let further = shot.pixel(90, 92)[0];
    assert!(further >= 252, "and nothing ten pixels on: {further}");
}

/// **A surface twenty-four high casts a shadow that fades into the page with no rim**
/// (milestone 746). A shadow is its shape blurred by a gaussian, whose deviation is a
/// little over half the blur, and a gaussian goes on three deviations out. The renderer
/// used to stop each shadow at one blur beyond its shape, fading it there along a curve of
/// its own: the widest shadows, a surface's highest, ended on a rim around the surface, at
/// a distance where the real fade still had a third of its way to go.
///
/// The shadow is drawn in white on a black page, where blending in linear light makes the
/// faintest of tails visible: a gaussian's last tenth of a percent shows; nothing past
/// the end of what is drawn does.
#[test]
fn a_surface_twenty_four_high_fades_into_the_page_with_no_rim() {
    let rect = Rect::new(100.0, 100.0, 96.0, 64.0);
    let mut scene = Scene::new();
    paint_elevation(
        &mut scene,
        rect,
        BorderRadius::uniform(12.0),
        24.0,
        Color::WHITE,
    );
    let Some(shot) = render_scene(&scene, 400, 300, Color::BLACK) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    // Out from the surface's right edge, through its middle, to the page's.
    let row: Vec<u8> = (196..400).map(|x| shot.pixel(x, 132)[0]).collect();
    for pair in row.windows(2) {
        assert!(pair[1] <= pair[0], "darker and darker going out: {row:?}");
    }
    // The widest of the three shadows (blur 46, spread 8) used to end 54 pixels out. Its
    // gaussian, of deviation 27, is still there 66 pixels out, and gone by 100.
    assert!(row[66] > 0, "the fade goes on past one blur: {row:?}");
    assert_eq!(row[100], 0, "and ends: {row:?}");
    // No step anywhere in the fade: neighbours differ by a few levels at most, and the
    // last lit pixel is barely lit.
    let steepest = row
        .windows(2)
        .map(|pair| pair[0] - pair[1])
        .max()
        .unwrap_or(0);
    assert!(steepest <= 4, "no step in the fade ({steepest}): {row:?}");
    let last_lit = row.iter().rposition(|&v| v > 0).expect("a shadow");
    assert!(row[last_lit] <= 2, "it fades out, not off: {row:?}");
}

/// The error function as the renderer computes it (Abramowitz and Stegun 7.1.27).
fn erf(x: f32) -> f32 {
    let a = x.abs();
    let mut t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    t *= t;
    x.signum() * (1.0 - 1.0 / (t * t))
}

/// One row of a rounded box blurred along x, as the renderer computes it.
fn shadow_row(x: f32, y: f32, sigma: f32, corner: f32, half: (f32, f32)) -> f32 {
    let delta = (half.1 - corner - y.abs()).min(0.0);
    let curved = half.0 - corner + (corner * corner - delta * delta).max(0.0).sqrt();
    let k = std::f32::consts::FRAC_1_SQRT_2 / sigma;
    0.5 * (erf((x + curved) * k) - erf((x - curved) * k))
}

/// The renderer's blurred box summed over its four rows, everywhere: what every pixel was
/// before the straight band took one row for four.
fn four_rows(p: (f32, f32), half: (f32, f32), corner: f32, sigma: f32) -> f32 {
    let start = (-3.0 * sigma).clamp(p.1 - half.1, p.1 + half.1);
    let end = (3.0 * sigma).clamp(p.1 - half.1, p.1 + half.1);
    let step = (end - start) / 4.0;
    let mut y = start + step * 0.5;
    let mut value = 0.0;
    for _ in 0..4 {
        let g = (-(y * y) / (2.0 * sigma * sigma)).exp() / (2.506_628_3 * sigma);
        value += shadow_row(p.0, p.1 - y, sigma, corner, half) * g * step;
        y += step;
    }
    value
}

fn to_srgb_level(linear: f32) -> f32 {
    let c = linear.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    s * 255.0
}

/// **A large shadow's straight middle is the same with one row as with four** (milestone
/// 835): between the corners every row of the column is the same row, and the renderer
/// takes one for four. Across the shadow's edge, at rows inside that band, just outside it
/// and round the corner, the rendered shadow matches the four-row sum to within a level,
/// with no step where the band ends.
#[test]
fn a_shadows_straight_middle_matches_the_four_row_sum() {
    // A 200×200 shape with corners of 8, blurred by 12: its rect reaches 12 further out.
    let shape = Rect::new(60.0, 40.0, 200.0, 200.0);
    let blur = 12.0;
    let shadow = BoxShadow::new(0.0, 0.0, blur, Color::WHITE);
    let bounds = shadow.bounds(shape);
    let mut scene = Scene::new();
    scene.shadow(
        bounds,
        Color::WHITE,
        BorderRadius::uniform(8.0).inflate(blur),
        blur,
    );
    let Some(shot) = render_scene(&scene, 320, 280, Color::BLACK) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    let sigma = blur * 0.57735 + 0.5;
    let half = (100.0, 100.0);
    let corner = 8.0;
    let centre = (160.0, 140.0);
    // The band: |y| + 3σ ≤ 100 − 8, so up to about 69.7 pixels from the centre.
    let band = half.1 - corner - 3.0 * sigma;
    let mut worst = 0.0f32;
    let mut inside = 0;
    let mut outside = 0;
    for y in (140..240).step_by(3) {
        let py = y as f32 + 0.5 - centre.1;
        if py.abs() <= band {
            inside += 1;
        } else {
            outside += 1;
        }
        // Out across the right edge, from well inside the shape to past the fade.
        for x in 240..282 {
            let px = x as f32 + 0.5 - centre.0;
            let expected = to_srgb_level(four_rows((px, py), half, corner, sigma));
            let got = shot.pixel(x, y)[0] as f32;
            worst = worst.max((got - expected).abs());
            assert!(
                (got - expected).abs() <= 1.5,
                "at ({x}, {y}), {py} from the centre (band ends at {band}): {got} rendered, {expected} summed over four rows"
            );
        }
    }
    assert!(
        inside > 5 && outside > 5,
        "rows on both sides of the band's end"
    );
    eprintln!("largest difference from the four-row sum: {worst:.2} levels");
    // No step where the band ends: down the column at the shape's edge, neighbours differ
    // by a few levels at most.
    let column: Vec<u8> = (140..215).map(|y| shot.pixel(260, y)[0]).collect();
    let steepest = column
        .windows(2)
        .map(|pair| (pair[0] as i32 - pair[1] as i32).abs())
        .max()
        .unwrap_or(0);
    assert!(
        steepest <= 3,
        "no step down the column ({steepest}): {column:?}"
    );
}
