//! `components!` and the literal macros through the facade only (Story 4.4):
//! the passing forms, the expansion, evaluation order, hygiene and the
//! boundaries the compile-time checks allow.

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use std::cell::RefCell;

use noctis::prelude::*;
use noctis::rules::{Limits, RuleId};
use noctis::ui::{
    ActionRow, Attachment, AttachmentSet, ButtonStyle, Components, Container, Content, File, Label,
    LegacyMessage, LinkButton, Media, MediaGallery, MediaItem, Placeholder, PremiumButton, Section,
    SelectOption, Separator, TextDisplay, Thumbnail, TopLevelComponent, UiError, V2Message,
    validate,
};
use serde_json::Value;
use twilight_model::id::Id;

#[derive(Route)]
#[route("t.press")]
struct Press {
    n: u8,
}
impl ::noctis::__private::ui::Handled for Press {}

#[derive(Route)]
#[route("t.pick", select)]
struct Pick;
impl ::noctis::__private::ui::Handled for Pick {}

/// A select Route the CID-02 check does not compare (a call, lowercase).
fn pick() -> Pick {
    Pick
}

#[derive(Route)]
#[route("t.pair")]
struct Pair {
    a: u8,
    b: u8,
}
impl ::noctis::__private::ui::Handled for Pair {}

#[derive(Route)]
#[route("t.refresh")]
struct Refresh;
impl ::noctis::__private::ui::Handled for Refresh {}

fn json(c: &Components) -> Value {
    serde_json::to_value(c).expect("components serialize")
}

fn valid(c: Components) -> usize {
    let mut msg = V2Message::new(c);
    validate(&mut msg, &Limits::default())
        .expect("a valid message")
        .components
}

// Literal macros (Matrix 1, 3, 5, 8, 9).

#[test]
fn cv2_08_label_literals() {
    let max =
        label!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(max.as_str().len(), 80);
    let emoji =
        label!("😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀");
    assert_eq!(emoji.as_str().chars().count(), 40);
    assert_eq!(label!("").as_str(), "");
    assert_eq!(label!("\u{1F600}").as_str(), "😀");
    assert_eq!(label!(r"x").as_str(), "x");
    assert_eq!(Label::try_from("Save"), Ok(label!("Save")));
}

const SAVE: Label = label!("Save");
const PICK: Placeholder = placeholder!("Pick one");
const HI: Content = content!("hi");
const WHITE: Rgb = rgb!(0xFFFFFF);

#[test]
fn cv2_08_label_in_const_and_builders() {
    let route = Press { n: 1 }.button().label(SAVE);
    let link = LinkButton::new("https://x.y").label(SAVE);
    let row = ActionRow::buttons([route]);
    let json = serde_json::to_value(&row).expect("serialize");
    assert_eq!(json["components"][0]["label"], "Save");
    assert_eq!(
        serde_json::to_value(&link).expect("serialize")["label"],
        "Save"
    );
    let select = Pick.user_select().placeholder(PICK);
    let body = Components::new().push(ActionRow::select(select));
    assert_eq!(
        json_with_unresolved(&body)[0]["components"][0]["placeholder"],
        "Pick one"
    );
}

#[test]
fn msg_01_content_literal() {
    let mut msg = LegacyMessage::new(content!("hi"));
    assert_eq!(msg.content_text(), "hi");
    validate(&mut msg, &Limits::default()).expect("valid content");
    assert_eq!(LegacyMessage::new(HI).content_text(), "hi");
    assert_eq!(content!("").as_str(), "");
}

#[test]
fn cv2_13_rgb_literals() {
    const BLACK: Rgb = rgb!(0x000000);
    assert_eq!(BLACK.value(), 0);
    assert_eq!(rgb!(0).value(), 0);
    assert_eq!(WHITE, Rgb::new(255, 255, 255));
    assert_eq!(rgb!(0x5865_F2), Rgb::new(0x58, 0x65, 0xF2));
    assert_eq!(rgb!(16_777_215_u32), WHITE);
    assert_eq!(Rgb::try_from(0x00FF_FFFF), Ok(WHITE));
    assert!(matches!(
        Rgb::try_from(0x0100_0000),
        Err(UiError::Limits { violations }) if violations[0].rule == RuleId::CV2_13
    ));
}

// `components!`: expansion (Matrix 22-37).

