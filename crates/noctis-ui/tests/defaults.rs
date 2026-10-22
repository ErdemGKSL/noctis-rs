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

// Story 9.3: per-render overrides of the declared defaults.

/// Validates `row` in `mode`, resolving the select nonce first.
fn check_row(row: ActionRow, mode: LimitMode) -> Result<V2Message, Vec<noctis_rules::Violation>> {
    let mut body = Components::new().push(row);
    for id in body.custom_ids_mut() {
        if id.get().is_none() {
            let nonce = SelectNonce::new(1).expect("in range");
            assert_eq!(id.resolve(&Parts::new().with_nonce(nonce)), Ok(None));
        }
    }
    let mut msg = V2Message::new(body);
    let mut limits = Limits::default();
    limits.mode = mode;
    match validate(&mut msg, &limits) {
        Ok(_) => Ok(msg),
        Err(UiError::Limits { violations }) => Err(violations),
        Err(other) => panic!("{other}"),
    }
}

/// The CV2-09 violations of `row` in both modes.
fn cv2_09_both_modes(row: ActionRow) -> noctis_rules::Violation {
    let strict = check_row(row.clone(), LimitMode::Strict).expect_err("Strict rejects");
    let lenient = check_row(row, LimitMode::Lenient).expect_err("Lenient rejects");
    assert_eq!(strict[0].rule, RuleId::CV2_09, "{strict:?}");
    assert_eq!(lenient[0].rule, RuleId::CV2_09, "{lenient:?}");
    strict.into_iter().next().expect("one violation")
}

fn first_select(msg: &V2Message) -> Value {
    let mut select = to_json(msg.components())[0]["components"][0].clone();
    if let Some(map) = select.as_object_mut() {
        map.remove("id");
    }
    select
}

/// B1, B4: no overrides, an empty overrides and a second render all equal
/// the defaults.
#[test]
fn b1_b4_no_overrides_equal_defaults() {
    let plain = to_json(&button(Buy { item: 1 }));
    let empty = to_json(&button(Buy { item: 1 }).overrides(noctis_ui::ButtonOverrides::default()));
    assert_eq!(plain, empty);
    let _changed = button(Buy { item: 1 }).overrides(noctis_ui::ButtonOverrides {
        label: Some("Again".into()),
        ..Default::default()
    });
    assert_eq!(to_json(&button(Buy { item: 1 })), plain);
}

/// B2, B5: several fields at once; the rest from the defaults.
#[test]
fn b2_b5_multi_field_overrides() {
    let b = to_json(
        &button(Buy { item: 1 }).overrides(noctis_ui::ButtonOverrides {
            label: Some("Again".into()),
            disabled: Some(true),
            ..Default::default()
        }),
    );
    assert_eq!(
        b,
        json!({"type": 2, "style": 1, "label": "Again", "custom_id": "0shop.buy:1", "disabled": true})
    );
    let b = to_json(
        &button(Buy { item: 1 }).overrides(noctis_ui::ButtonOverrides {
            emoji: Some(Emoji::unicode("🔥")),
            style: Some(ButtonStyle::Danger),
            ..Default::default()
        }),
    );
    assert_eq!(
        (&b["emoji"]["name"], &b["style"], &b["label"]),
        (&json!("🔥"), &json!(4), &json!("Buy"))
    );
}

/// B3: setters and `.overrides` compose; the later call wins.
#[test]
fn b3_later_call_wins() {
    let label = |b: RouteButton<Buy>| to_json(&b)["label"].clone();
    let over = |text: &'static str| noctis_ui::ButtonOverrides {
        label: Some(text.into()),
        ..Default::default()
    };
    assert_eq!(
        label(button(Buy { item: 1 }).label("a").overrides(over("b"))),
        json!("b")
    );
    assert_eq!(
        label(button(Buy { item: 1 }).overrides(over("b")).label("a")),
        json!("a")
    );
    let styled = button(Buy { item: 1 })
        .style(ButtonStyle::Danger)
        .overrides(noctis_ui::ButtonOverrides {
            style: Some(ButtonStyle::Success),
            ..Default::default()
        });
    assert_eq!(to_json(&styled)["style"], json!(3));
}

