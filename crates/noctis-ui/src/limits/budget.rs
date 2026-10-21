//! [`Budget`]: what is left of the CV2-03 and CV2-04 budgets beside the
//! fixed parts of a message, and how many items of one shape fit (FR-62).

use noctis_rules::Limits;

use super::{child_cost, cost};
use crate::V2Message;
use crate::placement::ContainerChild;

/// What is left of a [`V2Message`]'s component and text budgets, and how
/// many more Container children fit beside it (FR-62, UX-DR6).
///
/// Build the message with its fixed parts only (header, tabs, pager, every
/// variable fixed text at its largest), take [`Budget::of`] it, then size the
/// variable list with [`fits`](Self::fits) or
/// [`fits_prefix`](Self::fits_prefix). Noctis never paginates or splits a
/// message itself (FR-24); this is how a render function sizes a page.
///
/// - [`components_left`](Self::components_left) is
///   `limits.components - cost.components` (CV2-03: every component at every
///   depth, Section texts and accessories included, select options and
///   gallery items not), and [`text_left`](Self::text_left) is
///   `limits.text_total_len - cost.text_chars` (CV2-04: the `discord_len`
///   sum of every Text Display). Both saturate at 0.
/// - An item costs exactly what [`cost`] reports for a message holding only
///   that item. Placed in a Container it costs the same, because the
///   Container is one of the fixed parts.
/// - `fits(&item)` is `min(components_left / c, text_left / t)` for an item
///   of cost `(c, t)`, where an axis the item does not use (`t = 0`) does not
///   bind.
///
/// It counts the budgets only (CV2-03 and CV2-04), never per-field rules
/// (labels, URLs, option counts, CV2-06 shape) or CID-02: `n` copies of one
/// Route button with the same Payload fit the budget but fail CID-02, so
/// real rows differ by Payload. Lenient and Strict mode give the same
/// numbers.
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_ui::{ActionRow, Budget, Components, Container, LinkButton, Separator, TextDisplay, V2Message};
///
/// let link = || LinkButton::new("https://e.com");
/// let fixed = V2Message::new(Components::new().push(
///     Container::new()
///         .push(TextDisplay::new("## T"))
///         .push(ActionRow::buttons([link(), link(), link()]))
///         .push(Separator::small()),
/// ));
/// let budget = Budget::of(&fixed, &Limits::default());
/// assert_eq!((budget.components_left(), budget.text_left()), (33, 3996));
/// // A 230-character row: the text budget binds (3996 / 230).
/// assert_eq!(budget.fits(&TextDisplay::new("x".repeat(230))), 17);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    components_left: usize,
    text_left: usize,
}

impl Budget {
    /// What `limits` leaves beside `message`, which is only read: never
    /// changed, validated, truncated or logged.
    #[must_use]
    pub fn of(message: &V2Message, limits: &Limits) -> Self {
        let used = cost(message, limits);
        Self {
            components_left: limits.components.saturating_sub(used.components),
            text_left: limits.text_total_len.saturating_sub(used.text_chars),
        }
    }

    /// Components still allowed at any depth (CV2-03).
    #[must_use]
    pub fn components_left(&self) -> usize {
        self.components_left
    }

    /// Text Display characters still allowed, counted with `discord_len`
    /// (CV2-04).
    #[must_use]
    pub fn text_left(&self) -> usize {
        self.text_left
    }

    /// How many copies of `item` fit in what is left:
    /// `min(components_left / c, text_left / t)` for an item of cost
    /// `(c, t)`, where an axis of cost 0 does not bind.
    ///
    /// Per-field rules and CID-02 are not checked (see [`Budget`]).
    ///
    /// ```
    /// use noctis_rules::Limits;
    /// use noctis_ui::{Budget, Components, TextDisplay, V2Message};
    ///
    /// let budget = Budget::of(&V2Message::new(Components::new()), &Limits::default());
    /// assert_eq!(budget.fits(&TextDisplay::new("x".repeat(1000))), 4);
    /// ```
    #[must_use]
    pub fn fits<C: ContainerChild + Clone>(&self, item: &C) -> usize {
        let (c, t) = child_cost(&item.clone().into_child());
        let by_components = self.components_left.checked_div(c).unwrap_or(usize::MAX);
        let by_text = self.text_left.checked_div(t).unwrap_or(usize::MAX);
        by_components.min(by_text)
    }

