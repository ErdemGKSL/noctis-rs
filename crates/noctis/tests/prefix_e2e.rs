//! Story 15.4: prefix commands end to end through `noctis::testkit::TestApp`.
//! A gateway `MESSAGE_CREATE` reaches the real `PrefixPlugin`, which runs the
//! same `#[handler]` as the slash Command and replies to the message; a bad
//! argument is the slash decode's typed `OptionError`; GW-05 needs
//! `MESSAGE_CONTENT`.

#![cfg(all(feature = "prefix", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::prefix::PrefixPlugin;
use noctis::prelude::*;
use noctis::rules::command::OptionType;
use noctis::testkit::fake::{FakeResponse, Method, RecordedRequest, StatusCode};
use noctis::testkit::{TestApp, TestError};
use noctis::twilight::model::gateway::Intents;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::{BuildViolation, ErrorReport, OptionError, ReportKind};
use serde_json::{Value, json};

/// The bound on every wait.
const WAIT: Duration = Duration::from_secs(15);
const CHANNEL: &str = "/channels/5/messages";
/// `noctis.validation.failed` in en-US.
const VALIDATION: &str = "That input isn't valid. Check it and try again.";

/// What the `warn` handler saw.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(SlashCommand)]
#[command("warn", description = "Warn a member")]
struct Warn {
    #[option(description = "Who")]
    user: Id<UserMarker>,
    #[option(description = "How many days", min_value = 1, max_value = 30)]
    days: Option<i64>,
    #[option(description = "Why")]
    reason: Option<String>,
}

impl Warn {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let line = format!("{}:{:?}:{:?}", self.user, self.days, self.reason);
        SEEN.lock().unwrap().push(line.clone());
        ctx.reply_text(format!("warned {line}")).await
    }
}

/// The Command Error Sink's option errors.
#[derive(Default)]
struct OptionErrors(Mutex<Vec<OptionError>>);

impl ErrorSink for OptionErrors {
    fn report(&self, report: ErrorReport<'_>) {
        if let ReportKind::Error(error) = report.kind
            && let Some(error) = error.as_error().downcast_ref::<OptionError>()
        {
            self.0.lock().unwrap().push(error.clone());
        }
    }
}

fn builder(errors: &Arc<OptionErrors>) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .intents(Intents::MESSAGE_CONTENT)
        .error_sink(ErrorOrigin::Command, Arc::clone(errors))
        .module(Module::new("mod").add::<Warn>())
        .plugin(PrefixPlugin::new("!").command::<Warn>());
    builder
}

fn message(id: u64, content: &str, author: &Value) -> Value {
    json!({
        "id": id.to_string(), "channel_id": "5", "guild_id": "9", "author": author,
        "content": content, "timestamp": "2026-10-24T00:00:00.000000+00:00",
        "edited_timestamp": null, "tts": false, "mention_everyone": false,
        "mentions": [{ "id": "5", "username": "target", "discriminator": "0", "avatar": null, "public_flags": 0 }],
        "mention_roles": [], "attachments": [], "embeds": [], "pinned": false, "type": 0
    })
}

fn person() -> Value {
    json!({ "id": "3", "username": "mod", "discriminator": "0", "avatar": null })
}

