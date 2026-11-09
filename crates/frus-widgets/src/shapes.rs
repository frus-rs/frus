// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Rounded polygons and the morphs between them**: [`RoundedPolygon`], the Material 3
//! shapes built from it ([`MaterialShapes`]), [`Morph`] from one to another, and
//! [`MorphingShape`], the widget that paints one.
//!
//! A polygon is its vertices, each with a [`CornerRounding`]: the radius of the circle the
//! corner is rounded to, in the polygon's own units (a regular polygon or a star has radius
//! one). A rounding too large for the edges either side is reduced to what fits, so a
//! "cookie" is simply a star whose corners are rounded as far as they will go.
//!
//! A morph does not need the two shapes to have the same number of corners: both outlines
//! are measured, resampled at the same number of points evenly spaced along their length,
//! and the second turned so that its points sit as close as they can to the first's. In
//! between, every point travels in a straight line to its partner.

use std::f32::consts::{PI, TAU};

use frus_core::{Color, Path, Point, Rect, Scene};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// How round a corner is: the radius of its arc, in the polygon's units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRounding {
    /// The arc's radius; zero is a sharp corner.
    pub radius: f32,
}

impl CornerRounding {
    /// A sharp corner.
    pub const UNROUNDED: CornerRounding = CornerRounding { radius: 0.0 };

    /// A corner rounded to `radius`.
    ///
    /// ```
    /// use frus_widgets::CornerRounding;
    /// assert_eq!(CornerRounding::new(0.2).radius, 0.2);
    /// ```
    pub const fn new(radius: f32) -> Self {
        Self { radius }
    }
}

/// A closed outline of cubic Bézier segments: `(start, control, control, end)`, each
/// starting where the last ended.
#[derive(Clone, Debug, PartialEq)]
pub struct RoundedPolygon {
    cubics: Vec<[Point; 4]>,
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}
fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}
fn mul(a: Point, k: f32) -> Point {
    Point::new(a.x * k, a.y * k)
}
fn len(a: Point) -> f32 {
    (a.x * a.x + a.y * a.y).sqrt()
}
fn lerp(a: Point, b: Point, t: f32) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}
fn line(a: Point, b: Point) -> [Point; 4] {
    [a, lerp(a, b, 1.0 / 3.0), lerp(a, b, 2.0 / 3.0), b]
}

impl RoundedPolygon {
    /// A polygon through `vertices`, in order, each corner rounded by its entry in
    /// `rounding` (the last one repeats for any vertex past the end of the list).
    ///
    /// ```
    /// use frus_widgets::{CornerRounding, RoundedPolygon};
    /// use frus_core::Point;
    /// let square = RoundedPolygon::from_vertices(
    ///     &[Point::new(-1.0, -1.0), Point::new(1.0, -1.0), Point::new(1.0, 1.0), Point::new(-1.0, 1.0)],
    ///     &[CornerRounding::new(0.3)],
    /// );
    /// assert_eq!(square.cubics().len(), 8); // four arcs and four edges
    /// ```
    pub fn from_vertices(vertices: &[Point], rounding: &[CornerRounding]) -> Self {
        let n = vertices.len();
        if n < 3 {
            return Self { cubics: Vec::new() };
        }
        let r_of = |i: usize| {
            rounding
                .get(i)
                .or(rounding.last())
                .copied()
                .unwrap_or_default()
                .radius
                .max(0.0)
        };
        // For each corner: where the arc leaves the incoming edge, where it joins the
        // outgoing one, and its two control points.
        let mut corners: Vec<[Point; 4]> = Vec::with_capacity(n);
        for i in 0..n {
            let v = vertices[i];
            let prev = vertices[(i + n - 1) % n];
            let next = vertices[(i + 1) % n];
            let (a, b) = (sub(prev, v), sub(next, v));
            let (la, lb) = (len(a), len(b));
            if la <= 1e-6 || lb <= 1e-6 {
                corners.push([v, v, v, v]);
                continue;
            }
            let (ua, ub) = (mul(a, 1.0 / la), mul(b, 1.0 / lb));
            let cos = (ua.x * ub.x + ua.y * ub.y).clamp(-1.0, 1.0);
            let interior = cos.acos();
            let r = r_of(i);
            // The distance from the vertex to where the arc meets each edge, at most half
            // of either edge so that two neighbouring corners never overlap.
            let half_tan = (interior * 0.5).tan().max(1e-4);
            let cut = (r / half_tan).min(la * 0.5).min(lb * 0.5);
            if cut <= 1e-6 {
                corners.push([v, v, v, v]);
                continue;
            }
            let radius = cut * half_tan;
            let turn = PI - interior;
            let k = 4.0 / 3.0 * (turn / 4.0).tan() * radius;
            let p0 = add(v, mul(ua, cut));
            let p3 = add(v, mul(ub, cut));
            let c1 = sub(p0, mul(ua, k));
            let c2 = sub(p3, mul(ub, k));
            corners.push([p0, c1, c2, p3]);
        }
        let mut cubics = Vec::with_capacity(2 * n);
        for i in 0..n {
            let c = corners[i];
            if len(sub(c[0], c[3])) > 1e-6 {
                cubics.push(c);
            }
            let next = corners[(i + 1) % n];
            if len(sub(c[3], next[0])) > 1e-5 {
                cubics.push(line(c[3], next[0]));
            }
        }
        let mut poly = Self { cubics };
        poly.make_clockwise();
        poly
    }

