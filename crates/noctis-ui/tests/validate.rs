//! The runtime Limit validator: `validate` and `cost` against every row of
//! the Story 4.3 I/O matrix (FR-24, FR-78; CV2-03..17, CID-02, ATT-05,
//! ATT-06, MSG-01, MSG-04, MSG-09), Strict and Lenient.

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use core::fmt;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use noctis_codec::{
    Field, FieldWriter, Parts, Payload, Route, RouteId, RouteShape, SelectNonce, SelectRoute,
};
use noctis_rules::len::discord_len;
use noctis_rules::{Actual, Bound, LimitMode, Limits, RuleId, Violation};
use noctis_ui::__macro::{
    ButtonDefaults, ChannelSelectDefaults, ComponentValue, SelectDefaults, build_button,
    build_channel_select, build_mentionable_select, build_role_select, build_string_select_builder,
    build_user_select,
};
use noctis_ui::{
    ActionRow, Attachment, AttachmentSet, Button, ButtonStyle, ChannelSelect, Components,
    Container, CostReport, CreateFlags, File, Handled, LegacyMessage, LimitWarning, LinkButton,
    Media, MediaGallery, MediaItem, MentionableSelect, Mentions, PremiumButton, Rgb, RoleSelect,
    Roles, RouteButton, Section, SelectOption, StringSelect, TextDisplay, Thumbnail,
    TopLevelComponent, UiError, UserSelect, Users, V2Message, cost, validate,
};
use serde_json::Value;
use twilight_model::id::Id;

/// Select defaults that send nothing (as before Story 6.11's derives).
const NO_DEFAULTS: SelectDefaults = SelectDefaults {
    placeholder: None,
    min_values: None,
    max_values: None,
    disabled: false,
};

/// What `#[derive(Button)]`'s `.build()` expands to, with no declared
/// label, emoji or style beyond Secondary.
fn button<R: ComponentValue>(route: R) -> RouteButton<R> {
    build_button(
        route,
        &ButtonDefaults {
            style: ButtonStyle::Secondary,
            label: None,
            emoji: None,
            disabled: false,
        },
    )
}

/// A String Select of `route` with `options` and no declared defaults.
fn string_select<R: ComponentValue + SelectRoute>(
    route: R,
    options: impl IntoIterator<Item = SelectOption>,
) -> StringSelect {
    build_string_select_builder(route, &NO_DEFAULTS).options(options)
}

/// A User Select of `route` with no declared defaults.
fn user_select<R: ComponentValue + SelectRoute>(route: R) -> UserSelect {
    build_user_select(route, &NO_DEFAULTS)
}

/// A Role Select of `route` with no declared defaults.
fn role_select<R: ComponentValue + SelectRoute>(route: R) -> RoleSelect {
    build_role_select(route, &NO_DEFAULTS)
}

/// A Mentionable Select of `route` with no declared defaults.
fn mentionable_select<R: ComponentValue + SelectRoute>(route: R) -> MentionableSelect {
    build_mentionable_select(route, &NO_DEFAULTS)
}

/// A Channel Select of `route` with no declared defaults.
fn channel_select<R: ComponentValue + SelectRoute>(route: R) -> ChannelSelect {
    build_channel_select(
        route,
        &ChannelSelectDefaults {
            select: NO_DEFAULTS,
            channel_types: &[],
        },
    )
}

/// A hand-written handled Route whose Payload is itself.
macro_rules! route {
    ($name:ident { $($field:ident: $ty:ty),* } = $id:literal, $shape:expr $(, $select:ident)?) => {
        #[derive(Debug, Clone)]
        struct $name { $($field: $ty),* }

        impl Payload for $name {
            const MAX_ENCODED_LEN: Option<usize> =
                noctis_codec::sum_fields(&[$(<$ty as Field>::MAX_LEN),*]);

            #[allow(unused_variables, reason = "zero-field payloads write nothing")]
            fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
                $(out.field(&self.$field)?;)*
                Ok(())
            }
        }

        impl Route for $name {
            const ID: RouteId = RouteId::new($id);
            const SHAPE: RouteShape = $shape;
            const SCHEMA: u8 = 0;
            type Payload = Self;
        }

        impl noctis_codec::DeclaredRuntime for $name {}
        impl Handled for $name {}
        $(impl $select for $name {})?
    };
}

route!(Buy { item: u64 } = "shop.buy", RouteShape::INLINE);
route!(
    Pick { page: u8 } = "pick",
    RouteShape::INLINE.select(),
    SelectRoute
);

route!(Page { page: u16 } = "page", RouteShape::INLINE);
route!(Card { id: u64 } = "card", RouteShape::BY_REF);
route!(Vote { n: u8 } = "vote", RouteShape::INLINE.signed());

/// The CID-02 help (`noctis-macros` pins the same literal).
const HELP: &str = "give the components a Payload field that tells them apart, for example the target page of a prev/next pair (`Page { page: 2 }` and `Page { page: 4 }`)";

// Helpers.

/// `B(n)`: a Route button with payload `n`.
fn b(n: u64) -> RouteButton<Buy> {
    button(Buy { item: n })
}

fn pick() -> Pick {
    Pick { page: 1 }
}

fn strict() -> Limits {
    Limits::default()
}

fn lenient() -> Limits {
    let mut l = Limits::default();
    l.mode = LimitMode::Lenient;
    l
}

fn t(s: impl Into<String>) -> TextDisplay {
    TextDisplay::new(s.into())
}

fn msg(items: impl IntoIterator<Item = TopLevelComponent>) -> V2Message {
    V2Message::new(items.into_iter().fold(Components::new(), Components::push))
}

fn texts(n: usize, s: &str) -> V2Message {
    msg((0..n).map(|_| t(s).into()))
}

fn link(label: impl Into<String>) -> LinkButton {
    LinkButton::new("https://x.y").label(label.into())
}

fn row_of(button: impl Into<Button>) -> TopLevelComponent {
    ActionRow::buttons([button.into()]).into()
}

/// Limits that let the `try_*` constructors build the over-limit shapes the
/// arity bounds reject: 6 buttons, 4 Section texts, 11 gallery items.
fn relaxed() -> Limits {
    let mut l = Limits::default();
    l.row_buttons = 6;
    l.section_texts = 4;
    l.gallery_items = 11;
    l
}

/// A row of 6 Route buttons (payloads 1..=6), built under [`relaxed`].
fn six_buttons() -> ActionRow {
    ActionRow::try_buttons((1..=6).map(b), &relaxed()).expect("relaxed limits allow 6 buttons")
}

/// The violations of a failed `try_*` constructor.
fn try_errs<T: fmt::Debug>(r: Result<T, UiError>) -> Vec<Violation> {
    match r {
        Err(UiError::Limits { violations }) => violations,
        other => panic!("expected UiError::Limits, got {other:?}"),
    }
}

fn rep(s: &str, n: usize) -> String {
    s.repeat(n)
}

fn v(rule: RuleId, path: &str, limit: Bound, actual: Actual) -> Violation {
    Violation::new(rule, path, limit, actual)
}

