//! Story 8.3: `noctis::ui::Budget` (FR-62, UX-DR6, AD-27). The pagination
//! contract as properties (Q1-Q3: `n` copies fit, `n + 1` fail CV2-03 or
//! CV2-04) and 300 items paged with `fits` and prev/next Route buttons
//! (G1-G5).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::collections::HashMap;

use noctis::prelude::*;
use noctis::rules::{Limits, RuleId};
use noctis::ui::{
    ActionRow, Budget, Components, Container, ContainerComponent, CreateFlags, LinkButton, Media,
    MediaGallery, MediaItem, Section, Separator, TextDisplay, Thumbnail, UiError, V2Message,
    escape_markdown, validate,
};
use proptest::prelude::*;
use serde_json::Value;

// --- Properties --------------------------------------------------------------

/// Text of 0..=600 characters, some of them two UTF-16 units.
fn text() -> impl Strategy<Value = TextDisplay> {
    "([a-z ]|😀|é){0,600}".prop_map(TextDisplay::new)
}

fn link() -> impl Strategy<Value = LinkButton> {
    "[a-z]{1,80}".prop_map(|label| LinkButton::new("https://e.com").label(label))
}

/// A random Container child that passes every per-field rule; no Route
/// ids, so CID-02 cannot fire.
fn item() -> impl Strategy<Value = ContainerComponent> {
    prop_oneof![
        text().prop_map(ContainerComponent::from),
        (proptest::collection::vec(text(), 1..=3), link()).prop_map(|(texts, l)| {
            Section::try_new(texts, l, &Limits::default())
                .unwrap()
                .into()
        }),
        proptest::collection::vec(text(), 1..=3).prop_map(|texts| {
            let thumb = Thumbnail::new(Media::url("https://e.com/t.png"));
            Section::try_new(texts, thumb, &Limits::default())
                .unwrap()
                .into()
        }),
        proptest::collection::vec(link(), 1..=5).prop_map(|links| ActionRow::try_buttons(
            links,
            &Limits::default()
        )
        .unwrap()
        .into()),
        Just(Separator::small().into()),
        (1_usize..=10).prop_map(|n| {
            let items =
                (0..n).map(|i| MediaItem::new(Media::url(format!("https://e.com/{i}.png"))));
            MediaGallery::try_new(items, &Limits::default())
                .unwrap()
                .into()
        }),
    ]
}

/// The default Limits, or random `components` and `text_total_len`.
fn limits() -> impl Strategy<Value = Limits> {
    prop_oneof![
        Just(Limits::default()),
        (1_usize..=40, 0_usize..=4000).prop_map(|(components, text)| {
            let mut limits = Limits::default();
            limits.components = components;
            limits.text_total_len = text;
            limits
        }),
    ]
}

/// One Container: the fixed children, then `extra`.
fn message<'a>(
    fixed: &'a [ContainerComponent],
    extra: impl IntoIterator<Item = &'a ContainerComponent>,
) -> V2Message {
    let container = fixed
        .iter()
        .chain(extra)
        .cloned()
        .fold(Container::new(), Container::push);
    V2Message::new(Components::new().push(container))
}

/// Strict validation: `Ok`, or the violated rule ids.
fn check(mut msg: V2Message, limits: &Limits) -> Result<(), Vec<RuleId>> {
    let mut strict = limits.clone();
    strict.mode = noctis::rules::LimitMode::Strict;
    match validate(&mut msg, &strict) {
        Ok(_) => Ok(()),
        Err(UiError::Limits { violations }) => Err(violations.iter().map(|v| v.rule).collect()),
        Err(other) => panic!("{other}"),
    }
}

