//! Story 13.3 Matrix U1-U9: Publish leaves a disabled Module's
//! guild-scoped Commands out of that guild's target.

use std::sync::Arc;

use bytes::Bytes;
use noctis_ports::{BoxFuture, ModuleStateStore, PortError};

use super::*;
use crate::edge::test_support::TestServer;
use crate::{ModuleScope, Token};

const G1: Id<GuildMarker> = Id::new(1);
const G2: Id<GuildMarker> = Id::new(2);
const BOTH: &[Id<GuildMarker>] = &[G1, G2];
const ONE: &[Id<GuildMarker>] = &[G1];

/// `p`: global `/balance`, `/shop` on guilds 1 and 2, `cfg a` on guild 1;
/// `q`: `/admin` and `cfg b` on guild 1.
fn modules() -> Vec<Module> {
    vec![
        Module::new("p")
            .__add(command("p::balance", &["balance"], "d"))
            .__add(command_with(
                "p::shop",
                &["shop"],
                CommandMetadata::new().guilds(BOTH),
            ))
            .__add(command_with(
                "p::cfg_a",
                &["cfg a"],
                CommandMetadata::new().guilds(ONE),
            )),
        Module::new("q")
            .__add(command_with(
                "q::admin",
                &["admin"],
                CommandMetadata::new().guilds(ONE),
            ))
            .__add(command_with(
                "q::cfg_b",
                &["cfg b"],
                CommandMetadata::new().guilds(ONE),
            )),
    ]
}

/// An upstream answering the application and empty remote command lists.
async fn server() -> TestServer {
    TestServer::start_with(
        false,
        Arc::new(|seen| {
            let body: &'static [u8] = if seen.path.ends_with("/applications/@me") {
                br#"{"id": "7", "integration_types_config": {"0": {}}}"#
            } else {
                b"[]"
            };
            (hyper::StatusCode::OK, Bytes::from_static(body))
        }),
    )
    .await
}

fn publishing_app(server: &TestServer, store: Option<Arc<dyn ModuleStateStore>>) -> App {
    let mut builder = builder();
    builder
        .token(Token::new("SECRET123").unwrap())
        .http_proxy(server.authority());
    if let Some(store) = store {
        builder.module_state_store(store);
    }
    for module in modules() {
        builder.module(module);
    }
    builder.build().unwrap()
}

/// A command's name, with its subcommands as `{a,b}`.
fn label(command: &Value) -> String {
    let name = command["name"].as_str().unwrap_or_default();
    let subs: Vec<&str> = command["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["type"] == 1 || o["type"] == 2)
        .filter_map(|o| o["name"].as_str())
        .collect();
    if subs.is_empty() {
        name.to_owned()
    } else {
        format!("{name}{{{}}}", subs.join(","))
    }
}

/// Every target as (scope, command labels).
fn summary(report: &PublishReport) -> Vec<(PublishScope, Vec<String>)> {
    report
        .targets()
        .iter()
        .map(|t| {
            let json: Vec<Value> = serde_json::from_str(t.json()).unwrap();
            (t.scope(), json.iter().map(label).collect())
        })
        .collect()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|&s| s.to_owned()).collect()
}

