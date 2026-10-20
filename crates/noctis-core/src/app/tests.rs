use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use noctis_ports::{
    BoxFuture, PortError, RenderError, RenderJob, RenderOutput, TemplateDescriptor,
};

use super::*;
use crate::ports::PortSlot;
use crate::{HttpConfigError, Resource};

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
                "NOCTIS_TOKEN"
                    | "NOCTIS_HTTP_PROXY"
                    | "NOCTIS_GATEWAY_URL"
                    | "NOCTIS_SHARD_TOTAL"
                    | "NOCTIS_SHUTDOWN_GRACE"
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
            "NOCTIS_SHARD_TOTAL",
            "NOCTIS_SHUTDOWN_GRACE",
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
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
        Ok(())
    );

    tx.send(ShardEvent::Ready { shard: 0 }).unwrap();
    drop(tx);
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
        Err(StartError::GatewayStopped)
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Ended {
        shard: 1,
        close_code: Some(4014),
    })
    .unwrap();
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
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

/// A `ShardStatus` of App `"default"` over `total` shards.
fn status(total: u32) -> ShardStatus {
    ShardStatus {
        app: Arc::from("default"),
        total,
    }
}

// ---- shutdown_grace (Story 5.8 Matrix 1-8) ----

#[tokio::test]
async fn shutdown_grace_builder() {
    let grace = |value: Duration| App::builder().shutdown_grace(value).build();
    assert_eq!(
        App::builder().build().unwrap().shutdown_grace(),
        Duration::from_secs(10),
        "Matrix 1"
    );
    for ok in [
        Duration::ZERO,
        Duration::from_mins(15),
        Duration::from_nanos(1),
    ] {
        assert_eq!(grace(ok).unwrap().shutdown_grace(), ok, "Matrix 2: {ok:?}");
    }
    for bad in [
        Duration::from_mins(15) + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        let err = grace(bad).unwrap_err();
        assert_eq!(
            err.violations(),
            [BuildViolation::InvalidShutdownGrace { actual: bad }],
            "Matrix 3"
        );
    }
    assert_eq!(
        BuildViolation::InvalidShutdownGrace {
            actual: Duration::from_secs(901)
        }
        .to_string(),
        "shutdown grace 901s is out of range: use a value in 0 s..=900 s"
    );
    let err = App::builder()
        .http_timeout(Duration::ZERO)
        .handler_concurrency(0)
        .shutdown_grace(Duration::MAX)
        .build()
        .unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [
                BuildViolation::InvalidHttpTimeout { .. },
                BuildViolation::InvalidHandlerConcurrency { .. },
                BuildViolation::InvalidShutdownGrace { .. },
            ]
        ),
        "Matrix 3 order: {err:?}"
    );

    let mut builder = App::builder();
    builder
        .shutdown_grace(Duration::from_secs(1))
        .shutdown_grace(Duration::from_secs(2));
    assert!(format!("{builder:?}").contains("shutdown_grace: 2s"));
    let app = builder.build().unwrap();
    assert_eq!(app.shutdown_grace(), Duration::from_secs(2), "Matrix 4");
    assert!(format!("{app:?}").contains("shutdown_grace: 2s"));
    assert_eq!(
        builder.build().unwrap().shutdown_grace(),
        Duration::from_secs(10),
        "Matrix 4 reset"
    );
}

#[tokio::test]
async fn from_env_shutdown_grace() {
    let read = |raw: &str| {
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", Some(raw.into())))
            .map(|b| b.shutdown_grace)
    };
    for (raw, secs) in [("30", 30), (" 0 ", 0), ("0900", 900), ("9999", 9999)] {
        assert_eq!(
            read(raw),
            Ok(Duration::from_secs(secs)),
            "Matrix 5: {raw:?}"
        );
    }
    let err = AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", Some("9999".into())))
        .unwrap()
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidShutdownGrace {
            actual: Duration::from_secs(9999)
        }]
    );
    assert_eq!(
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", None))
            .unwrap()
            .shutdown_grace,
        Duration::from_secs(10),
        "Matrix 6: missing"
    );
    for raw in ["", "   "] {
        assert_eq!(read(raw), Ok(Duration::from_secs(10)), "Matrix 6: {raw:?}");
    }
    for raw in [
        "10000",
        "1.5",
        "-1",
        "+1",
        "10s",
        "1e3",
        "1_0",
        "\u{ff13}\u{ff10}",
        "\u{663}",
    ] {
        let err = read(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidShutdownGrace {
                var: "NOCTIS_SHUTDOWN_GRACE"
            },
            "Matrix 7: {raw:?}"
        );
        let text = err.to_string();
        assert!(text.contains("NOCTIS_SHUTDOWN_GRACE"), "{text}");
        assert!(!text.contains(raw), "{text}");
    }
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", Some("30".into()))).unwrap();
    builder.shutdown_grace(Duration::from_secs(5));
    assert_eq!(
        builder.build().unwrap().shutdown_grace(),
        Duration::from_secs(5),
        "Matrix 8 override"
    );
}

