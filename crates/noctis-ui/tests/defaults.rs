//! Story 6.11: components built from a Route value and its declared
//! `defaults(..)` (Matrix 29-31, 47-49), with the traits the kind derives
//! implement written by hand (this crate cannot use the macros).

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use core::fmt;

use noctis_codec::{
    Field, FieldWriter, Parts, Payload, Route, RouteId, RouteShape, SelectNonce, SelectRoute,
};
use noctis_rules::{LimitMode, Limits, RuleId};
use noctis_ui::__macro::{
    ButtonDefaults, ChannelSelectDefaults, ComponentRoute, DefaultEmoji, ModalDefaults,
    SelectDefaults, StringSelectDefaults, build_button, build_channel_select,
    build_mentionable_select, build_modal, build_role_select, build_string_select,
    build_string_select_builder, build_user_select, choices_options,
};
use noctis_ui::{
    ActionRow, Arity, ButtonStyle, Choices, Components, Emoji, Handled, RouteButton, SelectOption,
    StringSelect, StringSelectBuilder, UiError, V2Message, validate,
};
use serde_json::{Value, json};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;

/// A hand-written handled Route whose Payload is itself, with its declared
/// defaults.
macro_rules! route {
    ($name:ident { $($field:ident: $ty:ty),* } = $id:literal, $shape:expr, $defaults:ty = $value:expr $(, $select:ident)?) => {
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

        impl ComponentRoute for $name {
            type Defaults = $defaults;
            const DEFAULTS: $defaults = $value;
        }
    };
}

const fn select(placeholder: &'static str) -> SelectDefaults {
    SelectDefaults {
        placeholder: Some(placeholder),
        min_values: None,
        max_values: None,
        disabled: false,
    }
}

const fn button_defaults(
    label: Option<&'static str>,
    emoji: Option<DefaultEmoji>,
) -> ButtonDefaults {
    ButtonDefaults {
        style: ButtonStyle::Primary,
        label,
        emoji,
        disabled: false,
    }
}

route!(
    Buy { item: u64 } = "shop.buy",
    RouteShape::INLINE,
    ButtonDefaults = button_defaults(Some("Buy"), None)
);
route!(
    Next {} = "next",
    RouteShape::INLINE,
    ButtonDefaults = button_defaults(None, Some(DefaultEmoji::Unicode("➡️")))
);
route!(
    Both {} = "both",
    RouteShape::INLINE,
    ButtonDefaults = button_defaults(Some("Go"), Some(DefaultEmoji::Unicode("➡️")))
);
route!(
    Custom {} = "custom",
    RouteShape::INLINE,
    ButtonDefaults = button_defaults(
        None,
        Some(DefaultEmoji::Custom {
            id: 123,
            name: "pt",
            animated: false
        })
    )
);
route!(
    Animated {} = "animated",
    RouteShape::INLINE,
    ButtonDefaults = button_defaults(
        None,
        Some(DefaultEmoji::Custom {
            id: 123,
            name: "pt",
            animated: true
        })
    )
);
route!(
    Pick {} = "pick",
    RouteShape::INLINE.select(),
    SelectDefaults = select("Pick"),
    SelectRoute
);
route!(
    Many {} = "many",
    RouteShape::INLINE.select(),
    SelectDefaults = SelectDefaults {
        min_values: Some(0),
        max_values: Some(25),
        ..select("Pick")
    },
    SelectRoute
);
route!(
    Off {} = "off",
    RouteShape::INLINE.select(),
    SelectDefaults = SelectDefaults {
        disabled: true,
        ..select("Pick")
    },
    SelectRoute
);
route!(
    Colourful {} = "colourful",
    RouteShape::INLINE.select(),
    StringSelectDefaults = StringSelectDefaults {
        select: select("Colour"),
        options: Some(choices_options::<Colour>)
    },
    SelectRoute
);
route!(
    Ticket {} = "ticket",
    RouteShape::INLINE.select(),
    StringSelectDefaults = StringSelectDefaults {
        select: select("Ticket"),
        options: None
    },
    SelectRoute
);
route!(
    Where {} = "where",
    RouteShape::INLINE.select(),
    ChannelSelectDefaults = ChannelSelectDefaults {
        select: select("Where"),
        channel_types: &[ChannelType::GuildText, ChannelType::GuildVoice]
    },
    SelectRoute
);
route!(Bio { user: Id<twilight_model::id::marker::UserMarker> } = "bio", RouteShape::INLINE, ModalDefaults = ModalDefaults { title: "Bio" });

