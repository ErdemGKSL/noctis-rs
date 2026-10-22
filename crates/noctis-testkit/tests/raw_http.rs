//! Story 9.2: raw REST requests (`noctis::http::RawHttp`, FR-31, FR-52)
//! over `FakeDiscordHttp`, through the App's one twilight client, and raw
//! components sent through `ctx.respond` (CV2-03, CV2-16).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::http::{Method, RawHttp, RawHttpError, RawRequest};
use noctis::prelude::*;
use noctis::twilight::http::Client;
use noctis::twilight::http::error::ErrorType;
use noctis::twilight::http::request::Form;
use noctis::ui::{
    Attachment, AttachmentSet, Components, RawComponent, TextDisplay, UiError, V2Message,
};
use noctis::{ResponderError, Runtime, Token};
use noctis_testkit::TestApp;
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeResponse, HeaderName, HeaderValue, Method as HttpMethod, RecordedRequest,
    StatusCode,
};
use serde_json::{Value, json};

/// An App on `Runtime::for_test()` whose requests go to `fake`, with bot
/// token `t`.
fn app(fake: &FakeDiscordHttp, timeout: Option<Duration>) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .http_proxy(fake.url())
        .token(Token::new("t").unwrap());
    if let Some(timeout) = timeout {
        builder.http_timeout(timeout);
    }
    builder.build().unwrap()
}

/// The one request `fake` received.
fn only(fake: &FakeDiscordHttp) -> RecordedRequest {
    let requests = fake.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    requests.into_iter().next().unwrap()
}

fn header<'r>(request: &'r RecordedRequest, name: &str) -> Option<&'r str> {
    request.headers.get(name).and_then(|v| v.to_str().ok())
}

fn h(name: &'static str, value: &'static str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    )
}

/// H1: a GET with the bot token, decoded.
#[tokio::test]
async fn fr_52_raw_get() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        HttpMethod::GET,
        "/applications/@me",
        FakeResponse::json(StatusCode::OK, &json!({ "id": "7" })),
    );
    let app = app(&fake, None);
    let response = app
        .http()
        .raw(Method::Get, "applications/@me")
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.bytes(), br#"{"id":"7"}"#);
    assert_eq!(response.json::<Value>().unwrap(), json!({ "id": "7" }));
    let request = only(&fake);
    assert_eq!(request.method, HttpMethod::GET);
    assert_eq!(request.path, "/api/v10/applications/@me");
    assert_eq!(header(&request, "authorization"), Some("Bot t"));
}

/// Awaits `request` and returns the one request `fake` got, then forgets
/// it.
async fn sent_by(fake: &FakeDiscordHttp, request: RawRequest<'_>) -> RecordedRequest {
    request.await.unwrap();
    let sent = only(fake);
    fake.clear_requests();
    sent
}

/// H2-H4: JSON, raw and multipart bodies; the last one set wins.
#[tokio::test]
async fn fr_52_raw_bodies() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        HttpMethod::POST,
        "/channels/1/messages",
        FakeResponse::json(StatusCode::OK, &json!({})),
    );
    let app = app(&fake, None);
    let http = app.http();
    let body = json!({ "content": "hi" });
    let form = || Form::new().file_part(b"files[0]", b"a.txt", b"hello");
    let deliver = |request| sent_by(&fake, request);

    let sent = deliver(http.raw(Method::Post, "channels/1/messages").json(&body)).await;
    assert_eq!(&sent.body[..], serde_json::to_vec(&body).unwrap());
    assert_eq!(header(&sent, "content-type"), Some("application/json"));

    let sent = deliver(
        http.raw(Method::Post, "channels/1/messages")
            .body(b"{}".to_vec()),
    )
    .await;
    assert_eq!(&sent.body[..], b"{}");

    let sent = deliver(http.raw(Method::Post, "channels/1/messages").form(form())).await;
    let files = sent.files();
    assert_eq!(
        (files[0].name.as_str(), files[0].filename.as_str()),
        ("files[0]", "a.txt")
    );
    assert_eq!(&files[0].bytes[..], b"hello");

    let sent = deliver(
        http.raw(Method::Post, "channels/1/messages")
            .json(&body)
            .form(form()),
    )
    .await;
    assert_eq!(sent.files().len(), 1);
    let sent = deliver(
        http.raw(Method::Post, "channels/1/messages")
            .form(form())
            .json(&body),
    )
    .await;
    assert_eq!(sent.json(), Some(body));
    assert!(sent.files().is_empty());
}

