//! Declared rate limits through the facade (Story 13.4, FR-46, INT-14): every
//! kind attribute accepts `rate_limit(..)` and exposes it as `RATE_LIMITS`
//! (G1-G3, G14, G15), the stored descriptors carry the limits and the
//! `#[on_rate_limited]` method, and the declaration adds nothing to the
//! encoded `custom_id`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::time::Duration;

use noctis::__private::core::__testing::responded;
use noctis::__private::core::{__Component, __ContextMenu, __Modal, __SlashCommand};
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{
    Components, LegacyMessage, ResolvedMessage, ResolvedUser, TextDisplay, V2Message,
};
use noctis::{RateLimit, RateScope};

/// G1: one limit.
#[derive(SlashCommand)]
#[command("daily", description = "Claim", rate_limit(user, 3, per = "10s"))]
struct Daily;

impl Daily {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }

    #[on_rate_limited]
    async fn limited(_ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
        let text = format!("wait {}", hit.relative_timestamp());
        Ok(V2Message::new(
            Components::new().push(TextDisplay::new(text)),
        ))
    }
}

/// G2: two limits, declared order; a group Pattern.
#[derive(SlashCommand)]
#[command(
    "shop (buy|sell)",
    description = "Trade",
    rate_limit(user, 3, per = "10s"),
    rate_limit(guild, 100, per = "1m")
)]
struct Trade;

impl Trade {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: TradePath) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }
}

/// G3: manual, the 1 d maximum; G4: the boundaries.
#[derive(SlashCommand)]
#[command(
    "vote",
    description = "Vote",
    rate_limit(member, 1, per = "1d", manual),
    rate_limit(channel, 4294967295, per = "1s"),
    rate_limit(user, 1, per = "1h30m")
)]
struct Vote;

impl Vote {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }
}

/// No declaration: the default empty list.
#[derive(SlashCommand)]
#[command("plain", description = "Plain")]
struct Plain;

impl Plain {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }
}

#[derive(UserCommand)]
#[user_command("Wave", response = ephemeral, rate_limit(user, 2, per = "1m"))]
struct Wave;

impl Wave {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _target: ResolvedUser) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }
}

/// G14: `message` on a message command.
#[derive(MessageCommand)]
#[message_command(
    "Quote",
    rate_limit(message, 1, per = "1h"),
    rate_limit(user, 5, per = "1m")
)]
struct Quote;

impl Quote {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedMessage,
    ) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }

    #[on_rate_limited]
    async fn limited(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(V2Message::new(
            Components::new().push(TextDisplay::new("slow down")),
        ))
    }
}

mod auth {
    use super::{Guarded, Shaped};
    use noctis::prelude::*;

    #[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
    pub async fn ok(_ctx: &ComponentCtx, _this: &Guarded) -> HandlerResult<Access> {
        Ok(Access::Allow)
    }

    #[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
    pub async fn form_ok(_ctx: &ModalSubmitCtx, _this: &Shaped) -> HandlerResult<Access> {
        Ok(Access::Allow)
    }
}

/// G15: after `defaults(..)`, with `owner`, `guard` and `by_ref`; G14:
/// `message` on a button.
#[derive(Button)]
#[button(
    "rl.guarded",
    by_ref,
    owner = user,
    guard = auth::ok,
    defaults(style = Primary, label = "Go"),
    rate_limit(message, 1, per = "5s"),
    rate_limit(user, 2, per = "1m", manual)
)]
struct Guarded {
    user: Id<UserMarker>,
}

/// The codec-width pair: the same Route length with and without a limit.
#[derive(Button)]
#[button("rl.a", rate_limit(user, 1, per = "1s"), defaults(style = Secondary, label = "A"))]
struct WithLimit {
    n: u64,
}

#[derive(Button)]
#[button("rl.b", defaults(style = Secondary, label = "B"))]
struct WithoutLimit {
    n: u64,
}

#[derive(StringSelectMenu)]
#[string_select(
    "rl.str",
    rate_limit(message, 3, per = "1m"),
    defaults(placeholder = "Pick")
)]
struct Strs;

#[derive(UserSelectMenu)]
#[user_select(
    "rl.usr",
    rate_limit(message, 3, per = "1m"),
    defaults(placeholder = "Pick")
)]
struct Usrs;

#[derive(RoleSelectMenu)]
#[role_select(
    "rl.rol",
    rate_limit(message, 3, per = "1m"),
    defaults(placeholder = "Pick")
)]
struct Rols;

#[derive(MentionableSelectMenu)]
#[mentionable_select(
    "rl.men",
    rate_limit(message, 3, per = "1m"),
    defaults(placeholder = "Pick")
)]
struct Mens;

#[derive(ChannelSelectMenu)]
#[channel_select(
    "rl.chn",
    rate_limit(message, 3, per = "1m"),
    defaults(placeholder = "Pick")
)]
struct Chns;

macro_rules! stub_handlers {
    ($($ty:ident),*) => {$(
        impl $ty {
            #[handler]
            async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
                Ok(responded())
            }
        }
    )*};
}

stub_handlers!(Guarded, WithoutLimit, Strs, Usrs, Rols, Mens, Chns);

