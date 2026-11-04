use noctis::prelude::*;

// A Module id outside the grammar.
#[derive(SlashCommand)]
#[command("stats", description = "d", module = "Stats")]
struct Stats;

impl Stats {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

// An empty segment.
#[derive(EventHandler)]
#[event(module = "xp//award")]
struct Award;

// Not a string literal.
#[derive(Button)]
#[button("shop.buy", module = shop, defaults(label = "Buy"))]
struct Buy;

// Given twice.
#[derive(Hook)]
#[hook(module = "ops", module = "ops")]
struct Up;

fn main() {}