fn is_budget_miss(rules: &[RuleId]) -> bool {
    rules
        .iter()
        .any(|r| *r == RuleId::CV2_03 || *r == RuleId::CV2_04)
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        max_global_rejects: 1_000_000,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    /// Q1: `fits(item)` copies beside a valid fixed Container validate;
    /// one more fails a budget rule.
    #[test]
    fn fits_is_exact(
        fixed in proptest::collection::vec(item(), 1..=8),
        item in item(),
        limits in limits(),
    ) {
        prop_assume!(check(message(&fixed, []), &limits).is_ok());
        let n = Budget::of(&message(&fixed, []), &limits).fits(&item);
        let copies = vec![item; n + 1];
        prop_assert_eq!(check(message(&fixed, &copies[..n]), &limits), Ok(()));
        let over = check(message(&fixed, &copies), &limits).unwrap_err();
        prop_assert!(is_budget_miss(&over), "{:?}", over);
    }

    /// Q2: the first `fits_prefix(..)` items validate; with the next one
    /// a budget rule fails.
    #[test]
    fn fits_prefix_is_exact(
        fixed in proptest::collection::vec(item(), 1..=8),
        items in proptest::collection::vec(item(), 0..=30),
        limits in limits(),
    ) {
        prop_assume!(check(message(&fixed, []), &limits).is_ok());
        let k = Budget::of(&message(&fixed, []), &limits).fits_prefix(&items);
        prop_assert!(k <= items.len());
        prop_assert_eq!(check(message(&fixed, &items[..k]), &limits), Ok(()));
        if k < items.len() {
            let over = check(message(&fixed, &items[..=k]), &limits).unwrap_err();
            prop_assert!(is_budget_miss(&over), "{:?}", over);
        }
    }

    /// Q3: `fits` agrees with `fits_prefix` over copies of the item.
    #[test]
    fn fits_matches_fits_prefix(
        fixed in proptest::collection::vec(item(), 0..=8),
        item in item(),
        limits in limits(),
    ) {
        let budget = Budget::of(&message(&fixed, []), &limits);
        let n = budget.fits(&item);
        if n < 64 {
            prop_assert_eq!(n, budget.fits_prefix(std::iter::repeat_n(&item, 64)));
        }
    }
}

// --- Pagination over 300 items -----------------------------------------------

/// Turns the roles page; the target page is the Payload, so prev and next
/// have different ids (CID-02).
#[derive(Button, Debug, Clone, Copy)]
#[button("t.page", defaults(style = Secondary, label = "Next"))]
struct TurnPage {
    page: u16,
}

impl TurnPage {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(render(&roles(), self.page, ctx.limits())).await
    }
}

/// A row is at most a mention (23) + " · " + a 100-character name escaped
/// to 200.
const ROW_MAX: usize = 230;

/// 300 roles: names of 1..=100 characters full of Markdown, every 7th with
/// an emoji.
fn roles() -> Vec<(String, String)> {
    (0..300_usize)
        .map(|i| {
            let len = i * 37 % 100 + 1;
            let mut name: String = "a*_b".chars().cycle().take(len).collect();
            if i % 7 == 0 {
                name.replace_range(..1, "😀");
            }
            (
                format!("<@&{}>", 1_000_000_000_000_000_000_u64 + i as u64),
                name,
            )
        })
        .collect()
}

fn row(mention: &str, name: &str) -> String {
    format!("{mention} · {}", escape_markdown(name))
}

fn page_line(page: u16, pages: u16) -> TextDisplay {
    TextDisplay::new(format!("-# Page {} of {pages}", u32::from(page) + 1))
}

fn pager(page: u16, pages: u16) -> ActionRow {
    let prev = TurnPage {
        page: page.saturating_sub(1),
    }
    .build()
    .label("Previous")
    .disabled(page == 0);
    let next = TurnPage {
        page: page.saturating_add(1),
    }
    .build()
    .disabled(u32::from(page) + 1 >= u32::from(pages));
    ActionRow::buttons([prev, next])
}

fn frame(body: Vec<ContainerComponent>) -> V2Message {
    let link = || LinkButton::new("https://e.com");
    let container = body.into_iter().fold(
        Container::new()
            .push(TextDisplay::new("## Roles\n-# The server's roles"))
            .push(ActionRow::buttons([
                link().label("Overview"),
                link().label("Roles"),
                link().label("Modules"),
            ]))
            .push(Separator::small()),
        Container::push,
    );
    V2Message::new(Components::new().push(container))
}

/// Rows per page: worst-case rows beside the fixed parts, the page line at
/// its longest.
fn per_page(limits: &Limits) -> usize {
    let fixed = frame(vec![
        page_line(u16::MAX - 1, u16::MAX).into(),
        pager(0, 1).into(),
    ]);
    Budget::of(&fixed, limits)
        .fits(&TextDisplay::new("x".repeat(ROW_MAX)))
        .max(1)
}

