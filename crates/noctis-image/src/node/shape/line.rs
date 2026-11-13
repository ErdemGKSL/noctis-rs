//! [`Line`] and [`Area`]: a series drawn as a connected line or as the
//! filled area under it.

use std::fmt::Write as _;

use super::super::{Classes, Style, rgb24};

/// The most points one [`Line`] or [`Area`] draws; more fail the render
/// with `RenderError::Unsupported`.
pub const MAX_POINTS: usize = 512;

/// The default stroke width of a [`Line`], in pixels.
const DEFAULT_STROKE_PX: f32 = 2.0;

/// The widest stroke a [`Line`] takes, in pixels.
const MAX_STROKE_PX: f32 = 256.0;

/// A connected line through points, drawn as a band of
/// [`stroke_px`](Self::stroke_px) pixels (default 2) with bevel joins.
///
/// A point is `(x, y)` in fractions of the node's box: `(0, 0)` is the
/// bottom-left corner and `(1, 1)` the top-right one (y up). Points outside
/// `0..=1` are allowed; the band is clipped to the box. NaN and infinite
/// points are skipped; fewer than two finite points draw nothing, and more
/// than [`MAX_POINTS`] fail the render with `RenderError::Unsupported`.
///
/// The line fills its box like any shape (`w-full h-full` unless its
/// [`classes`](Self::classes) size it), so a line over an [`Area`] is two
/// `absolute inset-0` nodes in one `relative` box.
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct Line {
    pub(crate) points: Vec<(f64, f64)>,
    pub(crate) color: u32,
    pub(crate) stroke_px: f32,
    pub(crate) classes: Classes,
    pub(crate) style: Style,
}

impl Line {
    /// A line through `(x, y)` points (fractions of the box, y up) in
    /// `0xRRGGBB`.
    pub fn new<I, X, Y>(points: I, rgb: u32) -> Self
    where
        I: IntoIterator<Item = (X, Y)>,
        X: Into<f64>,
        Y: Into<f64>,
    {
        Self::from_points(collect(points), rgb)
    }

    /// A line through `values`, spread evenly on x (the first at the left
    /// edge, the last at the right one) and scaled on y to their finite
    /// minimum (the bottom edge) and maximum (the top edge); a flat series
    /// is drawn at mid-height. NaN and infinite values are skipped and keep
    /// their place on x.
    pub fn series<I, V>(values: I, rgb: u32) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<f64>,
    {
        Self::from_points(series(values), rgb)
    }

    fn from_points(points: Vec<(f64, f64)>, rgb: u32) -> Self {
        Self {
            points,
            color: rgb24(rgb),
            stroke_px: DEFAULT_STROKE_PX,
            classes: Classes::__literal(""),
            style: Style::new(),
        }
    }

    /// The band's width in pixels, clamped to `0..=256` (default 2; 0 draws
    /// nothing). A NaN or infinite width leaves it unchanged.
    pub const fn stroke_px(mut self, px: f32) -> Self {
        if px.is_finite() {
            self.stroke_px = px.clamp(0.0, MAX_STROKE_PX);
        }
        self
    }

    /// The node's classes, after the default `w-full h-full` (so `h-12`
    /// wins); a later call replaces them.
    pub fn classes(mut self, classes: Classes) -> Self {
        self.classes = classes;
        self
    }

    /// The node's [`Style`]; a later call replaces it.
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// The area between a line through points and the bottom edge of the
/// node's box, filled in one colour.
///
/// Points, [`series`](Self::series), skipped points and the
/// [`MAX_POINTS`] cap are as for [`Line`]. An area under its line is a
/// [`Line`] over an `Area` of the same points, stacked in one `relative`
/// box (`absolute inset-0` on both), the area first.
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct Area {
    pub(crate) points: Vec<(f64, f64)>,
    pub(crate) color: u32,
    pub(crate) classes: Classes,
    pub(crate) style: Style,
}

impl Area {
    /// The area under `(x, y)` points (fractions of the box, y up) in
    /// `0xRRGGBB`.
    pub fn new<I, X, Y>(points: I, rgb: u32) -> Self
    where
        I: IntoIterator<Item = (X, Y)>,
        X: Into<f64>,
        Y: Into<f64>,
    {
        Self::from_points(collect(points), rgb)
    }

    /// The area under `values`, placed as [`Line::series`] places them.
    pub fn series<I, V>(values: I, rgb: u32) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<f64>,
    {
        Self::from_points(series(values), rgb)
    }

    fn from_points(points: Vec<(f64, f64)>, rgb: u32) -> Self {
        Self {
            points,
            color: rgb24(rgb),
            classes: Classes::__literal(""),
            style: Style::new(),
        }
    }

    /// The node's classes, after the default `w-full h-full` (so `h-12`
    /// wins); a later call replaces them.
    pub fn classes(mut self, classes: Classes) -> Self {
        self.classes = classes;
        self
    }

    /// The node's [`Style`]; a later call replaces it.
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

fn collect<I, X, Y>(points: I) -> Vec<(f64, f64)>
where
    I: IntoIterator<Item = (X, Y)>,
    X: Into<f64>,
    Y: Into<f64>,
{
    points
        .into_iter()
        .map(|(x, y)| (x.into(), y.into()))
        .collect()
}

/// `values` spread evenly on x and scaled to their finite range on y.
fn series<I, V>(values: I) -> Vec<(f64, f64)>
where
    I: IntoIterator<Item = V>,
    V: Into<f64>,
{
    let values: Vec<f64> = values.into_iter().map(Into::into).collect();
    let (min, max) = values
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let range = max - min;
    // An index fits an f64 exactly far beyond any series a Template holds.
    #[allow(clippy::cast_precision_loss)]
    let last = values.len().saturating_sub(1).max(1) as f64;
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            #[allow(clippy::cast_precision_loss)]
            let x = i as f64 / last;
            let y = if range.is_finite() && range > 0.0 {
                (v - min) / range
            } else {
                0.5
            };
            // A skipped value stays non-finite.
            (x, if v.is_finite() { y } else { f64::NAN })
        })
        .collect()
}