fn at_most(rule: RuleId, path: &str, max: usize, count: usize) -> Violation {
    v(rule, path, Bound::AtMost(max), Actual::Count(count))
}

fn between(rule: RuleId, path: &str, min: usize, max: usize, count: usize) -> Violation {
    v(
        rule,
        path,
        Bound::Between { min, max },
        Actual::Count(count),
    )
}

/// The violations of a failed `validate`.
fn errs(r: Result<CostReport, UiError>) -> Vec<Violation> {
    match r {
        Err(UiError::Limits { violations }) => {
            assert!(!violations.is_empty(), "Limits is never empty");
            violations
        }
        other => panic!("expected UiError::Limits, got {other:?}"),
    }
}

/// The violations of a V2 message under `limits`.
fn errs_v2(mut m: V2Message, limits: &Limits) -> Vec<Violation> {
    errs(validate(&mut m, limits))
}

/// Asserts `m` fails with exactly `expected` (notes ignored).
fn fails_with(m: V2Message, expected: &[Violation]) {
    let got: Vec<Violation> = errs_v2(m, &strict())
        .into_iter()
        .map(|mut x| {
            x.notes.clear();
            x
        })
        .collect();
    assert_eq!(got, expected);
}

fn passes(mut m: V2Message) -> CostReport {
    validate(&mut m, &strict()).expect("validates")
}

fn json(m: &V2Message) -> Value {
    serde_json::to_value(m.create_body(CreateFlags::empty())).expect("serializes")
}

fn legacy_json(m: &LegacyMessage) -> Value {
    serde_json::to_value(m.create_body(CreateFlags::empty())).expect("serializes")
}

fn resolve(mut m: V2Message) -> V2Message {
    for id in m.custom_ids_mut() {
        if id.get().is_none() {
            let nonce = SelectNonce::new(1).expect("in range");
            assert_eq!(id.resolve(&Parts::new().with_nonce(nonce)), Ok(None));
        }
    }
    m
}

fn custom_id(button: &RouteButton<Buy>) -> String {
    serde_json::to_value(button).expect("serializes")["custom_id"]
        .as_str()
        .expect("resolved")
        .to_owned()
}

fn image() -> Media {
    Media::url("https://x.y/a.png")
}

fn truncated(rule: RuleId, path: &str, max: usize, count: usize) -> LimitWarning {
    LimitWarning::Truncated(at_most(rule, path, max, count))
}

fn select_row(select: impl noctis_ui::RowSelect) -> TopLevelComponent {
    ActionRow::select(select).into()
}

// Matrix 1-10: CV2-03 and CV2-04.

#[test]
fn cv2_03_minimal_message() {
    let report = passes(texts(1, "hi"));
    assert_eq!(
        (report.components, report.text_chars, report.warnings.len()),
        (1, 2, 0)
    );
    assert_eq!(cost(&texts(1, "hi"), &strict()), report);
}

#[test]
fn cv2_03_forty_components_pass() {
    let container = (0..39).fold(Container::new(), |c, _| c.push(t("a")));
    assert_eq!(passes(msg([container.into()])).components, 40);
}

#[test]
fn cv2_03_forty_one_components_fail() {
    let errs = errs_v2(texts(41, "a"), &strict());
    assert_eq!(errs.len(), 1);
    let notes: Vec<String> = (0..41).map(|i| format!("text[{i}]: 1")).collect();
    let mut want = at_most(RuleId::CV2_03, "components", 40, 41);
    want.notes = notes;
    assert_eq!(errs[0], want);
}

#[test]
fn cv2_03_nested_rows_count() {
    let mut n = 0;
    let container = (0..8).fold(Container::new(), |c, _| {
        c.push(ActionRow::buttons([(); 5].map(|()| {
            n += 1;
            b(n)
        })))
    });
    let m = msg([container.into()]);
    assert_eq!(cost(&m, &strict()).components, 49);
    for limits in [strict(), lenient()] {
        let errs = errs_v2(m.clone(), &limits);
        assert_eq!(errs.len(), 1);
        assert_eq!(
            (errs[0].rule, &errs[0].actual),
            (RuleId::CV2_03, &Actual::Count(49))
        );
    }
}

#[test]
fn cv2_03_lowered_limit() {
    let mut l = strict();
    l.components = 3;
    let errs = errs_v2(texts(4, "a"), &l);
    assert_eq!(
        (errs[0].rule, &errs[0].limit, &errs[0].actual),
        (RuleId::CV2_03, &Bound::AtMost(3), &Actual::Count(4))
    );
}

#[test]
fn cv2_03_accessory_counts() {
    let section = Section::new([t("a"), t("b"), t("c")], Thumbnail::new(image()));
    assert_eq!(cost(&msg([section.into()]), &strict()).components, 5);
}

#[test]
fn cv2_04_text_budget() {
    assert_eq!(passes(texts(1, &rep("a", 4000))).text_chars, 4000);
    fails_with(
        msg([t(rep("a", 4000)).into(), t("b").into()]),
        &[at_most(RuleId::CV2_04, "components", 4000, 4001)],
    );
}

#[test]
fn cv2_04_unicode() {
    assert_eq!(passes(texts(1, &rep("😀", 2000))).text_chars, 4000);
    fails_with(
        texts(1, &format!("{}a", rep("😀", 2000))),
        &[at_most(RuleId::CV2_04, "components", 4000, 4001)],
    );
    assert_eq!(passes(texts(1, &rep("é", 4000))).text_chars, 4000);
}

#[test]
fn cv2_04_nested_and_section_texts() {
    let m = msg([
        t("ab").into(),
        Container::new()
            .push(t("cde"))
            .push(Section::new([t("f"), t("gh")], link("x")))
            .into(),
        Section::new([t("ijkl")], link("y")).into(),
    ]);
    assert_eq!(passes(m).text_chars, 12);
}

// Matrix 11-27: per-component rules.

#[test]
fn cv2_06_row_buttons() {
    // 0 and 6 buttons no longer compile (`RowButtons`); the runtime path
    // rejects them, and the validator still counts a row built under a
    // relaxed `Limits`.
    assert_eq!(
        try_errs(ActionRow::try_buttons(Vec::<Button>::new(), &strict())),
        [between(RuleId::CV2_06, "", 1, 5, 0)]
    );
    assert_eq!(
        try_errs(ActionRow::try_buttons((1..=6).map(b), &strict())),
        [between(RuleId::CV2_06, "", 1, 5, 6)]
    );
    let mut two = strict();
    two.row_buttons = 2;
    assert_eq!(
        try_errs(ActionRow::try_buttons((1..=3).map(b), &two)),
        [between(RuleId::CV2_06, "", 1, 2, 3)]
    );
    assert!(ActionRow::try_buttons((1..=5).map(b), &strict()).is_ok());
    fails_with(
        msg([six_buttons().into()]),
        &[between(RuleId::CV2_06, "row[0]", 1, 5, 6)],
    );
    passes(msg([
        ActionRow::buttons([b(1), b(2), b(3), b(4), b(5)]).into()
    ]));
}