#[cfg(unix)]
#[test]
fn from_env_shutdown_grace_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_SHUTDOWN_GRACE",
        Some(OsString::from_vec(vec![b'1', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_SHUTDOWN_GRACE"
        },
        "Matrix 8"
    );
}

// ---- run, signals and errors (Story 5.8 Matrix 40) ----

#[test]
fn run_future_is_send() {
    fn assert_send<T: Send>(_: &T) {}
    let tokio_rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let _guard = tokio_rt.enter();
    let app = App::builder().build().unwrap();
    let run = app.run();
    assert_send(&run);
    drop(run);
    let app = App::builder().build().unwrap();
    let run = app.run_until(std::future::pending());
    assert_send(&run);
}

/// Matrix 40: a runtime without an IO driver cannot install the signal
/// handlers; `run` returns a typed error instead of panicking.
#[test]
fn run_without_io_driver() {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let outcome = tokio_rt.block_on(async {
        let app = App::builder().token(token()).build().unwrap();
        app.run().await
    });
    assert!(
        matches!(outcome, Err(Error::Start(StartError::SignalHandler))),
        "{outcome:?}"
    );
    assert_eq!(
        StartError::SignalHandler.to_string(),
        "the Ctrl-C/SIGTERM handler could not be installed: build the tokio runtime with \
         enable_all() (as #[tokio::main] does)"
    );
}

#[test]
fn run_error_message() {
    let error = Error::from(RunError::AllShardsEnded);
    assert!(matches!(error, Error::Run(RunError::AllShardsEnded)));
    assert_eq!(
        error.to_string(),
        "every shard ended without a shutdown request (see the \"shard closed fatally\" logs): \
         fix the cause, then restart"
    );
}

/// An already-resolved shutdown ends `run_until` with `Ok(())` before any
/// request is sent, logging `shutdown started` and `shutdown complete`.
#[tokio::test(flavor = "multi_thread")]
async fn run_until_already_requested() {
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let app = App::builder()
        .token(token())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(15), app.run_until(async {})).await;
    assert!(matches!(outcome, Ok(Ok(()))), "{outcome:?}");
    let text = capture.text();
    let started = text
        .find("shutdown started")
        .unwrap_or_else(|| panic!("shutdown started missing: {text}"));
    let complete = text
        .find("shutdown complete")
        .unwrap_or_else(|| panic!("shutdown complete missing: {text}"));
    assert!(started < complete, "{text}");
    assert!(
        text.contains("reason=\"requested\"") || text.contains("reason=requested"),
        "{text}"
    );
}

// ---- supervise and lifecycle (Story 5.8 Matrix 28-34) ----

/// Drives `supervise` over `events` (sender dropped by the caller or not)
/// with a shutdown that never resolves, recording metrics and logs.
struct Supervised {
    capture: crate::test_support::Capture,
    snapshot: Vec<(
        metrics_util::CompositeKey,
        Option<metrics::Unit>,
        Option<metrics::SharedString>,
        metrics_util::debugging::DebugValue,
    )>,
    lifecycle: Vec<LifecycleEvent>,
    stop: Option<Stop>,
}

/// Feeds `events` to `supervise` (total 2) and stops after they are handled.
async fn supervised(events: &[ShardEvent], close: bool) -> Supervised {
    use metrics_util::debugging::DebuggingRecorder;

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let lifecycle = Lifecycle::new();
    let mut rx_lifecycle = lifecycle.subscribe();
    let status = status(2);
    status.init();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for event in events {
        tx.send(*event).unwrap();
    }
    if close {
        drop(tx);
    } else {
        std::mem::forget(tx);
    }
    // `supervise` handles every queued event before a yield lets the
    // shutdown future (a 50 ms sleep) win.
    let stop = tokio::time::timeout(
        Duration::from_secs(15),
        supervise(
            &mut rx,
            &status,
            &lifecycle,
            None,
            tokio::time::sleep(Duration::from_millis(50)),
        ),
    )
    .await
    .ok();
    let snapshot = snapshotter.snapshot().into_vec();
    std::mem::forget(status);
    let mut seen = Vec::new();
    while let Ok(event) = rx_lifecycle.try_recv() {
        seen.push(event);
    }
    Supervised {
        capture,
        snapshot,
        lifecycle: seen,
        stop,
    }
}

