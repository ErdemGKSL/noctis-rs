//! [`Style`]: typed dynamic values for a node.

/// Typed dynamic style values: what a Template uses for data-driven widths,
/// colours and transforms, instead of formatting a class string.
///
/// The last setter on a property wins (`width_px(5).width_pct(50.0)` is 50 %).
/// A NaN or infinite `f32` leaves that property unchanged. Style values
/// override class values of the same property when the Renderer draws the
/// node.
///
/// ```
/// use noctis_image::Style;
///
/// let bar = Style::new().width_pct(0.42 * 100.0).bg_rgb(0x5865F2);
/// assert_eq!(bar, Style::new().bg_rgb(0x5865F2).width_pct(42.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Style {
    pub(crate) width: Option<Dimension>,
    pub(crate) height: Option<Dimension>,
    pub(crate) gap: Option<u32>,
    pub(crate) bg: Option<u32>,
    pub(crate) color: Option<u32>,
    pub(crate) border_color: Option<u32>,
    pub(crate) opacity: Option<f32>,
    pub(crate) translate: Option<(i32, i32)>,
    pub(crate) rotate: Option<f32>,
}

/// A width or height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Dimension {
    /// Pixels.
    Px(u32),
    /// Percent of the parent, `0..=100`.
    Percent(f32),
}

/// The low 24 bits of `rgb` (`0xRRGGBB`).
const fn rgb24(rgb: u32) -> u32 {
    rgb & 0x00FF_FFFF
}

impl Style {
    /// A style that sets nothing.
    pub const fn new() -> Self {
        Self {
            width: None,
            height: None,
            gap: None,
            bg: None,
            color: None,
            border_color: None,
            opacity: None,
            translate: None,
            rotate: None,
        }
    }

    /// The width in pixels.
    #[must_use]
    pub const fn width_px(mut self, px: u32) -> Self {
        self.width = Some(Dimension::Px(px));
        self
    }

    /// The width in percent of the parent, clamped to `0..=100`.
    #[must_use]
    pub const fn width_pct(mut self, pct: f32) -> Self {
        if pct.is_finite() {
            self.width = Some(Dimension::Percent(pct.clamp(0.0, 100.0)));
        }
        self
    }

    /// The height in pixels.
    #[must_use]
    pub const fn height_px(mut self, px: u32) -> Self {
        self.height = Some(Dimension::Px(px));
        self
    }

    /// The height in percent of the parent, clamped to `0..=100`.
    #[must_use]
    pub const fn height_pct(mut self, pct: f32) -> Self {
        if pct.is_finite() {
            self.height = Some(Dimension::Percent(pct.clamp(0.0, 100.0)));
        }
        self
    }

    /// The gap between children, in pixels.
    #[must_use]
    pub const fn gap_px(mut self, px: u32) -> Self {
        self.gap = Some(px);
        self
    }

    /// The background colour `0xRRGGBB` (the low 24 bits are used).
    #[must_use]
    pub const fn bg_rgb(mut self, rgb: u32) -> Self {
        self.bg = Some(rgb24(rgb));
        self
    }

    /// The text colour `0xRRGGBB` (the low 24 bits are used).
    #[must_use]
    pub const fn color_rgb(mut self, rgb: u32) -> Self {
        self.color = Some(rgb24(rgb));
        self
    }

    /// The border colour `0xRRGGBB` (the low 24 bits are used).
    #[must_use]
    pub const fn border_color_rgb(mut self, rgb: u32) -> Self {
        self.border_color = Some(rgb24(rgb));
        self
    }

    /// The opacity, clamped to `0..=1`.
    #[must_use]
    pub const fn opacity(mut self, opacity: f32) -> Self {
        if opacity.is_finite() {
            self.opacity = Some(opacity.clamp(0.0, 1.0));
        }
        self
    }

    /// A translation by `x`, `y` pixels.
    #[must_use]
    pub const fn translate_px(mut self, x: i32, y: i32) -> Self {
        self.translate = Some((x, y));
        self
    }

    /// A rotation in degrees (any finite value).
    #[must_use]
    pub const fn rotate_deg(mut self, deg: f32) -> Self {
        if deg.is_finite() {
            self.rotate = Some(deg);
        }
        self
    }
}
