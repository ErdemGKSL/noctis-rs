//! Base-command metadata: [`CommandMeta`], [`validate_meta`] (META-03) and the
//! per-base conflict checks [`validate_tree`](super::validate_tree) runs
//! (META-01, META-02, CMD-07).

use super::{CommandDecl, DeclKind, Scope, from_suffix, source_name};
use crate::{Actual, Bound, Limits, RuleId, Violation};

/// Where a command can be used (`contexts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InteractionContext {
    /// `GUILD` (0): inside guilds.
    Guild,
    /// `BOT_DM` (1): in the bot's DM.
    BotDm,
    /// `PRIVATE_CHANNEL` (2): in group DMs and DMs other than the bot's.
    PrivateChannel,
}

impl InteractionContext {
    /// The Discord value (0..=2).
    pub const fn value(self) -> u8 {
        match self {
            Self::Guild => 0,
            Self::BotDm => 1,
            Self::PrivateChannel => 2,
        }
    }

    /// The variant name, for example `"BotDm"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Guild => "Guild",
            Self::BotDm => "BotDm",
            Self::PrivateChannel => "PrivateChannel",
        }
    }
}

/// How a command can be installed (`integration_types`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntegrationType {
    /// `GUILD_INSTALL` (0).
    GuildInstall,
    /// `USER_INSTALL` (1).
    UserInstall,
}

impl IntegrationType {
    /// The Discord value (0 or 1).
    pub const fn value(self) -> u8 {
        match self {
            Self::GuildInstall => 0,
            Self::UserInstall => 1,
        }
    }

    /// The variant name, for example `"UserInstall"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::GuildInstall => "GuildInstall",
            Self::UserInstall => "UserInstall",
        }
    }
}

/// The metadata Discord keeps only on the base command (META-01, META-02,
/// CMD-07). `None` leaves a field unset; lists compare as sets.
///
/// The deprecated `dm_permission` does not exist (META-05); use `contexts`:
///
/// ```compile_fail
/// use noctis_rules::command::CommandMeta;
///
/// let _ = CommandMeta::new().dm_permission(true); // [META-05]
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandMeta<'a> {
    /// The permission bit set members need by default (META-01; `0` = admins
    /// only).
    pub default_member_permissions: Option<u64>,
    /// Where the command can be used (global commands only, META-03).
    pub contexts: Option<&'a [InteractionContext]>,
    /// How the command can be installed (global commands only, META-03).
    pub integration_types: Option<&'a [IntegrationType]>,
    /// Whether the command is age-restricted (CMD-07).
    pub nsfw: Option<bool>,
}

impl<'a> CommandMeta<'a> {
    /// Metadata with every field unset.
    pub const fn new() -> Self {
        Self {
            default_member_permissions: None,
            contexts: None,
            integration_types: None,
            nsfw: None,
        }
    }

    /// Sets `default_member_permissions`.
    #[must_use]
    pub const fn default_member_permissions(self, permissions: u64) -> Self {
        Self {
            default_member_permissions: Some(permissions),
            ..self
        }
    }

    /// Sets `contexts`.
    #[must_use]
    pub const fn contexts(self, contexts: &'a [InteractionContext]) -> Self {
        Self {
            contexts: Some(contexts),
            ..self
        }
    }

    /// Sets `integration_types`.
    #[must_use]
    pub const fn integration_types(self, types: &'a [IntegrationType]) -> Self {
        Self {
            integration_types: Some(types),
            ..self
        }
    }

    /// Sets `nsfw`.
    #[must_use]
    pub const fn nsfw(self, nsfw: bool) -> Self {
        Self {
            nsfw: Some(nsfw),
            ..self
        }
    }

    fn is_unset(&self) -> bool {
        *self == Self::new()
    }
}

/// `[A, B]`, sorted by Discord value and deduplicated, so equal sets render
/// equally.
fn render_set<T: Copy + Ord>(items: &[T], name: fn(T) -> &'static str) -> String {
    let mut sorted = items.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let names: Vec<&str> = sorted.into_iter().map(name).collect();
    format!("[{}]", names.join(", "))
}

