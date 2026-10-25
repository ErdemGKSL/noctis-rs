// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Operations: Lifecycle Hooks that log the App's own lifecycle (FR-54). A hook is a
//! unit struct with `#[derive(Hook)]` whose `#[handler]` takes the event it reacts to
//! (a `noctis::lifecycle` type). Hooks run on one ordered queue while the App runs, one
//! at a time, so keep them short and `tokio::spawn` slow work; they run even while
//! this Module is disabled at runtime, and an `Err` or a panic goes to the
//! `ErrorOrigin::Hook` Error Sink without stopping the App.

use noctis::lifecycle::{AllShardsReady, HookCtx, ModuleToggled, ShardFatal};
use noctis::prelude::*;

/// Logs once per run when every shard of this process is ready.
#[derive(Hook, Debug)]
pub struct Online;

impl Online {
    #[handler]
    async fn handle(self, ctx: HookCtx, ev: AllShardsReady) -> HandlerResult<()> {
        tracing::info!(
            module = ctx.module(),
            shards = ev.shards.len(),
            total = ev.total,
            "online"
        );
        Ok(())
    }
}

/// Logs a shard that closed for good (for example 4014: a privileged intent the
/// Developer Portal does not grant).
#[derive(Hook, Debug)]
pub struct ShardDown;

impl ShardDown {
    #[handler]
    async fn handle(self, ctx: HookCtx, ev: ShardFatal) -> HandlerResult<()> {
        tracing::error!(
            module = ctx.module(),
            shard = ev.shard,
            close_code = ?ev.close_code,
            "shard closed for good"
        );
        Ok(())
    }
}

/// Audits every Module toggle made through this App (the dashboard's Modules tab).
#[derive(Hook, Debug)]
pub struct ModuleAudit;

impl ModuleAudit {
    #[handler]
    async fn handle(self, ctx: HookCtx, ev: ModuleToggled) -> HandlerResult<()> {
        tracing::info!(
            module = ctx.module(),
            toggled = ev.module,
            scope = %ev.scope,
            enabled = ev.enabled,
            reset = ev.reset,
            "Module toggled"
        );
        Ok(())
    }
}

/// The `ops` Module: the three Lifecycle Hooks above.
pub fn module() -> Module {
    Module::new("ops")
        .add::<Online>()
        .add::<ShardDown>()
        .add::<ModuleAudit>()
}