/// The finite points of `points`.
pub(crate) fn drawn(points: &[(f64, f64)]) -> impl Iterator<Item = (f64, f64)> + '_ {
    points
        .iter()
        .copied()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
}

/// The inline `clip-path` polygon of the area under `points`, in percent of
/// the box; `None` with fewer than two finite points.
pub(crate) fn area_polygon(points: &[(f64, f64)]) -> Option<String> {
    let mut finite = drawn(points).peekable();
    let (first_x, _) = *finite.peek()?;
    let mut css = format!("polygon({:.3}% 100%", first_x * 100.0);
    let mut last_x = first_x;
    let mut count = 0_usize;
    for (x, y) in finite {
        let _ = write!(css, ", {:.3}% {:.3}%", x * 100.0, (1.0 - y) * 100.0);
        last_x = x;
        count += 1;
    }
    if count < 2 {
        return None;
    }
    let _ = write!(css, ", {:.3}% 100%)", last_x * 100.0);
    Some(css)
}

/// A point in pixels of the box, y down.
type Px = (f64, f64);

/// The inline `clip-path` polygon of a band `stroke` pixels wide through
/// `points`, in a box of `width` x `height` pixels; `None` when nothing is
/// drawn.
///
/// The band is the union of one rectangle per segment and one bevel
/// triangle per joint, all wound the same way. The outline walks the left
/// halves of the rectangles forward, touching the centre line at each
/// point, then the right halves back to the start; a triangle is walked
/// from its joint on the side it lies. Read as closed loops, each half
/// rectangle is closed by a centre-line edge that the other pass walks the
/// other way, so the `nonzero` rule fills the union and only the band's
/// long sides cross many rows.
pub(crate) fn line_polygon(
    points: &[(f64, f64)],
    stroke: f64,
    width: f64,
    height: f64,
) -> Option<String> {
    let half = stroke / 2.0;
    if half <= 0.0 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut px: Vec<Px> = Vec::with_capacity(points.len());
    for (x, y) in drawn(points) {
        let p = (x * width, (1.0 - y) * height);
        // A zero-length segment has no direction.
        if px.last() != Some(&p) {
            px.push(p);
        }
    }
    if px.len() < 2 {
        return None;
    }
    // Each segment's normal, `half` long, to its left in drawing order.
    let normals: Vec<Px> = px
        .windows(2)
        .map(|pair| {
            let (dx, dy) = (pair[1].0 - pair[0].0, pair[1].1 - pair[0].1);
            let len = dx.hypot(dy);
            (-dy / len * half, dx / len * half)
        })
        .collect();
    // A point moved by a normal to the line's left (`true`) or right.
    let shift = |p: Px, n: Px, left: bool| {
        if left {
            (p.0 + n.0, p.1 + n.1)
        } else {
            (p.0 - n.0, p.1 - n.1)
        }
    };
    // Every half rectangle has the winding of the first one (they are
    // rotations of each other); a triangle is turned to match it.
    let positive = |v: &[Px]| signed_area(v) > 0.0;
    let winding = positive(&[
        px[0],
        shift(px[0], normals[0], true),
        shift(px[1], normals[0], true),
        px[1],
    ]);
    // The bevel at each inner point: on which side of the line it lies
    // (`true` the normals' side) and its two outer corners.
    let bevels: Vec<Option<(bool, [Px; 2])>> = (1..px.len() - 1)
        .map(|i| {
            let (a, b, c) = (px[i - 1], px[i], px[i + 1]);
            let turn = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
            if turn == 0.0 {
                return None;
            }
            // The outer side of the turn: where the offsets do not overlap.
            let left = turn < 0.0;
            let mut tri = [
                b,
                shift(b, normals[i - 1], left),
                shift(b, normals[i], left),
            ];
            if positive(&tri) != winding {
                tri.swap(1, 2);
            }
            Some((left, [tri[1], tri[2]]))
        })
        .collect();
    let mut css = String::with_capacity(px.len() * 8 * 20);
    css.push_str("polygon(");
    push_point(&mut css, px[0], true);
    let bevel = |css: &mut String, at: usize, left: bool| {
        if let Some((side, [t1, t2])) = at
            .checked_sub(1)
            .and_then(|i| bevels.get(i).copied().flatten())
            && side == left
        {
            for v in [t1, t2, px[at]] {
                push_point(css, v, false);
            }
        }
    };
    for (i, &n) in normals.iter().enumerate() {
        bevel(&mut css, i, true);
        for v in [shift(px[i], n, true), shift(px[i + 1], n, true), px[i + 1]] {
            push_point(&mut css, v, false);
        }
    }
    for (i, &n) in normals.iter().enumerate().rev() {
        bevel(&mut css, i + 1, false);
        for v in [shift(px[i + 1], n, false), shift(px[i], n, false), px[i]] {
            push_point(&mut css, v, false);
        }
    }
    css.push(')');
    Some(css)
}

/// Twice the signed area of the polygon `v` (shoelace).
fn signed_area(v: &[Px]) -> f64 {
    v.iter()
        .zip(v.iter().cycle().skip(1))
        .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
        .sum()
}

fn push_point(css: &mut String, (x, y): Px, first: bool) {
    if !first {
        css.push_str(", ");
    }
    let _ = write!(css, "{x:.2}px {y:.2}px");
}

#[cfg(test)]
mod tests;
