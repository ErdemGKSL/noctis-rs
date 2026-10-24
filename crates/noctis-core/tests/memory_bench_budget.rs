//! Story 12.6: the `memory_per_guild` bench's budget logic and arguments
//! (U1-U17) and its allocator check without dhat's allocator (S0), through
//! the bench modules included by path.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

#[path = "../benches/memory_per_guild/budget.rs"]
mod budget;
#[path = "../benches/memory_per_guild/measure.rs"]
#[expect(dead_code, reason = "only the allocator check runs in this binary")]
mod measure;

use std::path::{Path, PathBuf};

use budget::{BudgetError, Measurement, MemoryBudgets, Status, Value};
use noctis::ports::memory::MemoryRenderCache;

fn real_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/budgets.toml")
}

fn real_text() -> String {
    std::fs::read_to_string(real_path()).unwrap()
}

fn real() -> MemoryBudgets {
    budget::load(&real_path()).unwrap()
}

fn parse(text: &str) -> Result<MemoryBudgets, BudgetError> {
    budget::parse(text, Path::new("budgets.toml"))
}

fn parse_error(text: &str) -> String {
    match parse(text) {
        Err(error @ BudgetError::Parse { .. }) => error.to_string(),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

/// A minimal valid file with `head` as the `[memory_per_guild]` body.
fn file_with(head: &str, framework: &str) -> String {
    format!(
        "[memory_per_guild]\n{head}\n\
         [memory_per_guild.framework_bytes_per_guild]\n{framework}\n\
         [memory_per_guild.discord_cache_bytes_per_guild]\nnote = \"d\"\n\
         [memory_per_guild.render_cache_bytes]\nmax = 10\nnote = \"r\"\n"
    )
}

const HEAD: &str = "nfr = \"NFR-1\"\nguilds = 10\ngate = false";

fn measurement(baseline: i64, none: i64, default: i64, render: u64) -> Measurement {
    Measurement {
        guilds: 10_000,
        baseline,
        none,
        default,
        render_cache_bytes: render,
    }
}

fn with_gate(gate: bool) -> MemoryBudgets {
    MemoryBudgets { gate, ..real() }
}

fn row(report: &budget::Report, metric: &str) -> budget::Row {
    report
        .rows
        .iter()
        .find(|row| row.metric == metric)
        .cloned()
        .unwrap()
}

const FRAMEWORK: &str = "Framework bytes per guild";
const DISCORD: &str = "Discord cache bytes per guild";
const DISCORD_TOTAL: &str = "Discord cache bytes (total)";
const RENDER: &str = "Renderer cache bytes";

#[test]
fn u1_real_file_parses() {
    let budgets = real();
    assert_eq!(budgets.nfr, "NFR-1");
    assert_eq!(budgets.guilds(), 10_000);
    assert!(!budgets.gate);
    assert_eq!(budgets.framework_bytes_per_guild.max, Some(1024));
    assert_eq!(budgets.discord_cache_bytes_per_guild.max, None);
    assert_eq!(budgets.render_cache_bytes.max, Some(67_108_864));
    assert_eq!(
        budgets.render_cache_bytes.max,
        Some(MemoryRenderCache::DEFAULT_LIMITS.max_bytes())
    );
}

#[test]
fn u2_other_top_level_tables_are_ignored() {
    let text = format!("{}\n[reference_card]\nmax_ms = 5\n", real_text());
    assert!(parse(&text).is_ok());
}

#[test]
fn u3_unknown_keys_are_errors() {
    let in_head = parse_error(&file_with(&format!("{HEAD}\nmaxx = 1"), "note = \"f\""));
    assert!(in_head.contains("maxx"), "{in_head}");
    let in_metric = parse_error(&file_with(HEAD, "maxx = 1\nnote = \"f\""));
    assert!(in_metric.contains("maxx"), "{in_metric}");
}

#[test]
fn u4_schema_violations_are_parse_errors() {
    parse_error(&file_with("nfr = \"NFR-1\"\nguilds = 10", "note = \"f\""));
    parse_error(&file_with(HEAD, "max = -1\nnote = \"f\""));
    parse_error(&file_with(HEAD, "max = 1.5\nnote = \"f\""));
    parse_error(&file_with(
        "nfr = \"NFR-1\"\nguilds = \"x\"\ngate = false",
        "note = \"f\"",
    ));
    parse_error(&file_with(HEAD, "max = 1"));
    let missing_table = format!(
        "[memory_per_guild]\n{HEAD}\n\
         [memory_per_guild.framework_bytes_per_guild]\nnote = \"f\"\n\
         [memory_per_guild.render_cache_bytes]\nnote = \"r\"\n"
    );
    parse_error(&missing_table);
}

#[test]
fn u5_missing_file_is_a_read_error() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/no-such-budgets.toml");
    let error = budget::load(&path).unwrap_err();
    assert!(matches!(error, BudgetError::Read { .. }), "{error:?}");
    assert!(
        error.to_string().contains("no-such-budgets.toml"),
        "{error}"
    );
}

#[test]
fn u6_zero_guilds() {
    let error = parse(&file_with(
        "nfr = \"NFR-1\"\nguilds = 0\ngate = false",
        "note = \"f\"",
    ));
    assert!(matches!(error, Err(BudgetError::ZeroGuilds)), "{error:?}");
    let zero = Measurement {
        guilds: 0,
        ..measurement(0, 0, 0, 0)
    };
    assert!(matches!(
        budget::evaluate(&real(), &zero),
        Err(BudgetError::ZeroGuilds)
    ));
}

#[test]
fn u7_guild_limit() {
    let at = parse(&file_with(
        "nfr = \"NFR-1\"\nguilds = 100000\ngate = false",
        "note = \"f\"",
    ));
    assert_eq!(at.unwrap().guilds(), 100_000);
    let over = parse(&file_with(
        "nfr = \"NFR-1\"\nguilds = 100001\ngate = false",
        "note = \"f\"",
    ));
    assert!(
        matches!(over, Err(BudgetError::TooManyGuilds { guilds: 100_001 })),
        "{over:?}"
    );
    let too_many = Measurement {
        guilds: 100_001,
        ..measurement(0, 0, 0, 0)
    };
    assert!(matches!(
        budget::evaluate(&real(), &too_many),
        Err(BudgetError::TooManyGuilds { guilds: 100_001 })
    ));
}

#[test]
fn u8_metrics_are_derived_from_the_runs() {
    let report =
        budget::evaluate(&real(), &measurement(1_000_000, 1_500_000, 24_616_000, 0)).unwrap();
    let framework = row(&report, FRAMEWORK);
    assert_eq!(framework.value, Value::PerGuild(50.0));
    assert_eq!(framework.status, Status::Ok);
    let discord = row(&report, DISCORD);
    assert_eq!(discord.value, Value::PerGuild(2311.6));
    assert_eq!(discord.status, Status::Reported);
    let total = row(&report, DISCORD_TOTAL);
    assert_eq!(total.value, Value::Total(23_116_000));
    assert_eq!(total.status, Status::Reported);
    let render = row(&report, RENDER);
    assert_eq!(render.value, Value::Total(0));
    assert_eq!(render.status, Status::Ok);
    assert!(!report.failed());
}

#[test]
fn u9_exactly_at_budget_is_ok() {
    let report = budget::evaluate(&real(), &measurement(0, 10_240_000, 10_240_000, 0)).unwrap();
    assert_eq!(row(&report, FRAMEWORK).status, Status::Ok);
}

#[test]
fn u10_over_budget_fails_only_with_the_gate() {
    let over = measurement(0, 10_241_000, 10_241_000, 0);
    let report = budget::evaluate(&with_gate(false), &over).unwrap();
    assert_eq!(row(&report, FRAMEWORK).status, Status::Over);
    assert!(report.markdown().contains("| 1024.1 | <= 1024 | over |"));
    assert!(!report.failed());
    let gated = budget::evaluate(&with_gate(true), &over).unwrap();
    assert!(gated.failed());
    assert!(
        gated
            .markdown()
            .contains("gate: on (over-budget rows fail the run)")
    );
}

#[test]
fn u11_negative_framework_is_shown_and_noted() {
    let report = budget::evaluate(&real(), &measurement(32_000, 0, 0, 0)).unwrap();
    let framework = row(&report, FRAMEWORK);
    assert_eq!(framework.value.to_string(), "-3.2");
    assert_eq!(framework.status, Status::Ok);
    assert!(
        framework
            .note
            .unwrap()
            .ends_with("(below measurement noise)")
    );
}

#[test]
fn u12_negative_discord_cache_is_reported_and_noted() {
    let report = budget::evaluate(&real(), &measurement(0, 50_000, 0, 0)).unwrap();
    let discord = row(&report, DISCORD);
    assert_eq!(discord.value.to_string(), "-5.0");
    assert_eq!(discord.status, Status::Reported);
    assert!(discord.note.unwrap().ends_with("(below measurement noise)"));
    let total = row(&report, DISCORD_TOTAL);
    assert_eq!(total.value, Value::Total(-50_000));
    assert_eq!(total.status, Status::Reported);
    assert_eq!(total.note, None);
}

#[test]
fn u13_render_cache_over_budget() {
    let report = budget::evaluate(&with_gate(true), &measurement(0, 0, 0, 67_108_865)).unwrap();
    assert_eq!(row(&report, RENDER).status, Status::Over);
    assert!(report.failed());
}

#[test]
fn u14_non_ascii_notes_are_verbatim() {
    let text = file_with(HEAD, "max = 1024\nnote = \"bütçe ≤ 1 KiB\"");
    let budgets = parse(&text).unwrap();
    let report = budget::evaluate(&budgets, &measurement(0, 0, 0, 0)).unwrap();
    assert!(
        report
            .markdown()
            .contains("- Framework bytes per guild: bütçe ≤ 1 KiB\n")
    );
}

#[test]
fn u15_markdown_golden() {
    let report =
        budget::evaluate(&real(), &measurement(1_000_000, 1_500_000, 24_616_000, 0)).unwrap();
    let expected = "\
## Memory per idle guild (NFR-1)

10000 idle guilds through FakeGateway; Discord cache: GUILD | CHANNEL | ROLE (no members, no messages); gate: off (reporting until Story 14.3)

| Metric | Measured | Budget | Status |
|---|---|---|---|
| Framework bytes per guild | 50.0 | <= 1024 | ok |
| Discord cache bytes per guild | 2311.6 | - | reported |
| Discord cache bytes (total) | 23116000 | - | reported |
| Renderer cache bytes | 0 | <= 67108864 | ok |

Notes:
- Framework bytes per guild: NFR-1 initial goal: at most 1 KiB of framework memory per idle guild, excluding the Discord cache
- Discord cache bytes per guild: reported only: NFR-1 sets no Discord cache target; default configuration GUILD | CHANNEL | ROLE (no members, no messages)
- Renderer cache bytes: MemoryRenderCache::DEFAULT_LIMITS byte budget (64 MiB); idle guilds render nothing
";
    assert_eq!(report.markdown(), expected);
}

#[test]
fn u16_extreme_deltas_do_not_overflow() {
    for (baseline, none, default) in [
        (i64::MIN, i64::MAX, i64::MIN),
        (i64::MAX, i64::MIN, i64::MAX),
    ] {
        let report = budget::evaluate(
            &with_gate(true),
            &measurement(baseline, none, default, u64::MAX),
        )
        .unwrap();
        let _ = report.markdown();
    }
}

#[test]
fn u17_args() {
    let parse = |args: &[&str]| budget::parse_args(args.iter().map(|&arg| arg.to_owned()));
    assert_eq!(parse(&["--bench"]), Ok(None));
    assert_eq!(parse(&["--bench", "--guilds", "5"]), Ok(Some(5)));
    assert_eq!(parse(&["--guilds=5"]), Ok(Some(5)));
    assert_eq!(parse(&["--guilds", "5", "--guilds=7"]), Ok(Some(7)));
    for bad in [
        &["--guilds", "0"][..],
        &["--guilds", "100001"],
        &["--guilds", "x"],
        &["--bench", "--guilds"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

/// S0: without dhat's global allocator nothing can be measured.
#[test]
fn s0_profiler_needs_the_dhat_allocator() {
    assert!(matches!(
        measure::start_profiler(),
        Err(measure::MeasureError::AllocatorMissing)
    ));
}