fn render_contexts(contexts: &[InteractionContext]) -> String {
    render_set(contexts, InteractionContext::name)
}

fn render_integration_types(types: &[IntegrationType]) -> String {
    render_set(types, IntegrationType::name)
}

/// Checks one declaration's metadata against its scope (META-03).
///
/// A guild-scoped declaration may not set `contexts` or `integration_types`.
/// The violations have the paths `contexts` and `integration_types` (in that
/// order), the set (sorted by Discord value, deduplicated) as the actual value
/// and a `scope: guild <id>` note.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::{CommandMeta, InteractionContext, Scope, validate_meta};
///
/// let meta = CommandMeta::new().contexts(&[InteractionContext::Guild]);
/// assert!(validate_meta(&meta, Scope::Global, &Limits::default()).is_empty());
/// let errs = validate_meta(&meta, Scope::Guild(42), &Limits::default());
/// assert_eq!(errs[0].rule, RuleId::META_03);
/// ```
pub fn validate_meta(meta: &CommandMeta<'_>, scope: Scope, limits: &Limits) -> Vec<Violation> {
    let _ = limits;
    let Scope::Guild(id) = scope else {
        return Vec::new();
    };
    let fields = [
        ("contexts", meta.contexts.map(render_contexts)),
        (
            "integration_types",
            meta.integration_types.map(render_integration_types),
        ),
    ];
    fields
        .into_iter()
        .filter_map(|(field, value)| {
            let value = value?;
            Some(
                Violation::new(
                    RuleId::META_03,
                    field,
                    Bound::Describe("only on global commands"),
                    Actual::Text(value),
                )
                .with_note(format!("scope: guild {id}")),
            )
        })
        .collect()
}

/// One base-command field: its rule, its description and its rendered value.
type Field = (RuleId, &'static str, fn(&CommandMeta<'_>) -> Option<String>);

const FIELDS: [Field; 4] = [
    (
        RuleId::META_01,
        "one default_member_permissions per base command",
        |m| m.default_member_permissions.map(|p| p.to_string()),
    ),
    (
        RuleId::META_02,
        "one contexts value per base command",
        |m| m.contexts.map(render_contexts),
    ),
    (
        RuleId::META_02,
        "one integration_types value per base command",
        |m| m.integration_types.map(render_integration_types),
    ),
    (RuleId::CMD_07, "one nsfw value per base command", |m| {
        m.nsfw.map(|n| n.to_string())
    }),
];