#[test]
fn golden_tree_equals_builders() {
    let name = String::from("noctis");
    let extra = vec![TextDisplay::new("x"), TextDisplay::new("y")];
    let footer = TextDisplay::new("-# footer");
    let by_macro = components! {
        container(accent = rgb!(0x5865_F2)) [
            text("### Settings"),
            separator(spacing = large),
            section [ text("Logs"), text(name); accessory = Press { n: 1 }.button("Change") ],
            gallery [ item("https://x.y/a.png", alt = "A") ],
            row [ Press { n: 2 }.button("Save").style(ButtonStyle::Primary), link(url = "https://x.y", label = "Docs") ],
            row [ Pick.user_select().placeholder("Who").max_values(3) ],
            ..extra,
        ],
        {footer},
    };

    let name = String::from("noctis");
    let by_hand = Components::new()
        .push(
            Container::new()
                .accent(Rgb::new(0x58, 0x65, 0xF2))
                .push(TextDisplay::new("### Settings"))
                .push(Separator::large())
                .push(Section::new(
                    [TextDisplay::new("Logs"), TextDisplay::new(name)],
                    Press { n: 1 }.button().label("Change"),
                ))
                .push(MediaGallery::new([MediaItem::new(Media::url(
                    "https://x.y/a.png",
                ))
                .alt("A")]))
                .push(ActionRow::buttons([
                    noctis::ui::Button::from(
                        Press { n: 2 }
                            .button()
                            .label("Save")
                            .style(ButtonStyle::Primary),
                    ),
                    noctis::ui::Button::from(LinkButton::new("https://x.y").label("Docs")),
                ]))
                .push(ActionRow::select(
                    Pick.user_select().placeholder("Who").max_values(3),
                ))
                .push(TextDisplay::new("x"))
                .push(TextDisplay::new("y")),
        )
        .push(TextDisplay::new("-# footer"));
    // The select's custom_id stays unresolved until core's Responder adds a
    // nonce, so compare with it removed.
    assert_eq!(
        json_with_unresolved(&by_macro),
        json_with_unresolved(&by_hand)
    );
}

/// The JSON of `c`, with unresolved select ids resolved the same way.
fn json_with_unresolved(c: &Components) -> Value {
    let mut c = c.clone();
    let nonce = noctis::__private::codec::SelectNonce::new(1).expect("nonce");
    for id in c.custom_ids_mut() {
        if id.get().is_none() {
            let parts = noctis::__private::codec::Parts::new().with_nonce(nonce);
            id.resolve(&parts).expect("resolve");
        }
    }
    json(&c)
}

#[test]
fn empty_body() {
    let c = components! {};
    assert_eq!(c.len(), 0);
}

#[test]
fn text_forms() {
    let var = String::from("v");
    let c = components! { text("lit"), text(format!("{}-{}", 1, 2)), text(var) };
    let j = json(&c);
    assert_eq!(j[0]["content"], "lit");
    assert_eq!(j[1]["content"], "1-2");
    assert_eq!(j[2]["content"], "v");
}

#[test]
fn container_forms() {
    const ACCENT: Rgb = rgb!(0x0057_F287);
    let c = components! {
        container [ text("a") ],
        container(accent = rgb!(0x5865_F2)) [ text("b") ],
        container(accent = 0x5865F2) [ text("c") ],
        container(accent = ACCENT) [],
    };
    let j = json(&c);
    assert!(j[0].get("accent_color").is_none());
    assert_eq!(j[1]["accent_color"], 0x0058_65F2);
    assert_eq!(j[2]["accent_color"], 0x0058_65F2);
    assert_eq!(j[3]["accent_color"], 0x0057_F287);
}

#[test]
fn separator_forms() {
    let c = components! {
        separator,
        separator(spacing = large),
        separator(divider = false, spacing = small),
    };
    let by_hand = Components::new()
        .push(Separator::new())
        .push(Separator::large())
        .push(Separator::small().divider(false));
    assert_eq!(json(&c), json(&by_hand));
}

#[test]
fn media_and_file_forms() {
    let mut files = AttachmentSet::new();
    let card = files.add(Attachment::new(&b"png"[..], "png").expect("png"));
    let card_ref = card.clone();
    let alt_var = String::from("Card");
    let c = components! {
        section [ text("a"); accessory = thumbnail("https://x.y/a.png") ],
        section [ text("b"); accessory = thumbnail(card_ref.clone(), alt = alt_var) ],
        gallery [ item(card_ref.clone(), alt = "Card"), item("https://x.y/b.png") ],
        file(card_ref),
    };
    let by_hand = Components::new()
        .push(Section::new(
            [TextDisplay::new("a")],
            Thumbnail::new(Media::url("https://x.y/a.png")),
        ))
        .push(Section::new(
            [TextDisplay::new("b")],
            Thumbnail::new(card.clone()).alt("Card"),
        ))
        .push(MediaGallery::new([
            MediaItem::new(card.clone()).alt("Card"),
            MediaItem::new(Media::url("https://x.y/b.png")),
        ]))
        .push(File::new(card));
    assert_eq!(json(&c), json(&by_hand));
}