#[test]
fn cv2_07_section_texts() {
    assert_eq!(
        try_errs(Section::try_new([], b(1), &strict())),
        [between(RuleId::CV2_07, "", 1, 3, 0)]
    );
    assert_eq!(
        try_errs(Section::try_new(
            [t("a"), t("b"), t("c"), t("d")],
            b(1),
            &strict()
        )),
        [between(RuleId::CV2_07, "", 1, 3, 4)]
    );
    for n in 1..=3 {
        assert!(Section::try_new((0..n).map(|_| t("a")), b(1), &strict()).is_ok());
    }
    let four = Section::try_new([t("a"), t("b"), t("c"), t("d")], b(1), &relaxed())
        .expect("relaxed limits allow 4 texts");
    fails_with(
        msg([four.into()]),
        &[between(RuleId::CV2_07, "section[0]", 1, 3, 4)],
    );
    passes(msg([Section::new([t("a")], b(1)).into()]));
    passes(msg([Section::new([t("a"), t("b"), t("c")], b(1)).into()]));
}

#[test]
fn cv2_08_label_boundaries() {
    let path = "row[0].button[0].label";
    for ok in [rep("a", 80), rep("😀", 40), String::new()] {
        passes(msg([row_of(b(1).label(ok))]));
    }
    fails_with(
        msg([row_of(b(1).label(rep("a", 81)))]),
        &[at_most(RuleId::CV2_08, path, 80, 81)],
    );
    fails_with(
        msg([row_of(link(rep("😀", 41)))]),
        &[at_most(RuleId::CV2_08, path, 80, 82)],
    );
}

#[test]
fn cv2_08_path_precision() {
    let m = msg([Container::new()
        .push(t("a"))
        .push(t("b"))
        .push(Section::new([t("c")], b(1).label(rep("a", 81))))
        .into()]);
    let errs = errs_v2(m, &strict());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].path, "container[0].section[2].accessory.label");
    assert_eq!(
        errs[0].to_string(),
        "[CV2-08] container[0].section[2].accessory.label: limit at most 80, actual 81"
    );
}

#[test]
fn cv2_08_link_url() {
    let path = "row[0].button[0].url";
    fails_with(
        msg([row_of(LinkButton::new(""))]),
        &[between(RuleId::CV2_08, path, 1, 512, 0)],
    );
    fails_with(
        msg([row_of(LinkButton::new(rep("h", 513)))]),
        &[between(RuleId::CV2_08, path, 1, 512, 513)],
    );
    passes(msg([row_of(LinkButton::new(rep("h", 512)))]));
}

#[test]
fn cv2_08_premium_button() {
    passes(msg([row_of(PremiumButton::new(Id::new(7)))]));
}

#[test]
fn cv2_09_placeholder() {
    let opt = || SelectOption::new("a", "a");
    passes(msg([select_row(
        string_select(pick(), [opt()]).placeholder(rep("a", 150)),
    )]));
    fails_with(
        msg([select_row(
            string_select(pick(), [opt()]).placeholder(rep("a", 151)),
        )]),
        &[at_most(
            RuleId::CV2_09,
            "row[0].select[0].placeholder",
            150,
            151,
        )],
    );
}

#[test]
fn cv2_09_options_count() {
    let options = |n: usize| (0..n).map(|i| SelectOption::new(format!("{i}"), format!("{i}")));
    let path = "row[0].select[0].options";
    fails_with(
        msg([select_row(string_select(pick(), options(0)))]),
        &[between(RuleId::CV2_09, path, 1, 25, 0)],
    );
    fails_with(
        msg([select_row(string_select(pick(), options(26)))]),
        &[between(RuleId::CV2_09, path, 1, 25, 26)],
    );
    passes(msg([select_row(string_select(pick(), options(1)))]));
    passes(msg([select_row(string_select(pick(), options(25)))]));
}

#[test]
fn cv2_09_option_fields() {
    let one = |o: SelectOption| msg([select_row(string_select(pick(), [o]))]);
    let p = "row[0].select[0].options[0]";
    fails_with(
        one(SelectOption::new("", "a")),
        &[between(RuleId::CV2_09, &format!("{p}.label"), 1, 100, 0)],
    );
    fails_with(
        one(SelectOption::new(rep("a", 101), "a")),
        &[between(RuleId::CV2_09, &format!("{p}.label"), 1, 100, 101)],
    );
    fails_with(
        one(SelectOption::new("a", rep("a", 101))),
        &[between(RuleId::CV2_09, &format!("{p}.value"), 1, 100, 101)],
    );
    fails_with(
        one(SelectOption::new("a", "a").description(rep("a", 101))),
        &[at_most(
            RuleId::CV2_09,
            &format!("{p}.description"),
            100,
            101,
        )],
    );
    passes(one(
        SelectOption::new(rep("a", 100), rep("a", 100)).description(rep("a", 100))
    ));
}

#[test]
fn cv2_09_min_max_values() {
    let p = "row[0].select[0]";
    fails_with(
        msg([select_row(user_select(pick()).min_values(26))]),
        &[between(
            RuleId::CV2_09,
            &format!("{p}.min_values"),
            0,
            25,
            26,
        )],
    );
    fails_with(
        msg([select_row(user_select(pick()).max_values(0))]),
        &[between(
            RuleId::CV2_09,
            &format!("{p}.max_values"),
            1,
            25,
            0,
        )],
    );
    fails_with(
        msg([select_row(user_select(pick()).max_values(26))]),
        &[between(
            RuleId::CV2_09,
            &format!("{p}.max_values"),
            1,
            25,
            26,
        )],
    );
    fails_with(
        msg([select_row(user_select(pick()).min_values(3).max_values(2))]),
        &[v(
            RuleId::CV2_09,
            p,
            Bound::Describe("min_values ≤ max_values"),
            Actual::Text("3 > 2".to_owned()),
        )],
    );
}

#[test]
fn cv2_09_default_values() {
    let path = "row[0].select[0].default_values";
    fails_with(
        msg([select_row(
            user_select(pick())
                .max_values(2)
                .default_users([Id::new(1), Id::new(2), Id::new(3)]),
        )]),
        &[between(RuleId::CV2_09, path, 1, 2, 3)],
    );
    passes(msg([select_row(
        user_select(pick())
            .max_values(2)
            .default_users([Id::new(1), Id::new(2)]),
    )]));
    fails_with(
        msg([select_row(string_select(
            pick(),
            [
                SelectOption::new("a", "a").default(true),
                SelectOption::new("b", "b").default(true),
            ],
        ))]),
        &[between(RuleId::CV2_09, path, 1, 1, 2)],
    );
}