    /// How many of `items`, taken in order, fit in what is left: it adds up
    /// their costs and stops at the first item that does not fit, even when
    /// a later one would. At most `k + 1` items are pulled for a result `k`.
    ///
    /// Per-field rules and CID-02 are not checked (see [`Budget`]).
    ///
    /// ```
    /// use noctis_rules::Limits;
    /// use noctis_ui::{Budget, Components, TextDisplay, V2Message};
    ///
    /// let budget = Budget::of(&V2Message::new(Components::new()), &Limits::default());
    /// let rows: Vec<TextDisplay> = (0..3).map(|_| TextDisplay::new("x".repeat(1500))).collect();
    /// assert_eq!(budget.fits_prefix(&rows), 2);
    /// ```
    #[must_use]
    pub fn fits_prefix<'a, C, I>(&self, items: I) -> usize
    where
        C: ContainerChild + Clone + 'a,
        I: IntoIterator<Item = &'a C>,
    {
        let (mut components, mut text, mut count) = (0_usize, 0_usize, 0_usize);
        for item in items {
            let (c, t) = child_cost(&item.clone().into_child());
            components = components.saturating_add(c);
            text = text.saturating_add(t);
            if components > self.components_left || text > self.text_left {
                break;
            }
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use core::fmt;
    use std::cell::Cell;

    use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape, SelectRoute};
    use noctis_rules::LimitMode;

    use super::*;
    use crate::route::CustomId;
    use crate::{
        ActionRow, Components, Container, ContainerComponent, Handled, LinkButton, Media,
        MediaGallery, MediaItem, Section, SelectOption, Separator, StringSelect, TextDisplay,
    };

    struct Pick;

    impl Payload for Pick {
        const MAX_ENCODED_LEN: Option<usize> = Some(0);
        fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result {
            Ok(())
        }
    }

    impl Route for Pick {
        const ID: RouteId = RouteId::new("pick");
        const SHAPE: RouteShape = RouteShape::INLINE.select();
        const SCHEMA: u8 = 0;
        type Payload = Self;
    }

    impl SelectRoute for Pick {}
    impl noctis_codec::DeclaredRuntime for Pick {}
    impl Handled for Pick {}

    fn t(s: &str) -> TextDisplay {
        TextDisplay::new(s.to_owned())
    }

    fn x(n: usize) -> TextDisplay {
        TextDisplay::new("x".repeat(n))
    }

    fn l() -> LinkButton {
        LinkButton::new("https://e.com")
    }

    fn msg(components: Components) -> V2Message {
        V2Message::new(components)
    }

    fn left(message: &V2Message, limits: &Limits) -> (usize, usize) {
        let b = Budget::of(message, limits);
        (b.components_left(), b.text_left())
    }

    fn b3() -> V2Message {
        msg(Components::new().push(
            Container::new()
                .push(t("## T"))
                .push(ActionRow::buttons([l(), l(), l()]))
                .push(Separator::small()),
        ))
    }

    fn budget(components_left: usize, text_left: usize) -> Budget {
        Budget {
            components_left,
            text_left,
        }
    }

    fn row5() -> ActionRow {
        ActionRow::buttons([l(), l(), l(), l(), l()])
    }

    #[test]
    fn budget_of_empty_message() {
        assert_eq!(
            left(&msg(Components::new()), &Limits::default()),
            (40, 4000)
        );
    }

    #[test]
    fn budget_of_one_text() {
        assert_eq!(
            left(&msg(Components::new().push(t("hi"))), &Limits::default()),
            (39, 3998)
        );
    }

    #[test]
    fn budget_of_container_counts_every_depth() {
        assert_eq!(left(&b3(), &Limits::default()), (33, 3996));
    }

    #[test]
    fn budget_of_section_counts_texts_and_accessory() {
        let section = Section::new([t("a"), t("b"), t("c")], l());
        assert_eq!(
            left(&msg(Components::new().push(section)), &Limits::default()),
            (35, 3997)
        );
    }

    #[test]
    fn budget_of_counts_utf16_units() {
        assert_eq!(
            left(&msg(Components::new().push(t("😀é"))), &Limits::default()),
            (39, 3997)
        );
    }

    #[test]
    fn budget_of_over_limit_saturates() {
        let body = (0..41).fold(Components::new(), |c, _| c.push(t("ab")));
        assert_eq!(left(&msg(body), &Limits::default()), (0, 3918));
    }

    #[test]
    fn budget_of_over_long_text_saturates() {
        assert_eq!(
            left(&msg(Components::new().push(x(4001))), &Limits::default()),
            (39, 0)
        );
    }

    #[test]
    fn budget_of_ignores_options_and_gallery_items() {
        let options = (0..25).map(|i| SelectOption::new(format!("o{i}"), format!("v{i}")));
        let select = StringSelect::from_id(CustomId::new::<Pick>(Pick), options.collect());
        let row = msg(Components::new().push(ActionRow::select(select)));
        assert_eq!(left(&row, &Limits::default()), (38, 4000));
        let items = core::array::from_fn::<_, 10, _>(|i| {
            MediaItem::new(Media::url(format!("https://e.com/{i}.png")))
        });
        let gallery = msg(Components::new().push(MediaGallery::new(items)));
        assert_eq!(left(&gallery, &Limits::default()), (39, 4000));
    }

    #[test]
    fn budget_of_reads_configured_limits() {
        let mut limits = Limits::default();
        limits.components = 10;
        limits.text_total_len = 100;
        assert_eq!(left(&b3(), &limits), (3, 96));
    }

    #[test]
    fn budget_of_limit_below_cost_saturates() {
        let mut limits = Limits::default();
        limits.components = 5;
        assert_eq!(left(&b3(), &limits), (0, 3996));
    }

    #[test]
    fn budget_of_lenient_equals_strict() {
        let mut limits = Limits::default();
        limits.mode = LimitMode::Lenient;
        assert_eq!(left(&b3(), &limits), (33, 3996));
    }

    #[test]
    fn fits_empty_text_is_text_unbounded() {
        assert_eq!(budget(40, 4000).fits(&t("")), 40);
    }

    #[test]
    fn fits_text_binds() {
        assert_eq!(budget(33, 3996).fits(&x(230)), 17);
    }

    #[test]
    fn fits_components_bind() {
        assert_eq!(budget(33, 3996).fits(&Section::new([x(10)], l())), 11);
    }

    #[test]
    fn fits_row_of_five() {
        assert_eq!(budget(33, 3996).fits(&row5()), 5);
    }

    #[test]
    fn fits_exact_fill_and_one_short() {
        assert_eq!(budget(6, 4000).fits(&row5()), 1);
        assert_eq!(budget(5, 4000).fits(&row5()), 0);
    }

    #[test]
    fn fits_no_components_left() {
        assert_eq!(budget(0, 4000).fits(&t("a")), 0);
    }

    #[test]
    fn fits_no_text_left() {
        assert_eq!(budget(12, 0).fits(&t("a")), 0);
        assert_eq!(budget(12, 0).fits(&Separator::small()), 12);
    }

    #[test]
    fn fits_text_boundary() {
        assert_eq!(budget(40, 460).fits(&x(230)), 2);
        assert_eq!(budget(40, 459).fits(&x(230)), 1);
    }

    #[test]
    fn fits_counts_utf16_units() {
        assert_eq!(budget(40, 4000).fits(&t(&"😀".repeat(100))), 20);
    }

    #[test]
    fn fits_over_long_item() {
        assert_eq!(budget(40, 4000).fits(&x(4001)), 0);
    }

    #[test]
    fn fits_container_component() {
        assert_eq!(budget(40, 4000).fits(&ContainerComponent::from(x(100))), 40);
    }

    #[test]
    fn fits_ignores_per_field_rules() {
        let row = ActionRow::buttons([l().label("x".repeat(81))]);
        assert_eq!(budget(40, 4000).fits(&row), 20);
    }

    #[test]
    fn fits_prefix_empty() {
        let items: [TextDisplay; 0] = [];
        assert_eq!(budget(40, 4000).fits_prefix(&items), 0);
    }

    #[test]
    fn fits_prefix_all_fit() {
        let items: Vec<TextDisplay> = (0..10).map(|_| t("a")).collect();
        assert_eq!(budget(40, 4000).fits_prefix(&items), 10);
    }

    #[test]
    fn fits_prefix_components_bind() {
        let items: Vec<Section> = (0..3).map(|_| Section::new([t("a")], l())).collect();
        assert_eq!(budget(7, 4000).fits_prefix(&items), 2);
    }

    #[test]
    fn fits_prefix_stops_at_first_miss() {
        let items = [
            ContainerComponent::from(t("a")),
            ContainerComponent::from(row5()),
            ContainerComponent::from(t("a")),
        ];
        assert_eq!(budget(5, 4000).fits_prefix(&items), 1);
    }

    #[test]
    fn fits_prefix_text_binds() {
        let items = [x(200), x(200), x(200)];
        assert_eq!(budget(40, 500).fits_prefix(&items), 2);
    }

    #[test]
    fn fits_prefix_exact_fill() {
        let items: Vec<Section> = (0..2).map(|_| Section::new([t("a")], l())).collect();
        assert_eq!(budget(6, 4000).fits_prefix(&items), 2);
    }

    #[test]
    fn fits_prefix_pulls_at_most_one_extra() {
        let items: Vec<TextDisplay> = (0..100).map(|_| t("a")).collect();
        let pulled = Cell::new(0_usize);
        let iter = items.iter().inspect(|_| pulled.set(pulled.get() + 1));
        assert_eq!(budget(3, 4000).fits_prefix(iter), 3);
        assert!(pulled.get() <= 4, "pulled {}", pulled.get());
    }

    #[test]
    fn fits_prefix_nothing_left() {
        let items: Vec<TextDisplay> = (0..5).map(|_| t("a")).collect();
        let pulled = Cell::new(0_usize);
        let iter = items.iter().inspect(|_| pulled.set(pulled.get() + 1));
        assert_eq!(budget(0, 4000).fits_prefix(iter), 0);
        assert!(pulled.get() <= 1, "pulled {}", pulled.get());
    }
}
