//! Story 11.5 AC 1-3: runtime locale overlays loaded through the
//! `LocaleSource` port at startup reach `reply_text`, `Text::Key`s and the
//! typed `Strings::of(..)` accessors; a reserved key, a placeholder
//! mismatch or a failed load stops startup with `Error::LocaleOverlay`.
//! Overlays are process-wide per catalog, so every installing test uses its
//! own `i18n!` invocation (its own slot).

#![cfg(all(feature = "i18n", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use noctis::__private::core::__testing::AppProbe;
use noctis::ports::{BoxFuture, LocaleSource, PortError};
use noctis::prelude::*;
use noctis::rules::Locale;
use noctis::testkit::{TestApp, TestError};
use noctis::{Error, LocaleOverlayError, OverlayViolation, Runtime};

/// A bot over `tests/fixtures/i18n/overlay` with its own catalog: `/greet`
/// replies `reply_text(keys::GREET_HELLO)`, `/typed` replies
/// `Strings::of(user locale).greet.hello()` and `/left` replies
/// `Strings::of(user locale).shop.left(3)`.
macro_rules! bot {
    ($name:ident) => {
        #[expect(deprecated, reason = "tr.yaml is partial (LOC-04)")]
        mod $name {
            use noctis::prelude::*;
            use noctis::ui::{Components, TextDisplay, V2Message};

            noctis::i18n!(dir = "tests/fixtures/i18n/overlay");

            fn text(text: impl Into<String>) -> V2Message {
                V2Message::new(Components::new().push(TextDisplay::new(text.into())))
            }

            #[derive(SlashCommand)]
            #[command("greet", locales = "tests/fixtures/i18n/overlay")]
            pub struct Greet;

            impl Greet {
                #[handler]
                async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
                    ctx.reply_text(keys::GREET_HELLO).public().await
                }
            }

            #[derive(SlashCommand)]
            #[command("typed", description = "Typed hello")]
            pub struct Typed;

            impl Typed {
                #[handler]
                async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
                    let hello = Strings::of(ctx.user_locale()).greet.hello();
                    ctx.respond(text(hello)).await
                }
            }

            #[derive(SlashCommand)]
            #[command("left", description = "Items left")]
            pub struct Left;

            impl Left {
                #[handler]
                async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
                    let left = Strings::of(ctx.user_locale()).shop.left(3);
                    ctx.respond(text(left)).await
                }
            }

            pub fn builder() -> AppBuilder {
                let mut builder = App::builder();
                builder
                    .module(
                        Module::new("overlay")
                            .add::<Greet>()
                            .add::<Typed>()
                            .add::<Left>(),
                    )
                    .locales(catalog());
                builder
            }
        }
    };
}

/// A `LocaleSource` answering from a map (absent codes: no overlay), or
/// failing for one code; it records every call.
#[derive(Default)]
struct MapSource {
    overlays: HashMap<&'static str, Vec<(String, String)>>,
    fail: Option<&'static str>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl MapSource {
    fn with(mut self, code: &'static str, pairs: &[(&str, &str)]) -> Self {
        self.overlays.insert(
            code,
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        );
        self
    }
}

impl LocaleSource for MapSource {
    fn load<'a>(
        &'a self,
        locale: &'a str,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>> {
        self.calls.lock().unwrap().push(locale.to_owned());
        Box::pin(async move {
            if self.fail == Some(locale) {
                return Err(PortError::unavailable("translations offline"));
            }
            Ok(self.overlays.get(locale).cloned().unwrap_or_default())
        })
    }
}

/// The Publish JSON of every command of `builder`'s App.
fn publish_json(mut builder: AppBuilder) -> String {
    builder.runtime(&Runtime::for_test());
    let app = builder.build().unwrap();
    let commands: Vec<_> = AppProbe::of(&app)
        .commands()
        .into_iter()
        .map(|(_, command)| command)
        .collect();
    assert!(commands.iter().any(|c| c["description"] == "Greet"));
    serde_json::to_string(&commands).unwrap()
}

/// The text `command` answers a `locale` user with.
async fn run(app: &TestApp, command: &str, locale: &str) -> String {
    app.command(command)
        .locale(locale)
        .run()
        .await
        .unwrap()
        .text()
}

/// The `Invalid` violations `TestApp::build` fails with.
async fn invalid(mut builder: AppBuilder, source: MapSource) -> Vec<OverlayViolation> {
    builder.locale_source(source);
    match TestApp::build(&mut builder).await {
        Err(TestError::Start(Error::LocaleOverlay(LocaleOverlayError::Invalid(v)))) => v,
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("startup succeeded"),
    }
}

bot!(fix);
bot!(add);
bot!(mismatch);
bot!(reserved);
bot!(load_fails);

