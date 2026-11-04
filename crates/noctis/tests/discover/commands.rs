//! A Command defined in its own file.

use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("shop", description = "Open the shop", module = "shop")]
pub struct Shop;

impl Shop {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(super::ui::view(1)).await
    }
}

#[derive(SlashCommand)]
#[command(
    "shopadmin reset",
    description = "Reset the shop",
    module = "shop/admin",
    flags = ["admin"]
)]
pub struct Reset;

impl Reset {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(super::ui::view(0)).await
    }
}
