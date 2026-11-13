//! Shape nodes: [`Pie`], [`Arc`], [`Line`] and [`Area`], drawn as one node
//! each.
//!
//! A shape is a child like any other (`{Pie::new(..)}` in `view!`) and fills
//! its box: by default it is `w-full h-full` of its parent, and
//! [`classes`](Pie::classes) / [`style`](Pie::style) size and place it like
//! any node (`size-32`, `absolute inset-0`, ..). The drawing is a circle of
//! the box's smaller side, centred in the box. Angles are in degrees,
//! clockwise from 12 o'clock. A [`Line`] or an [`Area`] draws over the
//! whole box instead, its points in fractions of the box (y up).
//!
//! The Renderer lowers a shape to CSS that takumi draws (a `conic-gradient`
//! background on a round, square box, with a `radial-gradient` mask for an
//! arc's hole; a box clipped to a `clip-path: polygon(..)` for a line or an
//! area); no SVG is involved. A shape is built from Template data in
//! the view, so its values are part of the render key through that data.
//!
//! ```
//! use noctis_image::shape::{Arc, Area, Line, Pie};
//!
//! let pie = Pie::new([(3, 0x5865F2), (1, 0x57F287)]).start_deg(90.0);
//! let gauge = Arc::new(0.72, 0x5865F2)
//!     .track(0x2B2D31)
//!     .start_deg(225.0)
//!     .sweep_deg(270.0)
//!     .thickness_pct(18.0)
//!     .round_caps(true);
//! let line = Line::series([3, 5, 4, 8, 6], 0x5865F2).stroke_px(3.0);
//! let area = Area::series([3, 5, 4, 8, 6], 0x2B2D31);
//! # let _ = (pie, gauge, line, area);
//! ```

use std::fmt::Write as _;

use super::{Classes, Node, Style, rgb24};

mod line;

pub use line::{Area, Line, MAX_POINTS};

/// The most slices one shape draws; a [`Pie`] with more fails its render
/// with `RenderError::Unsupported`.
pub const MAX_SLICES: usize = 64;

/// The default ring thickness of an [`Arc`], in percent of its radius.
const DEFAULT_THICKNESS_PCT: f32 = 20.0;

/// A pie chart: slices proportional to their values, clockwise from
/// 12 o'clock (or from [`start_deg`](Self::start_deg)).
///
/// Zero, negative, NaN and infinite values are skipped; a pie without a
/// positive value draws nothing. More than [`MAX_SLICES`] drawn slices fail
/// the render with `RenderError::Unsupported`.
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct Pie {
    pub(crate) slices: Vec<(f64, u32)>,
    pub(crate) start_deg: f32,
    classes: Classes,
    style: Style,
}

impl Pie {
    /// A pie of `(value, 0xRRGGBB)` slices, in order.
    pub fn new<I, V>(slices: I) -> Self
    where
        I: IntoIterator<Item = (V, u32)>,
        V: Into<f64>,
    {
        Self {
            slices: slices
                .into_iter()
                .map(|(value, rgb)| (value.into(), rgb24(rgb)))
                .collect(),
            start_deg: 0.0,
            classes: Classes::default(),
            style: Style::new(),
        }
    }

    /// Where the first slice starts, in degrees clockwise from 12 o'clock
    /// (default 0). A NaN or infinite angle leaves it unchanged.
    pub const fn start_deg(mut self, deg: f32) -> Self {
        if deg.is_finite() {
            self.start_deg = deg;
        }
        self
    }

    /// The node's classes, after the default `w-full h-full` (so `size-24`
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

    /// The drawn slices: positive finite values only.
    pub(crate) fn drawn(&self) -> impl Iterator<Item = (f64, u32)> + '_ {
        self.slices
            .iter()
            .copied()
            .filter(|(value, _)| value.is_finite() && *value > 0.0)
    }
}

/// A ring segment: an arc of `value` (`0..=1`) of its sweep, over an
/// optional track, with a transparent centre.
///
/// The defaults are a full ring (`sweep_deg(360.0)`) starting at 12 o'clock,
/// [`thickness_pct`](Self::thickness_pct) 20, no track and square caps. A
/// 270° gauge is `.start_deg(225.0).sweep_deg(270.0)`. Other nodes sit in
/// the hole by placing both in a `relative` box and the label `absolute`.
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct Arc {
    pub(crate) value: f32,
    pub(crate) color: u32,
    pub(crate) track: Option<u32>,
    pub(crate) thickness_pct: f32,
    pub(crate) start_deg: f32,
    pub(crate) sweep_deg: f32,
    pub(crate) round_caps: bool,
    classes: Classes,
    style: Style,
}