impl Supervised {
    fn gauge(&self, shard: &str) -> Option<f64> {
        self.snapshot.iter().find_map(|(key, .., value)| {
            let matches = key.key().name() == "noctis_shard_status"
                && key
                    .key()
                    .labels()
                    .any(|l| l.key() == "shard" && l.value() == shard);
            match value {
                metrics_util::debugging::DebugValue::Gauge(v) if matches => Some(v.0),
                _ => None,
            }
        })
    }
}

/// Matrix 28: a fatal close after startup is loud and the App keeps
/// running.
#[tokio::test]
async fn fatal_close_after_startup() {
    let run = supervised(
        &[
            ShardEvent::Ready { shard: 0 },
            ShardEvent::Ready { shard: 1 },
            ShardEvent::Ended {
                shard: 1,
                close_code: Some(4014),
            },
        ],
        false,
    )
    .await;
    assert_eq!(run.stop, Some(Stop::Requested), "supervise keeps running");
    let lines = run
        .capture
        .lines_with(&["shard closed fatally", "shard=1", "close_code=4014"]);
    assert_eq!(lines.len(), 1, "{}", run.capture.text());
    assert!(lines[0].contains("ERROR"), "{}", lines[0]);
    assert!(
        lines[0].contains("4014 (disallowed intents)"),
        "{}",
        lines[0]
    );
    assert!(!run.capture.text().contains("shard ended\n"));
    assert_eq!(run.gauge("1"), Some(0.0));
    assert_eq!(run.gauge("0"), Some(1.0));
    assert_eq!(
        run.lifecycle
            .iter()
            .filter(|e| matches!(e, LifecycleEvent::ShardFatal { .. }))
            .collect::<Vec<_>>(),
        [&LifecycleEvent::ShardFatal {
            shard: 1,
            close_code: Some(4014)
        }]
    );
}

/// Matrix 29: each fatal cause is logged with its typed error text.
#[tokio::test]
async fn fatal_close_causes() {
    for code in [None, Some(4004), Some(4010), Some(4012)] {
        let run = supervised(
            &[ShardEvent::Ended {
                shard: 0,
                close_code: code,
            }],
            false,
        )
        .await;
        let expected = start_error(0, code).to_string();
        let lines = run.capture.lines_with(&["shard closed fatally", &expected]);
        assert_eq!(lines.len(), 1, "{code:?}: {}", run.capture.text());
        assert_eq!(
            run.lifecycle,
            [LifecycleEvent::ShardFatal {
                shard: 0,
                close_code: code
            }]
        );
    }
    assert_eq!(start_error(0, None), StartError::ShardEnded { shard: 0 });
    assert_eq!(start_error(0, Some(4004)), StartError::InvalidToken);
}

/// Matrix 30: every shard ended (or the channel closed) stops supervise.
#[tokio::test]
async fn all_shards_ended() {
    let ended = |shard| ShardEvent::Ended {
        shard,
        close_code: Some(4004),
    };
    let run = supervised(&[ended(0), ended(1)], false).await;
    assert_eq!(run.stop, Some(Stop::AllEnded));
    let run = supervised(&[ended(0), ended(0)], false).await;
    assert_eq!(
        run.stop,
        Some(Stop::Requested),
        "one shard twice is not all"
    );
    let run = supervised(&[ended(0)], true).await;
    assert_eq!(run.stop, Some(Stop::AllEnded), "closed channel");
}

/// Matrix 31: an out-of-range shard is logged and broadcast; no gauge.
#[tokio::test]
async fn out_of_range_fatal() {
    let run = supervised(
        &[ShardEvent::Ended {
            shard: 9,
            close_code: Some(4014),
        }],
        false,
    )
    .await;
    assert_eq!(
        run.capture
            .lines_with(&["shard closed fatally", "shard=9"])
            .len(),
        1
    );
    assert_eq!(
        run.lifecycle,
        [LifecycleEvent::ShardFatal {
            shard: 9,
            close_code: Some(4014)
        }]
    );
    assert_eq!(run.gauge("9"), None);
    assert_eq!(run.stop, Some(Stop::Requested));
}

