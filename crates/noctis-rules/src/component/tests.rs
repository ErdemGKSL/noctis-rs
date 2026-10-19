use super::*;
use proptest::prelude::*;

use Component::{Opaque as O, Text as T};

const GO: Button<'static> = Button::new(ButtonStyle::Primary).label("Go");
const B: Component<'static> = Component::Button(GO);
const U: Component<'static> = Component::Select(Select::new(SelectKind::User));
const SP: Component<'static> = Component::Separator(Separator::new());

fn check(components: &[Component<'_>]) -> Vec<Violation> {
    check_with(components, &Limits::default())
}

fn check_with(components: &[Component<'_>], limits: &Limits) -> Vec<Violation> {
    match validate_components(components, limits) {
        Ok(()) => Vec::new(),
        Err(errs) => {
            assert!(!errs.is_empty());
            errs
        }
    }
}

fn legacy(components: &[Component<'_>]) -> Vec<Violation> {
    legacy_with(components, &Limits::default())
}

fn legacy_with(components: &[Component<'_>], limits: &Limits) -> Vec<Violation> {
    match validate_legacy_components(components, limits) {
        Ok(()) => Vec::new(),
        Err(errs) => {
            assert!(!errs.is_empty());
            errs
        }
    }
}

fn rules(errs: &[Violation]) -> Vec<RuleId> {
    errs.iter().map(|v| v.rule).collect()
}

fn paths(errs: &[Violation]) -> Vec<&str> {
    errs.iter().map(|v| v.path.as_str()).collect()
}

fn v(rule: RuleId, path: &str, limit: Bound, actual: Actual) -> Violation {
    Violation::new(rule, path, limit, actual)
}

fn btw(min: usize, max: usize) -> Bound {
    Bound::Between { min, max }
}

fn cnt(n: usize) -> Actual {
    Actual::Count(n)
}

fn txt(s: &str) -> Actual {
    Actual::Text(s.to_owned())
}

fn lowered(f: impl FnOnce(&mut Limits)) -> Limits {
    let mut l = Limits::default();
    f(&mut l);
    l
}

fn row(items: &[Component<'_>]) -> Vec<Violation> {
    check(&[Component::Row(items)])
}

fn select(s: Select<'_>) -> Vec<Violation> {
    check(&[Component::Row(&[Component::Select(s)])])
}

fn button(b: Button<'_>) -> Vec<Violation> {
    check(&[Component::Row(&[Component::Button(b)])])
}

const SEL: &str = "row[0].select[0]";

type Granular = fn(&str, &Limits) -> Vec<Violation>;

fn p(s: &str) -> Button<'_> {
    Button::new(ButtonStyle::Primary).label(s)
}

#[test]
fn cv2_03_empty_is_ok() {
    assert_eq!(validate_components(&[], &Limits::default()), Ok(()));
}

#[test]
fn cv2_05_ac_valid_tree() {
    let items = [B; 5];
    let gallery = [Media::new("https://a/x.png")];
    let texts = ["a"];
    let children = [
        T("# Hi"),
        SP,
        Component::Row(&items),
        Component::Gallery(&gallery),
        Component::Section(Section::new(&texts, Accessory::Button(GO))),
        Component::File("attachment://a.png"),
    ];
    let tree = [Component::Container(
        Container::new(&children).accent_color(0x0058_65F2),
    )];
    assert_eq!(check(&tree), []);
}

#[test]
fn cv2_03_total_components() {
    let texts = vec![T("a"); 41];
    assert_eq!(check(&texts[..40]), []);
    let errs = check(&texts);
    assert_eq!(rules(&errs), [RuleId::CV2_03]);
    assert_eq!(
        (&errs[0].path, &errs[0].limit, &errs[0].actual),
        (&"components".to_owned(), &Bound::AtMost(40), &cnt(41))
    );
    let notes: Vec<String> = (0..41).map(|i| format!("text[{i}]: 1")).collect();
    assert_eq!(errs[0].notes, notes);
    let l = lowered(|l| l.components = 10);
    let errs = check_with(&texts[..11], &l);
    assert_eq!(
        (&errs[0].limit, &errs[0].actual),
        (&Bound::AtMost(10), &cnt(11))
    );
}

#[test]
fn cv2_03_cv2_16_nested_and_opaque() {
    assert_eq!(check(&[O(39), T("a")]), []);
    let errs = check(&[O(40), T("a")]);
    assert_eq!(rules(&errs), [RuleId::CV2_03]);
    assert_eq!(errs[0].actual, cnt(41));
    assert_eq!(errs[0].notes, ["opaque[0]: 40", "text[1]: 1"]);
    let l = Limits::default();
    assert_eq!(cost(&[O(0)], &l).components, 1);
    let inner_children = [T("a")];
    let inner = [Component::Container(Container::new(&inner_children))];
    let tree = [Component::Container(Container::new(&inner))];
    assert_eq!(cost(&tree, &l).components, 3);
    assert_eq!(rules(&check(&tree)), [RuleId::CV2_05]);
}

#[test]
fn cv2_04_text_budget() {
    let a4000 = "a".repeat(4000);
    let a4001 = "a".repeat(4001);
    let e2000 = "😀".repeat(2000);
    let e2001 = "😀".repeat(2001);
    let budget = |n| v(RuleId::CV2_04, "components", Bound::AtMost(4000), cnt(n));
    assert_eq!(check(&[T(&a4000)]), []);
    assert_eq!(check(&[T(&a4001)]), [budget(4001)]);
    assert_eq!(check(&[T(&e2000)]), []);
    assert_eq!(check(&[T(&e2001)]), [budget(4002)]);

    let a2000 = "a".repeat(2000);
    let a1000 = "a".repeat(1000);
    let a1001 = "a".repeat(1001);
    let in_container = [T(&a1000)];
    let texts = [a1001.as_str()];
    let tree = [
        T(&a2000),
        Component::Container(Container::new(&in_container)),
        Component::Section(Section::new(&texts, Accessory::Button(GO))),
    ];
    assert_eq!(check(&tree), [budget(4001)]);

    let a1024 = "a".repeat(1024);
    let a80 = "a".repeat(80);
    let gallery = [Media::new("u").description(&a1024)];
    let items = [Component::Button(
        Button::new(ButtonStyle::Primary).label(&a80),
    )];
    let tree = [
        T(&a4000),
        Component::Gallery(&gallery),
        Component::Row(&items),
    ];
    assert_eq!(check(&tree), []);

    let misplaced = [T("ab")];
    let tree = [T(&a4000[..3999]), Component::Row(&misplaced)];
    let errs = check(&tree);
    assert_eq!(rules(&errs), [RuleId::CV2_04, RuleId::CV2_05]);
    assert_eq!(errs[0].actual, cnt(4001));

    let l = lowered(|l| l.text_total_len = 10);
    assert_eq!(
        check_with(&[T("abcdefghijk")], &l),
        [v(RuleId::CV2_04, "components", Bound::AtMost(10), cnt(11))]
    );
}

#[test]
fn cv2_05_placement() {
    let thumb = Media::new("https://a/x.png");
    assert_eq!(
        check(&[Component::Thumbnail(thumb)]),
        [v(
            RuleId::CV2_05,
            "thumbnail[0]",
            Bound::Describe("a Thumbnail only as a Section accessory"),
            txt("thumbnail")
        )]
    );
    let button_rule = Bound::Describe("a Button only in an Action Row or as a Section accessory");
    assert_eq!(
        check(&[B]),
        [v(
            RuleId::CV2_05,
            "button[0]",
            button_rule.clone(),
            txt("button")
        )]
    );
    assert_eq!(
        check(&[U]),
        [v(
            RuleId::CV2_05,
            "select[0]",
            Bound::Describe("a select only in an Action Row"),
            txt("select")
        )]
    );
    let inner_children = [T("a")];
    let inner = [Component::Container(Container::new(&inner_children))];
    assert_eq!(
        check(&[Component::Container(Container::new(&inner))]),
        [v(
            RuleId::CV2_05,
            "container[0].container[0]",
            Bound::Describe("no Container inside a Container"),
            txt("container")
        )]
    );
    let with_button = [B];
    assert_eq!(
        check(&[Component::Container(Container::new(&with_button))]),
        [v(
            RuleId::CV2_05,
            "container[0].button[0]",
            button_rule,
            txt("button")
        )]
    );
    let row_rule = Bound::Describe("only buttons and selects in an Action Row");
    assert_eq!(
        row(&[T("x")]),
        [v(
            RuleId::CV2_05,
            "row[0].text[0]",
            row_rule.clone(),
            txt("text")
        )]
    );
    let inner_six = [B; 6];
    let errs = row(&[Component::Row(&inner_six)]);
    assert_eq!(
        errs[0],
        v(RuleId::CV2_05, "row[0].row[0]", row_rule, txt("row"))
    );
    assert_eq!(rules(&errs), [RuleId::CV2_05, RuleId::CV2_06]);
    assert_eq!(errs[1].path, "row[0].row[0]");

    let opaque_in = [O(1)];
    assert_eq!(check(&[O(1)]), []);
    assert_eq!(
        check(&[Component::Container(Container::new(&opaque_in))]),
        []
    );
    assert_eq!(row(&opaque_in), []);
}

#[test]
fn cv2_06_action_rows() {
    let shape = |n| v(RuleId::CV2_06, "row[0]", btw(1, 5), cnt(n));
    let alone = |n| {
        v(
            RuleId::CV2_06,
            "row[0]",
            Bound::Describe("exactly one select, alone in its row"),
            cnt(n),
        )
    };
    assert_eq!(row(&[]), [shape(0)]);
    assert_eq!(row(&[B; 5]), []);
    assert_eq!(row(&[B; 6]), [shape(6)]);
    assert_eq!(row(&[U]), []);
    assert_eq!(row(&[U, U]), [alone(2)]);
    assert_eq!(row(&[U, B]), [alone(2)]);
    assert_eq!(row(&[B, O(1)]), []);
    assert_eq!(row(&[O(1); 6]), [shape(6)]);
    let l = lowered(|l| l.row_buttons = 2);
    assert_eq!(
        check_with(&[Component::Row(&[B; 3])], &l),
        [v(RuleId::CV2_06, "row[0]", btw(1, 2), cnt(3))]
    );
}

#[test]
fn cv2_07_section_texts() {
    let section = |texts: &[&str], l: &Limits| {
        check_with(
            &[Component::Section(Section::new(
                texts,
                Accessory::Button(GO),
            ))],
            l,
        )
    };
    let l = Limits::default();
    assert_eq!(
        section(&[], &l),
        [v(RuleId::CV2_07, "section[0]", btw(1, 3), cnt(0))]
    );
    assert_eq!(section(&["a"], &l), []);
    assert_eq!(section(&["a", "b", "c"], &l), []);
    assert_eq!(
        section(&["a", "b", "c", "d"], &l),
        [v(RuleId::CV2_07, "section[0]", btw(1, 3), cnt(4))]
    );
    let l = lowered(|l| l.section_texts = 1);
    assert_eq!(
        section(&["a", "b"], &l),
        [v(RuleId::CV2_07, "section[0]", btw(1, 1), cnt(2))]
    );
}

#[test]
fn cv2_08_button_label() {
    let label = |n| {
        v(
            RuleId::CV2_08,
            "row[0].button[0].label",
            Bound::AtMost(80),
            cnt(n),
        )
    };
    let (a80, a81) = ("a".repeat(80), "a".repeat(81));
    let (e40, e41) = ("😀".repeat(40), "😀".repeat(41));
    assert_eq!(button(p(&a80)), []);
    assert_eq!(button(p(&a81)), [label(81)]);
    assert_eq!(button(p(&e40)), []);
    assert_eq!(button(p(&e41)), [label(82)]);
    assert_eq!(button(p("")), []);
    assert_eq!(button(Button::new(ButtonStyle::Primary)), []);
    let texts = ["a"];
    let tree = [Component::Section(Section::new(
        &texts,
        Accessory::Button(p(&a81)),
    ))];
    assert_eq!(
        check(&tree),
        [v(
            RuleId::CV2_08,
            "section[0].accessory.label",
            Bound::AtMost(80),
            cnt(81)
        )]
    );
    let l = lowered(|l| l.button_label_len = 5);
    assert_eq!(
        check_with(&[Component::Row(&[Component::Button(p("abcdef"))])], &l),
        [v(
            RuleId::CV2_08,
            "row[0].button[0].label",
            Bound::AtMost(5),
            cnt(6)
        )]
    );
}

#[test]
fn cv2_08_link_and_premium() {
    let url = |n| v(RuleId::CV2_08, "row[0].button[0].url", btw(1, 512), cnt(n));
    let (a512, a513) = ("a".repeat(512), "a".repeat(513));
    assert_eq!(button(Button::new(ButtonStyle::Link(&a512))), []);
    assert_eq!(button(Button::new(ButtonStyle::Link(&a513))), [url(513)]);
    assert_eq!(button(Button::new(ButtonStyle::Link(""))), [url(0)]);
    assert_eq!(
        button(Button::new(ButtonStyle::Premium).label("x")),
        [v(
            RuleId::CV2_08,
            "row[0].button[0].label",
            Bound::Describe("no label on a premium button"),
            txt("x")
        )]
    );
    assert_eq!(
        button(Button::new(ButtonStyle::Premium).emoji(true)),
        [v(
            RuleId::CV2_08,
            "row[0].button[0].emoji",
            Bound::Describe("no emoji on a premium button"),
            txt("true")
        )]
    );
    assert_eq!(button(Button::new(ButtonStyle::Premium)), []);
    assert_eq!(
        button(Button::new(ButtonStyle::Link("https://x")).label("Docs")),
        []
    );
}

#[test]
fn cv2_09_placeholder_and_bounds() {
    let user = Select::new(SelectKind::User);
    let at = |field: &str| format!("{SEL}.{field}");
    let (a150, a151) = ("a".repeat(150), "a".repeat(151));
    assert_eq!(select(user.placeholder(&a150)), []);
    assert_eq!(
        select(user.placeholder(&a151)),
        [v(
            RuleId::CV2_09,
            &at("placeholder"),
            Bound::AtMost(150),
            cnt(151)
        )]
    );
    assert_eq!(select(user.placeholder("")), []);
    assert_eq!(select(user.min_values(0)), []);
    assert_eq!(select(user.min_values(25).max_values(25)), []);
    assert_eq!(
        select(user.min_values(26)),
        [v(RuleId::CV2_09, &at("min_values"), btw(0, 25), cnt(26))]
    );
    assert_eq!(select(user.max_values(1)), []);
    assert_eq!(select(user.max_values(25)), []);
    assert_eq!(
        select(user.max_values(0)),
        [v(RuleId::CV2_09, &at("max_values"), btw(1, 25), cnt(0))]
    );
    assert_eq!(
        select(user.max_values(26)),
        [v(RuleId::CV2_09, &at("max_values"), btw(1, 25), cnt(26))]
    );
    let order = |s: &str| {
        v(
            RuleId::CV2_09,
            SEL,
            Bound::Describe("min_values ≤ max_values"),
            txt(s),
        )
    };
    assert_eq!(select(user.min_values(3).max_values(2)), [order("3 > 2")]);
    assert_eq!(select(user.min_values(2)), [order("2 > 1")]);
    assert_eq!(
        select(user.min_values(26).max_values(2)),
        [v(RuleId::CV2_09, &at("min_values"), btw(0, 25), cnt(26))]
    );
    let l = lowered(|l| l.select_values = 5);
    let tree = [Component::Row(&[Component::Select(user.max_values(6))])];
    assert_eq!(
        check_with(&tree, &l),
        [v(RuleId::CV2_09, &at("max_values"), btw(1, 5), cnt(6))]
    );
}

#[test]
fn cv2_09_string_options() {
    let opts: Vec<SelectOption<'_>> = (0..26).map(|_| SelectOption::new("a", "a")).collect();
    let string = |o: &[SelectOption<'_>]| select(Select::new(SelectKind::String(o)));
    assert_eq!(string(&opts[..25]), []);
    let count = |n| {
        v(
            RuleId::CV2_09,
            "row[0].select[0].options",
            btw(1, 25),
            cnt(n),
        )
    };
    assert_eq!(string(&opts), [count(26)]);
    assert_eq!(string(&[]), [count(0)]);
    let (a100, a101) = ("a".repeat(100), "a".repeat(101));
    let field = |f: &str, bound, n| {
        v(
            RuleId::CV2_09,
            &format!("row[0].select[0].options[0].{f}"),
            bound,
            cnt(n),
        )
    };
    assert_eq!(
        string(&[SelectOption::new("", "v")]),
        [field("label", btw(1, 100), 0)]
    );
    assert_eq!(string(&[SelectOption::new(&a100, "v")]), []);
    assert_eq!(
        string(&[SelectOption::new(&a101, "v")]),
        [field("label", btw(1, 100), 101)]
    );
    assert_eq!(
        string(&[SelectOption::new("l", "")]),
        [field("value", btw(1, 100), 0)]
    );
    assert_eq!(
        string(&[SelectOption::new("l", &a101)]),
        [field("value", btw(1, 100), 101)]
    );
    assert_eq!(
        string(&[SelectOption::new("l", "v").description(&a100)]),
        []
    );
    assert_eq!(
        string(&[SelectOption::new("l", "v").description(&a101)]),
        [field("description", Bound::AtMost(100), 101)]
    );
    assert_eq!(string(&[SelectOption::new("l", "v").description("")]), []);
    let l = lowered(|l| l.select_options = 2);
    let s = Select::new(SelectKind::String(&opts[..3]));
    assert_eq!(
        check_with(&[Component::Row(&[Component::Select(s)])], &l),
        [v(
            RuleId::CV2_09,
            "row[0].select[0].options",
            btw(1, 2),
            cnt(3)
        )]
    );
}

#[test]
fn cv2_09_default_values() {
    let user = Select::new(SelectKind::User);
    let defaults = |min, max, n| {
        v(
            RuleId::CV2_09,
            "row[0].select[0].default_values",
            btw(min, max),
            cnt(n),
        )
    };
    let u13 = user.min_values(1).max_values(3);
    assert_eq!(select(u13.default_values(2)), []);
    assert_eq!(select(u13.default_values(4)), [defaults(1, 3, 4)]);
    assert_eq!(select(u13.default_values(0)), []);
    assert_eq!(
        select(user.min_values(2).max_values(3).default_values(1)),
        [defaults(2, 3, 1)]
    );
    let flagged = [
        SelectOption::new("a", "a").default(true),
        SelectOption::new("b", "b").default(true),
    ];
    assert_eq!(
        select(Select::new(SelectKind::String(&flagged))),
        [defaults(1, 1, 2)]
    );
    let plain = [SelectOption::new("a", "a")];
    assert_eq!(
        select(Select::new(SelectKind::String(&plain)).default_values(1)),
        [v(
            RuleId::CV2_09,
            "row[0].select[0].default_values",
            Bound::Describe("default_values only on user, role, mentionable or channel selects"),
            cnt(1)
        )]
    );
    let errs = select(user.min_values(3).max_values(2).default_values(5));
    assert_eq!(paths(&errs), [SEL]);
}

#[test]
fn cv2_09_disabled_in_a_message() {
    assert_eq!(select(Select::new(SelectKind::Role).disabled(true)), []);
}

#[test]
fn cv2_10_media_gallery() {
    let gallery = |items: &[Media<'_>], l: &Limits| check_with(&[Component::Gallery(items)], l);
    let l = Limits::default();
    let items = vec![Media::new("u"); 11];
    assert_eq!(
        gallery(&[], &l),
        [v(RuleId::CV2_10, "gallery[0]", btw(1, 10), cnt(0))]
    );
    assert_eq!(gallery(&items[..10], &l), []);
    assert_eq!(
        gallery(&items, &l),
        [v(RuleId::CV2_10, "gallery[0]", btw(1, 10), cnt(11))]
    );
    let desc = |n| {
        v(
            RuleId::CV2_10,
            "gallery[0].item[0].description",
            Bound::AtMost(1024),
            cnt(n),
        )
    };
    for (text, expected) in [
        ("a".repeat(1024), None),
        ("a".repeat(1025), Some(1025)),
        ("😀".repeat(512), None),
        ("😀".repeat(513), Some(1026)),
    ] {
        let errs = gallery(&[Media::new("u").description(&text)], &l);
        assert_eq!(errs, expected.map(desc).into_iter().collect::<Vec<_>>());
    }
    let low = lowered(|l| l.gallery_items = 3);
    assert_eq!(
        gallery(&items[..4], &low),
        [v(RuleId::CV2_10, "gallery[0]", btw(1, 3), cnt(4))]
    );
}

#[test]
fn cv2_11_thumbnail_description() {
    let (a1024, a1025) = ("a".repeat(1024), "a".repeat(1025));
    let texts = ["a"];
    let accessory = |d: &str| {
        check(&[Component::Section(Section::new(
            &texts,
            Accessory::Thumbnail(Media::new("u").description(d)),
        ))])
    };
    assert_eq!(accessory(&a1024), []);
    assert_eq!(
        accessory(&a1025),
        [v(
            RuleId::CV2_11,
            "section[0].accessory.description",
            Bound::AtMost(1024),
            cnt(1025)
        )]
    );
    let errs = check(&[Component::Thumbnail(Media::new("u").description(&a1025))]);
    assert_eq!(rules(&errs), [RuleId::CV2_05, RuleId::CV2_11]);
    assert_eq!(errs[1].path, "thumbnail[0].description");
}

#[test]
fn cv2_12_file_urls() {
    let bad = |url: &str| {
        vec![v(
            RuleId::CV2_12,
            "file[0]",
            Bound::Describe("an attachment://<filename> URL"),
            txt(url),
        )]
    };
    assert_eq!(check(&[Component::File("attachment://a.png")]), []);
    for url in ["https://x/a.png", "attachment://", ""] {
        assert_eq!(check(&[Component::File(url)]), bad(url), "{url}");
    }
}

#[test]
fn cv2_13_accent_color() {
    let colour = |c| check(&[Component::Container(Container::new(&[]).accent_color(c))]);
    assert_eq!(colour(0), []);
    assert_eq!(colour(0x00FF_FFFF), []);
    let bad = |s: &str| {
        vec![v(
            RuleId::CV2_13,
            "container[0].accent_color",
            Bound::Describe("a colour from 0x000000 to 0xFFFFFF"),
            txt(s),
        )]
    };
    assert_eq!(colour(0x0100_0000), bad("0x1000000"));
    assert_eq!(colour(u32::MAX), bad("0xFFFFFFFF"));
    assert_eq!(check(&[Component::Container(Container::new(&[]))]), []);
}

#[test]
fn cv2_17_media_urls() {
    let (a2048, a2049) = ("a".repeat(2048), "a".repeat(2049));
    let item = |u: &str, l: &Limits| check_with(&[Component::Gallery(&[Media::new(u)])], l);
    let l = Limits::default();
    let url = |n| {
        v(
            RuleId::CV2_17,
            "gallery[0].item[0].url",
            btw(1, 2048),
            cnt(n),
        )
    };
    assert_eq!(item(&a2048, &l), []);
    assert_eq!(item(&a2049, &l), [url(2049)]);
    assert_eq!(item("", &l), [url(0)]);
    let texts = ["a"];
    let tree = [Component::Section(Section::new(
        &texts,
        Accessory::Thumbnail(Media::new(&a2049)),
    ))];
    assert_eq!(
        check(&tree),
        [v(
            RuleId::CV2_17,
            "section[0].accessory.url",
            btw(1, 2048),
            cnt(2049)
        )]
    );
    let low = lowered(|l| l.media_url_len = 10);
    assert_eq!(
        item("abcdefghijk", &low),
        [v(
            RuleId::CV2_17,
            "gallery[0].item[0].url",
            btw(1, 10),
            cnt(11)
        )]
    );
}

#[test]
fn cv2_03_cv2_04_cv2_05_cv2_06_all_at_once() {
    let a4001 = "a".repeat(4001);
    let empty: [Component<'_>; 0] = [];
    let six = [B; 6];
    let children = [
        Component::Container(Container::new(&empty)),
        Component::Row(&six),
    ];
    let mut tree = vec![T(&a4001), Component::Container(Container::new(&children))];
    tree.extend([SP; 38]);
    assert_eq!(cost(&tree, &Limits::default()).components, 48);
    let errs = check(&tree);
    assert_eq!(
        rules(&errs),
        [
            RuleId::CV2_03,
            RuleId::CV2_04,
            RuleId::CV2_05,
            RuleId::CV2_06
        ]
    );
    assert_eq!(
        paths(&errs),
        [
            "components",
            "components",
            "container[1].container[0]",
            "container[1].row[1]"
        ]
    );
}

#[test]
fn cv2_03_cost() {
    let l = Limits::default();
    let c = |tree: &[Component<'_>]| {
        let c = cost(tree, &l);
        (c.components, c.text_len)
    };
    let texts1 = ["a"];
    let texts3 = ["a", "b", "c"];
    let options = [SelectOption::new("a", "a"); 25];
    let items = [Media::new("u"); 10];
    assert_eq!(c(&[]), (0, 0));
    assert_eq!(c(&[Component::Container(Container::new(&[]))]), (1, 0));
    assert_eq!(
        c(&[Component::Section(Section::new(
            &texts1,
            Accessory::Button(GO)
        ))]),
        (3, 1)
    );
    assert_eq!(
        c(&[Component::Section(Section::new(
            &texts3,
            Accessory::Thumbnail(Media::new("u"))
        ))]),
        (5, 3)
    );
    assert_eq!(c(&[Component::Row(&[B; 5])]), (6, 0));
    assert_eq!(
        c(&[Component::Row(&[Component::Select(Select::new(
            SelectKind::String(&options)
        ))])]),
        (2, 0)
    );
    assert_eq!(c(&[Component::Gallery(&items)]), (1, 0));
    assert_eq!(c(&[O(3)]), (3, 0));
    assert_eq!(c(&[O(0)]), (1, 0));

    let five = [B; 5];
    let four = [B; 4];
    let gallery = [Media::new("u")];
    let section = Component::Section(Section::new(&texts1, Accessory::Button(GO)));
    let children = [
        T("t"),
        SP,
        Component::Row(&five),
        Component::Gallery(&gallery),
        section,
        section,
        section,
        section,
        section,
        SP,
        Component::Row(&four),
    ];
    assert_eq!(c(&[Component::Container(Container::new(&children))]).0, 31);
    let inner = [T("ab")];
    assert_eq!(
        c(&[T("😀"), Component::Container(Container::new(&inner))]),
        (3, 4)
    );
}

#[test]
fn cv2_15_legacy_components() {
    let one = [B];
    let r = Component::Row(&one);
    assert_eq!(legacy(&[]), []);
    assert_eq!(legacy(&[r; 5]), []);
    assert_eq!(legacy(&[O(1)]), []);
    assert_eq!(
        legacy(&[r; 6]),
        [v(RuleId::CV2_15, "components", Bound::AtMost(5), cnt(6))]
    );
    let l = lowered(|l| l.legacy_action_rows = 1);
    assert_eq!(
        legacy_with(&[r; 2], &l),
        [v(RuleId::CV2_15, "components", Bound::AtMost(1), cnt(2))]
    );
    let kind = |path: &str, k: &str| {
        vec![v(
            RuleId::CV2_15,
            path,
            Bound::Describe("only Action Rows in a legacy message"),
            txt(k),
        )]
    };
    assert_eq!(legacy(&[T("x")]), kind("text[0]", "text"));
    let inner = [T("a"), B];
    assert_eq!(
        legacy(&[Component::Container(Container::new(&inner))]),
        kind("container[0]", "container")
    );
    assert_eq!(
        legacy(&[Component::Gallery(&[Media::new("u")])]),
        kind("gallery[0]", "gallery")
    );
    assert_eq!(rules(&legacy(&[Component::Row(&[B; 6])])), [RuleId::CV2_06]);
    let long = "a".repeat(81);
    let labelled = [Component::Button(
        Button::new(ButtonStyle::Primary).label(&long),
    )];
    let errs = legacy(&[Component::Row(&labelled)]);
    assert_eq!(rules(&errs), [RuleId::CV2_08]);
    assert_eq!(errs[0].path, "row[0].button[0].label");
    let errs = legacy(&[Component::Row(&[T("x")])]);
    assert_eq!(rules(&errs), [RuleId::CV2_05]);
    assert_eq!(errs[0].path, "row[0].text[0]");
    let a4001 = "a".repeat(4001);
    assert_eq!(
        rules(&legacy(&[Component::Row(&[T(&a4001)])])),
        [RuleId::CV2_05]
    );
}

#[test]
fn cv2_08_granular_button_label_and_link_url() {
    let l = Limits::default();
    for ok in [String::new(), "a".repeat(80), "😀".repeat(40)] {
        assert!(validate_button_label(&ok, &l).is_empty());
    }
    assert_eq!(
        validate_button_label(&"a".repeat(81), &l),
        [v(RuleId::CV2_08, "", Bound::AtMost(80), cnt(81))]
    );
    assert_eq!(
        validate_button_label(&"😀".repeat(41), &l),
        [v(RuleId::CV2_08, "", Bound::AtMost(80), cnt(82))]
    );
    let low = lowered(|l| l.button_label_len = 5);
    assert_eq!(
        validate_button_label("abcdef", &low),
        [v(RuleId::CV2_08, "", Bound::AtMost(5), cnt(6))]
    );
    assert!(validate_link_url("a", &l).is_empty());
    assert!(validate_link_url(&"a".repeat(512), &l).is_empty());
    assert_eq!(
        validate_link_url("", &l),
        [v(RuleId::CV2_08, "", btw(1, 512), cnt(0))]
    );
    assert_eq!(
        validate_link_url(&"a".repeat(513), &l),
        [v(RuleId::CV2_08, "", btw(1, 512), cnt(513))]
    );
}

#[test]
fn cv2_09_cv2_10_cv2_11_cv2_17_granular_text() {
    let l = Limits::default();
    assert!(validate_select_placeholder("", &l).is_empty());
    assert!(validate_select_placeholder(&"a".repeat(150), &l).is_empty());
    assert_eq!(
        validate_select_placeholder(&"a".repeat(151), &l),
        [v(RuleId::CV2_09, "", Bound::AtMost(150), cnt(151))]
    );
    let descriptions: [(Granular, RuleId); 2] = [
        (validate_gallery_description, RuleId::CV2_10),
        (validate_thumbnail_description, RuleId::CV2_11),
    ];
    for (f, rule) in descriptions {
        for ok in [String::new(), "a".repeat(1024), "😀".repeat(512)] {
            assert!(f(&ok, &l).is_empty());
        }
        assert_eq!(
            f(&"a".repeat(1025), &l),
            [v(rule, "", Bound::AtMost(1024), cnt(1025))]
        );
        assert_eq!(
            f(&"😀".repeat(513), &l),
            [v(rule, "", Bound::AtMost(1024), cnt(1026))]
        );
    }
    assert!(validate_media_url("a", &l).is_empty());
    assert!(validate_media_url(&"a".repeat(2048), &l).is_empty());
    assert_eq!(
        validate_media_url("", &l),
        [v(RuleId::CV2_17, "", btw(1, 2048), cnt(0))]
    );
    assert_eq!(
        validate_media_url(&"a".repeat(2049), &l),
        [v(RuleId::CV2_17, "", btw(1, 2048), cnt(2049))]
    );
}

#[test]
fn cv2_13_granular_colour_and_spacing() {
    let l = Limits::default();
    assert!(validate_accent_color(0, &l).is_empty());
    assert!(validate_accent_color(0x00FF_FFFF, &l).is_empty());
    let colour = Bound::Describe("a colour from 0x000000 to 0xFFFFFF");
    assert_eq!(
        validate_accent_color(0x0100_0000, &l),
        [v(RuleId::CV2_13, "", colour.clone(), txt("0x1000000"))]
    );
    assert_eq!(
        validate_accent_color(u32::MAX, &l),
        [v(RuleId::CV2_13, "", colour, txt("0xFFFFFFFF"))]
    );
    assert!(validate_spacing(1, &l).is_empty());
    assert!(validate_spacing(2, &l).is_empty());
    for bad in [0_u8, 3, 255] {
        assert_eq!(
            validate_spacing(bad, &l),
            [v(
                RuleId::CV2_13,
                "",
                Bound::Describe("1 (small) or 2 (large)"),
                txt(&bad.to_string())
            )]
        );
    }
}

#[test]
fn cv2_13_cv2_08_enum_values() {
    assert_eq!(Spacing::Small.value(), 1);
    assert_eq!(Spacing::Large.value(), 2);
    assert_eq!(Spacing::default(), Spacing::Small);
    assert_eq!(Separator::default(), Separator::new());
    let styles = [
        ButtonStyle::Primary,
        ButtonStyle::Secondary,
        ButtonStyle::Success,
        ButtonStyle::Danger,
        ButtonStyle::Link("u"),
        ButtonStyle::Premium,
    ];
    let values: Vec<u8> = styles.iter().map(|s| s.value()).collect();
    assert_eq!(values, [1, 2, 3, 4, 5, 6]);
}

proptest! {
    #[test]
    fn cv2_03_property_count(n in 0usize..=60) {
        let tree = vec![T("a"); n];
        let errs = check(&tree);
        prop_assert_eq!(rules(&errs).contains(&RuleId::CV2_03), n > 40);
    }

    #[test]
    fn cv2_04_property_text_budget(texts in proptest::collection::vec(".{0,2500}", 0..=3)) {
        let tree: Vec<Component<'_>> = texts.iter().map(|t| T(t)).collect();
        let sum: usize = texts.iter().map(|t| discord_len(t)).sum();
        prop_assert_eq!(cost(&tree, &Limits::default()).text_len, sum);
        prop_assert_eq!(rules(&check(&tree)).contains(&RuleId::CV2_04), sum > 4000);
    }
}
