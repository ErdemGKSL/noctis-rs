//! [`Style`]: typed dynamic values for a node.

/// Typed dynamic style values: what a Template uses for data-driven widths,
/// colours and transforms, instead of formatting a class string.
///
/// The last setter on a property wins (`width_px(5).width_pct(50.0)` is 50 %).
/// A NaN or infinite `f32` leaves that property unchanged. Style values
/// override class values of the same property when the Renderer draws the
/// node (`rounded_px(3)` beats `rounded-lg`, `left_px(40)` beats `left-4`).
///
/// The transforms compose in one fixed order around the node's centre (its
/// transform origin): [`translate_px`](Self::translate_px), then
/// [`rotate_deg`](Self::rotate_deg), then [`scale`](Self::scale), whatever
/// order the setters were called in. The translation is never scaled or
/// rotated: `scale(2.0).translate_px(4, 0)` moves the node 4 px.
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
    pub(crate) left: Option<Offset>,
    pub(crate) top: Option<Offset>,
    pub(crate) right: Option<Offset>,
    pub(crate) bottom: Option<Offset>,
    pub(crate) rounded: Option<u32>,
    pub(crate) border_width: Option<u32>,
    pub(crate) scale: Option<f32>,
    pub(crate) z_index: Option<i32>,
}

/// A width or height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Dimension {
    /// Pixels.
    Px(u32),
    /// Percent of the parent, `0..=100`.
    Percent(f32),
}

/// An inset (`left`, `top`, `right` or `bottom`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Offset {
    /// Pixels, negative allowed.
    Px(i32),
    /// Percent of the containing block, `0..=100`.
    Percent(f32),
}

/// The smallest and largest [`Style::scale`] factor.
const SCALE_RANGE: (f32, f32) = (0.0, 10.0);

/// `Some(Offset::Percent)` of `pct` clamped to `0..=100`; `None` when it is
/// NaN or infinite.
const fn offset_pct(pct: f32) -> Option<Offset> {
    if pct.is_finite() {
        Some(Offset::Percent(pct.clamp(0.0, 100.0)))
    } else {
        None
    }
}

/// Defines the pixel and percent setters of one inset.
macro_rules! inset_setters {
    ($($field:ident: $px:ident, $pct:ident;)*) => {$(
        #[doc = concat!("The `", stringify!($field), "` inset in pixels (negative allowed).")]
        ///
        /// Like the `inset` utilities, it moves a positioned node only: give
        /// the node `absolute` (or `relative`) in its classes.
        #[must_use]
        pub const fn $px(mut self, px: i32) -> Self {
            self.$field = Some(Offset::Px(px));
            self
        }

        #[doc = concat!("The `", stringify!($field), "` inset in percent of the containing block, clamped to `0..=100`.")]
        ///
        /// Like the `inset` utilities, it moves a positioned node only.
        #[must_use]
        pub const fn $pct(mut self, pct: f32) -> Self {
            if let Some(offset) = offset_pct(pct) {
                self.$field = Some(offset);
            }
            self
        }
    )*};
}

/// The low 24 bits of `rgb` (`0xRRGGBB`).
pub(super) const fn rgb24(rgb: u32) -> u32 {
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
            left: None,
            top: None,
            right: None,
            bottom: None,
            rounded: None,
            border_width: None,
            scale: None,
            z_index: None,
        }
    }

    inset_setters! {
        left: left_px, left_pct;
        top: top_px, top_pct;
        right: right_px, right_pct;
        bottom: bottom_px, bottom_pct;
    }

    /// The corner radius of all four corners, in pixels.
    #[must_use]
    pub const fn rounded_px(mut self, px: u32) -> Self {
        self.rounded = Some(px);
        self
    }

    /// The border width of all four sides, in pixels.
    ///
    /// The border is solid unless the node's classes pick a style
    /// (`border-dashed`, `border-dotted`, ...); its colour comes from
    /// [`border_color_rgb`](Self::border_color_rgb) or a `border-*` colour
    /// class.
    #[must_use]
    pub const fn border_px(mut self, px: u32) -> Self {
        self.border_width = Some(px);
        self
    }

    /// A uniform scale factor, clamped to `0..=10` (`1.0` is the natural
    /// size, `0.0` shrinks the node to nothing). It scales around the node's
    /// centre without changing the layout, and never scales the translation
    /// (see [`Style`] for the transform order).
    #[must_use]
    pub const fn scale(mut self, factor: f32) -> Self {
        if factor.is_finite() {
            self.scale = Some(factor.clamp(SCALE_RANGE.0, SCALE_RANGE.1));
        }
        self
    }

    /// The stacking order among siblings: a higher value is drawn on top.
    /// Like `z-index`, it applies to positioned nodes and flex or grid
    /// items.
    #[must_use]
    pub const fn z_index(mut self, z: i32) -> Self {
        self.z_index = Some(z);
        self
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
