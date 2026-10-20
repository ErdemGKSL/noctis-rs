use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use noctis_ports::{
    BoxFuture, PortError, RenderError, RenderJob, RenderOutput, TemplateDescriptor,
};

use super::*;
use crate::Resource;
use crate::ports::PortSlot;

const SECRET: &str = "SECRET123";

fn token() -> Token {
    Token::new(SECRET).unwrap()
}

/// A lookup answering only `NOCTIS_TOKEN` (every other variable is unset).
fn lookup(value: Option<OsString>) -> impl Fn(&str) -> Option<OsString> {
    lookup_var("NOCTIS_TOKEN", value)
}

/// A lookup answering only `name` (every other variable is unset).
fn lookup_var(name: &'static str, value: Option<OsString>) -> impl Fn(&str) -> Option<OsString> {
    move |var| {
        assert!(
            matches!(
                var,
                "NOCTIS_TOKEN" | "NOCTIS_HTTP_PROXY" | "NOCTIS_GATEWAY_URL" | "NOCTIS_SHARD_TOTAL"
            ),
            "unexpected variable {var}"
        );
        if var == name { value.clone() } else { None }
    }
}

// ---- from_env (Matrix 8) ----

#[test]
fn from_env_token_rules() {
    for raw in [None, Some(""), Some("  \n")] {
        let b = AppBuilder::from_lookup(lookup(raw.map(OsString::from))).unwrap();
        assert!(b.token.is_none(), "{raw:?}");
    }
    let b = AppBuilder::from_lookup(lookup(Some("  tok \n".into()))).unwrap();
    assert_eq!(b.token.as_ref().unwrap().expose(), "tok");

    let err = AppBuilder::from_lookup(lookup(Some("a b".into()))).unwrap_err();
    assert_eq!(
        err,
        EnvError::InvalidToken {
            var: "NOCTIS_TOKEN",
            source: crate::TokenError::InvalidChar { index: 1 },
        }
    );
    assert!(!err.to_string().contains("a b"));
    assert!(!format!("{err:?}").contains("a b"));
}

#[cfg(unix)]
#[test]
fn from_env_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err =
        AppBuilder::from_lookup(lookup(Some(OsString::from_vec(vec![b'a', 0xff])))).unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_TOKEN"
        }
    );
    let err: Error = err.into();
    assert!(matches!(err, Error::Env(EnvError::NotUnicode { .. })));
}

// ---- build (Matrix 9-19) ----

/// A `CacheStore` counting `get` calls and storing nothing.
struct CountingCache(Arc<AtomicUsize>);

impl CacheStore for CountingCache {
    fn get<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(None) })
    }
    fn put<'a>(
        &'a self,
        _: &'a str,
        _: Bytes,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async { Ok(()) })
    }
    fn put_if_absent<'a>(
        &'a self,
        _: &'a str,
        _: Bytes,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        Box::pin(async { Ok(false) })
    }
    fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async { Ok(()) })
    }
    fn incr_with_ttl<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        Box::pin(async { Ok(0) })
    }
}

