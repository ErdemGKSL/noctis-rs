//! Command boundary passes (CMD-02, CMD-04, CMD-10, CMD-13, CMD-14, CMD-15):
//! `#[command]` literals exactly at a limit compile.

use noctis::prelude::*;
use noctis::rules::command::PathKind;

#[derive(SlashCommand)]
#[command("settings logs channel", description = "d")]
struct Cmd10ThreeWords;

impl Cmd10ThreeWords {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", description = "d")]
struct Cmd02Word32;

impl Cmd02Word32 {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command("ğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğ", description = "d")]
struct Cmd02UnicodeWord32;

impl Cmd02UnicodeWord32 {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command(
    "desc",
    description = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
)]
struct Cmd04Description100;

impl Cmd04Description100 {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command(
    "emoji",
    description = "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"
)]
struct Cmd04DescriptionUtf16100;

impl Cmd04DescriptionUtf16100 {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command(
    "b (a1|a2|a3|a4|a5|a6|a7|a8|a9|a10|a11|a12|a13|a14|a15|a16|a17|a18|a19|a20|a21|a22|a23|a24|a25)",
    description = "d"
)]
struct Cmd13Children25;

impl Cmd13Children25 {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _path: Cmd13Children25Path,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command("(a|b) (c|d)", description = "d")]
struct Cmd14TwoBases;

impl Cmd14TwoBases {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _path: Cmd14TwoBasesPath,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command("mod (ban|kick)", description = "d")]
struct Cmd14Subcommands;

impl Cmd14Subcommands {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _path: Cmd14SubcommandsPath,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(SlashCommand)]
#[command(
    "(a|b) (c1|c2|c3|c4|c5|c6|c7|c8|c9|c10|c11|c12|c13)",
    description = "d",
    max_expansions = 26
)]
struct Cmd15RaisedCap;

impl Cmd15RaisedCap {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _path: Cmd15RaisedCapPath,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[test]
fn cmd_10_three_words_is_a_group_subcommand() {
    let handler = Cmd10ThreeWords::descriptor();
    assert_eq!(handler.paths(), ["settings logs channel"]);
    assert_eq!(
        PathKind::of(handler.paths()[0]),
        Some(PathKind::GroupSubcommand)
    );
}

#[test]
fn cmd_02_words_of_32_compile() {
    assert_eq!(
        Cmd02Word32::descriptor().paths(),
        ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
    );
    assert_eq!(
        Cmd02UnicodeWord32::descriptor().paths(),
        ["ğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğ"]
    );
}

#[test]
fn cmd_04_descriptions_of_100_compile() {
    assert_eq!(Cmd04Description100::descriptor().description().len(), 100);
    assert_eq!(
        Cmd04DescriptionUtf16100::descriptor()
            .description()
            .chars()
            .count(),
        50
    );
}

#[test]
fn cmd_13_children_25_compile() {
    use Cmd13Children25Path as P;
    assert_eq!(Cmd13Children25::descriptor().paths().len(), 25);
    assert_eq!(
        P::ALL,
        [
            P::A1,
            P::A2,
            P::A3,
            P::A4,
            P::A5,
            P::A6,
            P::A7,
            P::A8,
            P::A9,
            P::A10,
            P::A11,
            P::A12,
            P::A13,
            P::A14,
            P::A15,
            P::A16,
            P::A17,
            P::A18,
            P::A19,
            P::A20,
            P::A21,
            P::A22,
            P::A23,
            P::A24,
            P::A25
        ]
    );
}

#[test]
fn cmd_14_bases_and_subcommands_compile() {
    assert_eq!(
        Cmd14TwoBases::descriptor().paths(),
        ["a c", "a d", "b c", "b d"]
    );
    assert_eq!(Cmd14TwoBasesPath::ALL.len(), 4);
    assert_eq!(
        Cmd14Subcommands::descriptor().paths(),
        ["mod ban", "mod kick"]
    );
    assert_eq!(Cmd14SubcommandsPath::Kick.as_str(), "mod kick");
}

#[test]
fn cmd_15_raised_cap_compiles() {
    assert_eq!(Cmd15RaisedCap::descriptor().paths().len(), 26);
    assert_eq!(Cmd15RaisedCapPath::ALL.len(), 26);
    assert_eq!(Cmd15RaisedCapPath::BC13.as_str(), "b c13");
}
