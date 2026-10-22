//! Pagination (UD-6): a `page: u16` Payload field, a page sized with `Budget` against
//! the fixed parts at their largest, and Previous/Next disabled at the bounds.

// recipe: pagination
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    ActionRow, Budget, Components, Container, ContainerComponent, TextDisplay, V2Message,
    escape_markdown,
};

use crate::store::{Catalog, ITEM_NAME_MAX, missing};

/// The longest row: a row number of up to six digits plus `. ` (the `+ 8`), and a
/// name whose every character `escape_markdown` doubled. Sizing pages with it keeps page boundaries stable.
pub const ROW_MAX_CHARS: usize = 2 * ITEM_NAME_MAX + 8;

/// `/items`: the first page of the catalog.
#[derive(SlashCommand, Debug)]
#[command("items", description = "Browse the item list")]
pub struct Items;

impl Items {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let catalog = ctx.state::<Catalog>().ok_or_else(missing::<Catalog>)?;
        ctx.respond(render_items(&catalog.0, 0, ctx.limits())).await
    }
}

/// Previous and Next. The Payload is the target page, so the two ids differ (CID-02).
#[derive(Button, Debug, Clone, Copy)]
#[button("items.page", defaults(style = Secondary, label = "Next"))]
pub struct TurnPage {
    /// The page to show, 0-based.
    pub page: u16,
}

/// Page `page` (0-based, clamped to the last page) of `items`, sized to `limits`.
pub fn render_items(items: &[String], page: u16, limits: &Limits) -> V2Message {
    if items.is_empty() {
        return frame(vec![TextDisplay::new("-# No items yet.").into()]); // the empty state
    }
    // The fixed parts at their largest: the longest page line and the pager.
    let fixed = frame(vec![
        page_line(u16::MAX - 1, u16::MAX).into(),
        pager(0, u16::MAX).into(),
    ]);
    let per_page = Budget::of(&fixed, limits)
        .fits(&TextDisplay::new("x".repeat(ROW_MAX_CHARS)))
        .max(1);
    let pages = u16::try_from(items.len().div_ceil(per_page)).unwrap_or(u16::MAX);
    let page = page.min(pages - 1); // the page is client input: clamp it
    let start = usize::from(page) * per_page;
    let mut body: Vec<ContainerComponent> = items[start..]
        .iter()
        .take(per_page)
        .zip(start + 1..)
        .map(|(name, n)| TextDisplay::new(format!("{n}. {}", escape_markdown(name))).into())
        .collect();
    body.push(page_line(page, pages).into());
    body.push(pager(page, pages).into());
    frame(body)
}

/// `-# Page 2 of 7`.
fn page_line(page: u16, pages: u16) -> TextDisplay {
    TextDisplay::new(format!("-# Page {} of {pages}", u32::from(page) + 1))
}

/// Previous (disabled on the first page) and Next (disabled on the last).
fn pager(page: u16, pages: u16) -> ActionRow {
    ActionRow::buttons([
        TurnPage {
            page: page.saturating_sub(1),
        }
        .build()
        .label("Previous")
        .disabled(page == 0),
        TurnPage {
            page: page.saturating_add(1),
        }
        .build()
        .disabled(u32::from(page) + 1 >= u32::from(pages)),
    ])
}

/// The title, then `body`, in one Container.
fn frame(body: Vec<ContainerComponent>) -> V2Message {
    let header = Container::new().push(TextDisplay::new("## Items"));
    V2Message::new(Components::new().push(body.into_iter().fold(header, Container::push)))
}

impl TurnPage {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let catalog = ctx.state::<Catalog>().ok_or_else(missing::<Catalog>)?;
        ctx.update(render_items(&catalog.0, self.page, ctx.limits()))
            .await
    }
}
// recipe-end: pagination

#[cfg(test)]
mod tests {
    use noctis::ui::{cost, validate};

    use super::*;

    fn per_page(limits: &Limits) -> usize {
        let fixed = frame(vec![
            page_line(u16::MAX - 1, u16::MAX).into(),
            pager(0, u16::MAX).into(),
        ]);
        Budget::of(&fixed, limits).fits(&TextDisplay::new("x".repeat(ROW_MAX_CHARS)))
    }

    /// The message as sent, as compact JSON.
    fn text(msg: &V2Message) -> String {
        noctis::testkit::render_json(msg)
            .map(|json| json.to_string())
            .unwrap_or_default()
    }

    /// P1, P2: every page of 300 items validates, plain or with names that
    /// `escape_markdown` doubles.
    #[test]
    fn every_page_validates() {
        let limits = Limits::default();
        let p = per_page(&limits);
        assert!(p > 1, "{p}");
        let pages = 300_usize.div_ceil(p);
        for items in [Catalog::sample(300).0, vec!["*".repeat(ITEM_NAME_MAX); 300]] {
            for page in 0..pages {
                let mut msg = render_items(&items, u16::try_from(page).unwrap_or(0), &limits);
                let report = validate(&mut msg, &limits);
                assert!(report.is_ok(), "page {page}: {report:?}");
            }
            let last = render_items(&items, u16::try_from(pages - 1).unwrap_or(0), &limits);
            assert!(text(&last).contains(&format!("Page {pages} of {pages}")));
        }
    }

    /// P4: a page past the end renders the last page.
    #[test]
    fn page_is_clamped() {
        let items = Catalog::sample(300).0;
        let limits = Limits::default();
        let pages = 300_usize.div_ceil(per_page(&limits));
        let msg = render_items(&items, u16::MAX, &limits);
        assert!(text(&msg).contains(&format!("Page {pages} of {pages}")));
        assert!(text(&msg).contains("300. Item 300"));
    }

    /// P5: the empty state has no buttons.
    #[test]
    fn empty_list_has_no_pager() {
        let msg = render_items(&[], 0, &Limits::default());
        assert!(text(&msg).contains("No items yet."));
        assert_eq!(cost(&msg, &Limits::default()).components, 3);
    }

    /// P6: one item is one page; exactly two full pages are two pages.
    #[test]
    fn page_boundaries() {
        let limits = Limits::default();
        let one = text(&render_items(&["a".to_owned()], 0, &limits));
        assert!(one.contains("Page 1 of 1"));
        assert_eq!(one.matches("\"disabled\":true").count(), 2, "{one}");
        let items = Catalog::sample(2 * per_page(&limits)).0;
        let second = text(&render_items(&items, 1, &limits));
        assert!(second.contains("Page 2 of 2"));
        assert_eq!(second.matches("\"disabled\":true").count(), 1, "{second}");
    }
}