#[derive(Debug, PartialEq)]
struct Meta(&'static str);

#[tokio::test]
async fn build_happy() {
    let gets = Arc::new(AtomicUsize::new(0));
    let app = App::builder()
        .token(token())
        .state(Meta("noctis"))
        .cache_store(CountingCache(Arc::clone(&gets)))
        .build()
        .unwrap();
    assert!(Arc::ptr_eq(app.http(), app.http()));
    assert_eq!(app.http().token(), Some(&*format!("Bot {SECRET}")));
    let prefixed = App::builder()
        .token(Token::new("Bot abc").unwrap())
        .build()
        .unwrap();
    assert_eq!(prefixed.http().token(), Some("Bot abc"));
    assert_eq!(app.http_timeout(), Duration::from_secs(10));
    assert_eq!(app.intents(), Intents::GUILDS);
    assert_eq!(*app.state::<Meta>().unwrap(), Meta("noctis"));
    assert!(app.state::<u64>().is_none());
    assert!(!app.ports().is_default(PortSlot::CacheStore));
    assert_eq!(app.ports().cache_store.get("k").await.unwrap(), None);
    assert_eq!(gets.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn build_without_token_and_two_apps() {
    let a = App::builder().build().unwrap();
    let b = App::builder().build().unwrap();
    assert!(a.token.is_none());
    assert!(a.http().token().is_none());
    assert!(!Arc::ptr_eq(a.http(), b.http()));
}

#[tokio::test]
async fn defaults() {
    let app = App::builder().build().unwrap();
    for slot in PortSlot::ALL {
        assert!(app.ports().is_default(*slot), "{slot:?}");
    }
    assert_eq!(app.ports().default_slots().len(), 11);
    assert!(!app.has_renderer());
    let store = &app.ports().cache_store;
    store
        .put("k", Bytes::from_static(b"v"), None)
        .await
        .unwrap();
    assert_eq!(
        store.get("k").await.unwrap(),
        Some(Bytes::from_static(b"v"))
    );
}

#[tokio::test]
async fn custom_port_last_call_wins() {
    let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let app = App::builder()
        .cache_store(CountingCache(Arc::clone(&a)))
        .cache_store(CountingCache(Arc::clone(&b)))
        .build()
        .unwrap();
    app.ports().cache_store.get("k").await.unwrap();
    assert_eq!(b.load(Ordering::SeqCst), 1);
    assert_eq!(a.load(Ordering::SeqCst), 0);
    assert!(!app.ports().is_default(PortSlot::CacheStore));
    assert!(app.ports().is_default(PortSlot::RefStore));
}

#[tokio::test]
async fn timeout_bounds() {
    for (timeout, ok) in [
        (Duration::ZERO, false),
        (Duration::from_micros(999), false),
        (Duration::from_millis(1), true),
        (Duration::from_mins(5), true),
        (Duration::from_mins(5) + Duration::from_nanos(1), false),
        (Duration::MAX, false),
    ] {
        let result = App::builder().http_timeout(timeout).build();
        if ok {
            assert_eq!(result.unwrap().http_timeout(), timeout);
        } else {
            assert_eq!(
                result.unwrap_err().violations(),
                [BuildViolation::InvalidHttpTimeout { actual: timeout }]
            );
        }
    }
}

#[tokio::test]
async fn intents_union_with_guilds_floor() {
    let app = App::builder()
        .intents(Intents::GUILD_MESSAGES)
        .intents(Intents::MESSAGE_CONTENT)
        .build()
        .unwrap();
    assert_eq!(
        app.intents(),
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT
    );
    let app = App::builder().intents(Intents::empty()).build().unwrap();
    assert_eq!(app.intents(), Intents::GUILDS);
}

#[tokio::test]
async fn duplicate_state() {
    let mut builder = App::builder();
    builder.state(1_u32).state(2_u32);
    assert_eq!(*builder.states.get::<u32>().unwrap(), 1);
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicateState { type_name: "u32" }]
    );
}

#[test]
fn build_outside_runtime() {
    let err = App::builder().build().unwrap_err();
    assert_eq!(err.violations(), [BuildViolation::NoTokioRuntime]);
}

struct Reporter;

impl Plugin for Reporter {
    fn install(&self, app: &mut AppBuilder) {
        app.report(BuildViolation::plugin::<Self>("needs config"));
    }
}

#[test]
fn every_violation_at_once() {
    let err = App::builder()
        .state(1_u8)
        .state(2_u8)
        .http_timeout(Duration::ZERO)
        .plugin(Reporter)
        .plugin(Reporter)
        .build()
        .unwrap_err();
    let reporter = std::any::type_name::<Reporter>();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::DuplicateState { type_name: "u8" },
            BuildViolation::DuplicatePlugin {
                type_name: reporter
            },
            BuildViolation::Plugin {
                plugin: reporter,
                message: "needs config".into()
            },
            BuildViolation::InvalidHttpTimeout {
                actual: Duration::ZERO
            },
            BuildViolation::NoTokioRuntime,
        ]
    );
    let text = err.to_string();
    assert!(text.starts_with("5 App-build violation(s)\n"));
    assert_eq!(text.matches("\n  - ").count(), 5);
    assert_eq!(err.clone().into_violations().len(), 5);
}