impl WithLimit {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }

    #[on_rate_limited]
    async fn limited(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(V2Message::new(
            Components::new().push(TextDisplay::new("slow down")),
        ))
    }
}

/// G14/G15: `message` on a modal, with `owner` and `guard`.
#[derive(Modal)]
#[modal(
    "rl.form",
    owner = user,
    guard = auth::form_ok,
    rate_limit(message, 1, per = "1m"),
    rate_limit(member, 2, per = "2m"),
    defaults(title = "Form")
)]
struct Shaped {
    user: Id<UserMarker>,
    #[text_input("Bio")]
    bio: Option<String>,
}

impl Shaped {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }

    #[on_rate_limited]
    async fn limited(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(V2Message::new(
            Components::new().push(TextDisplay::new("slow down")),
        ))
    }
}

fn shape(limits: &[RateLimit]) -> Vec<(RateScope, u32, Duration, bool)> {
    limits
        .iter()
        .map(|l| (l.scope(), l.max_uses(), l.window(), l.is_manual()))
        .collect()
}

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// G1-G4: the slash declarations, in declared order.
#[test]
fn int_14_slash_rate_limits() {
    assert_eq!(
        shape(<Daily as __SlashCommand>::RATE_LIMITS),
        [(RateScope::User, 3, secs(10), false)]
    );
    assert_eq!(
        shape(<Trade as __SlashCommand>::RATE_LIMITS),
        [
            (RateScope::User, 3, secs(10), false),
            (RateScope::Guild, 100, secs(60), false),
        ]
    );
    assert_eq!(
        shape(<Vote as __SlashCommand>::RATE_LIMITS),
        [
            (RateScope::Member, 1, secs(86_400), true),
            (RateScope::Channel, u32::MAX, secs(1), false),
            (RateScope::User, 1, secs(5_400), false),
        ]
    );
    assert!(<Plain as __SlashCommand>::RATE_LIMITS.is_empty());
}

/// G14/G15: the context menus, components and modals.
#[test]
fn int_14_other_kinds_rate_limits() {
    assert_eq!(
        shape(<Wave as __ContextMenu>::RATE_LIMITS),
        [(RateScope::User, 2, secs(60), false)]
    );
    assert_eq!(
        shape(<Quote as __ContextMenu>::RATE_LIMITS),
        [
            (RateScope::Message, 1, secs(3_600), false),
            (RateScope::User, 5, secs(60), false),
        ]
    );
    assert_eq!(
        shape(<Guarded as __Component>::RATE_LIMITS),
        [
            (RateScope::Message, 1, secs(5), false),
            (RateScope::User, 2, secs(60), true),
        ]
    );
    let selects = [
        <Strs as __Component>::RATE_LIMITS,
        <Usrs as __Component>::RATE_LIMITS,
        <Rols as __Component>::RATE_LIMITS,
        <Mens as __Component>::RATE_LIMITS,
        <Chns as __Component>::RATE_LIMITS,
    ];
    for limits in selects {
        assert_eq!(shape(limits), [(RateScope::Message, 3, secs(60), false)]);
    }
    assert!(<WithoutLimit as __Component>::RATE_LIMITS.is_empty());
    assert_eq!(
        shape(<Shaped as __Modal>::RATE_LIMITS),
        [
            (RateScope::Message, 1, secs(60), false),
            (RateScope::Member, 2, secs(120), false),
        ]
    );
}

/// The stored descriptors carry the limits and the override.
#[test]
fn int_14_descriptors_show_rate_limits() {
    let daily = format!("{:?}", Daily::descriptor());
    assert!(
        daily.contains("rate_limits: [RateLimit { scope: User"),
        "{daily}"
    );
    assert!(daily.contains("on_rate_limited: true"), "{daily}");

    let plain = format!("{:?}", Plain::descriptor());
    assert!(plain.contains("rate_limits: []"), "{plain}");
    assert!(plain.contains("on_rate_limited: false"), "{plain}");

    let quote = format!("{:?}", Quote::descriptor());
    assert!(quote.contains("scope: Message"), "{quote}");
    assert!(quote.contains("on_rate_limited: true"), "{quote}");

    let guarded = format!("{:?}", Guarded::descriptor());
    assert!(guarded.contains("manual: true"), "{guarded}");
    assert!(guarded.contains("on_rate_limited: false"), "{guarded}");

    let with_limit = format!("{:?}", WithLimit::descriptor());
    assert!(with_limit.contains("on_rate_limited: true"), "{with_limit}");

    let shaped = format!("{:?}", Shaped::descriptor());
    assert!(shaped.contains("scope: Member"), "{shaped}");
    assert!(shaped.contains("on_rate_limited: true"), "{shaped}");
}

/// G15: a declaration adds nothing to the encoded `custom_id`.
#[test]
fn int_14_codec_width_unchanged() {
    let id = |value: serde_json::Value| value["custom_id"].as_str().unwrap().to_owned();
    let with = id(serde_json::to_value(WithLimit { n: 7 }.build()).unwrap());
    let without = id(serde_json::to_value(WithoutLimit { n: 7 }.build()).unwrap());
    assert_eq!(with.len(), without.len(), "{with} vs {without}");
}