/// H5 and H6: no token; query and percent escapes forwarded unchanged.
#[tokio::test]
async fn fr_52_raw_without_token_and_paths() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        HttpMethod::GET,
        "/channels/1/messages",
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    fake.route(
        HttpMethod::PUT,
        "/channels/1/messages/2/reactions/%F0%9F%91%8D/@me",
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let app = app(&fake, None);
    app.http()
        .raw(Method::Get, "channels/1/messages?limit=2")
        .without_token()
        .await
        .unwrap();
    let request = only(&fake);
    assert_eq!(header(&request, "authorization"), None);
    assert_eq!(request.path, "/api/v10/channels/1/messages");
    assert_eq!(request.query.as_deref(), Some("limit=2"));
    fake.clear_requests();

    let reacted = app
        .http()
        .raw(
            Method::Put,
            "channels/1/messages/2/reactions/%F0%9F%91%8D/@me",
        )
        .await
        .unwrap();
    assert_eq!(reacted.status(), 204);
    assert_eq!(
        only(&fake).path,
        "/api/v10/channels/1/messages/2/reactions/%F0%9F%91%8D/@me"
    );
}

/// H7 and H8: an empty 204, and typed error statuses.
#[tokio::test]
async fn fr_52_raw_statuses() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        HttpMethod::DELETE,
        "/channels/1",
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    fake.route(
        HttpMethod::GET,
        "/channels/404",
        FakeResponse::json(StatusCode::NOT_FOUND, &json!({ "code": 10003 })),
    );
    fake.route(
        HttpMethod::GET,
        "/channels/500",
        FakeResponse::json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({ "message": "x" }),
        ),
    );
    let app = app(&fake, None);
    let gone = app.http().raw(Method::Delete, "channels/1").await.unwrap();
    assert_eq!(gone.status(), 204);
    assert!(gone.bytes().is_empty());
    assert!(matches!(gone.json::<Value>(), Err(RawHttpError::Decode(_))));

    match app.http().raw(Method::Get, "channels/404").await {
        Err(RawHttpError::Status { status, body }) => {
            assert_eq!(status, 404);
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                json!({ "code": 10003 })
            );
        }
        other => panic!("expected a 404, got {other:?}"),
    }
    assert!(matches!(
        app.http().raw(Method::Get, "channels/500").await,
        Err(RawHttpError::Status { status: 500, .. })
    ));
}

/// H9 and H11: an invalid path or a JSON body that does not serialize is
/// an error, and nothing is sent.
#[tokio::test]
async fn fr_52_raw_rejections_send_nothing() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = app(&fake, None);
    for path in [
        "",
        "/channels/1",
        "api/v10/x",
        "API/x",
        "a//b",
        "a/",
        "?x=1",
        "a/./b",
        "a/../b",
        "a b",
        "a/ü",
        "a#f",
        "a\\b",
        "a\n",
    ] {
        match app.http().raw(Method::Get, path).await {
            Err(RawHttpError::InvalidPath { path: p, reason }) => {
                assert_eq!(p, path);
                assert!(!reason.is_empty());
            }
            other => panic!("{path:?}: {other:?}"),
        }
    }
    let unserializable: HashMap<(u8, u8), u8> = HashMap::from([((1, 2), 3)]);
    assert!(matches!(
        app.http()
            .raw(Method::Post, "channels/1/messages")
            .json(&unserializable)
            .await,
        Err(RawHttpError::Encode(_))
    ));
    assert!(fake.requests().is_empty(), "{:?}", fake.requests());
}

/// The fake answer of a bucket exhausted for `reset_after` seconds
/// (twilight reads the bucket headers only with a `user` scope).
fn exhausted(reset_after: &'static str) -> FakeResponse {
    let mut response = FakeResponse::json(StatusCode::OK, &json!([]));
    for (name, value) in [
        h("x-ratelimit-scope", "user"),
        h("x-ratelimit-bucket", "b"),
        h("x-ratelimit-limit", "1"),
        h("x-ratelimit-remaining", "0"),
        h("x-ratelimit-reset-after", reset_after),
    ] {
        response = response.header(name, value);
    }
    response
}

/// H12: through a proxied App there is no local rate limiter.
#[tokio::test]
async fn fr_52_raw_proxied_has_no_local_limiter() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(HttpMethod::GET, "/channels/1/messages", exhausted("60"));
    let app = app(&fake, None);
    app.http()
        .raw(Method::Get, "channels/1/messages")
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        app.http().raw(Method::Get, "channels/1/messages"),
    )
    .await
    .expect("a local rate limiter delayed the second request")
    .unwrap();
    assert_eq!(fake.requests().len(), 2);
}