#[test]
fn cv2_10_gallery_count() {
    let item = || MediaItem::new(image()).alt("x");
    assert_eq!(
        try_errs(MediaGallery::try_new([], &strict())),
        [between(RuleId::CV2_10, "", 1, 10, 0)]
    );
    assert_eq!(
        try_errs(MediaGallery::try_new((0..11).map(|_| item()), &strict())),
        [between(RuleId::CV2_10, "", 1, 10, 11)]
    );
    for n in [1, 10] {
        assert!(MediaGallery::try_new((0..n).map(|_| item()), &strict()).is_ok());
    }
    let eleven = MediaGallery::try_new((0..11).map(|_| item()), &relaxed())
        .expect("relaxed limits allow 11 items");
    fails_with(
        msg([eleven.into()]),
        &[between(RuleId::CV2_10, "gallery[0]", 1, 10, 11)],
    );
    passes(msg([MediaGallery::new([item()]).into()]));
    passes(msg([MediaGallery::new([(); 10].map(|()| item())).into()]));
}

#[test]
fn cv2_10_item_alt() {
    let gallery = |alt: String| msg([MediaGallery::new([MediaItem::new(image()).alt(alt)]).into()]);
    fails_with(
        gallery(rep("a", 1025)),
        &[at_most(
            RuleId::CV2_10,
            "gallery[0].item[0].description",
            1024,
            1025,
        )],
    );
    passes(gallery(rep("a", 1024)));
}

#[test]
fn cv2_11_thumbnail_alt() {
    fails_with(
        msg([Section::new([t("a")], Thumbnail::new(image()).alt(rep("a", 1025))).into()]),
        &[at_most(
            RuleId::CV2_11,
            "section[0].accessory.description",
            1024,
            1025,
        )],
    );
}

#[test]
fn cv2_17_media_url() {
    let gallery =
        |url: String| msg([MediaGallery::new([MediaItem::new(Media::url(url)).alt("x")]).into()]);
    let path = "gallery[0].item[0].url";
    fails_with(
        gallery(String::new()),
        &[between(RuleId::CV2_17, path, 1, 2048, 0)],
    );
    fails_with(
        gallery(rep("h", 2049)),
        &[between(RuleId::CV2_17, path, 1, 2048, 2049)],
    );
    passes(gallery(rep("h", 2048)));
}

#[test]
fn cv2_13_accent_white_passes() {
    passes(msg([Container::new()
        .accent(Rgb::new(255, 255, 255))
        .push(t("a"))
        .into()]));
}

/// Every component kind in its legal places: Ok, and so never CV2-05.
fn kitchen_sink() -> V2Message {
    let mut set = AttachmentSet::new();
    let file = set.add(Attachment::new(&b"report"[..], "txt").expect("valid ext"));
    msg([
        Container::new()
            .accent(Rgb::new(1, 2, 3))
            .push(t("a"))
            .push(Section::new([t("b")], b(1).label("Buy")))
            .push(Section::new([t("c")], Thumbnail::new(image()).alt("x")))
            .push(MediaGallery::new([MediaItem::new(image()).alt("y")]))
            .push(File::new(file))
            .push(noctis_ui::Separator::new())
            .push(ActionRow::buttons([
                Button::from(b(2)),
                Button::from(link("Docs")),
                Button::from(PremiumButton::new(Id::new(7))),
            ]))
            .into(),
        select_row(string_select(
            Pick { page: 1 },
            [SelectOption::new("a", "a")],
        )),
        select_row(role_select(Pick { page: 2 })),
        select_row(mentionable_select(Pick { page: 3 })),
        select_row(channel_select(Pick { page: 4 })),
        t("z").into(),
    ])
    .attachments(set)
}

#[test]
fn cv2_05_typed_trees_never_misplace() {
    passes(kitchen_sink());
    let mut m = texts(41, "a");
    let errs = errs(validate(&mut m, &strict()));
    assert!(errs.iter().all(|x| x.rule != RuleId::CV2_05));
}

// Matrix 28-33: CID-02 and CV2-14.

#[test]
fn cid_02_duplicate_in_row() {
    let id = custom_id(&b(1));
    let errs = errs_v2(msg([ActionRow::buttons([b(1), b(1)]).into()]), &strict());
    assert_eq!(
        errs,
        [v(
            RuleId::CID_02,
            "row[0].button[1].custom_id",
            Bound::Describe("a custom_id unique within the message"),
            Actual::Text(id),
        )
        .with_note("first used at row[0].button[0].custom_id")
        .with_help(HELP)]
    );
}

#[test]
fn cid_02_across_depth() {
    let m = msg([
        Section::new([t("a")], b(1)).into(),
        Container::new().push(ActionRow::buttons([b(1)])).into(),
    ]);
    let errs = errs_v2(m, &strict());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].path, "container[1].row[0].button[0].custom_id");
    assert_eq!(
        errs[0].notes,
        ["first used at section[0].accessory.custom_id"]
    );
}

#[test]
fn cid_02_triple() {
    let errs = errs_v2(
        msg([ActionRow::buttons([b(1), b(1), b(1)]).into()]),
        &strict(),
    );
    let paths: Vec<&str> = errs.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(
        paths,
        ["row[0].button[1].custom_id", "row[0].button[2].custom_id"]
    );
    for x in &errs {
        assert_eq!(x.rule, RuleId::CID_02);
        assert_eq!(x.notes, ["first used at row[0].button[0].custom_id"]);
        assert_eq!(x.help.as_deref(), Some(HELP));
    }
}

#[test]
fn cid_02_duplicate_select() {
    let m = resolve(msg([
        select_row(user_select(pick())),
        select_row(user_select(pick())),
    ]));
    let id = json(&m)["components"][0]["components"][0]["custom_id"]
        .as_str()
        .expect("resolved")
        .to_owned();
    assert_eq!(
        errs_v2(m, &strict()),
        [v(
            RuleId::CID_02,
            "row[1].select[0].custom_id",
            Bound::Describe("a custom_id unique within the message"),
            Actual::Text(id),
        )
        .with_note("first used at row[0].select[0].custom_id")
        .with_help(HELP)]
    );
}

#[test]
fn cid_02_distinct_ids_pass() {
    passes(msg([ActionRow::buttons([b(1), b(2)]).into()]));
}

#[test]
fn cid_02_unresolved_duplicates_are_reported() {
    let opt = || SelectOption::new("a", "a");
    let m = msg([
        select_row(string_select(pick(), [opt()])),
        select_row(string_select(pick(), [opt()])),
    ]);
    assert!(
        serde_json::to_value(m.create_body(CreateFlags::empty())).is_err(),
        "still unresolved"
    );
    assert_eq!(
        errs_v2(m, &strict()),
        [v(
            RuleId::CID_02,
            "row[1].select[0].custom_id",
            Bound::Describe("a custom_id unique within the message"),
            Actual::Text("route `pick`: equal Payload".to_owned()),
        )
        .with_note("first used at row[0].select[0].custom_id")
        .with_help(HELP)]
    );
}

#[test]
fn cid_02_unresolved_distinct_payloads_pass() {
    passes(msg([
        select_row(user_select(Pick { page: 1 })),
        select_row(user_select(Pick { page: 2 })),
    ]));
}

#[test]
fn cid_02_by_ref_duplicates() {
    let card = |id| button(Card { id });
    passes(msg([ActionRow::buttons([card(1), card(2)]).into()]));
    let errs = errs_v2(
        msg([ActionRow::buttons([card(1), card(1)]).into()]),
        &strict(),
    );
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].rule, RuleId::CID_02);
    assert_eq!(errs[0].path, "row[0].button[1].custom_id");
    assert_eq!(
        errs[0].actual,
        Actual::Text("route `card`: equal Payload".to_owned())
    );
}