/// The channel-message requests, once there are `n`.
async fn wait_sent(app: &TestApp, n: usize) -> Vec<RecordedRequest> {
    let start = Instant::now();
    loop {
        let sent: Vec<RecordedRequest> = app
            .http_fake()
            .requests()
            .into_iter()
            .filter(|r| r.route_path() == Some(CHANNEL))
            .collect();
        if sent.len() >= n {
            return sent;
        }
        assert!(start.elapsed() < WAIT, "{} of {n} requests", sent.len());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn body(request: &RecordedRequest) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

/// R1, R14 and R38, P8.
#[tokio::test(flavor = "multi_thread")]
async fn prefix_runs_the_slash_handler() {
    let errors = Arc::new(OptionErrors::default());
    let app = TestApp::build(&mut builder(&errors)).await.unwrap();
    app.http_fake().route(
        Method::POST,
        CHANNEL,
        FakeResponse::json(StatusCode::OK, &json!({ "id": "77" })),
    );

    // R38: a bot's message is ignored; the next one is served.
    let mut bot = person();
    bot["bot"] = json!(true);
    app.gateway_fake()
        .dispatch("MESSAGE_CREATE", message(5, "!warn <@5> 3", &bot));
    // R1.
    app.gateway_fake()
        .dispatch("MESSAGE_CREATE", message(6, "!warn <@5> 3", &person()));
    let sent = wait_sent(&app, 1).await;
    assert_eq!(sent[0].method, Method::POST);
    let reply = body(&sent[0]);
    assert_eq!(
        reply["message_reference"],
        json!({ "message_id": "6", "fail_if_not_exists": false })
    );
    assert_eq!(reply["components"][0]["content"], "warned 5:Some(3):None");
    assert_eq!(*SEEN.lock().unwrap(), ["5:Some(3):None"]);

    // R14: the slash decode's typed error, a public notice, no handler run.
    app.gateway_fake()
        .dispatch("MESSAGE_CREATE", message(7, "!warn <@5> abc", &person()));
    let sent = wait_sent(&app, 2).await;
    let notice = body(&sent[1]);
    assert_eq!(notice["components"][0]["content"], VALIDATION);
    assert_eq!(notice["flags"], 32768);
    assert_eq!(
        notice["message_reference"],
        json!({ "message_id": "7", "fail_if_not_exists": false })
    );
    let start = Instant::now();
    while errors.0.lock().unwrap().is_empty() {
        assert!(start.elapsed() < WAIT, "no Command Error Sink report");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        *errors.0.lock().unwrap(),
        [OptionError::WrongType {
            option: "days".into(),
            expected: OptionType::Integer,
            found: OptionType::String,
        }]
    );
    assert_eq!(SEEN.lock().unwrap().len(), 1);

    // The bot's message never got an answer: a later message is served,
    // and nothing replied to message 5.
    app.gateway_fake()
        .dispatch("MESSAGE_CREATE", message(8, "!warn <@5> 4", &person()));
    let sent = wait_sent(&app, 3).await;
    assert_eq!(sent.len(), 3);
    assert_eq!(body(&sent[2])["message_reference"]["message_id"], "8");
    assert!(
        sent.iter()
            .all(|r| body(r)["message_reference"]["message_id"] != "5")
    );
    app.stop().await.unwrap();
}

/// P8: the requested intent satisfies GW-05; `GUILD_MESSAGES` is derived.
#[tokio::test(flavor = "multi_thread")]
async fn prefix_intents() {
    let errors = Arc::new(OptionErrors::default());
    let app = builder(&errors).build().unwrap();
    assert!(
        app.intents()
            .contains(Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT)
    );
    let slash_only = App::builder()
        .module(Module::new("mod").add::<Warn>())
        .build()
        .unwrap();
    assert!(!slash_only.intents().contains(Intents::MESSAGE_CONTENT));
}

/// P7: without `MESSAGE_CONTENT` the build fails with GW-05.
#[tokio::test(flavor = "multi_thread")]
async fn gw_05_prefix_needs_message_content() {
    let mut builder = App::builder();
    builder
        .module(Module::new("mod").add::<Warn>())
        .plugin(PrefixPlugin::new("!").command::<Warn>());
    let Err(TestError::Build(err)) = TestApp::build(&mut builder).await else {
        panic!("the build must fail");
    };
    let missing: Vec<&BuildViolation> = err
        .violations()
        .iter()
        .filter(|v| matches!(v, BuildViolation::MissingIntent { .. }))
        .collect();
    assert_eq!(
        missing,
        [&BuildViolation::MissingIntent {
            module: "noctis-prefix",
            handler: "noctis_prefix::PrefixPlugin",
            event: "MESSAGE_CREATE",
            intent: Intents::MESSAGE_CONTENT,
        }]
    );
    let text = err.to_string();
    assert!(text.contains("[GW-05]"), "{text}");
    assert!(text.contains("MESSAGE_CONTENT"), "{text}");
}

/// P1 and P5 through the real derive: violations name the plugin.
#[tokio::test(flavor = "multi_thread")]
async fn plugin_violations() {
    #[derive(SlashCommand)]
    #[command("ban", description = "Ban", permissions = BAN_MEMBERS)]
    struct Ban;

    impl Ban {
        #[handler]
        async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
            ctx.reply_text("banned").await
        }
    }

    let err = App::builder()
        .intents(Intents::MESSAGE_CONTENT)
        .module(Module::new("mod").add::<Ban>())
        .plugin(PrefixPlugin::new("").command::<Ban>())
        .build()
        .unwrap_err();
    let messages: Vec<String> = err
        .violations()
        .iter()
        .filter_map(|v| match v {
            BuildViolation::Plugin { plugin, message } => {
                assert!(plugin.ends_with("PrefixPlugin"), "{plugin}");
                Some(message.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].starts_with("prefix is empty"));
    assert!(messages[1].contains("Ban` cannot be a prefix command"));
    assert!(messages[1].contains("`permissions`"));
}

/// What the guild-only handler saw as `ctx.guild_id()`.
static GUILD_SEEN: Mutex<Vec<Option<u64>>> = Mutex::new(Vec::new());

#[derive(SlashCommand)]
#[command("here", description = "Guild only", contexts = [Guild])]
struct Here;

impl Here {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        GUILD_SEEN.lock().unwrap().push(ctx.guild_id().map(Id::get));
        ctx.reply_text("here").await
    }
}

/// `contexts` gates the place (a DM is ignored) and the guild id reaches the
/// handler.
#[tokio::test(flavor = "multi_thread")]
async fn prefix_contexts_and_guild_id() {
    let mut builder = App::builder();
    builder
        .intents(Intents::MESSAGE_CONTENT | Intents::DIRECT_MESSAGES)
        .module(Module::new("mod").add::<Here>())
        .plugin(PrefixPlugin::new("!").command::<Here>());
    let app = TestApp::build(&mut builder).await.unwrap();
    app.http_fake().route(
        Method::POST,
        CHANNEL,
        FakeResponse::json(StatusCode::OK, &json!({ "id": "77" })),
    );

    let mut dm = message(10, "!here", &person());
    dm.as_object_mut().unwrap().remove("guild_id");
    dm["channel_id"] = json!("8");
    app.gateway_fake().dispatch("MESSAGE_CREATE", dm);
    app.gateway_fake()
        .dispatch("MESSAGE_CREATE", message(11, "!here", &person()));
    let sent = wait_sent(&app, 1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(body(&sent[0])["message_reference"]["message_id"], "11");
    assert_eq!(*GUILD_SEEN.lock().unwrap(), [Some(9)]);
    assert!(
        app.http_fake()
            .requests()
            .iter()
            .all(|r| r.route_path() != Some("/channels/8/messages"))
    );
    app.stop().await.unwrap();
}
