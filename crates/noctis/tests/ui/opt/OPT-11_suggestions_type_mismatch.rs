use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("warn", description = "d")]
struct Warn {
    #[option(description = "d", autocomplete)]
    rule: i64,
    #[option(description = "d", autocomplete)]
    other: i64,
    #[option(description = "d", autocomplete)]
    small: i64,
}

impl Warn {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }

    #[autocomplete(rule)]
    async fn rule(_ctx: AutocompleteCtx, _typed: &str, _filled: Partial<Self>) -> HandlerResult<Suggestions<String>> {
        Ok(Suggestions::new()) // [OPT-11]: an `i64` option suggests `i64`
    }

    #[autocomplete(other)]
    async fn other(_ctx: AutocompleteCtx, _typed: &str, _filled: Partial<Self>) -> HandlerResult<Responded> {
        unimplemented!()
    }

    #[autocomplete(small)]
    async fn small(_ctx: AutocompleteCtx, _typed: &str, _filled: Partial<Self>) -> HandlerResult<Suggestions<u8>> {
        unimplemented!()
    }
}

fn main() {}
