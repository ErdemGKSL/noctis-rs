//! `#[autocomplete(field)]` methods and the `<Struct>Partial` of
//! `#[derive(SlashCommand)]`, end to end through the JSON test seam
//! (Story 6.7, Matrix 2, 3; Story 6.11, Matrix 14, 26, 27).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;

use noctis::__private::core::__testing::call_autocomplete;
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::ChannelMarker;
use noctis::ui::{LegacyMessage, ResolvedChannel};
use noctis::{AutocompleteValue, CommandHandler, UnknownCommandPath};
use serde_json::{Value, json};

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Mode {
    #[choice("Fast")]
    Fast,
}

/// What the `value` method saw.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(SlashCommand)]
#[command("config (set|get) prefix", description = "Prefix settings")]
#[expect(dead_code, reason = "only the partial form is read")]
struct Config {
    #[option(description = "The prefix", autocomplete)]
    value: String,
    #[option(description = "How many", autocomplete)]
    limit: i64,
    #[option(description = "A ratio", autocomplete)]
    ratio: Option<f64>,
    #[option(description = "Where", autocomplete)]
    query: Option<String>,
    #[option(description = "Channel")]
    channel: Option<ResolvedChannel>,
    #[option(description = "Raw channel")]
    raw: Option<Id<ChannelMarker>>,
    #[option(description = "Mode")]
    mode: Option<Mode>,
}