    /// A regular polygon of `sides` corners on a circle of radius one, the first corner
    /// at `start` radians (0 points right, the angles turning clockwise on screen).
    ///
    /// ```
    /// use frus_widgets::{CornerRounding, RoundedPolygon};
    /// let pentagon = RoundedPolygon::regular(5, CornerRounding::new(0.2), -std::f32::consts::FRAC_PI_2);
    /// assert!(!pentagon.cubics().is_empty());
    /// ```
    pub fn regular(sides: usize, rounding: CornerRounding, start: f32) -> Self {
        let sides = sides.max(3);
        let vertices: Vec<Point> = (0..sides)
            .map(|i| {
                let a = start + TAU * i as f32 / sides as f32;
                Point::new(a.cos(), a.sin())
            })
            .collect();
        Self::from_vertices(&vertices, &[rounding])
    }

    /// A star of `points` tips on a circle of radius one, its valleys at `inner` (as a
    /// fraction of that), tips rounded by `rounding` and valleys by `inner_rounding`.
    ///
    /// ```
    /// use frus_widgets::{CornerRounding, RoundedPolygon};
    /// let sunny = RoundedPolygon::star(8, 0.8, CornerRounding::new(0.15), CornerRounding::new(0.15), 0.0);
    /// assert!(sunny.cubics().len() >= 16);
    /// ```
    pub fn star(
        points: usize,
        inner: f32,
        rounding: CornerRounding,
        inner_rounding: CornerRounding,
        start: f32,
    ) -> Self {
        let points = points.max(3);
        let mut vertices = Vec::with_capacity(points * 2);
        let mut round = Vec::with_capacity(points * 2);
        for i in 0..points * 2 {
            let a = start + PI * i as f32 / points as f32;
            let r = if i % 2 == 0 { 1.0 } else { inner };
            vertices.push(Point::new(a.cos() * r, a.sin() * r));
            round.push(if i % 2 == 0 { rounding } else { inner_rounding });
        }
        Self::from_vertices(&vertices, &round)
    }

    /// A circle of radius one, as four cubic arcs.
    pub fn circle() -> Self {
        let k = 0.552_284_8;
        let p = |x: f32, y: f32| Point::new(x, y);
        Self {
            cubics: vec![
                [p(1.0, 0.0), p(1.0, k), p(k, 1.0), p(0.0, 1.0)],
                [p(0.0, 1.0), p(-k, 1.0), p(-1.0, k), p(-1.0, 0.0)],
                [p(-1.0, 0.0), p(-1.0, -k), p(-k, -1.0), p(0.0, -1.0)],
                [p(0.0, -1.0), p(k, -1.0), p(1.0, -k), p(1.0, 0.0)],
            ],
        }
    }