/// H13: a client with its local limiter waits out an exhausted bucket.
#[tokio::test]
async fn fr_52_raw_client_limiter_is_honored() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(HttpMethod::GET, "/channels/1/messages", exhausted("0.5"));
    let client = Client::builder()
        .proxy(fake.addr().to_string(), true)
        .token("t".to_owned())
        .build();
    let first = Instant::now();
    client
        .raw(Method::Get, "channels/1/messages")
        .await
        .unwrap();
    client
        .raw(Method::Get, "channels/1/messages")
        .await
        .unwrap();
    assert!(
        first.elapsed() >= Duration::from_millis(400),
        "{:?}",
        first.elapsed()
    );
    assert_eq!(fake.requests().len(), 2);
}

/// H15: the client's timeout applies.
#[tokio::test]
async fn fr_52_raw_timeout() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        HttpMethod::GET,
        "/slow",
        FakeResponse::empty(StatusCode::OK).delay(Duration::from_secs(2)),
    );
    let app = app(&fake, Some(Duration::from_millis(200)));
    match app.http().raw(Method::Get, "slow").await {
        Err(RawHttpError::Http(error)) => {
            assert!(
                matches!(error.kind(), ErrorType::RequestTimedOut),
                "{error:?}"
            );
        }
        other => panic!("expected a timeout, got {other:?}"),
    }
}

/// H16: response headers, case-insensitive.
#[tokio::test]
async fn fr_52_raw_response_headers() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    let (name, value) = h("x-ratelimit-bucket", "b");
    fake.route(
        HttpMethod::GET,
        "/applications/@me",
        FakeResponse::json(StatusCode::OK, &json!({})).header(name, value),
    );
    let app = app(&fake, None);
    let response = app
        .http()
        .raw(Method::Get, "applications/@me")
        .await
        .unwrap();
    assert_eq!(response.header("X-RateLimit-Bucket"), Some("b"));
    assert_eq!(response.header("x-missing"), None);
}

// ---- H14 and the respond acceptance: TestApp handlers ----

/// What the handlers saw, shared through `State`.
#[derive(Default)]
struct Log(Mutex<Vec<String>>);

impl Log {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn log(ctx_log: Option<State<Arc<Log>>>, line: String) {
    ctx_log.expect("the Log is registered").push(line);
}

#[derive(Button)]
#[button("raw.btn", defaults(style = Primary, label = "Raw"))]
struct RawBtn;

impl RawBtn {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<V2Message> {
        let response = ctx.http().raw(Method::Get, "applications/@me").await?;
        log(ctx.state(), format!("click {}", response.status()));
        Ok(V2Message::new(
            Components::new().push(TextDisplay::new("clicked")),
        ))
    }
}

#[derive(SlashCommand)]
#[command("rawget", description = "Raw GET")]
struct RawGet;

impl RawGet {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let response = ctx.http().raw(Method::Get, "applications/@me").await?;
        log(ctx.state(), format!("command {}", response.status()));
        ctx.respond(V2Message::new(
            Components::new()
                .push(TextDisplay::new("raw"))
                .push(noctis::ui::ActionRow::buttons([RawBtn.build()])),
        ))
        .await
    }
}

/// The raw node of count 2 the `rawview` commands send.
fn raw_node() -> Value {
    json!({ "type": 99, "question": "Lunch? 😀", "components": [{ "type": 98, "x": [1, 2] }] })
}

/// `texts` Text Displays and the raw node; logs a send failure.
async fn send_raw_view(
    ctx: &CommandCtx,
    r: CommandResponder,
    texts: usize,
) -> HandlerResult<Responded> {
    let raw = RawComponent::new(raw_node())?;
    let body = (0..texts)
        .map(|i| TextDisplay::new(format!("t{i}")))
        .fold(Components::new(), Components::push)
        .push(raw);
    match r.reply(V2Message::new(body)).await {
        Ok(acked) => Ok(acked.done()),
        Err(ResponderError::Invalid(UiError::Limits { violations })) => {
            log(
                ctx.state(),
                format!("invalid {} {}", violations[0].rule, violations[0].actual),
            );
            Err(HandlerError::new(std::io::Error::other("invalid")))
        }
        Err(other) => Err(other.into()),
    }
}

#[derive(SlashCommand)]
#[command("rawview fits", description = "38 texts and a raw node")]
struct RawViewFits;