#[test]
fn cid_02_signed_duplicates_without_a_key() {
    let vote = |n| button(Vote { n });
    passes(msg([ActionRow::buttons([vote(1), vote(2)]).into()]));
    let errs = errs_v2(
        msg([ActionRow::buttons([vote(1), vote(1)]).into()]),
        &strict(),
    );
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].path, "row[0].button[1].custom_id");
    assert_eq!(errs[0].notes, ["first used at row[0].button[0].custom_id"]);
    assert_eq!(errs[0].help.as_deref(), Some(HELP));
}

#[test]
fn cid_02_help_suggests_a_distinguishing_field() {
    let page = |page| button(Page { page });
    let errs = errs_v2(
        msg([ActionRow::buttons([page(1), page(1)]).into()]),
        &strict(),
    );
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].notes, ["first used at row[0].button[0].custom_id"]);
    let shown = errs[0].to_string();
    assert!(shown.ends_with(&format!("\n  = help: {HELP}")), "{shown}");
    passes(msg([ActionRow::buttons([page(2), page(4)]).into()]));
}

fn collect_ids(value: &Value, out: &mut Vec<u64>) {
    match value {
        Value::Object(map) => {
            if let Some(id) = map.get("id").and_then(Value::as_u64) {
                out.push(id);
            }
            for v in map.values() {
                collect_ids(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_ids(v, out)),
        _ => {}
    }
}

#[test]
fn cv2_14_ids_are_unique_by_construction() {
    let container = (0..39).fold(Container::new(), |c, _| c.push(t("a")));
    let m = msg([container.into()]);
    passes(m.clone());
    let mut ids = Vec::new();
    collect_ids(&json(&m)["components"], &mut ids);
    assert_eq!(ids, (1..=40).collect::<Vec<_>>());
}

// Matrix 34-39: ATT-05 and ATT-06.

fn att(bytes: &'static [u8]) -> Attachment {
    Attachment::new(bytes, "png").expect("valid ext")
}

#[test]
fn att_05_file_of_this_message_passes() {
    let mut set = AttachmentSet::new();
    let r = set.add(att(b"a"));
    passes(msg([File::new(r).into()]).attachments(set));
}

#[test]
fn att_05_file_of_another_set() {
    let mut set = AttachmentSet::new();
    set.add(att(b"a"));
    let other = AttachmentSet::new().add(att(b"b"));
    let url = other.url().to_owned();
    fails_with(
        msg([File::new(other).into()]).attachments(set),
        &[v(
            RuleId::ATT_05,
            "file[0].url",
            Bound::Describe("attachment://<name> of a file in this message"),
            Actual::Text(url),
        )],
    );
}

#[test]
fn att_05_media_url_names_no_file() {
    for url in ["attachment://x.png", "attachment://"] {
        let m = msg([Section::new([t("a")], Thumbnail::new(Media::url(url)).alt("x")).into()]);
        let errs = errs_v2(m, &strict());
        assert_eq!(errs.len(), 1, "{url}");
        assert_eq!(
            (errs[0].rule, errs[0].path.as_str()),
            (RuleId::ATT_05, "section[0].accessory.url")
        );
    }
}

#[test]
fn att_05_other_urls_are_not_references() {
    for url in ["https://x.y/a.png", "ATTACHMENT://a"] {
        passes(msg([MediaGallery::new([
            MediaItem::new(Media::url(url)).alt("x")
        ])
        .into()]));
    }
}

#[test]
fn att_05_same_bytes_from_another_set_pass() {
    let mut set = AttachmentSet::new();
    set.add(att(b"same"));
    let other = AttachmentSet::new().add(att(b"same"));
    passes(msg([File::new(other).into()]).attachments(set));
}

#[test]
fn att_06_attachment_alt() {
    let with_alt = |alt: String| {
        let mut set = AttachmentSet::new();
        let r = set.add(att(b"a").alt(alt));
        (msg([t("x").into()]).attachments(set), r.name().to_owned())
    };
    let (m, name) = with_alt(rep("a", 1025));
    assert_eq!(
        errs_v2(m, &strict()),
        [
            at_most(RuleId::ATT_06, "attachments[0].description", 1024, 1025)
                .with_note(format!("file `{name}`"))
        ]
    );
    passes(with_alt(rep("a", 1024)).0);
}

// Matrix 40-45: legacy messages, MSG-01, MSG-04, MSG-09, ordering.

fn legacy_errs(mut m: LegacyMessage, limits: &Limits) -> Vec<Violation> {
    errs(validate(&mut m, limits))
}

#[test]
fn msg_01_legacy_content() {
    validate(&mut LegacyMessage::new(rep("a", 2000)), &strict()).expect("2000 fits");
    assert_eq!(
        legacy_errs(LegacyMessage::new(rep("a", 2001)), &strict()),
        [at_most(RuleId::MSG_01, "content", 2000, 2001)]
    );
    assert_eq!(
        legacy_errs(LegacyMessage::new(rep("😀", 1001)), &strict()),
        [at_most(RuleId::MSG_01, "content", 2000, 2002)]
    );
}

#[test]
fn msg_04_mention_ids() {
    let users = |n: u64| Users::only((1..=n).map(Id::new));
    passes(texts(1, "a").mentions(Mentions::none().users(users(100))));
    fails_with(
        texts(1, "a").mentions(Mentions::none().users(users(101))),
        &[at_most(RuleId::MSG_04, "allowed_mentions.users", 100, 101)],
    );
    fails_with(
        texts(1, "a").mentions(Mentions::none().roles(Roles::only((1..=101).map(Id::new)))),
        &[at_most(RuleId::MSG_04, "allowed_mentions.roles", 100, 101)],
    );
    assert_eq!(
        legacy_errs(
            LegacyMessage::new("x").mentions(Mentions::none().users(users(101))),
            &strict()
        ),
        [at_most(RuleId::MSG_04, "allowed_mentions.users", 100, 101)]
    );
}

#[test]
fn msg_09_v2_empty() {
    fails_with(
        V2Message::new(Components::new()),
        &[v(
            RuleId::MSG_09,
            "components",
            Bound::AtLeast(1),
            Actual::Count(0),
        )],
    );
}

#[test]
fn msg_09_legacy_empty() {
    for content in ["", "  \n"] {
        assert_eq!(
            legacy_errs(LegacyMessage::new(content), &strict()),
            [v(
                RuleId::MSG_09,
                "content",
                Bound::Describe("non-empty content or at least one Action Row"),
                Actual::Text(content.to_owned()),
            )]
        );
    }
    let row = ActionRow::buttons([link("a")]);
    validate(&mut LegacyMessage::new("").rows([row]), &strict()).expect("a row is a body");
    validate(&mut LegacyMessage::new("x"), &strict()).expect("content is a body");
}

#[test]
fn cv2_15_legacy_rows() {
    let row = |n: u64| ActionRow::buttons([b(n)]);
    validate(
        &mut LegacyMessage::new("x").rows([row(1), row(2), row(3), row(4), row(5)]),
        &strict(),
    )
    .expect("5 rows");
    assert_eq!(
        legacy_errs(LegacyMessage::new("x").rows([six_buttons()]), &strict()),
        [between(RuleId::CV2_06, "row[0]", 1, 5, 6)]
    );
    let mut l = strict();
    l.legacy_action_rows = 2;
    assert_eq!(
        legacy_errs(LegacyMessage::new("x").rows([row(1), row(2), row(3)]), &l),
        [at_most(RuleId::CV2_15, "components", 2, 3)]
    );
    assert_eq!(
        legacy_errs(
            LegacyMessage::new("x").rows([ActionRow::buttons([b(1), b(1)])]),
            &strict()
        )[0]
        .path,
        "row[0].button[1].custom_id"
    );
}

#[test]
fn cv2_03_many_violations_in_order() {
    let mut set = AttachmentSet::new();
    set.add(att(b"a").alt(rep("a", 1025)));
    let foreign = AttachmentSet::new().add(att(b"b"));
    let m = msg((0..41).map(|_| t("a").into()).chain([
        ActionRow::buttons([b(1), b(1)]).into(),
        File::new(foreign).into(),
    ]))
    .attachments(set)
    .mentions(Mentions::none().users(Users::only((1..=101).map(Id::new))));
    let rules: Vec<RuleId> = errs_v2(m, &strict()).iter().map(|x| x.rule).collect();
    assert_eq!(
        rules,
        [
            RuleId::CV2_03,
            RuleId::CID_02,
            RuleId::ATT_05,
            RuleId::ATT_06,
            RuleId::MSG_04
        ]
    );
    let legacy = LegacyMessage::new(rep("a", 2001))
        .rows([six_buttons()])
        .mentions(Mentions::none().roles(Roles::only((1..=101).map(Id::new))));
    let rules: Vec<RuleId> = legacy_errs(legacy, &strict())
        .iter()
        .map(|x| x.rule)
        .collect();
    assert_eq!(rules, [RuleId::CV2_06, RuleId::MSG_01, RuleId::MSG_04]);
}

// Matrix 46-55: Strict and Lenient.

/// A resolved message with every truncatable field over its limit.
fn over_long() -> V2Message {
    let mut set = AttachmentSet::new();
    set.add(att(b"a").alt(rep("a", 1025)));
    resolve(
        msg([
            select_row(
                string_select(
                    pick(),
                    [SelectOption::new(rep("l", 101), "v").description(rep("d", 101))],
                )
                .placeholder(rep("p", 151)),
            ),
            MediaGallery::new([MediaItem::new(image()).alt(rep("g", 1025))]).into(),
            Section::new([t("s")], Thumbnail::new(image()).alt(rep("t", 1025))).into(),
            row_of(b(1).label(rep("a", 81))),
        ])
        .attachments(set),
    )
}

#[test]
fn cv2_08_strict_never_mutates() {
    for m in [over_long(), kitchen_sink(), texts(41, &rep("a", 100))] {
        let mut m = resolve(m);
        let before = json(&m);
        let _ = validate(&mut m, &strict());
        assert_eq!(json(&m), before);
    }
}

#[test]
fn cv2_08_lenient_label() {
    let mut m = msg([row_of(b(1).label(rep("a", 81)))]);
    let report = validate(&mut m, &lenient()).expect("truncated to fit");
    let label = json(&m)["components"][0]["components"][0]["label"].clone();
    assert_eq!(label, Value::from(format!("{}…", rep("a", 79))));
    assert_eq!(discord_len(label.as_str().unwrap_or_default()), 80);
    assert_eq!(
        report.warnings,
        [truncated(RuleId::CV2_08, "row[0].button[0].label", 80, 81)]
    );
}

#[test]
fn cv2_08_lenient_accessory_label() {
    let mut m = msg([Container::new()
        .push(t("a"))
        .push(t("b"))
        .push(Section::new([t("c")], b(1).label(rep("a", 81))))
        .into()]);
    let report = validate(&mut m, &lenient()).expect("truncated to fit");
    assert_eq!(
        report.warnings,
        [truncated(
            RuleId::CV2_08,
            "container[0].section[2].accessory.label",
            80,
            81
        )]
    );
    let label = json(&m)["components"][0]["components"][2]["accessory"]["label"].clone();
    assert_eq!(label, Value::from(format!("{}…", rep("a", 79))));
}

#[test]
fn cv2_09_lenient_fields_in_pre_order() {
    let mut m = over_long();
    let name = m
        .attachment_set()
        .iter()
        .next()
        .map(|a| a.name().to_owned());
    let report = validate(&mut m, &lenient()).expect("truncated to fit");
    let note = format!("file `{}`", name.unwrap_or_default());
    assert_eq!(
        report.warnings,
        [
            truncated(RuleId::CV2_09, "row[0].select[0].placeholder", 150, 151),
            truncated(
                RuleId::CV2_09,
                "row[0].select[0].options[0].label",
                100,
                101
            ),
            truncated(
                RuleId::CV2_09,
                "row[0].select[0].options[0].description",
                100,
                101
            ),
            truncated(RuleId::CV2_10, "gallery[1].item[0].description", 1024, 1025),
            truncated(
                RuleId::CV2_11,
                "section[2].accessory.description",
                1024,
                1025
            ),
            truncated(RuleId::CV2_08, "row[3].button[0].label", 80, 81),
            LimitWarning::Truncated(
                at_most(RuleId::ATT_06, "attachments[0].description", 1024, 1025).with_note(note)
            ),
        ]
    );
    let j = json(&m);
    let len = |v: &Value| discord_len(v.as_str().unwrap_or_default());
    let select = &j["components"][0]["components"][0];
    assert_eq!(len(&select["placeholder"]), 150);
    assert_eq!(len(&select["options"][0]["label"]), 100);
    assert_eq!(len(&select["options"][0]["description"]), 100);
    assert_eq!(len(&j["components"][1]["items"][0]["description"]), 1024);
    assert_eq!(len(&j["components"][2]["accessory"]["description"]), 1024);
    let alt = m.attachment_set().iter().next().and_then(|a| a.alt_text());
    assert_eq!(alt.map(discord_len), Some(1024));
    assert!(alt.is_some_and(|a| a.ends_with('…')));

    let mut legacy = LegacyMessage::new(rep("a", 2001));
    let report = validate(&mut legacy, &lenient()).expect("truncated to fit");
    assert_eq!(
        report.warnings,
        [truncated(RuleId::MSG_01, "content", 2000, 2001)]
    );
    assert_eq!(discord_len(legacy.content_text()), 2000);
    assert_eq!(report.text_chars, 2000);
    assert!(legacy_json(&legacy)["content"].is_string());

    let mut legacy =
        LegacyMessage::new(rep("a", 2001)).rows([ActionRow::buttons([link(rep("a", 81))])]);
    let report = validate(&mut legacy, &lenient()).expect("truncated to fit");
    assert_eq!(
        report.warnings,
        [
            truncated(RuleId::CV2_08, "row[0].button[0].label", 80, 81),
            truncated(RuleId::MSG_01, "content", 2000, 2001),
        ]
    );
}

#[test]
fn cv2_08_lenient_emoji_cut() {
    let mut m = msg([row_of(link(rep("😀", 41)))]);
    validate(&mut m, &lenient()).expect("truncated to fit");
    assert_eq!(
        json(&m)["components"][0]["components"][0]["label"],
        Value::from(format!("{}…", rep("😀", 39)))
    );
}

#[test]
fn cv2_04_lenient_total() {
    let mut m =
        msg(std::iter::once(t(rep("a", 3990)).into())
            .chain((0..20).map(|_| t("bbbbbbbbbb").into())));
    let report = validate(&mut m, &lenient()).expect("truncated to fit");
    let j = json(&m);
    let contents: Vec<&str> = (0..21)
        .map(|i| j["components"][i]["content"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(contents[0], format!("{}…", rep("a", 3979)));
    assert!(contents[1..].iter().all(|c| *c == "…"));
    assert_eq!(contents.iter().map(|c| discord_len(c)).sum::<usize>(), 4000);
    assert_eq!(report.text_chars, 4000);
    let mut want = at_most(RuleId::CV2_04, "components", 4000, 4190);
    want.notes = (0..21).map(|i| format!("truncated text[{i}]")).collect();
    assert_eq!(report.warnings, [LimitWarning::Truncated(want)]);
}

#[test]
fn cv2_04_lenient_nested_paths() {
    let mut l = lenient();
    l.text_total_len = 4;
    let mut m = msg([
        Container::new().push(t("abcd")).into(),
        Section::new([t("ef")], link("x")).into(),
    ]);
    let report = validate(&mut m, &l).expect("truncated to fit");
    let LimitWarning::Truncated(w) = &report.warnings[0] else {
        panic!("a truncation");
    };
    assert_eq!(
        w.notes,
        [
            "truncated container[0].text[0]",
            "truncated section[1].text[0]"
        ]
    );
    assert_eq!(report.text_chars, 4);
}

#[test]
fn cv2_04_lenient_infeasible() {
    let mut l = lenient();
    l.text_total_len = 3;
    let mut m = texts(4, "ab");
    let before = json(&m);
    let errs = errs(validate(&mut m, &l));
    assert_eq!(errs, [at_most(RuleId::CV2_04, "components", 3, 8)]);
    assert_eq!(json(&m), before);
}

#[test]
fn cv2_03_lenient_structural_still_fails() {
    let cases: Vec<fn() -> V2Message> = vec![
        || texts(41, "a"),
        || {
            let mut n = 0;
            msg([(0..8)
                .fold(Container::new(), |c, _| {
                    c.push(ActionRow::buttons([(); 5].map(|()| {
                        n += 1;
                        b(n)
                    })))
                })
                .into()])
        },
        || msg([six_buttons().into()]),
        || msg([ActionRow::buttons([b(1), b(1)]).into()]),
        || texts(1, "a").mentions(Mentions::none().users(Users::only((1..=101).map(Id::new)))),
        || V2Message::new(Components::new()),
        || msg([row_of(LinkButton::new(rep("h", 513)))]),
        || msg([MediaGallery::new([MediaItem::new(Media::url(rep("h", 2049))).alt("x")]).into()]),
        || {
            msg([select_row(string_select(
                pick(),
                [SelectOption::new("a", rep("v", 101))],
            ))])
        },
    ];
    for case in cases {
        let s = errs_v2(case(), &strict());
        let l = errs_v2(case(), &lenient());
        assert_eq!(s, l);
    }
}

#[test]
fn cv2_03_lenient_partial_mutation() {
    let mut m = msg((0..40)
        .map(|_| t("a").into())
        .chain([row_of(link(rep("a", 81)))]));
    let errs = errs(validate(&mut m, &lenient()));
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].rule, RuleId::CV2_03);
    let label = json(&m)["components"][40]["components"][0]["label"].clone();
    assert_eq!(discord_len(label.as_str().unwrap_or_default()), 80);
}

#[test]
fn cv2_04_lenient_within_limits() {
    let mut m = resolve(kitchen_sink());
    let before = json(&m);
    let report = validate(&mut m, &lenient()).expect("valid");
    assert!(report.warnings.is_empty());
    assert_eq!(json(&m), before);
}

#[test]
fn cv2_08_lenient_limit_zero() {
    let mut l = lenient();
    l.button_label_len = 0;
    let mut m = msg([row_of(link("ab"))]);
    let report = validate(&mut m, &l).expect("truncated to fit");
    assert_eq!(
        json(&m)["components"][0]["components"][0]["label"],
        Value::from("")
    );
    assert_eq!(
        report.warnings,
        [truncated(RuleId::CV2_08, "row[0].button[0].label", 0, 2)]
    );
}

// Matrix 56-58: missing alt text.

#[test]
fn cv2_11_missing_alt_warns() {
    for alt in ["  ", ""] {
        let m = msg([
            Section::new([t("a")], Thumbnail::new(image())).into(),
            MediaGallery::new([MediaItem::new(Media::url("https://x.y/b.png")).alt(alt)]).into(),
        ]);
        let want = [
            LimitWarning::MissingAlt {
                path: "section[0].accessory".to_owned(),
            },
            LimitWarning::MissingAlt {
                path: "gallery[1].item[0]".to_owned(),
            },
        ];
        for limits in [strict(), lenient()] {
            let report = validate(&mut m.clone(), &limits).expect("warnings only");
            assert_eq!(report.warnings, want);
        }
        assert_eq!(cost(&m, &strict()).warnings, want);
    }
}

#[test]
fn cv2_11_rendered_image_needs_no_alt() {
    let mut set = AttachmentSet::new();
    let r = set.add(Attachment::rendered(Bytes::new(), [0xAB; 32], "webp").expect("valid ext"));
    let report = passes(msg([Section::new([t("a")], Thumbnail::new(r)).into()]).attachments(set));
    assert!(report.warnings.is_empty());
}

#[test]
fn cv2_10_plain_attachment_without_alt_warns() {
    let mut set = AttachmentSet::new();
    let r = set.add(att(b"a").alt("file alt does not count"));
    let report =
        passes(msg([MediaGallery::new([MediaItem::new(Media::from(r))]).into()]).attachments(set));
    assert_eq!(
        report.warnings,
        [LimitWarning::MissingAlt {
            path: "gallery[0].item[0]".to_owned()
        }]
    );
}

// Matrix 59: logging.

#[derive(Debug, Clone)]
struct Captured {
    level: tracing::Level,
    target: String,
    fields: Vec<(String, String)>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Captured>>>);

struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields(Vec::new());
        event.record(&mut fields);
        let captured = Captured {
            level: *event.metadata().level(),
            target: event.metadata().target().to_owned(),
            fields: fields.0,
        };
        self.0.lock().expect("not poisoned").push(captured);
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn cv2_08_warnings_are_logged() {
    let capture = Capture::default();
    let m = msg([
        row_of(link(rep("a", 81))),
        Section::new([t("a")], Thumbnail::new(image())).into(),
    ]);
    // With a single registered dispatcher, tracing-core computes a callsite's
    // interest from the *registering thread's* default, so a parallel test
    // with no subscriber that first hits the `warn!` callsite while ours is
    // active (it raises the global max level) caches `Interest::never`. A
    // second live dispatcher makes interest come from every registered one.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let report = tracing::subscriber::with_default(capture.clone(), || {
        // Callsites registered before `with_default` keep a stale interest.
        tracing::callsite::rebuild_interest_cache();
        let report = validate(&mut m.clone(), &lenient()).expect("warnings only");
        let _ = cost(&m, &lenient());
        report
    });
    let events = capture.0.lock().expect("not poisoned").clone();
    assert_eq!(events.len(), report.warnings.len());
    assert_eq!(events.len(), 2);
    for (event, warning) in events.iter().zip(&report.warnings) {
        assert_eq!(event.level, tracing::Level::WARN);
        assert_eq!(event.target, "noctis::ui");
        let field = |name: &str| {
            event
                .fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            field("rule"),
            Some(warning.rule().map_or("", RuleId::as_str).to_owned())
        );
        assert_eq!(field("path"), Some(warning.path().to_owned()));
        assert_eq!(field("message"), Some(warning.to_string()));
    }
    let rule = |e: &Captured| {
        e.fields
            .iter()
            .find(|(n, _)| n == "rule")
            .map(|(_, v)| v.clone())
    };
    assert_eq!(rule(&events[0]).as_deref(), Some("CV2-08"));
    assert_eq!(rule(&events[1]).as_deref(), Some(""));
}

