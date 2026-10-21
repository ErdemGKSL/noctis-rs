//! The Components V2 model against Discord JSON: every component's
//! serialization (insta snapshots), numeric ids, `custom_id` resolution and
//! attachments (FR-23, FR-30, FR-58, CV2-05..14).

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use core::fmt;
use std::borrow::Cow;

use bytes::Bytes;
use noctis_codec::{
    EncodeError, Field, FieldWriter, Parts, Payload, PayloadKey, Ref, Route, RouteId, RouteShape,
    SelectNonce, SelectRoute,
};
use noctis_ui::__macro::{
    ButtonDefaults, ChannelSelectDefaults, ComponentValue, SelectDefaults, build_button,
    build_channel_select, build_mentionable_select, build_role_select, build_string_select_builder,
    build_user_select,
};
use noctis_ui::{
    ActionRow, Attachment, AttachmentOrigin, AttachmentSet, Button, ButtonStyle, ChannelSelect,
    Components, Container, ContainerComponent, Emoji, File, Handled, LinkButton, Media,
    MediaGallery, MediaItem, MentionableSelect, PremiumButton, Rgb, RoleSelect, RouteButton,
    Section, SelectOption, Separator, Spacing, StringSelect, TextDisplay, Thumbnail,
    TopLevelComponent, UiError, UserSelect,
};
use serde_json::{Value, json};
use twilight_model::channel::ChannelType;
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
route!(Signed { n: u8 } = "signed", RouteShape::INLINE.signed());
route!(Tagged { n: u8 } = "tagged", RouteShape::INLINE.signed());
route!(Stored { n: u8 } = "stored", RouteShape::BY_REF);
route!(Spilled { text: String } = "spilled", RouteShape::SPILL);

