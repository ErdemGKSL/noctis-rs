//! `#[autocomplete]` and the `<Name>Partial` of `#[derive(Options)]`, end to
//! end through the JSON test seam (Story 6.7, Matrix 2, 3, 28).

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
use noctis::{AutocompleteHandler, AutocompleteValue, CommandHandler, UnknownCommandPath};
use serde_json::{Value, json};

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Mode {
    #[choice("Fast")]
    Fast,
}

#[derive(Options)]
#[expect(dead_code, reason = "only the partial form is read")]
struct PrefixOptions {
    #[option(description = "The prefix", autocomplete = prefix_suggest)]
    value: String,
    #[option(description = "How many")]
    limit: i64,
    #[option(description = "Where", autocomplete = anything)]
    query: Option<String>,
    #[option(description = "Channel")]
    channel: Option<ResolvedChannel>,
    #[option(description = "Raw channel")]
    raw: Option<Id<ChannelMarker>>,
    #[option(description = "Mode")]
    mode: Option<Mode>,
}

/// What `prefix_suggest` saw.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[autocomplete]
async fn prefix_suggest(
    ctx: AutocompleteCtx,
    partial: Partial<PrefixOptions>,
) -> HandlerResult<Responded> {
    // Every field is an `Option<T>` of the field's option type.
    let PrefixOptionsPartial {
        value,
        limit,
        query,
        channel,
        raw,
        mode,
    } = partial;
    let _: (Option<String>, Option<i64>, Option<String>) = (value.clone(), limit, query);
    let _: (Option<Id<ChannelMarker>>, Option<Mode>) = (raw, mode);
    SEEN.lock().unwrap().push(format!(
        "{}|{}|{value:?}|{limit:?}|{:?}|{mode:?}",
        ctx.command_path(),
        ctx.focused().name(),
        channel.map(|c| c.channel.id.get()),
    ));
    let typed = ctx.focused().value().to_owned();
    Ok(ctx.respond(
        ["!", "?", "."]
            .into_iter()
            .map(|p| AutocompleteChoice::new(format!("{typed}{p}"), p)),
    ))
}

/// Matrix 28: a plain handler fits any options struct.
#[autocomplete]
async fn anything(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
    Ok(ctx.respond([AutocompleteChoice::new("any", 1_i64)]))
}

#[command("config (set|get) prefix", description = "Prefix settings")]
async fn config(
    _ctx: CommandCtx,
    _path: ConfigPath,
    _options: PrefixOptions,
) -> HandlerResult<LegacyMessage> {
    Ok(LegacyMessage::new("ok"))
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

async fn suggest(handler: &CommandHandler, sub: &str, leaves: &Value) -> Option<Vec<String>> {
    call_autocomplete(handler, &interaction(sub, leaves))
        .await
        .unwrap()
        .map(|choices| choices.iter().map(|c| c.name().to_owned()).collect())
}

/// Matrix 2 and 3: the 3-word path, the focused text and the lenient
/// partial input.
#[tokio::test]
async fn opt_12_partial_options_reach_the_handler() {
    let handler = config();
    let leaves = json!([
        { "type": 3, "name": "value", "value": "pr", "focused": true },
        { "type": 7, "name": "channel", "value": "5" },
        { "type": 7, "name": "raw", "value": "6" },
        { "type": 3, "name": "mode", "value": "Slow" },
        { "type": 3, "name": "unknown", "value": "x" },
    ]);
    let names = suggest(&handler, "set", &leaves).await.unwrap();
    assert_eq!(names, ["pr!", "pr?", "pr."]);
    let seen = SEEN.lock().unwrap().clone();
    assert!(
        seen.contains(&"config set prefix|value|Some(\"pr\")|None|Some(5)|None".to_owned()),
        "{seen:?}"
    );
    // A channel missing from the resolved data is `None`.
    let leaves = json!([
        { "type": 3, "name": "value", "value": "x", "focused": true },
        { "type": 7, "name": "channel", "value": "6" },
        { "type": 3, "name": "mode", "value": "Fast" },
    ]);
    suggest(&handler, "get", &leaves).await.unwrap();
    let seen = SEEN.lock().unwrap().clone();
    assert!(
        seen.contains(&"config get prefix|value|Some(\"x\")|None|None|Some(Fast)".to_owned()),
        "{seen:?}"
    );
}

/// Matrix 28 and 24: the plain handler runs; an option without a handler
/// is `None`.
#[tokio::test]
async fn plain_handlers_and_unlinked_options() {
    let handler = config();
    let leaves = json!([{ "type": 3, "name": "query", "value": "", "focused": true }]);
    let choices = call_autocomplete(&handler, &interaction("set", &leaves))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(choices[0].value(), &AutocompleteValue::Integer(1));
    let leaves = json!([{ "type": 4, "name": "limit", "value": "1", "focused": true }]);
    assert_eq!(suggest(&handler, "set", &leaves).await, None);
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

/// The derive links each `autocomplete = path` field by option name.
#[test]
fn options_link_their_handlers() {
    let linked: Option<AutocompleteHandler<PrefixOptions>> = PrefixOptions::autocomplete("value");
    assert!(linked.unwrap().name().ends_with("::prefix_suggest"));
    assert!(
        PrefixOptions::autocomplete("query")
            .unwrap()
            .name()
            .ends_with("::anything")
    );
    assert!(PrefixOptions::autocomplete("limit").is_none());
    assert!(format!("{:?}", prefix_suggest()).contains("prefix_suggest"));
    assert!(PrefixOptions::OPTIONS[0].autocomplete);
    let _: AutocompleteHandler<PrefixOptions> = anything();
}
