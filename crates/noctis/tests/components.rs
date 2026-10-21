//! Components built through the facade: kind-derived structs built with
//! `.build()`, a
//! Container/Section/row tree from `noctis::ui` and `noctis::prelude`, and a
//! select `custom_id` resolved through the hidden hooks (FR-23, FR-58,
//! AD-18).

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use noctis::__private::codec::{Parts, SelectNonce};
use noctis::codec::SelectRoute;
use noctis::ui::{
    ActionRow, ButtonStyle, Components, Container, Rgb, Section, SelectOption, Separator,
    TextDisplay,
};
use serde_json::json;

#[derive(::noctis::Button)]
#[button("shop.buy", defaults(style = Secondary, label = "Go"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::StringSelectMenu)]
#[string_select("shop.size", defaults(placeholder = "Pick"))]
struct Size {
    item: u64,
}

impl Size {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

const fn assert_select<R: SelectRoute>() {}
const _: () = assert_select::<Size>();

#[test]
fn derived_routes_build_a_tree() {
    let mut body = Components::new()
        .push(
            Container::new()
                .accent(Rgb::new(0x58, 0x65, 0xF2))
                .push(Section::new(
                    [TextDisplay::new("**Sword**")],
                    Buy { item: 255 }
                        .build()
                        .style(ButtonStyle::Primary)
                        .label("Buy"),
                ))
                .push(Separator::small())
                .push(ActionRow::select(
                    Size { item: 255 }
                        .build()
                        .options([SelectOption::new("Small", "s").default(true)])
                        .placeholder("Size"),
                )),
        )
        .push(ActionRow::buttons([Buy { item: 1 }.build()]));

    // The select id waits for the per-render nonce.
    assert!(serde_json::to_string(&body).is_err());
    let nonce = SelectNonce::new(1).expect("in range");
    for id in body.custom_ids_mut() {
        if id.get().is_none() {
            assert_eq!(id.resolve(&Parts::new().with_nonce(nonce)), Ok(None));
        }
    }

    assert_eq!(
        serde_json::to_value(&body).expect("serializes"),
        json!([
            {
                "type": 17,
                "id": 1,
                "components": [
                    {
                        "type": 9,
                        "id": 2,
                        "components": [{"type": 10, "id": 3, "content": "**Sword**"}],
                        "accessory": {
                            "type": 2,
                            "id": 4,
                            "style": 1,
                            "label": "Buy",
                            "custom_id": "0shop.buy:47"
                        }
                    },
                    {"type": 14, "id": 5, "divider": true, "spacing": 1},
                    {
                        "type": 1,
                        "id": 6,
                        "components": [{
                            "type": 3,
                            "id": 7,
                            "custom_id": "4shop.size:0147",
                            "options": [{"label": "Small", "value": "s", "default": true}],
                            "placeholder": "Size"
                        }]
                    }
                ],
                "accent_color": 5_793_266
            },
            {
                "type": 1,
                "id": 8,
                "components": [{"type": 2, "id": 9, "style": 2, "label": "Go", "custom_id": "0shop.buy:1"}]
            }
        ])
    );
}
