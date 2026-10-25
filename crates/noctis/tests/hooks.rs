//! `#[derive(Hook)]` through the facade (Story 13.5: H1-H3, R24, R26).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};

use noctis::lifecycle::{AppStarted, HookCtx, ModuleToggled, PublishFailed};
use noctis::prelude::*;
use noctis::publish::{PublishError, PublishScope};
use noctis::{BootFlags, Runtime};

/// What the hooks saw.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn push(&self, item: String) {
        self.0.lock().unwrap().push(item);
    }

    fn get(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// H1: a bare derive.
#[derive(Hook)]
pub struct Up;

impl Up {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: AppStarted) -> HandlerResult<()> {
        Ok(())
    }
}

/// H2: an empty attribute.
#[derive(Hook)]
#[hook()]
pub struct Audit;

impl Audit {
    #[handler]
    async fn handle(self, ctx: HookCtx, ev: PublishFailed) -> HandlerResult<()> {
        let seen = ctx.state::<Arc<Seen>>().expect("registered");
        seen.push(format!("audit@{}: {:?}", ctx.module(), ev.error));
        Ok(())
    }
}

/// H3: a flagged hook.
#[derive(Hook)]
#[hook(flags = ["beta"])]
pub struct BetaAudit;

impl BetaAudit {
    #[handler]
    async fn handle(self, ctx: HookCtx, _ev: PublishFailed) -> HandlerResult<()> {
        let seen = ctx.state::<Arc<Seen>>().expect("registered");
        seen.push("beta".to_owned());
        Ok(())
    }
}

/// Records every Module toggle.
#[derive(Hook)]
pub struct Toggles;

impl Toggles {
    #[handler]
    async fn handle(self, ctx: HookCtx, ev: ModuleToggled) -> HandlerResult<()> {
        let seen = ctx.state::<Arc<Seen>>().expect("registered");
        seen.push(format!("{} {} {}", ev.module, ev.scope, ev.enabled));
        Ok(())
    }
}

fn ops() -> Module {
    Module::new("ops")
        .add::<Up>()
        .add::<Audit>()
        .add::<BetaAudit>()
        .add::<Toggles>()
}

fn app(selection: BootFlags, seen: &Arc<Seen>) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .boot_flags(selection)
        .state(Arc::clone(seen))
        .module(ops());
    builder.build().unwrap()
}

#[test]
fn descriptors_and_module_debug() {
    let up = format!("{:?}", Up::descriptor());
    assert!(
        up.contains("hooks::Up") && up.contains("APP_STARTED"),
        "{up}"
    );
    let audit = format!("{:?}", Audit::descriptor());
    assert!(audit.contains("PUBLISH_FAILED"), "{audit}");
    let module = format!("{:?}", ops());
    assert!(
        module.contains(
            "hooks: [\"hooks::Up\", \"hooks::Audit\", \"hooks::BetaAudit\", \"hooks::Toggles\"]"
        ),
        "{module}"
    );
    assert!(
        module.contains("(\"hooks::BetaAudit\", [\"beta\"])"),
        "{module}"
    );
}

/// H3 and R24: outside a run a Publish fires its hooks before it returns;
/// a Boot Flag selection without `beta` drops the flagged one.
#[tokio::test(flavor = "multi_thread")]
async fn boot_flags_select_hooks() {
    for (selection, expected) in [
        (BootFlags::all(), vec!["audit@ops: MissingToken", "beta"]),
        (BootFlags::none(), vec!["audit@ops: MissingToken"]),
    ] {
        let seen = Arc::new(Seen::default());
        let app = app(selection, &seen);
        let result = app.publish(PublishScope::Global).await;
        assert_eq!(result, Err(PublishError::MissingToken));
        assert_eq!(seen.get(), expected);
    }
}

/// R26: a hook runs while its own Module is disabled.
#[tokio::test(flavor = "multi_thread")]
async fn disabled_module_still_hooks() {
    let seen = Arc::new(Seen::default());
    let app = app(BootFlags::all(), &seen);
    let modules = app.modules();
    modules
        .set_enabled("ops", ModuleScope::Global, false)
        .await
        .unwrap();
    assert!(!modules.is_enabled("ops", ModuleScope::Global));
    let _ = app.publish(PublishScope::Global).await;
    assert_eq!(
        seen.get(),
        ["ops global false", "audit@ops: MissingToken", "beta"]
    );
}
