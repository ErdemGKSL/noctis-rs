//! Command boundary passes (CMD-02, CMD-04, CMD-10, CMD-13, CMD-14, CMD-15):
//! `#[command]` literals exactly at a limit compile.

use noctis::prelude::*;
use noctis::rules::command::PathKind;

#[command("settings logs channel", description = "d")]
async fn cmd_10_three_words(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", description = "d")]
async fn cmd_02_word_32(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("ğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğ", description = "d")]
async fn cmd_02_unicode_word_32(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command(
    "desc",
    description = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
)]
async fn cmd_04_description_100(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command(
    "emoji",
    description = "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"
)]
async fn cmd_04_description_utf16_100(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command(
    "b (a1|a2|a3|a4|a5|a6|a7|a8|a9|a10|a11|a12|a13|a14|a15|a16|a17|a18|a19|a20|a21|a22|a23|a24|a25)",
    description = "d"
)]
async fn cmd_13_children_25(_ctx: CommandCtx, _path: Cmd13Children25Path) -> HandlerResult<()> {
    Ok(())
}

#[command("(a|b) (c|d)", description = "d")]
async fn cmd_14_two_bases(_ctx: CommandCtx, _path: Cmd14TwoBasesPath) -> HandlerResult<()> {
    Ok(())
}

#[command("mod (ban|kick)", description = "d")]
async fn cmd_14_subcommands(_ctx: CommandCtx, _path: Cmd14SubcommandsPath) -> HandlerResult<()> {
    Ok(())
}

#[command(
    "(a|b) (c1|c2|c3|c4|c5|c6|c7|c8|c9|c10|c11|c12|c13)",
    description = "d",
    max_expansions = 26
)]
async fn cmd_15_raised_cap(_ctx: CommandCtx, _path: Cmd15RaisedCapPath) -> HandlerResult<()> {
    Ok(())
}

#[test]
fn cmd_10_three_words_is_a_group_subcommand() {
    let handler = cmd_10_three_words();
    assert_eq!(handler.paths(), ["settings logs channel"]);
    assert_eq!(
        PathKind::of(handler.paths()[0]),
        Some(PathKind::GroupSubcommand)
    );
}

#[test]
fn cmd_02_words_of_32_compile() {
    assert_eq!(
        cmd_02_word_32().paths(),
        ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
    );
    assert_eq!(
        cmd_02_unicode_word_32().paths(),
        ["ğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğğ"]
    );
}

#[test]
fn cmd_04_descriptions_of_100_compile() {
    assert_eq!(cmd_04_description_100().description().len(), 100);
    assert_eq!(
        cmd_04_description_utf16_100().description().chars().count(),
        50
    );
}

#[test]
fn cmd_13_children_25_compile() {
    use Cmd13Children25Path as P;
    assert_eq!(cmd_13_children_25().paths().len(), 25);
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
    assert_eq!(cmd_14_two_bases().paths(), ["a c", "a d", "b c", "b d"]);
    assert_eq!(Cmd14TwoBasesPath::ALL.len(), 4);
    assert_eq!(cmd_14_subcommands().paths(), ["mod ban", "mod kick"]);
    assert_eq!(Cmd14SubcommandsPath::Kick.as_str(), "mod kick");
}

#[test]
fn cmd_15_raised_cap_compiles() {
    assert_eq!(cmd_15_raised_cap().paths().len(), 26);
    assert_eq!(Cmd15RaisedCapPath::ALL.len(), 26);
    assert_eq!(Cmd15RaisedCapPath::BC13.as_str(), "b c13");
}