#[tokio::test]
async fn build_resets_builder() {
    let runtime = Runtime::for_test();
    let mut builder = App::builder();
    builder
        .runtime(&runtime)
        .token(token())
        .state(1_u8)
        .cache_store(CountingCache(Arc::default()));
    let first = builder.build().unwrap();
    assert!(first.token.is_some());
    let second = builder.build().unwrap();
    assert!(second.token.is_none());
    assert!(second.state::<u8>().is_none());
    assert!(second.ports().is_default(PortSlot::CacheStore));
    assert_eq!(second.runtime(), &runtime);
}

#[tokio::test]
async fn runtime_attach() {
    let app = App::builder().build().unwrap();
    assert_eq!(app.runtime(), Runtime::global());
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
    assert!(app.runtime().is_test());
    assert_ne!(app.runtime(), Runtime::global());
}

#[tokio::test]
async fn redaction() {
    let mut builder = App::builder();
    builder.token(token()).state(1_u8);
    let builder_debug = format!("{builder:?}");
    assert!(builder_debug.contains("Token(<redacted>)"));
    let app = builder.build().unwrap();
    let mut failing = App::builder();
    failing.token(token()).http_timeout(Duration::ZERO);
    let build_err = failing.build().unwrap_err();
    let env_err = AppBuilder::from_lookup(lookup(Some(format!("{SECRET} x").into()))).unwrap_err();
    for text in [
        builder_debug,
        format!("{app:?}"),
        format!("{app:#?}"),
        format!("{build_err:?}"),
        build_err.to_string(),
        format!("{env_err:?}"),
        env_err.to_string(),
    ] {
        assert!(!text.contains(SECRET), "{text}");
    }
}

// ---- plugins (Matrix 25-28) ----

struct Pool;

impl Resource for Pool {
    type Config = u8;

    fn init(_: &u8) -> Self {
        Self
    }
}

struct PoolPlugin(u8);

impl Plugin for PoolPlugin {
    fn install(&self, app: &mut AppBuilder) {
        if let Err(conflict) = app.current_runtime().resource_or_init::<Pool>(self.0) {
            app.report(conflict.into());
        }
    }
}

#[tokio::test]
async fn resource_conflict_via_plugin() {
    let runtime = Runtime::for_test();
    App::builder()
        .runtime(&runtime)
        .plugin(PoolPlugin(1))
        .build()
        .unwrap();
    let err = App::builder()
        .runtime(&runtime)
        .plugin(PoolPlugin(2))
        .build()
        .unwrap_err();
    assert!(matches!(
        err.violations(),
        [BuildViolation::ResourceConflict(c)] if c.frozen == "1" && c.requested == "2"
    ));
    // Separate test Runtimes do not interfere.
    for config in [1, 2] {
        App::builder()
            .runtime(&Runtime::for_test())
            .plugin(PoolPlugin(config))
            .build()
            .unwrap();
    }
}

struct FakeRenderer;

impl Renderer for FakeRenderer {
    fn render(&self, _: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        Box::pin(async { Err(RenderError::failed("fake")) })
    }

    fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

struct RendererPlugin(Arc<AtomicUsize>);

impl Plugin for RendererPlugin {
    fn install(&self, app: &mut AppBuilder) {
        self.0.fetch_add(1, Ordering::SeqCst);
        app.renderer(Arc::new(FakeRenderer));
    }
}

#[tokio::test]
async fn plugin_installs_renderer_once() {
    let installs = Arc::new(AtomicUsize::new(0));
    let app = App::builder()
        .plugin(RendererPlugin(Arc::clone(&installs)))
        .build()
        .unwrap();
    assert!(app.has_renderer());
    assert_eq!(installs.load(Ordering::SeqCst), 1);
}

type Log = Arc<Mutex<Vec<&'static str>>>;

struct P1(Log);
struct P2(Log);
struct P3(Log);

impl Plugin for P1 {
    fn install(&self, app: &mut AppBuilder) {
        self.0.lock().unwrap().push("P1");
        app.plugin(P3(Arc::clone(&self.0)));
    }
}

impl Plugin for P2 {
    fn install(&self, _: &mut AppBuilder) {
        self.0.lock().unwrap().push("P2");
    }
}

impl Plugin for P3 {
    fn install(&self, _: &mut AppBuilder) {
        self.0.lock().unwrap().push("P3");
    }
}

#[tokio::test]
async fn plugin_order_and_nesting() {
    let log = Log::default();
    App::builder()
        .plugin(P1(Arc::clone(&log)))
        .plugin(P2(Arc::clone(&log)))
        .build()
        .unwrap();
    assert_eq!(*log.lock().unwrap(), ["P1", "P2", "P3"]);
}

struct Value(u8, Arc<AtomicUsize>);

impl Plugin for Value {
    fn install(&self, _: &mut AppBuilder) {
        self.1.store(usize::from(self.0), Ordering::SeqCst);
    }
}

#[tokio::test]
async fn duplicate_plugin_and_plugin_mut() {
    let seen = Arc::new(AtomicUsize::new(0));
    let mut builder = App::builder();
    builder
        .plugin(Value(1, Arc::clone(&seen)))
        .plugin(Value(2, Arc::clone(&seen)));
    assert!(builder.plugin_mut::<P2>().is_none());
    let value = builder.plugin_mut::<Value>().unwrap();
    assert_eq!(value.0, 1);
    value.0 = 9;
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicatePlugin {
            type_name: std::any::type_name::<Value>()
        }]
    );
    assert_eq!(seen.load(Ordering::SeqCst), 9);
}

// ---- run (Matrix 29-31) ----

#[test]
fn run_outside_runtime() {
    let tokio_rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let app = {
        let _guard = tokio_rt.enter();
        App::builder().token(token()).build().unwrap()
    };
    let mut run = std::pin::pin!(app.run());
    let poll = run.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(matches!(
        poll,
        Poll::Ready(Err(Error::Start(StartError::NoRuntime)))
    ));
}

#[tokio::test]
async fn run_current_thread() {
    let app = App::builder().token(token()).build().unwrap();
    assert!(matches!(
        app.run().await,
        Err(Error::Start(StartError::NotMultiThread))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_multi_thread() {
    // Past the token check, `run` reads GET /gateway/bot: nothing listens on
    // port 1, so it fails there without touching the network.
    let app = App::builder()
        .token(token())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    assert!(matches!(
        app.run().await,
        Err(Error::Start(StartError::GatewayInfo { status: None }))
    ));
    let app = App::builder().build().unwrap();
    assert!(matches!(
        app.run().await,
        Err(Error::Start(StartError::MissingToken))
    ));
}

// ---- http_proxy (Story 5.2 Matrix 9-11) ----

#[tokio::test]
async fn http_proxy_wiring() {
    let app = App::builder().http_proxy("http://p:1").build().unwrap();
    assert_eq!(app.http_proxy(), Some("p:1"));
    assert!(format!("{app:?}").contains("http_proxy: Some(\"p:1\")"));

    let app = App::builder()
        .http_proxy("a:1")
        .http_proxy("b:2")
        .build()
        .unwrap();
    assert_eq!(app.http_proxy(), Some("b:2"));

    let app = App::builder().build().unwrap();
    assert_eq!(app.http_proxy(), None);

    let err = App::builder()
        .http_proxy("https://x")
        .http_timeout(Duration::ZERO)
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpTimeout {
                actual: Duration::ZERO
            },
            BuildViolation::InvalidHttpProxy {
                reason: crate::HttpProxyError::UnsupportedScheme
            },
        ]
    );
}

#[tokio::test]
async fn http_proxy_redaction() {
    let mut builder = App::builder();
    builder.http_proxy("user:SECRET@h");
    let debug = format!("{builder:?}");
    assert!(debug.contains("http_proxy: true"), "{debug}");
    assert!(!debug.contains("SECRET"), "{debug}");
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidHttpProxy {
            reason: crate::HttpProxyError::NotAnAuthority
        }]
    );
    assert!(err.to_string().contains("invalid HTTP proxy: "));
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains("SECRET"), "{text}");
    }
}