impl Arc {
    /// An arc of `value` of the sweep in `0xRRGGBB`; `value` clamps to
    /// `0..=1` (NaN is 0).
    pub const fn new(value: f32, rgb: u32) -> Self {
        Self {
            value: if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            },
            color: rgb24(rgb),
            track: None,
            thickness_pct: DEFAULT_THICKNESS_PCT,
            start_deg: 0.0,
            sweep_deg: 360.0,
            round_caps: false,
            classes: Classes::__literal(""),
            style: Style::new(),
        }
    }

    /// The track drawn over the whole sweep behind the value, in
    /// `0xRRGGBB`.
    pub const fn track(mut self, rgb: u32) -> Self {
        self.track = Some(rgb24(rgb));
        self
    }

    /// The ring's thickness in percent of its radius, clamped to `0..=100`
    /// (100 is a filled sector). A NaN or infinite value leaves it
    /// unchanged.
    pub const fn thickness_pct(mut self, pct: f32) -> Self {
        if pct.is_finite() {
            self.thickness_pct = pct.clamp(0.0, 100.0);
        }
        self
    }

    /// Where the sweep starts, in degrees clockwise from 12 o'clock
    /// (default 0). A NaN or infinite angle leaves it unchanged.
    pub const fn start_deg(mut self, deg: f32) -> Self {
        if deg.is_finite() {
            self.start_deg = deg;
        }
        self
    }

    /// The sweep in degrees, clamped to `0..=360` (default 360). A NaN or
    /// infinite value leaves it unchanged.
    pub const fn sweep_deg(mut self, deg: f32) -> Self {
        if deg.is_finite() {
            self.sweep_deg = deg.clamp(0.0, 360.0);
        }
        self
    }

    /// Whether the value's two ends are round: two circles of the ring's
    /// thickness, placed by Noctis at the value's start and end (not drawn
    /// for a value of 0). The track's ends stay square.
    pub const fn round_caps(mut self, round: bool) -> Self {
        self.round_caps = round;
        self
    }

    /// The node's classes, after the default `w-full h-full` (so `size-24`
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

/// A shape of a [`Node`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Shape {
    Pie(Pie),
    Arc(Arc),
    Line(Line),
    Area(Area),
}

impl Shape {
    /// The shape's name in an error.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Pie(_) => "Pie",
            Self::Arc(_) => "Arc",
            Self::Line(_) => "Line",
            Self::Area(_) => "Area",
        }
    }

    /// Why the shape is over its cap: [`MAX_SLICES`] slices or
    /// [`MAX_POINTS`] points.
    pub(crate) fn over_cap(&self) -> Option<String> {
        let (count, cap, what) = match self {
            Self::Pie(pie) => (pie.drawn().count(), MAX_SLICES, "slices"),
            Self::Arc(arc) => (arc_segments(arc).len(), MAX_SLICES, "slices"),
            Self::Line(Line { points, .. }) | Self::Area(Area { points, .. }) => {
                (line::drawn(points).count(), MAX_POINTS, "points")
            }
        };
        (count > cap).then(|| {
            format!(
                "shape `{}` draws {count} {what}, more than the cap of {cap}",
                self.name()
            )
        })
    }

    /// The inline CSS of a pie's or an arc's round, square box and of each
    /// of its round caps; `None` for a line or an area.
    pub(crate) fn css(&self) -> Option<(String, Vec<String>)> {
        match self {
            Self::Pie(pie) => Some((pie_css(pie), Vec::new())),
            Self::Arc(arc) => Some(arc_css(arc)),
            Self::Line(_) | Self::Area(_) => None,
        }
    }

    /// The `clip-path` of a line drawn in a box of `width` x `height`
    /// pixels, or of an area; `None` when nothing is drawn.
    pub(crate) fn polygon(&self, size: Option<(f64, f64)>) -> Option<String> {
        match self {
            Self::Line(line) => {
                let (width, height) = size?;
                line::line_polygon(&line.points, f64::from(line.stroke_px), width, height)
            }
            Self::Area(area) => line::area_polygon(&area.points),
            Self::Pie(_) | Self::Arc(_) => None,
        }
    }

    /// The `0xRRGGBB` colour of a line or an area.
    pub(crate) fn fill(&self) -> Option<u32> {
        match self {
            Self::Line(line) => Some(line.color),
            Self::Area(area) => Some(area.color),
            Self::Pie(_) | Self::Arc(_) => None,
        }
    }
}

impl From<Pie> for Node {
    fn from(pie: Pie) -> Self {
        let (classes, style) = (pie.classes.clone(), pie.style);
        Node::shape(Shape::Pie(pie)).classes(classes).style(style)
    }
}