/// The roles View on `page`, clamped to the last page.
fn render(roles: &[(String, String)], page: u16, limits: &Limits) -> V2Message {
    let per_page = per_page(limits);
    let pages = roles.len().div_ceil(per_page);
    let page = usize::from(page).min(pages - 1);
    let rows = roles[page * per_page..]
        .iter()
        .take(per_page)
        .map(|(mention, name)| TextDisplay::new(row(mention, name)).into());
    let page = u16::try_from(page).unwrap();
    let pages = u16::try_from(pages).unwrap();
    frame(
        rows.chain([page_line(page, pages).into(), pager(page, pages).into()])
            .collect(),
    )
}

fn json(msg: &V2Message) -> Value {
    serde_json::to_value(msg.create_body(CreateFlags::empty())).unwrap()
}

/// The Container's children.
fn children(page: &Value) -> &Vec<Value> {
    page["components"][0]["components"].as_array().unwrap()
}

/// The pager's prev and next buttons.
fn prev_next(page: &Value) -> (&Value, &Value) {
    let pager = children(page).last().unwrap()["components"]
        .as_array()
        .unwrap();
    (&pager[0], &pager[1])
}

/// The role rows: between the Separator and the page line.
fn page_rows(page: &Value) -> Vec<String> {
    let children = children(page);
    children[3..children.len() - 2]
        .iter()
        .map(|c| c["content"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn pages_300_items_within_limits() {
    let limits = Limits::default();
    let roles = roles();
    let expected: Vec<String> = roles.iter().map(|(m, n)| row(m, n)).collect();
    for r in &expected {
        assert!(
            noctis::rules::discord_len(r) <= ROW_MAX,
            "{r}: the sample is not the worst case"
        );
    }
    let per_page = per_page(&limits);
    let pages = roles.len().div_ceil(per_page);
    assert!(pages > 1, "{per_page} rows per page");

    // The target page of every TurnPage id.
    let target: HashMap<String, u16> = (0..=u16::try_from(pages).unwrap())
        .map(|k| {
            let msg = V2Message::new(
                Components::new().push(ActionRow::buttons([TurnPage { page: k }.build()])),
            );
            let id = json(&msg)["components"][0]["components"][0]["custom_id"]
                .as_str()
                .unwrap()
                .to_owned();
            (id, k)
        })
        .collect();
    let target_of = |button: &Value| target[button["custom_id"].as_str().unwrap()];

    // G1: every page validates in Strict mode.
    for page in 0..u16::try_from(pages).unwrap() {
        let mut msg = render(&roles, page, &limits);
        validate(&mut msg, &limits).unwrap_or_else(|e| panic!("page {page}: {e}"));
    }

    // G2 and G3: following next from page 0 visits every page once, in order.
    let mut visited = Vec::new();
    let mut rows = Vec::new();
    let mut page = 0_u16;
    loop {
        let body = json(&render(&roles, page, &limits));
        visited.push(page);
        rows.extend(page_rows(&body));
        let (prev, next) = prev_next(&body);
        assert_ne!(prev["custom_id"], next["custom_id"], "page {page}");
        assert_eq!(prev["disabled"] == true, page == 0, "page {page}: {prev}");
        if next["disabled"] == true {
            break;
        }
        page = target_of(next);
    }
    let all: Vec<u16> = (0..u16::try_from(pages).unwrap()).collect();
    assert_eq!(visited, all);
    assert_eq!(rows, expected);

    // G3: next is disabled on the last page only.
    let last = u16::try_from(pages - 1).unwrap();
    let body = json(&render(&roles, last, &limits));
    assert_eq!(prev_next(&body).1["disabled"], true);

    // G4: following prev from the last page reaches page 0.
    let mut page = last;
    let mut steps = 0;
    loop {
        let body = json(&render(&roles, page, &limits));
        let (prev, _) = prev_next(&body);
        if prev["disabled"] == true {
            break;
        }
        page = target_of(prev);
        steps += 1;
        assert!(steps <= pages, "prev never reached page 0");
    }
    assert_eq!((page, steps), (0, pages - 1));

    // G5: an out-of-range page clamps to the last page.
    assert_eq!(
        json(&render(&roles, u16::MAX, &limits)),
        json(&render(&roles, last, &limits))
    );
}