#[tokio::test]
async fn from_env_http_proxy() {
    for raw in [None, Some(""), Some("  \n")] {
        let b = AppBuilder::from_lookup(lookup_var("NOCTIS_HTTP_PROXY", raw.map(OsString::from)))
            .unwrap();
        assert!(b.http_proxy.is_none(), "{raw:?}");
    }
    let app = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_HTTP_PROXY",
        Some(" http://p:1 \n".into()),
    ))
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(app.http_proxy(), Some("p:1"));

    let err = AppBuilder::from_lookup(lookup_var("NOCTIS_HTTP_PROXY", Some("https://x".into())))
        .unwrap()
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidHttpProxy {
            reason: crate::HttpProxyError::UnsupportedScheme
        }]
    );
}

#[cfg(unix)]
#[test]
fn from_env_http_proxy_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_HTTP_PROXY",
        Some(OsString::from_vec(vec![b'a', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_HTTP_PROXY"
        }
    );
}

#[test]
fn from_env_reads_token_first() {
    let seen = Mutex::new(Vec::new());
    let _ = AppBuilder::from_lookup(|var| {
        seen.lock().unwrap().push(var.to_owned());
        None
    })
    .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "NOCTIS_TOKEN",
            "NOCTIS_HTTP_PROXY",
            "NOCTIS_GATEWAY_URL",
            "NOCTIS_SHARD_TOTAL"
        ]
    );
}

// ---- gateway_url (Story 5.3 Matrix 10-12) ----

#[tokio::test]
async fn gateway_url_wiring() {
    let app = App::builder().gateway_url("WS://p:1/").build().unwrap();
    assert_eq!(app.gateway_url(), Some("ws://p:1"));
    assert!(format!("{app:?}").contains("gateway_url: Some(\"ws://p:1\")"));

    let app = App::builder()
        .gateway_url("ws://a:1")
        .gateway_url("ws://b:2")
        .build()
        .unwrap();
    assert_eq!(app.gateway_url(), Some("ws://b:2"));

    let app = App::builder().build().unwrap();
    assert_eq!(app.gateway_url(), None);

    let err = App::builder()
        .gateway_url("http://x")
        .http_proxy("https://x")
        .http_timeout(Duration::ZERO)
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpTimeout {
                actual: Duration::ZERO
            },
            BuildViolation::InvalidHttpProxy {
                reason: crate::HttpProxyError::UnsupportedScheme
            },
            BuildViolation::InvalidGatewayUrl {
                reason: crate::GatewayUrlError::UnsupportedScheme
            },
        ]
    );
}

#[tokio::test]
async fn gateway_url_redaction() {
    let mut builder = App::builder();
    builder.gateway_url("wss://u:SECRET@h");
    let debug = format!("{builder:?}");
    assert!(debug.contains("gateway_url: true"), "{debug}");
    assert!(!debug.contains("SECRET"), "{debug}");
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidGatewayUrl {
            reason: crate::GatewayUrlError::NotAnAuthority
        }]
    );
    assert!(err.to_string().contains("invalid gateway URL: "));
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains("SECRET"), "{text}");
    }
}

#[tokio::test]
async fn from_env_gateway_url() {
    for raw in [None, Some(""), Some(" \t")] {
        let b = AppBuilder::from_lookup(lookup_var("NOCTIS_GATEWAY_URL", raw.map(OsString::from)))
            .unwrap();
        assert!(b.gateway_url.is_none(), "{raw:?}");
    }
    let app = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_GATEWAY_URL",
        Some(" ws://p:1 \n".into()),
    ))
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(app.gateway_url(), Some("ws://p:1"));

    let err = AppBuilder::from_lookup(lookup_var("NOCTIS_GATEWAY_URL", Some("http://x".into())))
        .unwrap()
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidGatewayUrl {
            reason: crate::GatewayUrlError::UnsupportedScheme
        }]
    );
}