/// The metadata checks of one base command.
///
/// `decls` are the declarations attached to the base's subtree, in input
/// order. Meta declarations below the base that set metadata are META-02
/// (misplaced) and take no further part. Per field: with an explicit base
/// declaration (a meta declaration at `base` that sets the field) every
/// declaration that sets it must agree and unset handlers inherit it; without
/// one every handler must agree, unset included.
pub(crate) fn check_base(base: &str, decls: &[&CommandDecl<'_>], out: &mut Vec<Violation>) {
    let is_meta = |d: &CommandDecl<'_>| matches!(d.kind, DeclKind::Meta);
    let mut participants = Vec::new();
    for &decl in decls {
        if is_meta(decl) && decl.path != base && !decl.metadata.is_unset() {
            out.push(
                Violation::new(
                    RuleId::META_02,
                    decl.path,
                    Bound::Describe("metadata only on the base command"),
                    Actual::Text(decl.path.to_owned()),
                )
                .with_note(format!("declared by {}", source_name(decl.source)))
                .with_help(format!("declare it on `{base}`")),
            );
        } else {
            participants.push(decl);
        }
    }

    for (rule, describe, value) in FIELDS {
        let explicit = participants
            .iter()
            .any(|d| is_meta(d) && d.path == base && value(&d.metadata).is_some());
        let entries: Vec<(&CommandDecl<'_>, Option<String>)> = participants
            .iter()
            .map(|d| (*d, value(&d.metadata)))
            .filter(|(d, v)| if explicit { v.is_some() } else { !is_meta(d) })
            .collect();
        if entries.iter().all(|(_, v)| *v == entries[0].1) {
            continue;
        }
        let mut v = Violation::new(
            rule,
            base,
            Bound::Describe(describe),
            Actual::Text(base.to_owned()),
        );
        for (decl, value) in &entries {
            let what = match value {
                Some(value) => format!("sets `{value}`"),
                None => "leaves it unset".to_owned(),
            };
            v = v.with_note(format!(
                "`{}` {what}{}",
                decl.path,
                from_suffix(decl.source)
            ));
        }
        if entries.iter().any(|(_, v)| v.is_none()) {
            v = v.with_help(format!(
                "set it on every command under `{base}`, or once on `{base}` with a metadata declaration"
            ));
        }
        out.push(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{LocalizedText, validate_tree};

    use IntegrationType::{GuildInstall, UserInstall};
    use InteractionContext::{BotDm, Guild, PrivateChannel};

    const D: LocalizedText<'static> = LocalizedText::new("d");

    fn h<'a>(source: &'a str, path: &'a str) -> CommandDecl<'a> {
        CommandDecl::handler(source, path).description(D)
    }

    fn m<'a>(source: &'a str, path: &'a str) -> CommandDecl<'a> {
        CommandDecl::meta(source, path)
    }

    fn perm(p: u64) -> CommandMeta<'static> {
        CommandMeta::new().default_member_permissions(p)
    }

    fn check(decls: &[CommandDecl<'_>]) -> Vec<Violation> {
        match validate_tree(decls, &Limits::default()) {
            Ok(()) => Vec::new(),
            Err(errs) => {
                assert!(!errs.is_empty());
                errs
            }
        }
    }

    fn rules(errs: &[Violation]) -> Vec<RuleId> {
        errs.iter().map(|v| v.rule).collect()
    }

    fn meta(meta: &CommandMeta<'_>, scope: Scope) -> Vec<Violation> {
        validate_meta(meta, scope, &Limits::default())
    }

    fn meta_03(path: &str, actual: &str) -> Violation {
        Violation::new(
            RuleId::META_03,
            path,
            Bound::Describe("only on global commands"),
            Actual::Text(actual.into()),
        )
        .with_note("scope: guild 42")
    }

    #[test]
    fn meta_03_global_accepts_every_field() {
        let all = CommandMeta::new()
            .default_member_permissions(8)
            .contexts(&[Guild, BotDm, PrivateChannel])
            .integration_types(&[GuildInstall, UserInstall])
            .nsfw(true);
        assert_eq!(meta(&all, Scope::Global), []);
    }

    #[test]
    fn meta_03_guild_rejects_contexts_and_integration_types() {
        let g = Scope::Guild(42);
        assert_eq!(
            meta(&CommandMeta::new().contexts(&[Guild]), g),
            [meta_03("contexts", "[Guild]")]
        );
        assert_eq!(
            meta(&CommandMeta::new().integration_types(&[UserInstall]), g),
            [meta_03("integration_types", "[UserInstall]")]
        );
        assert_eq!(
            meta(
                &CommandMeta::new()
                    .contexts(&[Guild])
                    .integration_types(&[UserInstall]),
                g
            ),
            [
                meta_03("contexts", "[Guild]"),
                meta_03("integration_types", "[UserInstall]")
            ]
        );
        assert_eq!(
            meta(&CommandMeta::new().contexts(&[]), g),
            [meta_03("contexts", "[]")]
        );
        assert_eq!(
            meta(
                &CommandMeta::new().contexts(&[PrivateChannel, Guild, Guild]),
                g
            ),
            [meta_03("contexts", "[Guild, PrivateChannel]")]
        );
        assert_eq!(meta(&perm(8).nsfw(true), g), []);
    }

    #[test]
    fn meta_03_values_and_names() {
        assert_eq!(
            [Guild, BotDm, PrivateChannel].map(InteractionContext::value),
            [0, 1, 2]
        );
        assert_eq!(
            [GuildInstall, UserInstall].map(IntegrationType::value),
            [0, 1]
        );
        assert_eq!(BotDm.name(), "BotDm");
        assert_eq!(UserInstall.name(), "UserInstall");
        assert_eq!(CommandMeta::new(), CommandMeta::default());
    }

    fn meta_01(notes: &[&str]) -> Violation {
        let mut v = Violation::new(
            RuleId::META_01,
            "config",
            Bound::Describe("one default_member_permissions per base command"),
            Actual::Text("config".into()),
        );
        for note in notes {
            v = v.with_note(*note);
        }
        v
    }

    const HELP: &str =
        "set it on every command under `config`, or once on `config` with a metadata declaration";

    #[test]
    fn meta_01_agreeing_permissions() {
        assert_eq!(
            check(&[
                h("m1", "config set").metadata(perm(8)),
                h("m2", "config get").metadata(perm(8)),
            ]),
            []
        );
    }

    #[test]
    fn meta_01_differing_permissions() {
        assert_eq!(
            check(&[
                h("m1", "config set").metadata(perm(8)),
                h("m2", "config get").metadata(perm(32)),
            ]),
            [meta_01(&[
                "`config set` sets `8` (from `m1`)",
                "`config get` sets `32` (from `m2`)"
            ])]
        );
    }

    #[test]
    fn meta_01_set_versus_unset() {
        assert_eq!(
            check(&[
                h("m1", "config set").metadata(perm(8)),
                h("m2", "config get"),
            ]),
            [meta_01(&[
                "`config set` sets `8` (from `m1`)",
                "`config get` leaves it unset (from `m2`)"
            ])
            .with_help(HELP)]
        );
    }

    #[test]
    fn meta_01_explicit_base_declaration() {
        assert_eq!(
            check(&[
                m("m0", "config").metadata(perm(8)),
                h("m1", "config set"),
                h("m2", "config get").metadata(perm(8)),
            ]),
            []
        );
        assert_eq!(
            check(&[
                m("m0", "config").metadata(perm(8)),
                h("m1", "config set"),
                h("m2", "config get").metadata(perm(32)),
            ]),
            [meta_01(&[
                "`config` sets `8` (from `m0`)",
                "`config get` sets `32` (from `m2`)"
            ])]
        );
        assert_eq!(
            rules(&check(&[
                m("m0", "config").metadata(perm(8)),
                m("m9", "config").metadata(perm(32)),
                h("m1", "config set"),
            ])),
            [RuleId::META_01]
        );
    }

    #[test]
    fn meta_02_contexts_compare_as_sets() {
        let c = |contexts| CommandMeta::new().contexts(contexts);
        assert_eq!(
            check(&[
                h("m1", "config set").metadata(c(&[Guild, BotDm])),
                h("m2", "config get").metadata(c(&[BotDm, Guild, Guild])),
            ]),
            []
        );
        let errs = check(&[
            h("m1", "config set").metadata(c(&[Guild])),
            h("m2", "config get").metadata(c(&[Guild, BotDm])),
        ]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::META_02,
                "config",
                Bound::Describe("one contexts value per base command"),
                Actual::Text("config".into())
            )
            .with_note("`config set` sets `[Guild]` (from `m1`)")
            .with_note("`config get` sets `[Guild, BotDm]` (from `m2`)")]
        );
        let errs = check(&[
            h("m1", "config set").metadata(c(&[])),
            h("m2", "config get"),
        ]);
        assert_eq!(rules(&errs), [RuleId::META_02]);
        assert_eq!(
            errs[0].notes,
            [
                "`config set` sets `[]` (from `m1`)",
                "`config get` leaves it unset (from `m2`)"
            ]
        );
    }

    #[test]
    fn meta_02_integration_types() {
        let i = |types| CommandMeta::new().integration_types(types);
        let errs = check(&[
            h("m1", "config set").metadata(i(&[GuildInstall])),
            h("m2", "config get").metadata(i(&[UserInstall])),
        ]);
        assert_eq!(rules(&errs), [RuleId::META_02]);
        assert_eq!(
            errs[0].limit,
            Bound::Describe("one integration_types value per base command")
        );
    }

    #[test]
    fn cmd_07_nsfw_conflicts() {
        let n = |nsfw| CommandMeta::new().nsfw(nsfw);
        for other in [n(false), CommandMeta::new()] {
            let first = if other.nsfw.is_some() {
                n(true)
            } else {
                n(false)
            };
            let errs = check(&[
                h("m1", "config set").metadata(first),
                h("m2", "config get").metadata(other),
            ]);
            assert_eq!(rules(&errs), [RuleId::CMD_07]);
            assert_eq!(
                errs[0].limit,
                Bound::Describe("one nsfw value per base command")
            );
        }
    }

    #[test]
    fn meta_02_misplaced_metadata() {
        let errs = check(&[
            m("m1", "config set").metadata(CommandMeta::new().nsfw(true)),
            h("m2", "config set"),
        ]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::META_02,
                "config set",
                Bound::Describe("metadata only on the base command"),
                Actual::Text("config set".into())
            )
            .with_note("declared by `m1`")
            .with_help("declare it on `config`")]
        );
        assert_eq!(
            check(&[
                m("m1", "config set")
                    .metadata(CommandMeta::new())
                    .description(D),
                h("m2", "config set"),
            ]),
            []
        );
    }

    #[test]
    fn meta_01_bases_are_independent() {
        assert_eq!(
            check(&[
                h("m1", "config").metadata(perm(8)),
                h("m2", "ping").metadata(perm(32)),
            ]),
            []
        );
        let all = perm(8)
            .contexts(&[Guild])
            .integration_types(&[GuildInstall])
            .nsfw(true);
        assert_eq!(check(&[h("m", "ping").metadata(all)]), []);
    }

    #[test]
    fn meta_01_meta_02_cmd_07_every_field() {
        let a = perm(8)
            .contexts(&[Guild])
            .integration_types(&[GuildInstall])
            .nsfw(true);
        let b = perm(32)
            .contexts(&[BotDm])
            .integration_types(&[UserInstall])
            .nsfw(false);
        let errs = check(&[
            h("m1", "config set").metadata(a),
            h("m2", "config get").metadata(b),
        ]);
        assert_eq!(
            rules(&errs),
            [
                RuleId::META_01,
                RuleId::META_02,
                RuleId::META_02,
                RuleId::CMD_07
            ]
        );
        assert_eq!(
            errs[2].limit,
            Bound::Describe("one integration_types value per base command")
        );
    }

    #[test]
    fn meta_01_ignored_declarations() {
        // A CMD-10-dropped decl, an orphan meta decl and an empty path never
        // take part in the metadata checks.
        let errs = check(&[
            h("m1", "config set").metadata(perm(8)),
            h("m2", "config a b c").metadata(perm(32)),
            m("m3", "zzz").metadata(perm(32)),
            h("m4", "").metadata(perm(32)),
        ]);
        assert_eq!(
            rules(&errs),
            [RuleId::CMD_10, RuleId::CMD_10, RuleId::REG_04]
        );
    }

    #[test]
    fn meta_02_order_per_base() {
        let errs = check(&[
            m("d1", "a").description(LocalizedText::new("x")),
            m("d2", "a").description(LocalizedText::new("y")),
            h("m1", "a b").metadata(perm(1).nsfw(true)),
            h("m2", "a c").metadata(perm(2)),
            m("m3", "a b").metadata(CommandMeta::new().nsfw(true)),
            h("m1", "z b").metadata(CommandMeta::new().contexts(&[Guild])),
            h("m2", "z c"),
        ]);
        let got: Vec<(RuleId, &str)> = errs.iter().map(|v| (v.rule, v.path.as_str())).collect();
        assert_eq!(
            got,
            [
                (RuleId::REG_04, "a"),
                (RuleId::META_02, "a b"),
                (RuleId::META_01, "a"),
                (RuleId::CMD_07, "a"),
                (RuleId::META_02, "z"),
            ]
        );
    }
}