    /// The outline's cubic segments.
    pub fn cubics(&self) -> &[[Point; 4]] {
        &self.cubics
    }

    /// The same outline transformed point by point.
    pub fn map(&self, f: impl Fn(Point) -> Point) -> Self {
        let mut poly = Self {
            cubics: self
                .cubics
                .iter()
                .map(|c| [f(c[0]), f(c[1]), f(c[2]), f(c[3])])
                .collect(),
        };
        poly.make_clockwise();
        poly
    }

    /// Scaled by `sx`, `sy` about the origin.
    pub fn scaled(&self, sx: f32, sy: f32) -> Self {
        self.map(|p| Point::new(p.x * sx, p.y * sy))
    }

    /// Turned `radians` about the origin.
    pub fn rotated(&self, radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        self.map(|p| Point::new(p.x * c - p.y * s, p.x * s + p.y * c))
    }

    /// The smallest box around the outline (measured on the curve, not its control points).
    pub fn bounds(&self) -> Rect {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let pts = self
            .cubics
            .iter()
            .flat_map(|c| (0..=32).map(move |s| bezier(c, s as f32 / 32.0)));
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if x0 > x1 {
            return Rect::new(0.0, 0.0, 0.0, 0.0);
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// The outline as `count` points evenly spaced along it, clockwise on screen.
    pub fn sample(&self, count: usize) -> Vec<Point> {
        let mut poly: Vec<Point> = Vec::new();
        for c in &self.cubics {
            for s in 0..16 {
                poly.push(bezier(c, s as f32 / 16.0));
            }
        }
        resample(&poly, count)
    }

    fn make_clockwise(&mut self) {
        // Screen y points down, so clockwise on screen is a positive shoelace sum.
        let pts: Vec<Point> = self.cubics.iter().map(|c| c[0]).collect();
        if signed_area(&pts) < 0.0 {
            self.cubics = self
                .cubics
                .iter()
                .rev()
                .map(|c| [c[3], c[2], c[1], c[0]])
                .collect();
        }
    }

    /// The outline as a path, fitted into `rect` (keeping its proportions, centred).
    pub fn to_path(&self, rect: Rect) -> Path {
        let f = fit(self.bounds(), rect);
        let mut path = Path::new();
        for (i, c) in self.cubics.iter().enumerate() {
            if i == 0 {
                path = path.move_to(f(c[0]));
            }
            path = path.cubic_to(f(c[1]), f(c[2]), f(c[3]));
        }
        path.close()
    }
}

/// The map from a shape's own box into `rect`: uniform scale, centred.
fn fit(from: Rect, rect: Rect) -> impl Fn(Point) -> Point {
    let s = if from.width > 0.0 && from.height > 0.0 {
        (rect.width / from.width).min(rect.height / from.height)
    } else {
        1.0
    };
    let (cx, cy) = (from.x + from.width * 0.5, from.y + from.height * 0.5);
    let (tx, ty) = (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    move |p: Point| Point::new(tx + (p.x - cx) * s, ty + (p.y - cy) * s)
}

fn bezier(c: &[Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    let (a, b, cc, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point::new(
        a * c[0].x + b * c[1].x + cc * c[2].x + d * c[3].x,
        a * c[0].y + b * c[1].y + cc * c[2].y + d * c[3].y,
    )
}

fn signed_area(pts: &[Point]) -> f32 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        * 0.5
}

/// `count` points evenly spaced along the closed polyline `poly`.
fn resample(poly: &[Point], count: usize) -> Vec<Point> {
    let n = poly.len();
    if n == 0 || count == 0 {
        return Vec::new();
    }
    let mut cum = Vec::with_capacity(n + 1);
    cum.push(0.0f32);
    for i in 0..n {
        let d = len(sub(poly[(i + 1) % n], poly[i]));
        cum.push(cum[i] + d);
    }
    let total = cum[n];
    if total <= 0.0 {
        return vec![poly[0]; count];
    }
    let mut out = Vec::with_capacity(count);
    let mut seg = 0;
    for k in 0..count {
        let target = total * k as f32 / count as f32;
        while seg + 1 < n && cum[seg + 1] < target {
            seg += 1;
        }
        let span = cum[seg + 1] - cum[seg];
        let t = if span > 0.0 {
            (target - cum[seg]) / span
        } else {
            0.0
        };
        out.push(lerp(poly[seg], poly[(seg + 1) % n], t));
    }
    out
}

/// The points a morph is made of: enough that a 48-pixel shape shows no facets.
pub const MORPH_POINTS: usize = 180;

/// A **morph** from one rounded polygon to another.
///
/// ```
/// use frus_widgets::{MaterialShapes, Morph};
/// let m = Morph::new(&MaterialShapes::circle(), &MaterialShapes::sunny());
/// let halfway = m.points_at(0.5);
/// assert_eq!(halfway.len(), frus_widgets::MORPH_POINTS);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Morph {
    from: Vec<Point>,
    to: Vec<Point>,
}

impl Morph {
    /// The morph from `from` to `to`, both normalised to the same box first, so that
    /// shapes of different sizes morph without moving.
    pub fn new(from: &RoundedPolygon, to: &RoundedPolygon) -> Self {
        let a = normalised(from).sample(MORPH_POINTS);
        let b = normalised(to).sample(MORPH_POINTS);
        let shift = best_shift(&a, &b);
        let n = b.len();
        let to = (0..n).map(|i| b[(i + shift) % n]).collect();
        Self { from: a, to }
    }

    /// The outline at `progress` (0 is `from`, 1 is `to`; a spring's overshoot past either
    /// end carries on along the same straight lines), as points in a box centred on the
    /// origin, of side two.
    pub fn points_at(&self, progress: f32) -> Vec<Point> {
        self.from
            .iter()
            .zip(&self.to)
            .map(|(a, b)| lerp(*a, *b, progress))
            .collect()
    }

    /// The outline at `progress` as a closed path, fitted into `rect`, turned `rotation`
    /// radians about its centre.
    pub fn path_at(&self, progress: f32, rect: Rect, rotation: f32) -> Path {
        let pts = self.points_at(progress);
        let s = rect.width.min(rect.height) * 0.5;
        let (cx, cy) = (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        let (sn, cs) = rotation.sin_cos();
        let place = |p: &Point| {
            Point::new(
                cx + (p.x * cs - p.y * sn) * s,
                cy + (p.x * sn + p.y * cs) * s,
            )
        };
        let mut path = Path::new();
        for (i, p) in pts.iter().enumerate() {
            path = if i == 0 {
                path.move_to(place(p))
            } else {
                path.line_to(place(p))
            };
        }
        path.close()
    }
}

/// `poly` centred on the origin and scaled so its larger side is two.
fn normalised(poly: &RoundedPolygon) -> RoundedPolygon {
    let b = poly.bounds();
    let side = b.width.max(b.height);
    if side <= 0.0 {
        return poly.clone();
    }
    let (cx, cy) = (b.x + b.width * 0.5, b.y + b.height * 0.5);
    let k = 2.0 / side;
    poly.map(|p| Point::new((p.x - cx) * k, (p.y - cy) * k))
}

/// The rotation of `b`'s points that sits closest to `a`'s, by the sum of squared
/// distances.
fn best_shift(a: &[Point], b: &[Point]) -> usize {
    let n = a.len().min(b.len());
    let mut best = (f32::MAX, 0);
    for shift in 0..n {
        let mut d = 0.0;
        for i in 0..n {
            let q = b[(i + shift) % n];
            let (dx, dy) = (a[i].x - q.x, a[i].y - q.y);
            d += dx * dx + dy * dy;
            if d >= best.0 {
                break;
            }
        }
        if d < best.0 {
            best = (d, shift);
        }
    }
    best.1
}

/// The **Material 3 shapes**: the expressive set of rounded polygons, each fitting a box
/// of side two centred on the origin.
pub struct MaterialShapes;

impl MaterialShapes {
    /// A circle.
    pub fn circle() -> RoundedPolygon {
        RoundedPolygon::circle()
    }
    /// A square with rounded corners.
    pub fn square() -> RoundedPolygon {
        RoundedPolygon::regular(4, CornerRounding::new(0.42), PI / 4.0).rotated(0.0)
    }
    /// A square leaning to the right.
    pub fn slanted() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(-0.8, -1.0),
                Point::new(1.0, -1.0),
                Point::new(0.8, 1.0),
                Point::new(-1.0, 1.0),
            ],
            &[CornerRounding::new(0.35)],
        )
    }
    /// An arch: round at the top, square-ish at the bottom.
    pub fn arch() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(-1.0, -1.0),
                Point::new(1.0, -1.0),
                Point::new(1.0, 1.0),
                Point::new(-1.0, 1.0),
            ],
            &[
                CornerRounding::new(1.0),
                CornerRounding::new(1.0),
                CornerRounding::new(0.2),
                CornerRounding::new(0.2),
            ],
        )
    }
    /// A half circle, flat side down.
    pub fn semicircle() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(-1.0, 0.0),
                Point::new(-1.0, -1.0),
                Point::new(1.0, -1.0),
                Point::new(1.0, 0.0),
            ],
            &[
                CornerRounding::new(0.15),
                CornerRounding::new(1.0),
                CornerRounding::new(1.0),
                CornerRounding::new(0.15),
            ],
        )
    }
    /// An oval on the diagonal.
    pub fn oval() -> RoundedPolygon {
        RoundedPolygon::circle()
            .scaled(1.0, 0.64)
            .rotated(-PI / 4.0)
    }
    /// A pill on the diagonal.
    pub fn pill() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(-1.0, -0.55),
                Point::new(1.0, -0.55),
                Point::new(1.0, 0.55),
                Point::new(-1.0, 0.55),
            ],
            &[CornerRounding::new(1.0)],
        )
        .rotated(-PI / 4.0)
    }
    /// A triangle pointing up, its corners rounded.
    pub fn triangle() -> RoundedPolygon {
        RoundedPolygon::regular(3, CornerRounding::new(0.2), -PI / 2.0)
    }
    /// An arrow head pointing up.
    pub fn arrow() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(0.0, -1.0),
                Point::new(1.0, 0.85),
                Point::new(0.0, 0.45),
                Point::new(-1.0, 0.85),
            ],
            &[
                CornerRounding::new(0.3),
                CornerRounding::new(0.25),
                CornerRounding::new(0.1),
                CornerRounding::new(0.25),
            ],
        )
    }
    /// A diamond, taller than it is wide.
    pub fn diamond() -> RoundedPolygon {
        RoundedPolygon::regular(4, CornerRounding::new(0.25), -PI / 2.0).scaled(0.85, 1.0)
    }
    /// A clam shell: a hexagon squashed flat.
    pub fn clam_shell() -> RoundedPolygon {
        RoundedPolygon::regular(6, CornerRounding::new(0.3), 0.0).scaled(1.0, 0.8)
    }
    /// A pentagon, point up.
    pub fn pentagon() -> RoundedPolygon {
        RoundedPolygon::regular(5, CornerRounding::new(0.3), -PI / 2.0)
    }
    /// A gem: an irregular hexagon.
    pub fn gem() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(-0.5, -1.0),
                Point::new(0.5, -1.0),
                Point::new(1.0, -0.2),
                Point::new(0.0, 1.0),
                Point::new(-1.0, -0.2),
            ],
            &[CornerRounding::new(0.3)],
        )
    }
    /// A sun of eight shallow rays.
    pub fn sunny() -> RoundedPolygon {
        RoundedPolygon::star(
            8,
            0.8,
            CornerRounding::new(0.15),
            CornerRounding::new(0.15),
            0.0,
        )
    }
    /// A sun of eight deep rays.
    pub fn very_sunny() -> RoundedPolygon {
        RoundedPolygon::star(
            8,
            0.65,
            CornerRounding::new(0.15),
            CornerRounding::new(0.15),
            0.0,
        )
    }
    /// A four-lobed cookie.
    pub fn cookie4() -> RoundedPolygon {
        RoundedPolygon::star(
            4,
            0.7,
            CornerRounding::new(0.6),
            CornerRounding::new(0.35),
            PI / 4.0,
        )
    }
    /// A six-lobed cookie.
    pub fn cookie6() -> RoundedPolygon {
        RoundedPolygon::star(
            6,
            0.75,
            CornerRounding::new(0.5),
            CornerRounding::new(0.3),
            -PI / 2.0,
        )
    }
    /// A seven-lobed cookie.
    pub fn cookie7() -> RoundedPolygon {
        RoundedPolygon::star(
            7,
            0.75,
            CornerRounding::new(0.5),
            CornerRounding::new(0.3),
            -PI / 2.0,
        )
    }
    /// A nine-lobed cookie.
    pub fn cookie9() -> RoundedPolygon {
        RoundedPolygon::star(
            9,
            0.8,
            CornerRounding::new(0.5),
            CornerRounding::new(0.3),
            -PI / 2.0,
        )
    }
    /// A twelve-lobed cookie.
    pub fn cookie12() -> RoundedPolygon {
        RoundedPolygon::star(
            12,
            0.8,
            CornerRounding::new(0.5),
            CornerRounding::new(0.3),
            -PI / 2.0,
        )
    }
    /// A four-leaf clover.
    pub fn clover4() -> RoundedPolygon {
        RoundedPolygon::star(
            4,
            0.25,
            CornerRounding::new(0.9),
            CornerRounding::new(0.05),
            PI / 4.0,
        )
    }
    /// An eight-leaf clover.
    pub fn clover8() -> RoundedPolygon {
        RoundedPolygon::star(
            8,
            0.6,
            CornerRounding::new(0.6),
            CornerRounding::new(0.05),
            0.0,
        )
    }
    /// A burst of twelve sharp points.
    pub fn burst() -> RoundedPolygon {
        RoundedPolygon::star(
            12,
            0.7,
            CornerRounding::new(0.03),
            CornerRounding::new(0.03),
            -PI / 2.0,
        )
    }
    /// A burst of ten soft points.
    pub fn soft_burst() -> RoundedPolygon {
        RoundedPolygon::star(
            10,
            0.72,
            CornerRounding::new(0.12),
            CornerRounding::new(0.12),
            -PI / 2.0,
        )
    }
    /// A boom: fifteen deep, sharp points.
    pub fn boom() -> RoundedPolygon {
        RoundedPolygon::star(
            15,
            0.45,
            CornerRounding::new(0.03),
            CornerRounding::new(0.03),
            -PI / 2.0,
        )
    }
    /// A soft boom: sixteen deep, rounded points.
    pub fn soft_boom() -> RoundedPolygon {
        RoundedPolygon::star(
            16,
            0.5,
            CornerRounding::new(0.1),
            CornerRounding::new(0.06),
            -PI / 2.0,
        )
    }
    /// A flower of eight round petals.
    pub fn flower() -> RoundedPolygon {
        RoundedPolygon::star(
            8,
            0.6,
            CornerRounding::new(0.45),
            CornerRounding::new(0.05),
            -PI / 2.0,
        )
    }
    /// A puffy cloud of twelve bumps.
    pub fn puffy() -> RoundedPolygon {
        RoundedPolygon::star(
            12,
            0.85,
            CornerRounding::new(0.4),
            CornerRounding::new(0.02),
            -PI / 2.0,
        )
        .scaled(1.0, 0.8)
    }
    /// A puffy diamond.
    pub fn puffy_diamond() -> RoundedPolygon {
        RoundedPolygon::star(
            4,
            0.55,
            CornerRounding::new(0.6),
            CornerRounding::new(0.15),
            -PI / 2.0,
        )
    }
    /// A heart.
    pub fn heart() -> RoundedPolygon {
        RoundedPolygon::from_vertices(
            &[
                Point::new(0.0, -0.45),
                Point::new(0.55, -1.0),
                Point::new(1.0, -0.55),
                Point::new(1.0, -0.1),
                Point::new(0.0, 1.0),
                Point::new(-1.0, -0.1),
                Point::new(-1.0, -0.55),
                Point::new(-0.55, -1.0),
            ],
            &[
                CornerRounding::new(0.1),
                CornerRounding::new(0.3),
                CornerRounding::new(0.45),
                CornerRounding::new(0.3),
                CornerRounding::new(0.15),
                CornerRounding::new(0.3),
                CornerRounding::new(0.45),
                CornerRounding::new(0.3),
            ],
        )
    }

    /// Every shape, with its name.
    pub fn all() -> Vec<(&'static str, RoundedPolygon)> {
        vec![
            ("circle", Self::circle()),
            ("square", Self::square()),
            ("slanted", Self::slanted()),
            ("arch", Self::arch()),
            ("semicircle", Self::semicircle()),
            ("oval", Self::oval()),
            ("pill", Self::pill()),
            ("triangle", Self::triangle()),
            ("arrow", Self::arrow()),
            ("diamond", Self::diamond()),
            ("clam shell", Self::clam_shell()),
            ("pentagon", Self::pentagon()),
            ("gem", Self::gem()),
            ("sunny", Self::sunny()),
            ("very sunny", Self::very_sunny()),
            ("cookie 4", Self::cookie4()),
            ("cookie 6", Self::cookie6()),
            ("cookie 7", Self::cookie7()),
            ("cookie 9", Self::cookie9()),
            ("cookie 12", Self::cookie12()),
            ("clover 4", Self::clover4()),
            ("clover 8", Self::clover8()),
            ("burst", Self::burst()),
            ("soft burst", Self::soft_burst()),
            ("boom", Self::boom()),
            ("soft boom", Self::soft_boom()),
            ("flower", Self::flower()),
            ("puffy", Self::puffy()),
            ("puffy diamond", Self::puffy_diamond()),
            ("heart", Self::heart()),
        ]
    }
}