#[test]
fn row_buttons_and_holes() {
    let sku = Id::new(7);
    let b = Press { n: 9 }.button();
    let c = components! {
        row [ Press { n: 1 }.button(), link(url = "https://x.y", label = "Docs"), premium(sku), {b} ],
    };
    let j = json(&c);
    let styles: Vec<_> = j[0]["components"]
        .as_array()
        .expect("row")
        .iter()
        .map(|b| b["style"].clone())
        .collect();
    assert_eq!(styles, [2, 5, 6, 2]);
    let _ = PremiumButton::new(sku);
}

#[test]
fn button_args_and_setters() {
    let name = String::from("Named");
    let c = components! {
        row [
            Press { n: 1 }.button(),
            Press { n: 2 }.button("Save"),
            Press { n: 3 }.button(label!("Label")),
            Press { n: 4 }.button(name),
            Press { n: 5 }.button("Go").style(ButtonStyle::Danger),
        ],
        row [ Press { n: 6 }.button().label("x").disabled(true) ],
    };
    let j = json(&c);
    let labels: Vec<_> = j[0]["components"]
        .as_array()
        .expect("row")
        .iter()
        .map(|b| b["label"].clone())
        .collect();
    assert_eq!(
        labels,
        [
            Value::Null,
            "Save".into(),
            "Label".into(),
            "Named".into(),
            "Go".into()
        ]
    );
    assert_eq!(j[0]["components"][4]["style"], 4);
    assert_eq!(j[1]["components"][0]["label"], "x");
    assert_eq!(j[1]["components"][0]["disabled"], true);
}

#[test]
fn select_forms() {
    let options = vec![SelectOption::new("b", "b")];
    let c = components! {
        row [ Pick.string_select([SelectOption::new("a", "a")]) ],
        row [ pick().string_select(options) ],
        row [ pick().user_select().placeholder("Pick").min_values(1).max_values(3) ],
        row [ pick().role_select() ],
        row [ pick().mentionable_select() ],
        row [ pick().channel_select() ],
    };
    assert_eq!(c.len(), 6);
    let j = json_with_unresolved(&c);
    assert_eq!(j[2]["components"][0]["placeholder"], "Pick");
    assert_eq!(j[2]["components"][0]["max_values"], 3);
    let mut c = c;
    assert_eq!(c.custom_ids_mut().count(), 6);
}

#[test]
fn holes_and_spreads() {
    let top = TextDisplay::new("top");
    let inner = TextDisplay::new("inner");
    let acc = Press { n: 1 }.button().label("Acc");
    let tops: Vec<TopLevelComponent> = vec![TextDisplay::new("s1").into(), Separator::new().into()];
    let children = vec![TextDisplay::new("c1")];
    let c = components! {
        {top},
        ..tops,
        container [ {inner}, ..children, section [ text("s"); accessory = {acc} ] ],
        { let n = 2; TextDisplay::new(format!("n{n}")) },
    };
    let j = json(&c);
    assert_eq!(j.as_array().map(Vec::len), Some(5));
    assert_eq!(j[0]["content"], "top");
    assert_eq!(j[1]["content"], "s1");
    assert_eq!(j[3]["components"][0]["content"], "inner");
    assert_eq!(j[3]["components"][1]["content"], "c1");
    assert_eq!(j[3]["components"][2]["accessory"]["label"], "Acc");
    assert_eq!(j[4]["content"], "n2");
}

#[test]
fn evaluation_order() {
    let log = RefCell::new(Vec::new());
    let note = |s: &'static str| {
        log.borrow_mut().push(s);
        s
    };
    let item = |s: &'static str| {
        log.borrow_mut().push(s);
        vec![TextDisplay::new(s)]
    };
    let c = components! {
        text(note("a")),
        container(accent = { note("accent"); rgb!(0x000001) }) [
            text(note("b")),
            ..item("spread"),
            section [ text(note("c")), text(note("d")); accessory = Press { n: 1 }.button(note("e")) ],
        ],
        text(note("f")),
    };
    assert_eq!(c.len(), 3);
    assert_eq!(
        *log.borrow(),
        ["a", "accent", "b", "spread", "c", "d", "e", "f"]
    );
}