// Matrix 60-64: cost and Display.

#[test]
fn cv2_03_cost_of_legacy() {
    let row = ActionRow::buttons([link("a"), link("b")]);
    let report = cost(&LegacyMessage::new("héllo").rows([row]), &strict());
    assert_eq!(
        (report.components, report.text_chars, report.warnings.len()),
        (3, 5, 0)
    );
}

#[test]
fn cv2_08_cost_never_mutates() {
    let m = msg([
        row_of(b(1).label(rep("a", 81))),
        Section::new([t("a")], Thumbnail::new(image())).into(),
    ]);
    let before = json(&m);
    let report = cost(&m, &lenient());
    assert_eq!(json(&m), before);
    assert_eq!(
        report.warnings,
        [LimitWarning::MissingAlt {
            path: "section[1].accessory".to_owned()
        }]
    );
}

#[test]
fn cid_02_cost_with_unresolved_ids() {
    let mut m = msg([select_row(user_select(pick()))]);
    assert_eq!(cost(&m, &strict()).components, 2);
    assert_eq!(validate(&mut m, &strict()).map(|r| r.components), Ok(2));
}

#[test]
fn cv2_03_limits_error_display() {
    let a = at_most(RuleId::CV2_03, "components", 40, 41);
    let b = at_most(RuleId::MSG_04, "allowed_mentions.users", 100, 101);
    let err = UiError::Limits {
        violations: vec![a.clone(), b.clone()],
    };
    assert_eq!(err.to_string(), format!("{a}\n{b}"));
}