#[cfg(unix)]
#[test]
fn from_env_gateway_url_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_GATEWAY_URL",
        Some(OsString::from_vec(vec![b'a', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_GATEWAY_URL"
        }
    );
}

// ---- shard_total (Story 5.4 Matrix 8, 9, 18) ----

#[tokio::test]
async fn shard_total_builder() {
    assert_eq!(
        App::builder().shard_total(5).build().unwrap().shard_total(),
        Some(5)
    );
    assert_eq!(App::builder().build().unwrap().shard_total(), None);
    let err = App::builder().shard_total(0).build().unwrap_err();
    assert_eq!(err.violations(), [BuildViolation::InvalidShardTotal]);
    assert_eq!(
        err.violations()[0].to_string(),
        "shard total must be at least 1: omit shard_total to use Discord's recommendation"
    );
    assert_eq!(
        App::builder()
            .shard_total(0)
            .shard_total(5)
            .build()
            .unwrap()
            .shard_total(),
        Some(5)
    );
    assert_eq!(
        App::builder()
            .shard_total(u32::MAX)
            .build()
            .unwrap()
            .shard_total(),
        Some(u32::MAX)
    );
    let err = App::builder()
        .shard_total(0)
        .http_timeout(Duration::ZERO)
        .gateway_url("http://x")
        .build()
        .unwrap_err();
    assert!(matches!(
        err.violations(),
        [
            BuildViolation::InvalidHttpTimeout { .. },
            BuildViolation::InvalidGatewayUrl { .. },
            BuildViolation::InvalidShardTotal,
        ]
    ));
}

#[tokio::test]
async fn shard_total_debug() {
    let mut builder = App::builder();
    builder.shard_total(7);
    assert!(format!("{builder:?}").contains("shard_total: Some(7)"));
    let app = builder.build().unwrap();
    assert!(format!("{app:?}").contains("shard_total: Some(7)"));
}

#[test]
fn from_env_shard_total() {
    let read = |raw: &str| {
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_TOTAL", Some(raw.into())))
            .map(|b| b.shard_total)
    };
    assert_eq!(
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_TOTAL", None))
            .unwrap()
            .shard_total,
        None
    );
    for (raw, expected) in [
        ("", None),
        (" \t", None),
        (" 12 \n", Some(12)),
        ("007", Some(7)),
        ("4294967295", Some(u32::MAX)),
        ("0", Some(0)),
    ] {
        assert_eq!(read(raw), Ok(expected), "{raw:?}");
    }
    for raw in [
        "4294967296",
        "-1",
        "+5",
        "1_000",
        "1e3",
        "abc",
        "\u{ff11}\u{ff12}",
        "12345678901",
    ] {
        let err = read(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidShardTotal {
                var: "NOCTIS_SHARD_TOTAL"
            },
            "{raw:?}"
        );
        assert_eq!(
            err.to_string(),
            "environment variable `NOCTIS_SHARD_TOTAL` is not a shard total: set a whole number \
             of shards such as 16"
        );
    }
}

#[tokio::test]
async fn from_env_shard_total_zero_fails_at_build() {
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_TOTAL", Some("0".into()))).unwrap();
    let err = builder.build().unwrap_err();
    assert_eq!(err.violations(), [BuildViolation::InvalidShardTotal]);
}

#[cfg(unix)]
#[test]
fn from_env_shard_total_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_SHARD_TOTAL",
        Some(OsString::from_vec(vec![b'1', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_SHARD_TOTAL"
        }
    );
}

#[test]
fn startup_close_codes_map_to_start_errors() {
    assert_eq!(start_error(3, Some(4004)), StartError::InvalidToken);
    assert_eq!(
        start_error(3, Some(4013)),
        StartError::InvalidIntents { shard: 3 }
    );
    assert_eq!(
        start_error(3, Some(4014)),
        StartError::DisallowedIntents { shard: 3 }
    );
    for code in [4010, 4011, 4012] {
        assert_eq!(
            start_error(3, Some(code)),
            StartError::FatalClose { shard: 3, code }
        );
    }
    assert_eq!(start_error(3, None), StartError::ShardEnded { shard: 3 });
}