/// Matrix 32: the lifecycle mapping; `Closed` broadcasts nothing.
#[tokio::test]
async fn lifecycle_mapping() {
    let run = supervised(
        &[
            ShardEvent::Ready { shard: 0 },
            ShardEvent::Resumed { shard: 0 },
            ShardEvent::Disconnected {
                shard: 0,
                close_code: Some(4000),
            },
            ShardEvent::Ready { shard: 1 },
            ShardEvent::Closed { shard: 1 },
        ],
        false,
    )
    .await;
    assert_eq!(
        run.lifecycle,
        [
            LifecycleEvent::ShardReady { shard: 0 },
            LifecycleEvent::ShardResumed { shard: 0 },
            LifecycleEvent::ShardDisconnected {
                shard: 0,
                close_code: Some(4000)
            },
            LifecycleEvent::ShardReady { shard: 1 },
        ]
    );
    assert_eq!(run.gauge("1"), Some(0.0));
    assert!(!run.capture.text().contains("fatally"));
    assert_eq!(
        run.capture
            .lines_with(&["DEBUG", "shard closed", "shard=1"])
            .len(),
        1
    );
}

/// Matrix 33: no receivers or a lagging one never block or fail.
#[tokio::test]
async fn lifecycle_without_receivers_and_lag() {
    let lifecycle = Lifecycle::new();
    lifecycle.send(LifecycleEvent::ShutdownStarted);
    let mut rx = lifecycle.subscribe();
    for shard in 0..300 {
        lifecycle.shard(&ShardEvent::Ready { shard });
    }
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(44))
    ));
    assert_eq!(rx.try_recv(), Ok(LifecycleEvent::ShardReady { shard: 44 }));
}

/// `await_startup` broadcasts shard events, and a fatal close during
/// startup is a `ShardFatal` too.
#[tokio::test]
async fn startup_broadcasts_lifecycle() {
    let lifecycle = Lifecycle::new();
    let mut seen = lifecycle.subscribe();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Ready { shard: 0 }).unwrap();
    tx.send(ShardEvent::Ended {
        shard: 1,
        close_code: Some(4014),
    })
    .unwrap();
    assert_eq!(
        await_startup(&mut rx, &status(2), &lifecycle).await,
        Err(StartError::DisallowedIntents { shard: 1 })
    );
    assert_eq!(seen.try_recv(), Ok(LifecycleEvent::ShardReady { shard: 0 }));
    assert_eq!(
        seen.try_recv(),
        Ok(LifecycleEvent::ShardFatal {
            shard: 1,
            close_code: Some(4014)
        })
    );
}

/// Matrix 34: the App's lifecycle receiver sees exactly one
/// `ShutdownStarted` per shutdown (the sequence itself is covered end to end
/// in `noctis-testkit/tests/shutdown.rs`).
#[tokio::test]
async fn app_lifecycle_channel() {
    let app = App::builder().build().unwrap();
    let mut rx = app.subscribe_lifecycle();
    app.lifecycle.send(LifecycleEvent::ShutdownStarted);
    assert_eq!(rx.try_recv(), Ok(LifecycleEvent::ShutdownStarted));
    assert!(rx.try_recv().is_err());
}

/// Matrix 34: one `ShutdownStarted`, broadcast before any shard reports
/// `Closed`; the sequence logs in order.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_sequence_order() {
    use std::num::NonZeroU16;

    use noctis_gateway::EventTypeFlags;

    use crate::worker::Shutdown;

    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .shutdown_grace(Duration::from_millis(100))
        .build()
        .unwrap();
    let mut lifecycle = app.subscribe_lifecycle();
    let controls = Shutdown::default();
    let (sender, receiver) = noctis_gateway::lanes(app.lane_config());
    let mut drain = AbortOnDrop(tokio::spawn(drain(receiver, app.workers(controls.clone()))));
    // Nothing listens on port 1: the shards never identify, so they stop
    // at once without a close frame.
    let options = GatewayOptions {
        app: Arc::from("default"),
        token: SECRET.to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some("ws://127.0.0.1:1".to_owned()),
        event_types: EventTypeFlags::INTERACTION_CREATE,
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::MIN)),
        total: NonZeroU32::new(2).unwrap(),
        max_concurrency: NonZeroU16::MIN,
        close_timeout: SHARD_CLOSE_TIMEOUT,
    };
    let (gateway, mut events) = Gateway::start(options, Arc::new(sender)).unwrap();
    let status = status(2);
    let watch = Watch {
        events: &mut events,
        status: &status,
        lifecycle: &app.lifecycle,
    };
    tokio::time::timeout(
        Duration::from_secs(15),
        app.shut_down(
            Stop::Requested,
            "requested",
            gateway,
            watch,
            &controls,
            &mut drain,
        ),
    )
    .await
    .unwrap();
    let mut seen = Vec::new();
    while let Ok(event) = lifecycle.try_recv() {
        seen.push(event);
    }
    let started: Vec<_> = seen
        .iter()
        .filter(|e| **e == LifecycleEvent::ShutdownStarted)
        .collect();
    assert_eq!(started.len(), 1, "{seen:?}");
    let text = capture.text();
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing: {text}"))
    };
    assert!(
        text.contains("reason=\"requested\"") || text.contains("reason=requested"),
        "{text}"
    );
    assert!(at("shutdown started") < at("shard closed"), "{text}");
    assert!(at("shard closed") < at("shutdown complete"), "{text}");
    assert_eq!(text.matches("shard closed").count(), 2, "{text}");
    assert!(!text.contains("handlers still running"), "{text}");
    assert!(drain.0.is_finished());
    assert!(controls.tracker.is_closed() && controls.tracker.is_empty());
}