impl RawViewFits {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        send_raw_view(&ctx, r, 38).await
    }
}

#[derive(SlashCommand)]
#[command("rawview over", description = "39 texts and a raw node")]
struct RawViewOver;

impl RawViewOver {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        send_raw_view(&ctx, r, 39).await
    }
}

/// V5: a raw node referencing an attachment of the set.
#[derive(SlashCommand)]
#[command("rawfile", description = "A raw node with a file")]
struct RawFile;

impl RawFile {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let mut files = AttachmentSet::new();
        let card = files.add(Attachment::new(&b"raw file"[..], "png")?);
        let _unused = files.add(Attachment::new(&b"unused"[..], "png")?);
        let node = RawComponent::new(json!({ "type": 99, "media": { "url": card.url() } }))?;
        ctx.respond(V2Message::new(Components::new().push(node)).attachments(files))
            .await
    }
}

async fn test_app() -> (TestApp, Arc<Log>) {
    let log = Arc::new(Log::default());
    let mut builder = App::builder();
    builder.state(Arc::clone(&log)).module(
        Module::new("raw")
            .add::<RawGet>()
            .add::<RawBtn>()
            .add::<RawViewFits>()
            .add::<RawViewOver>()
            .add::<RawFile>(),
    );
    let app = TestApp::build(&mut builder).await.unwrap();
    app.http_fake().route(
        HttpMethod::GET,
        "/applications/@me",
        FakeResponse::json(StatusCode::OK, &json!({ "id": "7" })),
    );
    (app, log)
}

/// The calls of `response` to `/applications/@me`.
fn raw_calls(response: &noctis_testkit::Response) -> usize {
    response
        .http_calls()
        .iter()
        .filter(|c| c.path() == "/applications/@me")
        .count()
}

/// The JSON bodies of the callbacks of `response`.
fn callbacks(response: &noctis_testkit::Response) -> Vec<Value> {
    response
        .http_calls()
        .iter()
        .filter(|c| c.path().ends_with("/callback"))
        .filter_map(|c| c.json().cloned())
        .collect()
}

/// H14: command and component handlers reach the `TestApp`'s fake through
/// the App's one client, next to their callbacks.
#[tokio::test(flavor = "multi_thread")]
async fn fr_52_raw_from_handlers_uses_the_app_client() {
    let (app, log) = test_app().await;
    let command = app.command("rawget").run().await.unwrap();
    assert_eq!(raw_calls(&command), 1, "{:?}", command.http_calls());
    assert_eq!(callbacks(&command).len(), 1);
    let click = app
        .click(command.button("Raw").unwrap())
        .run()
        .await
        .unwrap();
    assert_eq!(raw_calls(&click), 1, "{:?}", click.http_calls());
    assert_eq!(callbacks(&click).len(), 1);
    assert_eq!(log.lines(), ["command 200", "click 200"]);
}

/// The acceptance rows: 38 texts and a raw node of count 2 is one type-4
/// callback holding the raw node verbatim and without an `id`; 39 texts is
/// CV2-03 with actual 41, and no request carries the message.
#[tokio::test(flavor = "multi_thread")]
async fn cv2_03_raw_view_through_respond() {
    let (app, log) = test_app().await;
    let fits = app.command("rawview fits").run().await.unwrap();
    let sent = callbacks(&fits);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["type"], 4);
    let components = sent[0]["data"]["components"].as_array().unwrap();
    assert_eq!(components.len(), 39);
    assert_eq!(components[38], raw_node());
    assert_eq!(components[37]["id"], 38);

    let over = app.command("rawview over").run().await;
    assert_eq!(log.lines(), ["invalid CV2-03 41"]);
    if let Ok(over) = over {
        let carried = over
            .http_calls()
            .iter()
            .any(|c| c.json().is_some_and(|b| b.to_string().contains("Lunch?")));
        assert!(!carried, "{:?}", over.http_calls());
    }
}

/// V5: the Responder uploads the attachment a raw node references, and
/// only that one.
#[tokio::test(flavor = "multi_thread")]
async fn att_05_raw_reference_is_uploaded() {
    let (app, _log) = test_app().await;
    let response = app.command("rawfile").run().await.unwrap();
    let call = response
        .http_calls()
        .iter()
        .find(|c| c.path().ends_with("/callback"))
        .expect("a callback");
    let files = call.files();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].name, "files[0]");
    assert_eq!(&files[0].bytes[..], b"raw file");
}