#[tokio::test]
async fn startup_waits_for_every_ready_and_maps_closed_channel() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    for event in [
        ShardEvent::Ready { shard: 1 },
        ShardEvent::Ready { shard: 1 },
        ShardEvent::Disconnected {
            shard: 0,
            close_code: Some(4000),
        },
        ShardEvent::Resumed { shard: 0 },
        ShardEvent::Ready { shard: 9 },
        ShardEvent::Ready { shard: 0 },
    ] {
        tx.send(event).unwrap();
    }
    assert_eq!(await_startup(&mut rx, 2).await, Ok(()));

    tx.send(ShardEvent::Ready { shard: 0 }).unwrap();
    drop(tx);
    assert_eq!(
        await_startup(&mut rx, 2).await,
        Err(StartError::GatewayStopped)
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Ended {
        shard: 1,
        close_code: Some(4014),
    })
    .unwrap();
    assert_eq!(
        await_startup(&mut rx, 2).await,
        Err(StartError::DisallowedIntents { shard: 1 })
    );
}

#[test]
fn start_error_messages_name_cause_and_fix() {
    let errors = [
        StartError::InvalidToken,
        StartError::GatewayInfo { status: Some(500) },
        StartError::GatewayInfo { status: None },
        StartError::InvalidGatewayInfo,
        StartError::ShardTotalTooSmall {
            total: 4,
            minimum: 8,
        },
        StartError::SessionStartLimit {
            required: 20,
            remaining: 19,
            reset_after: Duration::from_secs(5),
        },
        StartError::InvalidIntents { shard: 0 },
        StartError::DisallowedIntents { shard: 0 },
        StartError::FatalClose {
            shard: 0,
            code: 4011,
        },
        StartError::ShardEnded { shard: 0 },
        StartError::GatewayStopped,
    ];
    for error in errors {
        let text = error.to_string();
        assert!(text.contains(':'), "{text}");
    }
    assert!(
        StartError::InvalidToken
            .to_string()
            .starts_with("Discord rejected the bot token (HTTP 401 or close 4004)")
    );
    assert!(
        StartError::GatewayInfo { status: Some(500) }
            .to_string()
            .contains("HTTP 500")
    );
    assert!(
        StartError::DisallowedIntents { shard: 0 }
            .to_string()
            .contains(
                "enable the privileged intents in the Developer Portal or stop requesting them"
            )
    );
}

/// One lane item for the drain tests.
fn lane_item(kind: noctis_gateway::InboundKind) -> Inbound {
    Inbound {
        shard: twilight_model::gateway::ShardId::ONE,
        received_at: tokio::time::Instant::now(),
        kind,
    }
}

/// Matrix 28: `drain` returns once every sender has dropped and the lanes
/// are empty.
#[tokio::test]
async fn drain_ends_when_senders_drop() {
    use noctis_gateway::{InboundKind, InboundSink};
    use twilight_model::gateway::event::Event;

    let (sender, receiver) = noctis_gateway::lanes(LaneConfig::default());
    sender.push(lane_item(InboundKind::Event(Box::new(Event::Resumed))));
    sender.push(lane_item(InboundKind::Event(Box::new(
        Event::GatewayHeartbeatAck,
    ))));
    drop(sender);
    tokio::time::timeout(Duration::from_secs(5), drain(receiver))
        .await
        .unwrap();
}

/// Matrix 28: dropping `AbortOnDrop` aborts a pending drain task.
#[tokio::test]
async fn abort_on_drop_aborts_pending_drain() {
    let (sender, receiver) = noctis_gateway::lanes(LaneConfig::default());
    let handle = tokio::spawn(drain(receiver));
    let abort = handle.abort_handle();
    tokio::task::yield_now().await;
    assert!(!abort.is_finished());
    drop(AbortOnDrop(handle));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !abort.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(sender);
}