// ---- HttpConfig and the edge (Story 5.9 Matrix 1-5, 7, 24) ----

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Matrix 1: the default config.
#[tokio::test]
async fn http_config_default() {
    let app = App::builder().build().unwrap();
    let config = app.http_config();
    assert_eq!(config, HttpConfig::default());
    assert_eq!(config.ping_interval, Duration::from_secs(20));
    assert_eq!(config.ping_timeout, Duration::from_secs(5));
    assert_eq!(config.hedge_after, ms(1000));
    assert_eq!(config.connect_timeout, ms(1500));
    assert_eq!(config.dns_ttl_floor, Duration::from_secs(30));
    assert_eq!(config.dns_ttl_cap, Duration::from_mins(5));
    assert!(!config.upstream_h2c);
    assert!(format!("{app:?}").contains("http_config"));
    assert!(format!("{:?}", App::builder()).contains("http_config"));
}

/// Matrix 2: both ends of every range build.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_bounds_ok() {
    let low = {
        let mut c = HttpConfig::default();
        c.ping_interval = ms(50);
        c.ping_timeout = ms(50);
        c.hedge_after = ms(300);
        c.connect_timeout = ms(100);
        c.dns_ttl_floor = Duration::from_secs(1);
        c.dns_ttl_cap = Duration::from_secs(1);
        c
    };
    let high = {
        let mut c = HttpConfig::default();
        c.ping_interval = Duration::from_mins(5);
        c.ping_timeout = Duration::from_mins(1);
        c.hedge_after = ms(2800);
        c.connect_timeout = Duration::from_secs(30);
        c.dns_ttl_floor = Duration::from_hours(24);
        c.dns_ttl_cap = Duration::from_hours(24);
        c
    };
    for config in [low, high] {
        let app = App::builder().http(config).build().unwrap();
        assert_eq!(app.http_config(), config);
    }
}

/// Matrix 3: one violation per bad field, in field order, among the
/// others, with exact messages.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_bounds_bad() {
    let ns = Duration::from_nanos(1);
    for bad in [
        ms(49),
        Duration::ZERO,
        Duration::from_mins(5) + ns,
        Duration::MAX,
    ] {
        let mut config = HttpConfig::default();
        config.ping_interval = bad;
        let err = App::builder().http(config).build().unwrap_err();
        assert_eq!(
            err.violations(),
            [BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::PingInterval(bad)
            }]
        );
    }
    let mut config = HttpConfig::default();
    config.ping_interval = ms(49);
    config.ping_timeout = Duration::from_mins(1) + ns;
    config.hedge_after = ms(299);
    config.connect_timeout = ms(99);
    config.dns_ttl_floor = ms(999);
    config.dns_ttl_cap = Duration::from_hours(24) + ns;
    let err = App::builder()
        .http_timeout(Duration::ZERO)
        .http(config)
        .shutdown_grace(Duration::MAX)
        .build()
        .unwrap_err();
    let messages: Vec<String> = err.violations().iter().map(ToString::to_string).collect();
    assert_eq!(
        messages,
        [
            "HTTP timeout 0ns is out of range: use a value in 1 ms..=300 s".to_owned(),
            format!(
                "shutdown grace {:?} is out of range: use a value in 0 s..=900 s",
                Duration::MAX
            ),
            "invalid HTTP config: HttpConfig.ping_interval 49ms is out of range: use 50 ms..=300 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.ping_timeout 60.000000001s is out of range: use 50 ms..=60 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.hedge_after 299ms is out of range: use 300 ms..=2800 ms"
                .to_owned(),
            "invalid HTTP config: HttpConfig.connect_timeout 99ms is out of range: use 100 ms..=30 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.dns_ttl_floor 999ms is out of range: use 1 s..=86400 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.dns_ttl_cap 86400.000000001s is out of range: use 1 s..=86400 s"
                .to_owned(),
        ]
    );
    let mut config = HttpConfig::default();
    config.hedge_after = ms(2800) + ns;
    config.connect_timeout = Duration::from_secs(30) + ns;
    config.ping_timeout = ms(49);
    let err = App::builder().http(config).build().unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::PingTimeout(ms(49))
            },
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::HedgeAfter(ms(2800) + ns)
            },
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::ConnectTimeout(Duration::from_secs(30) + ns)
            },
        ]
    );
}