fn buy() -> Buy {
    Buy { item: 255 }
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

fn nonce() -> SelectNonce {
    SelectNonce::new(1).expect("in range")
}

/// Resolves every unresolved select id of `body` with a nonce.
fn resolve_selects(body: &mut Components) {
    for id in body.custom_ids_mut() {
        if id.get().is_none() {
            assert_eq!(id.resolve(&Parts::new().with_nonce(nonce())), Ok(None));
        }
    }
}

fn media_ref() -> noctis_ui::AttachmentRef {
    AttachmentSet::new().add(Attachment::new(Bytes::new(), "png").expect("valid ext"))
}

// Text Display, Separator, Container.

#[test]
fn text_display() {
    insta::assert_json_snapshot!("text_display", TextDisplay::new("## Hi 😀"));
    assert_eq!(
        to_json(&TextDisplay::new("")),
        json!({"type": 10, "content": ""})
    );
    let owned: Cow<'static, str> = Cow::Owned(String::from("x"));
    assert_eq!(TextDisplay::new(owned).content(), "x");
}

#[test]
fn separator() {
    assert_eq!(Separator::new(), Separator::default());
    assert_eq!(Separator::new(), Separator::small());
    insta::assert_json_snapshot!("separator_default", Separator::new());
    insta::assert_json_snapshot!("separator_large", Separator::large().divider(false));
    assert_eq!(
        to_json(&Separator::large().divider(false).spacing(Spacing::Small)),
        json!({"type": 14, "divider": false, "spacing": 1})
    );
    assert_eq!(Spacing::default(), Spacing::Small);
}

#[test]
fn container() {
    insta::assert_json_snapshot!("container_minimal", Container::new());
    insta::assert_json_snapshot!(
        "container_full",
        Container::new()
            .accent(Rgb::new(0x58, 0x65, 0xF2))
            .spoiler(true)
            .push(TextDisplay::new("a"))
    );
}

#[test]
fn rgb_bounds() {
    const BLACK: Rgb = Rgb::new(0, 0, 0);
    assert_eq!(BLACK.value(), 0);
    assert_eq!(Rgb::new(255, 255, 255).value(), 16_777_215);
    assert_eq!(
        to_json(&Container::new().accent(BLACK))["accent_color"],
        json!(0)
    );
    assert_eq!(
        to_json(&Container::new().accent(Rgb::new(255, 255, 255)))["accent_color"],
        json!(16_777_215)
    );
}

// Buttons.

#[test]
fn route_button() {
    insta::assert_json_snapshot!("route_button_default", button(buy()));
    insta::assert_json_snapshot!(
        "route_button_full",
        button(buy())
            .style(ButtonStyle::Danger)
            .label("Buy")
            .emoji(Emoji::unicode("🛒"))
            .disabled(true)
    );
    for (style, code) in [
        (ButtonStyle::Primary, 1),
        (ButtonStyle::Secondary, 2),
        (ButtonStyle::Success, 3),
        (ButtonStyle::Danger, 4),
    ] {
        assert_eq!(to_json(&button(buy()).style(style))["style"], json!(code));
    }
    assert_eq!(ButtonStyle::default(), ButtonStyle::Secondary);
}

#[test]
fn emoji() {
    insta::assert_json_snapshot!("emoji_custom", Emoji::custom(Id::new(42), "pog", true));
    assert_eq!(
        to_json(&Emoji::custom(Id::new(42), "pog", false)),
        json!({"id": "42", "name": "pog"})
    );
}

#[test]
fn link_button() {
    insta::assert_json_snapshot!("link_button", LinkButton::new("https://x.y").label("Docs"));
    assert_eq!(
        to_json(
            &LinkButton::new("https://x.y")
                .emoji(Emoji::unicode("📖"))
                .disabled(true)
        ),
        json!({"type": 2, "style": 5, "emoji": {"name": "📖"}, "url": "https://x.y", "disabled": true})
    );
}

#[test]
fn premium_button() {
    insta::assert_json_snapshot!(
        "premium_button",
        PremiumButton::new(Id::new(7)).disabled(true)
    );
}

#[test]
fn action_row() {
    let b1 = button(buy()).label("1");
    let b2 = button(Buy { item: 1 }).label("2");
    insta::assert_json_snapshot!("action_row_buttons", ActionRow::buttons([b1, b2]));
    insta::assert_json_snapshot!(
        "action_row_mixed",
        ActionRow::buttons([
            Button::from(button(buy())),
            Button::from(LinkButton::new("https://x.y")),
        ])
    );
    // Arity is bounded (`RowButtons`): 0 and 6 buttons do not compile, and
    // the runtime path rejects them.
    let links = |n: usize| (0..n).map(|i| LinkButton::new(format!("https://{i}")));
    let limits = noctis_rules::Limits::default();
    for n in [0, 6] {
        assert!(matches!(
            ActionRow::try_buttons(links(n), &limits),
            Err(UiError::Limits { violations }) if violations[0].rule == noctis_rules::RuleId::CV2_06
        ));
    }
    let five = ActionRow::try_buttons(links(5), &limits).expect("5 buttons");
    assert_eq!(
        to_json(&five)["components"].as_array().map(Vec::len),
        Some(5)
    );
}

#[test]
fn action_row_select() {
    let mut body = Components::new().push(ActionRow::select(role_select(Pick { page: 0 })));
    resolve_selects(&mut body);
    insta::assert_json_snapshot!("action_row_select", body);
}

// Section and media.

#[test]
fn section() {
    insta::assert_json_snapshot!(
        "section_thumbnail",
        Section::new(
            [TextDisplay::new("a")],
            Thumbnail::new(Media::url("https://i/x.png"))
        )
    );
    let text = || [TextDisplay::new("a")];
    let accessories = [
        to_json(&Section::new(text(), button(buy()))),
        to_json(&Section::new(text(), LinkButton::new("https://x.y"))),
        to_json(&Section::new(text(), PremiumButton::new(Id::new(7)))),
        to_json(&Section::new(
            text(),
            Button::from(LinkButton::new("https://x.y")),
        )),
    ];
    let expected = [
        to_json(&button(buy())),
        to_json(&LinkButton::new("https://x.y")),
        to_json(&PremiumButton::new(Id::new(7))),
        to_json(&LinkButton::new("https://x.y")),
    ];
    for (section, accessory) in accessories.iter().zip(expected) {
        assert_eq!(section["accessory"], accessory);
    }
}

#[test]
fn thumbnail() {
    insta::assert_json_snapshot!(
        "thumbnail_full",
        Thumbnail::new(Media::url("https://i/x.png"))
            .alt("A")
            .spoiler(true)
    );
}

#[test]
fn media_gallery() {
    insta::assert_json_snapshot!(
        "media_gallery",
        MediaGallery::new([MediaItem::new(Media::url("https://i/x.png")).alt("x")])
    );
    assert_eq!(
        to_json(&MediaItem::new(Media::url("u")).spoiler(true)),
        json!({"media": {"url": "u"}, "spoiler": true})
    );
}

#[test]
fn cv2_12_file_url_is_attachment_ref() {
    let r = media_ref();
    let file = File::new(r.clone()).spoiler(true);
    assert_eq!(file.attachment(), &r);
    assert_eq!(
        to_json(&file),
        json!({"type": 13, "file": {"url": "attachment://af1349b9f5f9a1a6.png"}, "spoiler": true})
    );
    insta::assert_json_snapshot!("file", file);
}

#[test]
fn media_from_attachment() {
    let r = media_ref();
    let media = Media::from(r.clone());
    assert_eq!(
        to_json(&media),
        json!({"url": "attachment://af1349b9f5f9a1a6.png"})
    );
    assert_eq!(media.attachment(), Some(&r));
    assert_eq!(
        media.attachment().map(noctis_ui::AttachmentRef::origin),
        Some(AttachmentOrigin::Plain)
    );
    assert_eq!(Media::url("https://x").attachment(), None);
}

// Selects.

#[test]
fn string_select_options() {
    let select = string_select(
        Pick { page: 0 },
        [SelectOption::new("A", "a")
            .description("d")
            .emoji(Emoji::unicode("🅰"))
            .default(true)],
    )
    .placeholder("p")
    .min_values(1)
    .max_values(2)
    .disabled(true);
    let mut body = Components::new().push(ActionRow::select(select));
    resolve_selects(&mut body);
    insta::assert_json_snapshot!("string_select", body);

    let mut empty = Components::new().push(ActionRow::select(string_select(Pick { page: 0 }, [])));
    resolve_selects(&mut empty);
    assert_eq!(
        to_json(&empty)[0]["components"][0],
        json!({"type": 3, "id": 2, "custom_id": "4pick:010", "options": []})
    );
}

#[test]
fn auto_populated_selects() {
    let mut body = Components::new()
        .push(ActionRow::select(
            user_select(Pick { page: 0 }).default_users([Id::new(1)]),
        ))
        .push(ActionRow::select(
            role_select(Pick { page: 1 }).default_roles([Id::new(2)]),
        ))
        .push(ActionRow::select(
            mentionable_select(Pick { page: 2 })
                .default_roles([Id::new(4)])
                .default_users([Id::new(3)]),
        ))
        .push(ActionRow::select(
            channel_select(Pick { page: 3 })
                .default_channels([Id::new(5)])
                .channel_types([ChannelType::GuildText, ChannelType::GuildVoice]),
        ))
        .push(ActionRow::select(
            channel_select(Pick { page: 4 })
                .default_channels(None)
                .channel_types([]),
        ));
    resolve_selects(&mut body);
    insta::assert_json_snapshot!("auto_populated_selects", body);
    let json = to_json(&body);
    let types: Vec<_> = (0..5)
        .map(|i| json[i]["components"][0]["type"].clone())
        .collect();
    assert_eq!(types, [json!(5), json!(6), json!(7), json!(8), json!(8)]);
    let last = &json[4]["components"][0];
    assert!(last.get("default_values").is_none() && last.get("channel_types").is_none());
}

// Components and numeric ids.

#[test]
fn cv2_14_ids_are_preorder() {
    let body = Components::new()
        .push(Container::new().push(Section::new(
            [TextDisplay::new("a"), TextDisplay::new("b")],
            button(buy()),
        )))
        .push(ActionRow::buttons([button(Buy { item: 1 })]));
    insta::assert_json_snapshot!("components_ids", body);
    let json = to_json(&body);
    let section = &json[0]["components"][0];
    assert_eq!(json[0]["id"], 1);
    assert_eq!(section["id"], 2);
    assert_eq!(section["components"][0]["id"], 3);
    assert_eq!(section["components"][1]["id"], 4);
    assert_eq!(section["accessory"]["id"], 5);
    assert_eq!(json[1]["id"], 6);
    assert_eq!(json[1]["components"][0]["id"], 7);
    // `id` comes right after `type`.
    let text = serde_json::to_string(&body).expect("serializes");
    assert!(text.starts_with(r#"[{"type":17,"id":1,"#), "{text}");
    // A lone component carries no id.
    assert!(to_json(&TextDisplay::new("a")).get("id").is_none());
}

#[test]
fn cv2_14_gallery_items_and_options_have_no_id() {
    let mut body = Components::new()
        .push(MediaGallery::new([MediaItem::new(Media::url("u"))]))
        .push(ActionRow::select(string_select(
            Pick { page: 0 },
            [SelectOption::new("A", "a")],
        )));
    resolve_selects(&mut body);
    let json = to_json(&body);
    assert_eq!(json[0]["items"][0], json!({"media": {"url": "u"}}));
    assert_eq!(json[1]["components"][0]["id"], 3);
    assert_eq!(
        json[1]["components"][0]["options"][0],
        json!({"label": "A", "value": "a"})
    );
}

#[test]
fn empty_body() {
    let body = Components::new();
    assert_eq!(to_json(&body), json!([]));
    assert_eq!(body.len(), 0);
    assert!(body.is_empty());
    assert!(Components::default().is_empty());
}

#[test]
fn opaque_components() {
    let children: Vec<ContainerComponent> = vec![
        TextDisplay::new("a").into(),
        Separator::new().into(),
        ActionRow::buttons([button(buy())]).into(),
    ];
    let container = children.into_iter().fold(Container::new(), Container::push);
    let top: Vec<TopLevelComponent> = vec![
        container.into(),
        TextDisplay::new("b").into(),
        ContainerComponent::from(Separator::large()).into(),
    ];
    let body = top.into_iter().fold(Components::new(), Components::push);
    assert_eq!(body.len(), 3);
    let json = to_json(&body);
    assert_eq!(
        json[0]["components"][2]["components"][0]["custom_id"],
        "0shop.buy:47"
    );
    assert_eq!(json[2]["spacing"], 2);
}

// `custom_id` resolution.

#[test]
fn plain_route_is_eager() {
    let mut body = Components::new().push(ActionRow::buttons([button(buy())]));
    let ids: Vec<_> = body.custom_ids_mut().collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].get(), Some("0shop.buy:47"));
    assert_eq!(ids[0].route(), RouteId::new("shop.buy"));
}

#[test]
fn unresolved_routes_fail_to_serialize() {
    let bodies = [
        (
            "signed",
            Components::new().push(ActionRow::buttons([button(Signed { n: 1 })])),
        ),
        (
            "tagged",
            Components::new().push(ActionRow::buttons([button(Tagged { n: 1 })])),
        ),
        (
            "stored",
            Components::new().push(ActionRow::buttons([button(Stored { n: 1 })])),
        ),
        (
            "pick",
            Components::new().push(ActionRow::select(user_select(Pick { page: 1 }))),
        ),
    ];
    for (route, mut body) in bodies {
        assert_eq!(body.custom_ids_mut().next().and_then(|id| id.get()), None);
        let err = serde_json::to_string(&body)
            .expect_err("unresolved")
            .to_string();
        assert!(err.contains(route) && err.contains("unresolved"), "{err}");
    }
}

#[test]
fn signed_routes_resolve() {
    let key = PayloadKey::from_bytes([7; 32]);
    let mut body = Components::new()
        .push(ActionRow::buttons([button(Signed { n: 1 })]))
        .push(ActionRow::buttons([button(Tagged { n: 1 })]));
    let mut ids = body.custom_ids_mut();
    let signed = ids.next().expect("signed id");
    assert_eq!(
        signed.resolve(&Parts::new()),
        Err(EncodeError::MissingSigner)
    );
    assert_eq!(signed.resolve(&Parts::new().with_signer(&key)), Ok(None));
    let tagged = ids.next().expect("tagged id");
    assert_eq!(tagged.resolve(&Parts::new().with_signer(&key)), Ok(None));
    drop(ids);
    assert!(serde_json::to_string(&body).is_ok());
}

/// Story 7.2 Matrix 17-19: `resolve_for_send` passes the nonce only to a
/// select Route and the signer only to a signed Route; a plain id is
/// already resolved, a Ref Route gives its slot, and a signed Route without
/// a signer is `MissingSigner`.
#[test]
fn resolve_for_send_matrix() {
    use noctis_codec::Sign;

    let key = PayloadKey::from_bytes([7; 32]);
    let ring: &dyn Sign = &key;
    let mut body = Components::new()
        .push(ActionRow::buttons([button(Buy { item: 1 })]))
        .push(ActionRow::buttons([button(Signed { n: 1 })]))
        .push(ActionRow::buttons([button(Stored { n: 1 })]))
        .push(ActionRow::select(user_select(Pick { page: 1 })));
    let mut ids = body.custom_ids_mut();
    let plain = ids.next().expect("plain id");
    let before = plain.get().map(str::to_owned);
    assert!(before.is_some());
    assert_eq!(plain.resolve_for_send(nonce(), Some(ring)), Ok(None));
    assert_eq!(plain.get().map(str::to_owned), before);
    let signed = ids.next().expect("signed id");
    assert_eq!(
        signed.resolve_for_send(nonce(), None),
        Err(EncodeError::MissingSigner)
    );
    assert_eq!(signed.get(), None);
    assert_eq!(signed.resolve_for_send(nonce(), Some(ring)), Ok(None));
    let mut expected = Components::new().push(ActionRow::buttons([button(Signed { n: 1 })]));
    let reference = expected.custom_ids_mut().next().expect("one id");
    assert_eq!(reference.resolve(&Parts::new().with_signer(&key)), Ok(None));
    assert_eq!(signed.get(), reference.get());
    let stored = ids.next().expect("stored id");
    let slot = stored
        .resolve_for_send(nonce(), Some(ring))
        .expect("encodes");
    assert!(slot.is_some());
    assert_eq!(stored.get(), None);
    let select = ids.next().expect("select id");
    assert_eq!(select.resolve_for_send(nonce(), None), Ok(None));
    assert_eq!(select.get(), Some("4pick:011"));
}

#[test]
fn section_accessory_ids_resolve() {
    let key = PayloadKey::from_bytes([7; 32]);
    let mut body = Components::new()
        .push(Container::new().push(Section::new(
            [TextDisplay::new("a")],
            button(Signed { n: 1 }),
        )))
        .push(Section::new(
            [TextDisplay::new("b")],
            button(Tagged { n: 1 }),
        ));
    let ids: Vec<_> = body.custom_ids_mut().collect();
    assert_eq!(
        ids.iter().map(|id| id.route()).collect::<Vec<_>>(),
        [RouteId::new("signed"), RouteId::new("tagged")]
    );
    let [signed, tagged] = <[_; 2]>::try_from(ids).expect("exactly two ids");
    assert_eq!(signed.resolve(&Parts::new().with_signer(&key)), Ok(None));
    assert_eq!(tagged.resolve(&Parts::new().with_signer(&key)), Ok(None));
    assert!(serde_json::to_string(&body).is_ok());
}

#[test]
fn spill_route() {
    let mut fits = Components::new().push(ActionRow::buttons([button(Spilled {
        text: "short".to_owned(),
    })]));
    assert_eq!(
        fits.custom_ids_mut().next().and_then(|id| id.get()),
        Some("0spilled:short")
    );

    let mut long = Components::new().push(ActionRow::buttons([button(Spilled {
        text: "x".repeat(200),
    })]));
    let id = long.custom_ids_mut().next().expect("one id");
    assert_eq!(id.get(), None);
    let slot = id.resolve(&Parts::new()).expect("encodes").expect("a slot");
    assert_eq!(slot.route(), RouteId::new("spilled"));
}

#[test]
fn resolve_with_wrong_parts_keeps_state() {
    let mut body = Components::new().push(ActionRow::select(user_select(Pick { page: 1 })));
    let id = body.custom_ids_mut().next().expect("one id");
    assert_eq!(id.resolve(&Parts::new()), Err(EncodeError::MissingNonce));
    assert_eq!(id.get(), None);
    assert_eq!(id.resolve(&Parts::new().with_nonce(nonce())), Ok(None));
    assert_eq!(id.get(), Some("4pick:011"));
}

#[test]
fn finish_a_slot() {
    let mut body = Components::new()
        .push(ActionRow::buttons([button(Stored { n: 1 })]))
        .push(ActionRow::buttons([button(Spilled {
            text: "x".repeat(200),
        })]));
    let mut ids = body.custom_ids_mut();
    let stored = ids.next().expect("stored id");
    let spilled = ids.next().expect("spilled id");

    let slot = stored
        .resolve(&Parts::new())
        .expect("encodes")
        .expect("a slot");
    let other = spilled
        .resolve(&Parts::new())
        .expect("encodes")
        .expect("a slot");

    let r = Ref::mint().expect("mint");
    assert_eq!(
        stored.finish(&other, r),
        Err(UiError::SlotMismatch {
            expected: RouteId::new("stored"),
            actual: RouteId::new("spilled"),
        })
    );
    assert_eq!(stored.get(), None);

    assert_eq!(stored.finish(&slot, r), Ok(()));
    assert_eq!(stored.get(), Some(noctis_codec::finish(&slot, r).as_str()));
    let debug = format!("{stored:?}");
    assert!(
        debug.contains("stored") && debug.contains(r.as_str()),
        "{debug}"
    );
}

// Attachments.

#[test]
fn attachment_naming() {
    let a = Attachment::new(Bytes::new(), "png").expect("valid");
    assert_eq!(a.name(), "af1349b9f5f9a1a6.png");
    assert_eq!(a.origin(), AttachmentOrigin::Plain);
    for ext in ["a", "jpeg", "PNG", "abcdefghij"] {
        let a = Attachment::new(Bytes::new(), ext).expect("valid ext");
        assert_eq!(a.name(), format!("af1349b9f5f9a1a6.{ext}"));
    }
    for ext in ["", "abcdefghijk", ".png", "pn g", "pñg", "png/x"] {
        let expected = Err(UiError::InvalidExtension {
            ext: ext.to_owned(),
        });
        assert_eq!(Attachment::new(Bytes::new(), ext), expected);
        assert_eq!(
            Attachment::with_digest(Bytes::new(), [0; 32], ext),
            expected
        );
    }
}

#[test]
fn attachment_digests() {
    let a = Attachment::with_digest(Bytes::new(), [0xAB; 32], "png").expect("valid");
    assert_eq!(a.name(), "abababababababab.png");
    assert_eq!(a.origin(), AttachmentOrigin::Plain);
    let zero = Attachment::with_digest(Bytes::new(), [0; 32], "png").expect("valid");
    assert_eq!(zero.name(), "0000000000000000.png");
    let r = Attachment::rendered(Bytes::new(), [0xAB; 32], "webp").expect("valid");
    assert_eq!(r.name(), "abababababababab.webp");
    assert_eq!(r.origin(), AttachmentOrigin::Rendered);
    assert_eq!(
        Attachment::rendered(Bytes::new(), [0; 32], "we bp"),
        Err(UiError::InvalidExtension {
            ext: "we bp".to_owned()
        })
    );
}

#[test]
fn attachment_fields() {
    let a = Attachment::new(&b"img"[..], "png").expect("valid");
    assert_eq!(a.alt_text(), None);
    assert_eq!(a.mime(), None);
    assert_eq!(a.bytes(), &Bytes::from_static(b"img"));
    let a = a.alt("x").content_type("image/png");
    assert_eq!(a.alt_text(), Some("x"));
    assert_eq!(a.mime(), Some("image/png"));
}

#[test]
fn attachment_set() {
    let mut set = AttachmentSet::new();
    assert!(set.is_empty());
    let first = Attachment::new(&b"same"[..], "png")
        .expect("valid")
        .alt("first");
    let second = Attachment::new(&b"same"[..], "png")
        .expect("valid")
        .alt("second");
    let r1 = set.add(first);
    let r2 = set.add(second);
    assert_eq!(r1, r2);
    assert_eq!(set.len(), 1);
    assert_eq!(
        set.get(r1.name()).and_then(Attachment::alt_text),
        Some("first")
    );
    assert_eq!(r1.url(), format!("attachment://{}", r1.name()));
    assert_eq!(r1.origin(), AttachmentOrigin::Plain);

    let d1 = set.add(Attachment::with_digest(&b"a"[..], [1; 32], "png").expect("valid"));
    let d2 =
        set.add(Attachment::rendered(Bytes::from_static(b"b"), [1; 32], "png").expect("valid"));
    assert_eq!(d1, d2);
    assert_eq!(set.len(), 2);

    let z = set.add(Attachment::with_digest(&b"z"[..], [0; 32], "gif").expect("valid"));
    let names: Vec<_> = set.iter().map(Attachment::name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    assert_eq!(names[0], z.name());
    assert!(set.get("missing.png").is_none());
    assert!(set.get(&"x".repeat(100)).is_none());
}

const fn assert_send_sync<T: Send + Sync + Clone + fmt::Debug>() {}

const _: () = {
    assert_send_sync::<Components>();
    assert_send_sync::<Container>();
    assert_send_sync::<ContainerComponent>();
    assert_send_sync::<TopLevelComponent>();
    assert_send_sync::<Section>();
    assert_send_sync::<ActionRow>();
    assert_send_sync::<Button>();
    assert_send_sync::<RouteButton<Buy>>();
    assert_send_sync::<StringSelect>();
    assert_send_sync::<ChannelSelect>();
    assert_send_sync::<AttachmentSet>();
    assert_send_sync::<noctis_ui::AttachmentRef>();
    assert_send_sync::<UiError>();
};