/// A hand-written `Choices` enum (what `#[derive(Choices)]` implements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Colour {
    Red,
    Blue,
}

impl Choices for Colour {
    type Count = Arity<2>;
    const ALL: &'static [Self] = &[Self::Red, Self::Blue];

    fn label(&self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Blue => "Blue",
        }
    }

    fn value(&self) -> &'static str {
        match self {
            Self::Red => "r",
            Self::Blue => "b",
        }
    }

    fn description(&self) -> Option<&'static str> {
        match self {
            Self::Red => Some("Warm"),
            Self::Blue => None,
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.value() == value)
    }
}

/// What the derive's inherent `.build()` expands to, per kind.
fn button<R: ComponentRoute<Defaults = ButtonDefaults> + Route<Payload = R>>(
    route: R,
) -> RouteButton<R> {
    build_button(route, &R::DEFAULTS)
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

/// Serializes a select after resolving its nonce, as core does.
fn select_json(row: ActionRow) -> Value {
    let mut body = Components::new().push(row);
    for id in body.custom_ids_mut() {
        if id.get().is_none() {
            let nonce = SelectNonce::new(1).expect("in range");
            assert_eq!(id.resolve(&Parts::new().with_nonce(nonce)), Ok(None));
        }
    }
    let mut select = to_json(&body)[0]["components"][0].clone();
    if let Some(map) = select.as_object_mut() {
        map.remove("id");
    }
    select
}

/// Matrix 29.
#[test]
fn button_defaults_json() {
    assert_eq!(
        to_json(&button(Buy { item: 255 })),
        json!({"type": 2, "style": 1, "label": "Buy", "custom_id": "0shop.buy:47"})
    );
    assert_eq!(
        to_json(&button(Next {})),
        json!({"type": 2, "style": 1, "emoji": {"name": "➡️"}, "custom_id": "0next:"})
    );
    assert_eq!(
        to_json(&button(Both {})),
        json!({"type": 2, "style": 1, "label": "Go", "emoji": {"name": "➡️"}, "custom_id": "0both:"})
    );
    assert_eq!(
        to_json(&button(Custom {}))["emoji"],
        json!({"id": "123", "name": "pt"})
    );
    assert_eq!(
        to_json(&button(Animated {}))["emoji"],
        json!({"id": "123", "name": "pt", "animated": true})
    );
}

/// Matrix 30.
#[test]
fn select_defaults_json() {
    assert_eq!(
        select_json(ActionRow::select(build_user_select(
            Pick {},
            &Pick::DEFAULTS
        ))),
        json!({"type": 5, "custom_id": "4pick:01", "placeholder": "Pick"})
    );
    let many = select_json(ActionRow::select(build_role_select(
        Many {},
        &Many::DEFAULTS,
    )));
    assert_eq!(
        (many["min_values"].clone(), many["max_values"].clone()),
        (json!(0), json!(25))
    );
    let off = select_json(ActionRow::select(build_mentionable_select(
        Off {},
        &Off::DEFAULTS,
    )));
    assert_eq!(off["disabled"], json!(true));
    let colours = select_json(ActionRow::select(build_string_select(
        Colourful {},
        &Colourful::DEFAULTS,
    )));
    assert_eq!(
        colours["options"],
        json!([
            {"label": "Red", "value": "r", "description": "Warm"},
            {"label": "Blue", "value": "b"}
        ])
    );
    let channels = select_json(ActionRow::select(build_channel_select(
        Where {},
        &Where::DEFAULTS,
    )));
    assert_eq!(channels["channel_types"], json!([0, 2]));
    assert_eq!(channels["placeholder"], json!("Where"));
}

/// Matrix 31 (the derive's `blank` and `build` both call this).
#[test]
fn modal_defaults_json() {
    let user = Id::new(9);
    let modal = build_modal::<Bio>(Bio { user }, Bio::DEFAULTS.title);
    assert_eq!(
        to_json(&modal),
        json!({"custom_id": "0bio:9", "title": "Bio", "components": []})
    );
}

/// Matrix 47: the builder converts with `.options(..)`; an empty list is
/// a runtime CV2-09 (Strict error, Lenient warning).
#[test]
fn string_select_typestate() {
    let builder: StringSelectBuilder<Ticket> =
        build_string_select_builder(Ticket {}, &Ticket::DEFAULTS.select);
    let select: StringSelect = builder
        .clone()
        .placeholder("p")
        .options([SelectOption::new("A", "a")]);
    let json = select_json(ActionRow::select(select));
    assert_eq!(json["placeholder"], json!("p"));
    assert_eq!(json["options"][0]["value"], json!("a"));

    let empty = builder.options(Vec::new());
    let mut msg = V2Message::new(Components::new().push(ActionRow::select(empty)));
    let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
        panic!("an empty String Select fails CV2-09");
    };
    assert_eq!(violations[0].rule, RuleId::CV2_09);
    let mut lenient = Limits::default();
    lenient.mode = LimitMode::Lenient;
    match validate(&mut msg, &lenient) {
        Ok(report) => assert!(
            report
                .warnings
                .iter()
                .any(|w| w.rule() == Some(RuleId::CV2_09)),
            "{report:?}"
        ),
        Err(UiError::Limits { violations }) => assert_eq!(violations[0].rule, RuleId::CV2_09),
        Err(other) => panic!("{other}"),
    }
}

