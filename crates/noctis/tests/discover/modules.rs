//! Module declarations (Story 18.2), each in the file of its feature.

use noctis::prelude::*;

/// A declared root whose Boot Flag prunes its discovered registrations.
#[noctis::module]
fn beta() -> Module {
    Module::new("beta").flags(["beta"])
}

/// A declared nested Module: its parent `beta` is declared too.
#[noctis::module]
pub(crate) fn beta_tools() -> Module {
    Module::new("beta/tools")
}

#[derive(SlashCommand)]
#[command("betastats", description = "Beta stats", module = "beta/tools")]
pub struct BetaStats;

impl BetaStats {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(super::ui::view(9)).await
    }
}
