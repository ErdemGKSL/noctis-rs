use noctis::prelude::*;

// Story 6.11 Matrix 4: the path parameter follows the Pattern.
#[derive(SlashCommand)]
#[command("config (set|get)", description = "d")]
struct Config;

impl Config {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(SlashCommand)]
#[command("ping", description = "d")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(SlashCommand)]
#[command("other (a|b)", description = "d")]
struct Other;

impl Other {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