/// Matrix 48: each override replaces one field; the rest come from the
/// defaults.
#[test]
fn overrides_replace_one_field() {
    let b = to_json(
        &button(Buy { item: 1 })
            .label("x")
            .style(ButtonStyle::Danger)
            .disabled(true)
            .emoji(Emoji::unicode("🛒")),
    );
    assert_eq!(
        b,
        json!({"type": 2, "style": 4, "label": "x", "emoji": {"name": "🛒"}, "custom_id": "0shop.buy:1", "disabled": true})
    );
    let pick = select_json(ActionRow::select(
        build_user_select(Pick {}, &Pick::DEFAULTS)
            .placeholder("p")
            .max_values(3),
    ));
    assert_eq!(
        pick,
        json!({"type": 5, "custom_id": "4pick:01", "placeholder": "p", "max_values": 3})
    );
    let colours = select_json(ActionRow::select(
        build_string_select(Colourful {}, &Colourful::DEFAULTS)
            .options([SelectOption::new("o", "o")]),
    ));
    assert_eq!(colours["options"], json!([{"label": "o", "value": "o"}]));
    assert_eq!(colours["placeholder"], json!("Colour"));
    let channels = select_json(ActionRow::select(
        build_channel_select(Where {}, &Where::DEFAULTS).channel_types([ChannelType::GuildForum]),
    ));
    assert_eq!(channels["channel_types"], json!([15]));
}

/// Matrix 49: a select's id waits for its nonce.
#[test]
fn unresolved_select_id_is_a_serde_error() {
    let err =
        serde_json::to_value(build_user_select(Pick {}, &Pick::DEFAULTS)).expect_err("unresolved");
    assert!(
        err.to_string()
            .contains("custom_id of route `pick` is unresolved"),
        "{err}"
    );
}