/// Story 11.5 Matrix 1 (AC 1): a fixed translation reaches `reply_text`
/// and the typed accessors; en-US keeps the compiled text.
#[tokio::test(flavor = "multi_thread")]
async fn overlay_fixes_reply_text_and_typed_accessors() {
    let before = publish_json(fix::builder());
    let mut builder = fix::builder();
    builder.locale_source(MapSource::default().with("tr", &[("greet.hello", "Selam")]));
    let app = TestApp::build(&mut builder).await.unwrap();
    assert!(run(&app, "greet", "tr").await.contains("Selam"));
    assert!(run(&app, "typed", "tr").await.contains("Selam"));
    assert!(run(&app, "greet", "en-US").await.contains("Hello"));
    assert!(run(&app, "typed", "en-US").await.contains("Hello"));
    assert_eq!(fix::Strings::of(Locale::Tr).greet.hello(), "Selam");
    // Matrix 28: Publish keeps the compiled command texts.
    let after = publish_json(fix::builder());
    assert_eq!(after, before);
    app.stop().await.unwrap();
}

/// Story 11.5 Matrix 2 (AC 1): an overlay adds a translation the tr file
/// lacks; other locales keep the default text.
#[tokio::test(flavor = "multi_thread")]
async fn overlay_adds_a_missing_translation() {
    let mut builder = add::builder();
    builder.locale_source(MapSource::default().with("tr", &[("shop.left", "{n} kaldı")]));
    let app = TestApp::build(&mut builder).await.unwrap();
    assert!(run(&app, "left", "tr").await.contains("3 kaldı"));
    assert!(run(&app, "left", "de").await.contains("3 left"));
    app.stop().await.unwrap();
}

/// Story 11.5 Matrix 13 (AC 3): a missing placeholder fails startup.
#[tokio::test(flavor = "multi_thread")]
async fn placeholder_mismatch_fails_startup() {
    let violations = invalid(
        mismatch::builder(),
        MapSource::default().with("tr", &[("shop.left", "kaldı")]),
    )
    .await;
    assert_eq!(
        violations,
        [OverlayViolation::Placeholders {
            locale: Locale::Tr,
            key: "shop.left".into(),
            expected: vec!["n".into()],
            found: vec![],
        }]
    );
    assert_eq!(
        violations[0].to_string(),
        "locale `tr`: overlay of `shop.left` must use the placeholders `{n}`; missing `{n}`"
    );
    assert_eq!(mismatch::Strings::of(Locale::Tr).shop.left(3), "3 left");
}

/// Story 11.5 Matrix 16 (AC 2): reserved keys fail startup and nothing is
/// installed.
#[tokio::test(flavor = "multi_thread")]
async fn reserved_keys_fail_startup() {
    let violations = invalid(
        reserved::builder(),
        MapSource::default()
            .with("en-US", &[("cmd.greet.description", "Hi")])
            .with(
                "tr",
                &[
                    ("cmdword.greet", "selamla"),
                    ("ctxcmd.k.name", "x"),
                    ("noctis.expired", "Süresi doldu"),
                    ("greet.hello", "Selam"),
                ],
            ),
    )
    .await;
    let key = |locale, key: &str| OverlayViolation::ReservedKey {
        locale,
        key: key.to_owned(),
    };
    assert_eq!(
        violations,
        [
            key(Locale::EnUs, "cmd.greet.description"),
            key(Locale::Tr, "cmdword.greet"),
            key(Locale::Tr, "ctxcmd.k.name"),
            key(Locale::Tr, "noctis.expired"),
        ]
    );
    assert_eq!(reserved::Strings::of(Locale::Tr).greet.hello(), "Merhaba");
}

/// Story 11.5 Matrix 23: a failed load names the locale, keeps the port's
/// error as the source and stops loading.
#[tokio::test(flavor = "multi_thread")]
async fn load_failure_fails_startup() {
    let mut builder = load_fails::builder();
    let source = MapSource {
        fail: Some("tr"),
        ..MapSource::default()
    };
    let calls = Arc::clone(&source.calls);
    builder.locale_source(source);
    let err = match TestApp::build(&mut builder).await {
        Err(TestError::Start(Error::LocaleOverlay(err))) => err,
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("startup succeeded"),
    };
    assert!(
        matches!(
            err,
            LocaleOverlayError::Load {
                locale: Locale::Tr,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "the LocaleSource could not load the `tr` overlay"
    );
    assert!(err.violations().is_empty());
    let source = std::error::Error::source(&err).expect("the port error");
    assert!(source.downcast_ref::<PortError>().is_some());
    assert_eq!(*calls.lock().unwrap(), ["en-US", "tr"]);
}