impl Config {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("ok"))
    }

    #[autocomplete(value)]
    async fn suggest_value(
        ctx: AutocompleteCtx,
        typed: &str,
        filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        // Every field is an `Option<T>` of the field's option type.
        let ConfigPartial {
            value,
            limit,
            ratio,
            query,
            channel,
            raw,
            mode,
        } = filled;
        let _: (Option<String>, Option<i64>, Option<f64>, Option<String>) =
            (value.clone(), limit, ratio, query);
        let _: (Option<Id<ChannelMarker>>, Option<Mode>) = (raw, mode);
        SEEN.lock().unwrap().push(format!(
            "{}|{}|{typed}|{value:?}|{limit:?}|{:?}|{mode:?}",
            ctx.command_path(),
            ctx.focused().name(),
            channel.map(|c| c.channel.id.get()),
        ));
        Ok(["!", "?", "."]
            .into_iter()
            .map(|p| (p.to_owned(), format!("{typed}{p}")))
            .collect())
    }

    /// Story 6.11 Matrix 14 and 27: an INTEGER option's typed text and
    /// integer values.
    #[autocomplete(limit)]
    async fn suggest_limit(
        _ctx: AutocompleteCtx,
        typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<i64>> {
        let mut out = Suggestions::new();
        out.push(7, format!("typed `{typed}`"));
        Ok(out)
    }

    #[autocomplete(ratio)]
    async fn suggest_ratio(
        _ctx: AutocompleteCtx,
        _typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<f64>> {
        Ok([(1.5, "one and a half"), (f64::NAN, "dropped")]
            .into_iter()
            .collect())
    }

    #[autocomplete(query)]
    async fn suggest_query(
        ctx: AutocompleteCtx,
        _typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        let guild = ctx.guild_id()?;
        Ok([(guild.to_string(), "guild")].into_iter().collect())
    }
}

/// An autocomplete interaction for `config <sub> prefix` with `leaves`.
fn interaction(sub: &str, leaves: &Value) -> Vec<u8> {
    json!({
        "type": 4,
        "data": {
            "name": "config",
            "type": 1,
            "options": [{ "type": 2, "name": sub, "options": [
                { "type": 1, "name": "prefix", "options": leaves }
            ]}],
            "resolved": { "channels": { "5": {
                "id": "5", "name": "general", "type": 0, "permissions": "0"
            }}}
        }
    })
    .to_string()
    .into_bytes()
}

async fn choices(
    handler: &CommandHandler,
    sub: &str,
    leaves: &Value,
) -> Option<Vec<(String, AutocompleteValue)>> {
    call_autocomplete(handler, &interaction(sub, leaves))
        .await
        .unwrap()
        .map(|choices| {
            choices
                .iter()
                .map(|c| (c.name().to_owned(), c.value().clone()))
                .collect()
        })
}

async fn names(handler: &CommandHandler, sub: &str, leaves: &Value) -> Option<Vec<String>> {
    choices(handler, sub, leaves)
        .await
        .map(|c| c.into_iter().map(|(name, _)| name).collect())
}

/// Matrix 2 and 3: the 3-word path, the focused text and the lenient
/// partial input.
#[tokio::test]
async fn opt_12_partial_options_reach_the_method() {
    let handler = Config::descriptor();
    let leaves = json!([
        { "type": 3, "name": "value", "value": "pr", "focused": true },
        { "type": 7, "name": "channel", "value": "5" },
        { "type": 7, "name": "raw", "value": "6" },
        { "type": 3, "name": "mode", "value": "Slow" },
        { "type": 3, "name": "unknown", "value": "x" },
    ]);
    let got = names(&handler, "set", &leaves).await.unwrap();
    assert_eq!(got, ["pr!", "pr?", "pr."]);
    let seen = SEEN.lock().unwrap().clone();
    assert!(
        seen.contains(&"config set prefix|value|pr|Some(\"pr\")|None|Some(5)|None".to_owned()),
        "{seen:?}"
    );
    // A channel missing from the resolved data is `None`; a mistyped option
    // is `None` too (Story 6.11 Matrix 27).
    let leaves = json!([
        { "type": 3, "name": "value", "value": "x", "focused": true },
        { "type": 7, "name": "channel", "value": "6" },
        { "type": 4, "name": "limit", "value": "many" },
        { "type": 3, "name": "mode", "value": "Fast" },
    ]);
    names(&handler, "get", &leaves).await.unwrap();
    let seen = SEEN.lock().unwrap().clone();
    assert!(
        seen.contains(&"config get prefix|value|x|Some(\"x\")|None|None|Some(Fast)".to_owned()),
        "{seen:?}"
    );
}

/// Story 6.11 Matrix 14 and 27: typed text verbatim, a JSON number as its
/// text, `""` when absent; integer and float values.
#[tokio::test]
async fn typed_text_and_value_types() {
    let handler = Config::descriptor();
    for (value, typed) in [(json!("-"), "-"), (json!(12), "12"), (Value::Null, "")] {
        let mut leaf = json!({ "type": 4, "name": "limit", "focused": true });
        if !value.is_null() {
            leaf["value"] = value;
        }
        let got = choices(&handler, "set", &json!([leaf])).await.unwrap();
        assert_eq!(
            got,
            [(format!("typed `{typed}`"), AutocompleteValue::Integer(7))]
        );
    }
    let leaves = json!([{ "type": 10, "name": "ratio", "value": "", "focused": true }]);
    let got = choices(&handler, "set", &leaves).await.unwrap();
    assert_eq!(
        got,
        [("one and a half".to_owned(), AutocompleteValue::Number(1.5))],
        "the NaN suggestion is dropped (Matrix 26)"
    );
}

/// Story 6.11 Matrix 28: `ctx.guild_id()?` outside a guild is an error.
#[tokio::test]
async fn guild_id_outside_a_guild_is_an_error() {
    let handler = Config::descriptor();
    let leaves = json!([{ "type": 3, "name": "query", "value": "", "focused": true }]);
    let err = call_autocomplete(&handler, &interaction("set", &leaves))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not in a guild"), "{err}");
}

/// Matrix 24: an option without a method is `None`; an unknown path is an
/// error.
#[tokio::test]
async fn unflagged_options_and_unknown_paths() {
    let handler = Config::descriptor();
    let leaves = json!([{ "type": 3, "name": "mode", "value": "", "focused": true }]);
    assert_eq!(names(&handler, "set", &leaves).await, None);
    let other = json!({
        "type": 4,
        "data": { "name": "ping", "type": 1, "options": [
            { "type": 3, "name": "value", "value": "", "focused": true }
        ]}
    });
    let error = call_autocomplete(&handler, other.to_string().as_bytes())
        .await
        .unwrap_err();
    assert!(error.as_error().is::<UnknownCommandPath>());
}

/// The derive declares each flagged field as autocomplete, and registers
/// the methods with the Command.
#[test]
fn flagged_options_are_declared() {
    assert!(Config::OPTIONS[0].autocomplete);
    assert!(Config::OPTIONS[1].autocomplete);
    assert!(!Config::OPTIONS[4].autocomplete);
    let _ = Module::new("config").add::<Config>();
}