/// A widget that paints a **morph** between two shapes at a progress — or one shape, when
/// both are the same.
///
/// ```
/// use frus_widgets::{MaterialShapes, MorphingShape};
/// let _s: MorphingShape = MorphingShape::new(MaterialShapes::circle(), MaterialShapes::cookie9(), 0.3).size(64.0);
/// let _one: MorphingShape = MorphingShape::shape(MaterialShapes::heart());
/// ```
pub struct MorphingShape {
    morph: Morph,
    progress: f32,
    rotation: f32,
    size: f32,
    color: Option<Color>,
}

impl MorphingShape {
    /// `from` turning into `to`, `progress` of the way.
    pub fn new(from: RoundedPolygon, to: RoundedPolygon, progress: f32) -> Self {
        Self {
            morph: Morph::new(&from, &to),
            progress,
            rotation: 0.0,
            size: 48.0,
            color: None,
        }
    }

    /// One shape, still.
    pub fn shape(shape: RoundedPolygon) -> Self {
        Self::new(shape.clone(), shape, 0.0)
    }

    /// The side of its square; forty-eight unless told.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(0.0);
        self
    }

    /// Turned `radians` about its centre.
    pub fn rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    /// The fill; `primary` unless told.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl<Msg> Widget<Msg> for MorphingShape {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(self.size),
            height: Dimension::Length(self.size),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let color = self
            .color
            .unwrap_or(theme.scheme.primary)
            .fade(status.opacity);
        scene.fill_path(
            &self.morph.path_at(self.progress, bounds, self.rotation),
            color,
        );
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius_range(points: &[Point]) -> (f32, f32) {
        points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            let r = len(*p);
            (lo.min(r), hi.max(r))
        })
    }

    #[test]
    fn a_rounded_corner_is_tangent_to_both_edges() {
        let sq = RoundedPolygon::from_vertices(
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0),
            ],
            &[CornerRounding::new(2.0)],
        );
        // The arc at (10, 0) leaves the top edge at x = 8 and joins the right one at y = 2.
        let arc = sq
            .cubics()
            .iter()
            .find(|c| (c[0].x - 8.0).abs() < 1e-4 && c[0].y.abs() < 1e-4)
            .expect("an arc starting at (8, 0)");
        assert!((arc[3].x - 10.0).abs() < 1e-4 && (arc[3].y - 2.0).abs() < 1e-4);
        // Its middle is on the circle of radius two about (8, 2).
        let mid = bezier(arc, 0.5);
        let d = len(sub(mid, Point::new(8.0, 2.0)));
        assert!((d - 2.0).abs() < 0.01, "{d}");
    }

    #[test]
    fn a_rounding_too_large_for_its_edges_is_reduced_to_fit() {
        let tri = RoundedPolygon::regular(3, CornerRounding::new(100.0), 0.0);
        // Still a closed outline: three arcs that meet at the middle of each edge.
        assert_eq!(tri.cubics().len(), 3);
        for w in tri.cubics().windows(2) {
            assert!(len(sub(w[0][3], w[1][0])) < 1e-4);
        }
    }

    #[test]
    fn a_morph_starts_and_ends_on_its_shapes() {
        let a = MaterialShapes::square();
        let b = MaterialShapes::sunny();
        let m = Morph::new(&a, &b);
        let start = m.points_at(0.0);
        let end = m.points_at(1.0);
        let sa = normalised(&a).sample(MORPH_POINTS);
        let sb = normalised(&b).sample(MORPH_POINTS);
        assert_eq!(start, sa);
        // The end is the second shape's points, turned: the same set.
        for p in &end {
            assert!(sb.iter().any(|q| len(sub(*p, *q)) < 1e-4));
        }
    }

    #[test]
    fn half_way_every_point_is_half_way() {
        let m = Morph::new(&MaterialShapes::circle(), &MaterialShapes::cookie9());
        let (a, b, h) = (m.points_at(0.0), m.points_at(1.0), m.points_at(0.5));
        for i in 0..h.len() {
            let mid = lerp(a[i], b[i], 0.5);
            assert!(len(sub(mid, h[i])) < 1e-5);
        }
        // A circle to a cookie: the halfway shape's radius swings half as much as the
        // cookie's.
        let (lo_b, hi_b) = radius_range(&b);
        let (lo_h, hi_h) = radius_range(&h);
        assert!(
            (hi_h - lo_h) < (hi_b - lo_b) * 0.75,
            "{lo_h}..{hi_h} vs {lo_b}..{hi_b}"
        );
        assert!((hi_h - lo_h) > (hi_b - lo_b) * 0.25);
    }

    #[test]
    fn the_partner_points_are_the_nearest_rotation() {
        // A circle against itself turned: the best rotation undoes the turn, so the morph
        // barely moves.
        let a = MaterialShapes::circle();
        let b = MaterialShapes::circle().rotated(1.0);
        let m = Morph::new(&a, &b);
        let (s, e) = (m.points_at(0.0), m.points_at(1.0));
        let worst = s
            .iter()
            .zip(&e)
            .map(|(p, q)| len(sub(*p, *q)))
            .fold(0.0, f32::max);
        assert!(worst < 0.05, "{worst}");
    }

    #[test]
    fn every_material_shape_is_closed_clockwise_and_fills_its_box() {
        for (name, shape) in MaterialShapes::all() {
            let n = normalised(&shape);
            let b = n.bounds();
            assert!((b.width.max(b.height) - 2.0).abs() < 1e-3, "{name}: {b:?}");
            let pts = n.sample(64);
            assert!(signed_area(&pts) > 0.0, "{name} is clockwise");
            for w in shape.cubics().windows(2) {
                assert!(len(sub(w[0][3], w[1][0])) < 1e-3, "{name} is joined up");
            }
        }
    }
}