async fn dry(app: &App, scope: PublishScope) -> Vec<(PublishScope, Vec<String>)> {
    summary(&app.publish(scope).dry_run(true).await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn u1_all_enabled() {
    let server = server().await;
    let app = publishing_app(&server, None);
    assert_eq!(
        dry(&app, PublishScope::Global).await,
        [
            (PublishScope::Global, names(&["balance"])),
            (
                PublishScope::Guild(G1),
                names(&["shop", "cfg{a,b}", "admin"])
            ),
            (PublishScope::Guild(G2), names(&["shop"])),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn u2_u5_u8_disabled_in_one_guild() {
    let server = server().await;
    let app = publishing_app(&server, None);
    app.modules().set_enabled("p", G1, false).await.unwrap();
    // U2 (and U8: a dry run).
    assert_eq!(
        dry(&app, PublishScope::Global).await,
        [
            (PublishScope::Global, names(&["balance"])),
            (PublishScope::Guild(G1), names(&["cfg{b}", "admin"])),
            (PublishScope::Guild(G2), names(&["shop"])),
        ]
    );
    // U5: the mirror keeps the global Commands.
    assert_eq!(
        dry(&app, PublishScope::Guild(G1)).await,
        [(
            PublishScope::Guild(G1),
            names(&["balance", "cfg{b}", "admin"])
        )]
    );
    assert!(server.seen().iter().all(|s| s.method != hyper::Method::PUT));
}

#[tokio::test(flavor = "multi_thread")]
async fn u3_disabled_globally_keeps_global_commands() {
    let server = server().await;
    let app = publishing_app(&server, None);
    app.modules()
        .set_enabled("p", ModuleScope::Global, false)
        .await
        .unwrap();
    assert_eq!(
        dry(&app, PublishScope::Global).await,
        [
            (PublishScope::Global, names(&["balance"])),
            (PublishScope::Guild(G1), names(&["cfg{b}", "admin"])),
            (PublishScope::Guild(G2), Vec::new()),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn u4_every_module_disabled_drops_the_base() {
    let server = server().await;
    let app = publishing_app(&server, None);
    app.modules().set_enabled("p", G1, false).await.unwrap();
    app.modules().set_enabled("q", G1, false).await.unwrap();
    let targets = dry(&app, PublishScope::Global).await;
    assert_eq!(targets[1], (PublishScope::Guild(G1), Vec::new()));
}

/// A store whose every call fails.
struct Broken;

impl ModuleStateStore for Broken {
    fn get<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        Box::pin(async { Err(PortError::unavailable("down")) })
    }

    fn put<'a>(
        &'a self,
        _: &'a str,
        _: Bytes,
        _: Option<std::time::Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async { Err(PortError::unavailable("down")) })
    }

    fn version(&self) -> BoxFuture<'_, Result<u64, PortError>> {
        Box::pin(async { Err(PortError::unavailable("down")) })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn u6_u7_a_failing_store_stops_publish_but_not_clear() {
    let server = server().await;
    let app = publishing_app(&server, Some(Arc::new(Broken)));
    let error = app.publish(PublishScope::Global).await.unwrap_err();
    assert_eq!(
        error,
        PublishError::ModuleState {
            reason: "the ModuleStateStore failed: port unavailable: down".into()
        }
    );
    assert_eq!(
        error.to_string(),
        "could not read the Module state from the ModuleStateStore (the ModuleStateStore \
         failed: port unavailable: down): check the store and retry; nothing was sent"
    );
    assert!(server.seen().is_empty(), "no HTTP request");
    // U7: clear reads no Module state.
    let report = app
        .publish(PublishScope::Global)
        .clear(true)
        .dry_run(true)
        .await
        .unwrap();
    assert_eq!(summary(&report), [(PublishScope::Global, Vec::new())]);
}

/// U9: REG-02 counts the mirror after the filter.
#[tokio::test]
async fn u9_reg_02_counts_after_filtering() {
    let paths: Vec<&'static [&'static str]> = (0..99)
        .map(|i| crate::test_support::leak_paths(&format!("g{i}")))
        .collect();
    let mut p = Module::new("p").__add(command("p::balance", &["balance"], "d"));
    let mut q = Module::new("q");
    for (i, path) in paths.into_iter().enumerate() {
        let name = crate::test_support::leak(format!("q::g{i}"));
        let handler = command_with(name, path, CommandMetadata::new().guilds(ONE));
        if i == 0 {
            // One guild Command of `p`: 101 in the mirror with it, 100 without.
            p = p.__add(command_with(
                "p::extra",
                &["extra"],
                CommandMetadata::new().guilds(ONE),
            ));
        }
        q = q.__add(handler);
    }
    let app = app(vec![p, q]);
    let commands = app.registry().commands();
    let all = |_: &'static str, _: Id<GuildMarker>| true;
    assert!(matches!(
        targets(commands, PublishScope::Guild(G1), false, &all),
        Err(PublishError::TooManyCommands { count: 101, .. })
    ));
    let no_p = |module: &'static str, _: Id<GuildMarker>| module != "p";
    let mirror = targets(commands, PublishScope::Guild(G1), false, &no_p).unwrap();
    assert_eq!(mirror[0].local.len(), 100);
}