/// T6: an override label of 80 emoji passes, 81 is CV2-08 (`discord_len`).
#[test]
fn t6_unicode_label_bounds() {
    let row = |n: usize| ActionRow::buttons([button(Buy { item: 1 }).label("😀".repeat(n))]);
    assert!(check_row(row(40), LimitMode::Strict).is_ok());
    let violations = check_row(row(41), LimitMode::Strict).expect_err("81 > 80");
    assert_eq!(violations[0].rule, RuleId::CV2_08);
}

/// S1: a placeholder of 150 passes; 151 is CV2-09 Strict and is truncated
/// Lenient.
#[test]
fn s1_placeholder_bounds() {
    let row = |n: usize| {
        ActionRow::select(build_user_select(Pick {}, &Pick::DEFAULTS).overrides(
            noctis_ui::UserSelectOverrides {
                placeholder: Some("p".repeat(n).into()),
                ..Default::default()
            },
        ))
    };
    assert!(check_row(row(150), LimitMode::Strict).is_ok());
    assert_eq!(
        check_row(row(151), LimitMode::Strict).expect_err("151")[0].rule,
        RuleId::CV2_09
    );
    let lenient = check_row(row(151), LimitMode::Lenient).expect("Lenient truncates");
    let placeholder = first_select(&lenient)["placeholder"]
        .as_str()
        .map(|p| p.chars().count());
    assert_eq!(placeholder, Some(150));
}

/// S2: value counts are checked in both modes.
#[test]
fn s2_counts() {
    let row = |min: Option<u8>, max: Option<u8>| {
        ActionRow::select(build_role_select(Pick {}, &Pick::DEFAULTS).overrides(
            noctis_ui::RoleSelectOverrides {
                min_values: min,
                max_values: max,
                ..Default::default()
            },
        ))
    };
    assert!(check_row(row(None, Some(25)), LimitMode::Strict).is_ok());
    assert!(check_row(row(Some(0), None), LimitMode::Strict).is_ok());
    cv2_09_both_modes(row(None, Some(26)));
    cv2_09_both_modes(row(Some(26), None));
    cv2_09_both_modes(row(Some(3), Some(2)));
}

fn abc() -> Vec<SelectOption> {
    vec![
        SelectOption::new("A", "a").default(true),
        SelectOption::new("B", "b"),
        SelectOption::new("C", "c"),
    ]
}

