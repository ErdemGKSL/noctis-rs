//! Components V2 boundary passes (CV2-*): runtime-built trees exactly at a
//! limit validate through `noctis::ui::validate`.

use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    ActionRow, Components, Container, LinkButton, Media, MediaGallery, MediaItem, SelectOption,
    TextDisplay, V2Message, validate,
};
use noctis::{components, label, placeholder, rgb};

#[derive(Route)]
#[route("b.press")]
struct Press {
    n: u8,
}

impl ::noctis::__private::ui::Handled for Press {}

#[derive(Route)]
#[route("b.pick", select)]
struct Choose;

impl ::noctis::__private::ui::Handled for Choose {}

fn passes(mut msg: V2Message) -> usize {
    match validate(&mut msg, &Limits::default()) {
        Ok(report) => report.components,
        Err(e) => panic!("expected a pass, got {e}"),
    }
}

#[test]
fn cv2_06_exactly_five_buttons() {
    let row = ActionRow::buttons([0, 1, 2, 3, 4].map(|n| Press { n }.button()));
    assert_eq!(passes(V2Message::new(Components::new().push(row))), 6);
}

#[test]
fn cv2_03_exactly_forty_components() {
    let container = (0..39).fold(Container::new(), |c, _| c.push(TextDisplay::new("a")));
    assert_eq!(
        passes(V2Message::new(Components::new().push(container))),
        40
    );
}

#[test]
fn cv2_04_exactly_4000_characters() {
    let body = Components::new()
        .push(TextDisplay::new("a".repeat(2000)))
        .push(TextDisplay::new("😀".repeat(1000)));
    assert_eq!(passes(V2Message::new(body)), 2);
}

#[test]
fn cv2_08_exactly_80_character_label() {
    let row = ActionRow::buttons([LinkButton::new("https://x.y").label("a".repeat(80))]);
    passes(V2Message::new(Components::new().push(row)));
}

#[test]
fn cv2_09_exactly_25_options() {
    let options = (0..25).map(|i| SelectOption::new(format!("o{i}"), format!("{i}")));
    let row = ActionRow::select(Choose.string_select(options));
    assert_eq!(passes(V2Message::new(Components::new().push(row))), 2);
}

#[test]
fn cv2_10_exactly_10_gallery_items() {
    let items = [(); 10].map(|()| MediaItem::new(Media::url("https://x.y/a.png")).alt("a"));
    assert_eq!(
        passes(V2Message::new(
            Components::new().push(MediaGallery::new(items))
        )),
        1
    );
}

// Compile-time boundaries (Story 4.4): each literal or fixed shape exactly at
// its limit compiles and validates.

#[test]
fn cv2_06_components_exactly_five_buttons() {
    let body = components! {
        row [ Press { n: 1 }.button(), Press { n: 2 }.button(), Press { n: 3 }.button(), Press { n: 4 }.button(), Press { n: 5 }.button() ],
    };
    assert_eq!(passes(V2Message::new(body)), 6);
}

#[test]
fn cv2_03_components_exactly_forty() {
    let body = components! { container [ text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a"), text("a") ] };
    assert_eq!(passes(V2Message::new(body)), 40);
}

#[test]
fn cv2_04_components_exactly_4000_characters() {
    let body = components! {
        text("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        container [ section [ text("😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"); accessory = Press { n: 1 }.button() ] ],
    };
    assert_eq!(passes(V2Message::new(body)), 5);
}

#[test]
fn cv2_08_label_macro_exactly_80() {
    let row = ActionRow::buttons([LinkButton::new("https://x.y").label(label!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ))]);
    passes(V2Message::new(Components::new().push(row)));
}

#[test]
fn cv2_09_placeholder_macro_exactly_150() {
    let select = Choose.user_select().placeholder(placeholder!("pppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppp"));
    passes(V2Message::new(
        Components::new().push(ActionRow::select(select)),
    ));
}

#[test]
fn cv2_13_rgb_macro_max() {
    let body = components! { container(accent = rgb!(0xFFFFFF)) [ text("a") ] };
    assert_eq!(passes(V2Message::new(body)), 2);
    assert_eq!(rgb!(0xFFFFFF).value(), 0x00FF_FFFF);
}

#[test]
fn cv2_10_components_exactly_10_items() {
    let body = components! {
        gallery [
            item("https://x.y/0.png", alt = "a"), item("https://x.y/1.png", alt = "a"),
            item("https://x.y/2.png", alt = "a"), item("https://x.y/3.png", alt = "a"),
            item("https://x.y/4.png", alt = "a"), item("https://x.y/5.png", alt = "a"),
            item("https://x.y/6.png", alt = "a"), item("https://x.y/7.png", alt = "a"),
            item("https://x.y/8.png", alt = "a"), item("https://x.y/9.png", alt = "a"),
        ],
    };
    assert_eq!(passes(V2Message::new(body)), 1);
}

#[test]
fn cv2_07_components_exactly_3_section_texts() {
    let body = components! {
        section [ text("a"), text("b"), text("c"); accessory = link(url = "https://x.y", label = "Go") ],
    };
    assert_eq!(passes(V2Message::new(body)), 5);
}