#[test]
fn cv2_08_warning_display() {
    let x = at_most(RuleId::CV2_08, "row[0].button[0].label", 80, 81);
    assert_eq!(
        LimitWarning::Truncated(x.clone()).to_string(),
        format!("truncated to fit: {x}")
    );
    let alt = LimitWarning::MissingAlt {
        path: "section[0].accessory".to_owned(),
    };
    assert_eq!(
        alt.to_string(),
        "section[0].accessory: image has no alt text"
    );
    assert_eq!(alt.path(), "section[0].accessory");
    assert_eq!(alt.rule(), None);
}

#[test]
fn types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync + Clone + fmt::Debug>() {}
    assert_send_sync::<CostReport>();
    assert_send_sync::<LimitWarning>();
    assert_send_sync::<UiError>();
}

// Story 4.4: checked literals and the validator backstop.

#[test]
fn cv2_08_label_runtime_path() {
    use noctis_ui::Label;
    assert_eq!(
        Label::try_from(rep("a", 80)).map(|l| l.as_str().len()),
        Ok(80)
    );
    assert_eq!(
        try_errs(Label::try_from(rep("a", 81))),
        [at_most(RuleId::CV2_08, "", 80, 81)]
    );
    assert_eq!(
        try_errs(Label::try_from(rep("😀", 41))),
        [at_most(RuleId::CV2_08, "", 80, 82)]
    );
    let mut l = lenient();
    l.button_label_len = 3;
    // Never truncates, even in Lenient mode.
    assert_eq!(
        try_errs(Label::try_new("abcd", &l)),
        [at_most(RuleId::CV2_08, "", 3, 4)]
    );
    let label = Label::try_from("Save").expect("4 characters");
    passes(msg([row_of(b(1).label(label))]));
}