impl From<Arc> for Node {
    fn from(arc: Arc) -> Self {
        let (classes, style) = (arc.classes.clone(), arc.style);
        Node::shape(Shape::Arc(arc)).classes(classes).style(style)
    }
}

impl From<Line> for Node {
    fn from(line: Line) -> Self {
        let (classes, style) = (line.classes.clone(), line.style);
        Node::shape(Shape::Line(line)).classes(classes).style(style)
    }
}

impl From<Area> for Node {
    fn from(area: Area) -> Self {
        let (classes, style) = (area.classes.clone(), area.style);
        Node::shape(Shape::Area(area)).classes(classes).style(style)
    }
}

/// The style of the square, round box every shape draws in.
const BOX: &str = "position: relative; aspect-ratio: 1; width: 100%; max-height: 100%; \
                   border-radius: 50%";

/// One slice of a conic gradient: `0xRRGGBB` from and to an angle relative
/// to the start.
type Segment = (u32, f64, f64);

fn pie_css(pie: &Pie) -> String {
    let total: f64 = pie.drawn().map(|(value, _)| value).sum();
    let mut segments = Vec::new();
    if total.is_finite() && total > 0.0 {
        let mut at = 0.0;
        let count = pie.drawn().count();
        for (i, (value, rgb)) in pie.drawn().enumerate() {
            let to = if i + 1 == count {
                360.0
            } else {
                at + value / total * 360.0
            };
            segments.push((rgb, at, to));
            at = to;
        }
    }
    with_conic(BOX, f64::from(pie.start_deg), &segments)
}

/// The value, track and empty segments of `arc`, without empty ones.
fn arc_segments(arc: &Arc) -> Vec<Segment> {
    let sweep = f64::from(arc.sweep_deg);
    let end = f64::from(arc.value) * sweep;
    let mut segments = Vec::with_capacity(3);
    if end > 0.0 {
        segments.push((arc.color, 0.0, end));
    }
    if let Some(track) = arc.track
        && sweep > end
    {
        segments.push((track, end, sweep));
    }
    segments
}

fn arc_css(arc: &Arc) -> (String, Vec<String>) {
    let thickness = f64::from(arc.thickness_pct);
    let segments = if thickness > 0.0 {
        arc_segments(arc)
    } else {
        Vec::new()
    };
    let mut css = with_conic(BOX, f64::from(arc.start_deg), &segments);
    if !segments.is_empty() && thickness < 100.0 {
        // The hole: a mask transparent inside the ring's inner radius, with a
        // 1 %-of-radius ramp as its anti-aliased edge.
        let inner = 100.0 - thickness;
        let soft = (inner - 1.0).max(0.0);
        let _ = write!(
            css,
            "; mask-image: radial-gradient(circle closest-side, transparent {soft:.3}%, \
             #000 {inner:.3}%)"
        );
    }
    let mut caps = Vec::new();
    let end = f64::from(arc.value) * f64::from(arc.sweep_deg);
    if arc.round_caps && end > 0.0 && thickness > 0.0 {
        // In percent of the square box: the cap's diameter is the ring's
        // thickness, its centre on the ring's middle radius.
        let diameter = thickness / 2.0;
        let middle = 50.0 - diameter / 2.0;
        let start = f64::from(arc.start_deg);
        for angle in [start, start + end] {
            let (sin, cos) = angle.to_radians().sin_cos();
            let left = 50.0 + middle * sin - diameter / 2.0;
            let top = 50.0 - middle * cos - diameter / 2.0;
            caps.push(format!(
                "position: absolute; left: {left:.3}%; top: {top:.3}%; \
                 width: {diameter:.3}%; height: {diameter:.3}%; border-radius: 50%; \
                 background-color: #{:06x}",
                arc.color
            ));
        }
    }
    (css, caps)
}

/// `base` with a conic-gradient background of `segments` from `start`
/// degrees; `base` alone without segments.
fn with_conic(base: &str, start: f64, segments: &[Segment]) -> String {
    let mut css = base.to_owned();
    if segments.is_empty() {
        return css;
    }
    let from = start.rem_euclid(360.0);
    let _ = write!(css, "; background-image: conic-gradient(from {from:.3}deg");
    let mut last = 0.0;
    for &(rgb, from, to) in segments {
        let _ = write!(css, ", #{rgb:06x} {from:.3}deg {to:.3}deg");
        last = to;
    }
    if last < 360.0 {
        let _ = write!(css, ", transparent {last:.3}deg 360deg");
    }
    css.push(')');
    css
}

#[cfg(test)]
mod tests;