#[test]
fn hygiene() {
    #[allow(non_snake_case, reason = "mirrors the expansion's local names")]
    let __noctis_c = TextDisplay::new("user c");
    let c = TextDisplay::new("user plain c");
    let xs = vec![TextDisplay::new("x")];
    let body = components! { {__noctis_c}, ..xs, {c} };
    let j = json(&body);
    assert_eq!(j[0]["content"], "user c");
    assert_eq!(j[2]["content"], "user plain c");
}

/// No prelude import: the expansion names everything through
/// `::noctis::__private::ui`.
mod no_prelude {
    #[test]
    fn expands_without_imports() {
        let c = noctis::components! { text("hi"), container [ separator ] };
        assert_eq!(c.len(), 2);
    }
}

#[test]
fn usable_as_a_value() {
    let mut msg = V2Message::new(components! { text("hi") });
    validate(&mut msg, &Limits::default()).expect("valid");
}

// Compile-time rule boundaries (Matrix 38, 40-42, 44).

#[test]
fn cv2_03_exactly_forty_components() {
    let c = components! {
        container [
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
        ],
    };
    assert_eq!(valid(c), 40);
}

#[test]
fn cv2_03_eight_rows_of_four() {
    let c = components! {
        row [ Press { n: 1 }.button(), Press { n: 2 }.button(), Press { n: 3 }.button(), Press { n: 4 }.button() ],
        row [ Press { n: 5 }.button(), Press { n: 6 }.button(), Press { n: 7 }.button(), Press { n: 8 }.button() ],
        row [ Press { n: 9 }.button(), Press { n: 10 }.button(), Press { n: 11 }.button(), Press { n: 12 }.button() ],
        row [ Press { n: 13 }.button(), Press { n: 14 }.button(), Press { n: 15 }.button(), Press { n: 16 }.button() ],
        row [ Press { n: 17 }.button(), Press { n: 18 }.button(), Press { n: 19 }.button(), Press { n: 20 }.button() ],
        row [ Press { n: 21 }.button(), Press { n: 22 }.button(), Press { n: 23 }.button(), Press { n: 24 }.button() ],
        row [ Press { n: 25 }.button(), Press { n: 26 }.button(), Press { n: 27 }.button(), Press { n: 28 }.button() ],
        row [ Press { n: 29 }.button(), Press { n: 30 }.button(), Press { n: 31 }.button(), Press { n: 32 }.button() ],
    };
    assert_eq!(valid(c), 40);
}

#[test]
fn cv2_03_holes_count_one_spreads_zero() {
    let x = TextDisplay::new("x");
    let xs: Vec<TextDisplay> = Vec::new();
    // 39 texts + a hole = 40; a spread adds nothing at compile time.
    let c = components! {
        container [
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            text("a"), text("a"), text("a"), text("a"), text("a"), text("a"),
            {x},
            ..xs,
        ],
    };
    assert_eq!(valid(c), 40);
}

#[test]
fn cv2_03_sections_with_accessories() {
    let t = |s: &str| Media::url(format!("https://x.y/{s}.png"));
    let c = components! {
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("1")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("2")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("3")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("4")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("5")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("6")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("7")) ],
        section [ text("a"), text("b"), text("c"); accessory = thumbnail(t("8")) ],
    };
    assert_eq!(valid(c), 40);
}

#[test]
fn cv2_04_exactly_4000_literal_characters_and_non_literal() {
    let var = String::new();
    let c = components! {
        text("😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"),
        text(var),
    };
    assert_eq!(valid(c), 2);
}

// CID-02: receivers the macro does not compare (Matrix 58).

#[test]
fn cid_02_not_compared() {
    let n = 6;
    let x = 3;
    let base = Pair { a: 4, b: 4 };
    let route_var = Refresh;
    let c = components! {
        row [ Press { n: 1 }.button(), Press { n: 2 }.button() ],
        row [ Press { n }.button(), Press { n }.button() ],
        row [ Press { n: x }.button(), Press { n: x }.button() ],
        row [ Pair { a: 5, ..base }.button() ],
        row [ route_var.button() ],
        row [ Press { n: 7 }.button(), Press { n: 7u8 }.button() ],
    };
    // The runtime validator catches the real duplicates.
    let mut msg = V2Message::new(c);
    let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
        panic!("duplicates at runtime");
    };
    let paths: Vec<_> = violations
        .iter()
        .filter(|v| v.rule == RuleId::CID_02)
        .map(|v| v.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "row[1].button[1].custom_id",
            "row[2].button[1].custom_id",
            "row[5].button[1].custom_id"
        ]
    );
}