#[test]
fn cv2_09_placeholder_runtime_path() {
    use noctis_ui::Placeholder;
    assert!(Placeholder::try_from(rep("a", 150)).is_ok());
    assert_eq!(
        try_errs(Placeholder::try_from(rep("a", 151))),
        [at_most(RuleId::CV2_09, "", 150, 151)]
    );
    let mut l = strict();
    l.select_placeholder_len = 3;
    assert_eq!(
        try_errs(Placeholder::try_new("abcd", &l)),
        [at_most(RuleId::CV2_09, "", 3, 4)]
    );
}

#[test]
fn msg_01_content_runtime_path() {
    use noctis_ui::Content;
    assert!(Content::try_from(rep("a", 2000)).is_ok());
    assert!(Content::try_from("").is_ok());
    assert_eq!(
        try_errs(Content::try_from(rep("a", 2001))),
        [at_most(RuleId::MSG_01, "content", 2000, 2001)]
    );
    assert_eq!(
        try_errs(Content::try_from(rep("😀", 1001))),
        [at_most(RuleId::MSG_01, "content", 2000, 2002)]
    );
    let mut m = LegacyMessage::new(Content::try_from("hi").expect("2 characters"));
    validate(&mut m, &strict()).expect("valid content");
}

#[test]
fn cv2_13_rgb_runtime_path() {
    assert_eq!(Rgb::try_from(0x00FF_FFFF), Ok(Rgb::new(255, 255, 255)));
    for bad in [0x0100_0000, u32::MAX] {
        let errs = try_errs(Rgb::try_from(bad));
        assert_eq!(errs.len(), 1);
        assert_eq!((errs[0].rule, errs[0].path.as_str()), (RuleId::CV2_13, ""));
        assert_eq!(
            errs[0].limit,
            Bound::Describe("a colour from 0x000000 to 0xFFFFFF")
        );
    }
    assert_eq!(
        try_errs(Rgb::try_from(0x0100_0000))[0].actual,
        Actual::Text("0x1000000".to_owned())
    );
}