fn defaults_of(select: &Value) -> Vec<&str> {
    select["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["default"] == json!(true))
        .filter_map(|o| o["value"].as_str())
        .collect()
}

/// S3: the options override replaces the declared options; empty or 26
/// options are CV2-09 in both modes.
#[test]
fn s3_options_override() {
    let with = |options: Vec<SelectOption>| {
        ActionRow::select(
            build_string_select(Colourful {}, &Colourful::DEFAULTS).overrides(
                noctis_ui::StringSelectOverrides {
                    options: Some(options),
                    ..Default::default()
                },
            ),
        )
    };
    let msg =
        check_row(with(vec![SelectOption::new("o", "o")]), LimitMode::Strict).expect("one option");
    assert_eq!(
        first_select(&msg)["options"],
        json!([{"label": "o", "value": "o"}])
    );
    cv2_09_both_modes(with(Vec::new()));
    cv2_09_both_modes(with(
        (0..26)
            .map(|i| SelectOption::new(format!("{i}"), format!("{i}")))
            .collect(),
    ));
}

/// S4: a default list replaces every option's own flag, in either order.
#[test]
fn s4_string_default_list() {
    let after = build_string_select(Ticket {}, &Ticket::DEFAULTS)
        .options(abc())
        .default_values(["b"]);
    let builder: StringSelectBuilder<Ticket> =
        build_string_select_builder(Ticket {}, &Ticket::DEFAULTS.select);
    let before = builder.default_values(["b"]).options(abc());
    for select in [after, before] {
        let msg = check_row(ActionRow::select(select), LimitMode::Strict).expect("valid");
        assert_eq!(defaults_of(&first_select(&msg)), ["b"]);
    }
    // Without a list, the options' own flags stand.
    let own = build_string_select(Ticket {}, &Ticket::DEFAULTS).options(abc());
    let msg = check_row(ActionRow::select(own), LimitMode::Strict).expect("valid");
    assert_eq!(defaults_of(&first_select(&msg)), ["a"]);
}

/// S5: a default that no option has is CV2-09 at its index, both modes.
#[test]
fn s5_unknown_default() {
    let select = build_string_select(Ticket {}, &Ticket::DEFAULTS)
        .options(abc())
        .default_values(["z"]);
    let v = cv2_09_both_modes(ActionRow::select(select));
    assert!(v.path.ends_with("default_values[0]"), "{}", v.path);
    assert_eq!(v.actual, noctis_rules::Actual::Text("z".into()));
}

/// S6: the default count is checked against min and max; duplicates count
/// once.
#[test]
fn s6_default_count() {
    let with = |values: &[&'static str], min: Option<u8>, max: Option<u8>| {
        ActionRow::select(
            build_string_select(Ticket {}, &Ticket::DEFAULTS)
                .options(abc())
                .overrides(noctis_ui::StringSelectOverrides {
                    min_values: min,
                    max_values: max,
                    default_values: Some(values.iter().map(|v| (*v).into()).collect()),
                    ..Default::default()
                }),
        )
    };
    cv2_09_both_modes(with(&["a", "b"], None, Some(1)));
    assert!(check_row(with(&["a", "b"], Some(2), Some(3)), LimitMode::Strict).is_ok());
    let msg = check_row(with(&["b", "b"], None, None), LimitMode::Strict).expect("one default");
    assert_eq!(defaults_of(&first_select(&msg)), ["b"]);
}

/// S7: entity defaults, their count and their dedup.
#[test]
fn s7_entity_defaults() {
    let users = |ids: &[u64], max: u8| {
        ActionRow::select(
            build_user_select(Pick {}, &Pick::DEFAULTS)
                .max_values(max)
                .default_values(ids.iter().map(|i| Id::new(*i))),
        )
    };
    let msg = check_row(users(&[1, 2], 2), LimitMode::Strict).expect("two of two");
    assert_eq!(
        first_select(&msg)["default_values"],
        json!([{"id": "1", "type": "user"}, {"id": "2", "type": "user"}])
    );
    cv2_09_both_modes(users(&[1, 2, 3], 2));
    let msg = check_row(users(&[], 2), LimitMode::Strict).expect("none");
    assert!(first_select(&msg).get("default_values").is_none());
    let msg = check_row(users(&[1, 1], 2), LimitMode::Strict).expect("deduplicated");
    assert_eq!(
        first_select(&msg)["default_values"],
        json!([{"id": "1", "type": "user"}])
    );
}

/// S8: mentionable defaults keep their kind and order.
#[test]
fn s8_mentionable_defaults() {
    let select = build_mentionable_select(Pick {}, &Pick::DEFAULTS).overrides(
        noctis_ui::MentionableSelectOverrides {
            max_values: Some(2),
            default_values: Some(vec![
                noctis_ui::MentionableId::User(Id::new(1)),
                noctis_ui::MentionableId::Role(Id::new(2)),
            ]),
            ..Default::default()
        },
    );
    let msg = check_row(ActionRow::select(select), LimitMode::Strict).expect("valid");
    assert_eq!(
        first_select(&msg)["default_values"],
        json!([{"id": "1", "type": "user"}, {"id": "2", "type": "role"}])
    );
}

/// S9: channel types and channel defaults through the overrides.
#[test]
fn s9_channel_overrides() {
    let select = build_channel_select(Where {}, &Where::DEFAULTS).overrides(
        noctis_ui::ChannelSelectOverrides {
            channel_types: Some(vec![ChannelType::GuildText]),
            default_values: Some(vec![Id::new(9)]),
            ..Default::default()
        },
    );
    let msg = check_row(ActionRow::select(select), LimitMode::Strict).expect("valid");
    let json = first_select(&msg);
    assert_eq!(json["channel_types"], json!([0]));
    assert_eq!(
        json["default_values"],
        json!([{"id": "9", "type": "channel"}])
    );
}
