//! Registration boundary passes after merging Modules at App build (REG-01,
//! REG-02, CMD-13, CMD-05): exactly at each limit, split over two Modules.

use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::GuildMarker;
use noctis::ui::{ResolvedMessage, ResolvedUser};
use noctis::{App, BaseMeta, CommandCtx, CommandHandler, CommandMetadata, Module, Runtime};
use noctis::{ContextMenuHandler, HandlerResult};

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A hand-built handler of the one path `path`.
fn handler(path: String, description: &'static str) -> CommandHandler {
    let paths: &'static [&'static str] = Box::leak(vec![leak(path)].into_boxed_slice());
    CommandHandler::__new_plain("reg::h", paths, description, |_: CommandCtx| async {
        Ok(())
    })
}

/// `count` handlers, the first `split` in Module `a`, the rest in `b`.
fn split(count: usize, split: usize, path: impl Fn(usize) -> String) -> Vec<Module> {
    split_with(count, split, path, CommandMetadata::new())
}

fn split_with(
    count: usize,
    split: usize,
    path: impl Fn(usize) -> String,
    metadata: CommandMetadata,
) -> Vec<Module> {
    let (mut a, mut b) = (Module::new("a"), Module::new("b"));
    for i in 0..count {
        let h = handler(path(i), "d").__with_metadata(metadata);
        if i < split {
            a = a.add(h);
        } else {
            b = b.add(h);
        }
    }
    vec![a, b]
}

fn builds(modules: Vec<Module>) {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    for module in modules {
        builder.module(module);
    }
    if let Err(err) = builder.build() {
        panic!("{err}");
    }
}

#[tokio::test]
async fn reg_01_100_global_commands() {
    builds(split(100, 50, |i| format!("c{i}")));
    // A merged base counts once.
    let mut modules = split(100, 50, |i| format!("c{i} x"));
    modules.push(Module::new("c").add(handler("c0 y".to_owned(), "d")));
    builds(modules);
}

async fn user(_: CommandCtx, _: ResolvedUser) -> HandlerResult<()> {
    Ok(())
}

async fn message(_: CommandCtx, _: ResolvedMessage) -> HandlerResult<()> {
    Ok(())
}

#[tokio::test]
async fn reg_01_15_user_15_message() {
    let (mut a, mut b) = (Module::new("a"), Module::new("b"));
    for i in 0..15 {
        let name = leak(format!("Menu {i}"));
        a = a.add(ContextMenuHandler::__new_user(
            "reg::u",
            name,
            CommandMetadata::new(),
            user,
        ));
        b = b.add(ContextMenuHandler::__new_message(
            "reg::m",
            name,
            CommandMetadata::new(),
            message,
        ));
    }
    builds(vec![a, b]);
}

#[tokio::test]
async fn reg_02_guild_counts_separately() {
    const GUILD_1: &[Id<GuildMarker>] = &[Id::new(1)];
    let mut modules = split(100, 50, |i| format!("c{i}"));
    modules.extend(split_with(
        100,
        50,
        |i| format!("c{i}"),
        CommandMetadata::new().guilds(GUILD_1),
    ));
    builds(modules);
}

#[tokio::test]
async fn cmd_13_25_merged_children() {
    builds(split(25, 13, |i| format!("x s{i}")));
    builds(split(25, 12, |i| format!("x g s{i}")));
}

/// Base `x` (71-character description), groups `g0`, `g1` with 25 and `g2`
/// with 24 subcommands, every group and subcommand described with 100
/// characters: `1 + 71 + 2 * (2 + 100 + 25 * 103) + (2 + 100 + 24 * 103)`
/// is exactly 8000.
#[tokio::test]
async fn cmd_05_8000_merged() {
    let d100 = leak("d".repeat(100));
    let (mut a, mut b) = (Module::new("a"), Module::new("b"));
    for (group, subs) in [(0, 25), (1, 25), (2, 24)] {
        let path = leak(format!("x g{group}"));
        let module = if group < 2 { &mut a } else { &mut b };
        for s in 0..subs {
            *module = module
                .clone()
                .add(handler(format!("x g{group} s{s:02}"), d100));
        }
        *module = module.clone().group(path, d100);
    }
    a = a.base("x", BaseMeta::new().description(leak("b".repeat(71))));
    builds(vec![a, b]);
}