/// Matrix 4: the TTL order rule.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_ttl_order() {
    let mut config = HttpConfig::default();
    config.dns_ttl_floor = Duration::from_mins(1);
    config.dns_ttl_cap = Duration::from_secs(30);
    let err = App::builder().http(config).build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidHttpConfig {
            reason: HttpConfigError::DnsTtlOrder {
                floor: Duration::from_mins(1),
                cap: Duration::from_secs(30)
            }
        }]
    );
    assert_eq!(
        err.violations()[0].to_string(),
        "invalid HTTP config: HttpConfig.dns_ttl_floor 60s is above dns_ttl_cap 30s"
    );
    config.dns_ttl_floor = Duration::ZERO;
    config.dns_ttl_cap = Duration::ZERO;
    let err = App::builder().http(config).build().unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::DnsTtlFloor(Duration::ZERO)
            },
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::DnsTtlCap(Duration::ZERO)
            },
        ]
    );
}

/// Matrix 5: the last call wins and `build()` resets it.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_last_wins_and_reset() {
    let mut a = HttpConfig::default();
    a.hedge_after = ms(500);
    let mut b = HttpConfig::default();
    b.hedge_after = ms(700);
    let mut builder = App::builder();
    let app = builder.http(a).http(b).build().unwrap();
    assert_eq!(app.http_config(), b);
    let again = builder.build().unwrap();
    assert_eq!(again.http_config(), HttpConfig::default());
}

/// The edge-bind violation's message.
#[test]
fn http_edge_violation_message() {
    let v = BuildViolation::HttpEdge {
        kind: std::io::ErrorKind::AddrInUse,
    };
    assert_eq!(
        v.to_string(),
        "the Discord HTTP edge could not bind 127.0.0.1:0 (AddrInUse)"
    );
}

/// Matrix 7: a runtime without an IO driver still builds; the edge logs
/// that it could not start and nothing panics out.
#[test]
fn build_without_io_driver() {
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let tokio_rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tokio_rt.block_on(async {
        let app = App::builder().token(token()).build().unwrap();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        drop(app);
    });
    assert!(
        capture.text().contains(
            "the Discord HTTP edge could not start: build the tokio runtime with enable_all()"
        ),
        "{}",
        capture.text()
    );
}

/// Matrix 24: only a disconnect without a close frame asks the keeper for a
/// redial.
#[tokio::test]
async fn shard_network_loss_requests_redial() {
    let app = App::builder().build().unwrap();
    let keeper = app.edge().keeper();
    let lifecycle = Lifecycle::new();
    let status = status(2);
    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Disconnected {
        shard: 0,
        close_code: None,
    })
    .unwrap();
    tx.send(ShardEvent::Disconnected {
        shard: 0,
        close_code: Some(4000),
    })
    .unwrap();
    let stop = supervise(
        &mut rx,
        &status,
        &lifecycle,
        Some(&keeper),
        tokio::time::sleep(Duration::from_millis(50)),
    )
    .await;
    assert_eq!(stop, Stop::Requested);
    assert_eq!(keeper.redial_requests(), 1);
}

/// The 2.9 s deadline error's message.
#[test]
fn ack_deadline_missed_message() {
    let error = crate::AckDeadlineMissed {
        elapsed: Duration::from_millis(2950),
    };
    assert_eq!(
        error.to_string(),
        "interaction not acknowledged in time: 2.95s after receipt, past the 2.9 s ack deadline \
         (Discord allows 3 s; INT-01): defer slow work"
    );
}
