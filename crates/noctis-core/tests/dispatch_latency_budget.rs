//! Story 14.3: the `dispatch_latency` bench's budget table, run validity
//! and verdicts (Matrix L1-L13, E1-E11, E14, E15), through the bench's
//! `budget` module included by path; no run is made.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

#[path = "../benches/dispatch_latency/budget.rs"]
mod budget;

use std::path::{Path, PathBuf};

use budget::{BudgetError, LatencyBudgets, Measurement, RunError, RunStats, Status};

fn real_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/budgets.toml")
}

fn parse(text: &str) -> Result<LatencyBudgets, BudgetError> {
    budget::parse(text, Path::new("budgets.toml"))
}

fn parse_error(text: &str) -> String {
    match parse(text) {
        // The message carries the TOML error, which names the key.
        Err(error @ BudgetError::Parse { .. }) => error.to_string(),
        other => panic!("expected a parse error for {text:?}, got {other:?}"),
    }
}

const HEAD: &str = "[dispatch_latency]\nnfr = \"NFR-3\"\ngate = true\n";
const EVENT: &str = "[dispatch_latency.event_p99_us]\nmax = 1000\nnote = \"events\"\n";
const INTERACTION: &str =
    "[dispatch_latency.interaction_p99_us]\nmax = 1000\nnote = \"interactions\"\n";

fn minimal() -> String {
    format!("{HEAD}{EVENT}{INTERACTION}")
}

/// The minimal file with the event metric table replaced by `event`.
fn with_event(event: &str) -> String {
    format!("{HEAD}[dispatch_latency.event_p99_us]\n{event}\n{INTERACTION}")
}

fn budgets(gate: bool, event_max: Option<u64>, interaction_max: Option<u64>) -> LatencyBudgets {
    let mut budgets = parse(&minimal()).unwrap();
    budgets.gate = gate;
    budgets.event_p99_us.max = event_max;
    budgets.interaction_p99_us.max = interaction_max;
    budgets
}

/// A valid run with event and interaction p99 `event_ns` / `interaction_ns`.
fn run(event_ns: u64, interaction_ns: u64) -> RunStats {
    RunStats {
        event_p50_ns: Some(20_000),
        event_p99_ns: Some(event_ns),
        event_p999_ns: Some(event_ns + 1),
        interaction_p50_ns: Some(90_000),
        interaction_p99_ns: Some(interaction_ns),
        interaction_p999_ns: None,
        achieved_per_s: 10_000,
        max_lag_ns: 5_000_000,
    }
}

fn measured(runs: &[RunStats]) -> Measurement {
    Measurement {
        runs: runs.to_vec(),
    }
}

fn status(report: &budget::Report, metric: &str) -> Status {
    report
        .rows
        .iter()
        .find(|row| row.metric == metric)
        .expect("the row exists")
        .status
}

// --- Budget table (L) ---------------------------------------------------------

#[test]
fn l1_real_file_parses() {
    let budgets = budget::load(&real_path()).unwrap();
    assert_eq!(budgets.nfr, "NFR-3");
    assert!(budgets.gate);
    assert!(budgets.event_p99_us.max.is_some());
    assert!(budgets.interaction_p99_us.max.is_some());
}

#[test]
fn l2_valid_minimal_table_parses() {
    let budgets = parse(&minimal()).unwrap();
    assert_eq!(budgets.nfr, "NFR-3");
    assert!(budgets.gate);
    assert_eq!(budgets.event_p99_us.max, Some(1000));
    assert_eq!(budgets.event_p99_us.note, "events");
    assert_eq!(budgets.interaction_p99_us.max, Some(1000));
    assert_eq!(budgets.interaction_p99_us.note, "interactions");
}

#[test]
fn l3_other_tables_are_ignored() {
    let text = format!(
        "[memory_per_guild]\nnfr = \"NFR-1\"\nguilds = 10000\ngate = true\n\
         [memory_per_guild.framework_bytes_per_guild]\nmax = 1024\nnote = \"x\"\n\
         [reference_card]\np50_ms = 10.0\np99_ms = 50.0\ngate = true\n{}",
        minimal()
    );
    assert_eq!(parse(&text).unwrap().event_p99_us.max, Some(1000));
}

#[test]
fn l4_missing_file_names_the_path() {
    let path = Path::new("/nonexistent/noctis/budgets.toml");
    let error = budget::load(path).unwrap_err();
    assert!(matches!(error, BudgetError::Read { .. }), "{error:?}");
    assert!(
        error
            .to_string()
            .contains("/nonexistent/noctis/budgets.toml"),
        "{error}"
    );
}

#[test]
fn l5_missing_table_is_named() {
    let error = parse_error("[reference_card]\np50_ms = 10.0\np99_ms = 50.0\n");
    assert!(error.contains("dispatch_latency"), "{error}");
}

#[test]
fn l6_missing_fields_are_named() {
    let cases = [
        (
            format!("[dispatch_latency]\ngate = true\n{EVENT}{INTERACTION}"),
            "nfr",
        ),
        (
            format!("[dispatch_latency]\nnfr = \"NFR-3\"\n{EVENT}{INTERACTION}"),
            "gate",
        ),
        (format!("{HEAD}{INTERACTION}"), "event_p99_us"),
        (format!("{HEAD}{EVENT}"), "interaction_p99_us"),
        (with_event("max = 1000"), "note"),
    ];
    for (text, key) in cases {
        let error = parse_error(&text);
        assert!(error.contains(key), "{key}: {error}");
    }
}

#[test]
fn l7_wrong_types_are_rejected() {
    parse_error(&format!(
        "[dispatch_latency]\nnfr = \"NFR-3\"\ngate = \"yes\"\n{EVENT}{INTERACTION}"
    ));
    parse_error(&with_event("max = 1.5\nnote = \"x\""));
    parse_error(&with_event("max = \"1000\"\nnote = \"x\""));
    parse_error(&with_event("max = 1000\nnote = 3"));
}

#[test]
fn l8_negative_max_is_rejected() {
    parse_error(&with_event("max = -1\nnote = \"x\""));
}

#[test]
fn l9_zero_max_is_rejected() {
    let error = parse(&with_event("max = 0\nnote = \"x\"")).unwrap_err();
    assert!(
        matches!(
            error,
            BudgetError::ZeroMax {
                metric: "event_p99_us"
            }
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("event_p99_us"), "{error}");
}

#[test]
fn l10_omitted_max_reports_only() {
    let budgets = parse(&with_event("note = \"reported\"")).unwrap();
    assert_eq!(budgets.event_p99_us.max, None);
    let report = budget::evaluate(&budgets, &measured(&[run(u64::MAX, 1)]), false).unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Reported);
    assert!(!report.failed());
}

#[test]
fn l11_unknown_keys_are_rejected() {
    let error = parse_error(&format!("{HEAD}p95_us = 1\n{EVENT}{INTERACTION}"));
    assert!(error.contains("p95_us"), "{error}");
    let error = parse_error(&with_event("max = 1000\nnote = \"x\"\np95_us = 1"));
    assert!(error.contains("p95_us"), "{error}");
}

#[test]
fn l12_unicode_note_is_verbatim() {
    let budgets = parse(&with_event("max = 1000\nnote = \"≤ 1 ms — NFR-3 ✓\"")).unwrap();
    let report = budget::evaluate(&budgets, &measured(&[run(1, 1)]), false).unwrap();
    assert!(
        report
            .markdown()
            .contains("- Event p99: ≤ 1 ms — NFR-3 ✓\n"),
        "{}",
        report.markdown()
    );
}

#[test]
fn l13_huge_max_never_overflows() {
    let budgets = parse(&with_event("max = 18446744073709551615\nnote = \"x\"")).unwrap();
    assert_eq!(budgets.event_p99_us.max, Some(u64::MAX));
    let report = budget::evaluate(&budgets, &measured(&[run(u64::MAX, 1)]), false).unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Ok);
}

// --- Evaluation (E) -----------------------------------------------------------

#[test]
fn e1_within_budget() {
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[run(999_999, 999_999)]),
        false,
    )
    .unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Ok);
    assert!(!report.failed());
}

#[test]
fn e2_equal_is_within() {
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[run(1_000_000, 1_000_000)]),
        false,
    )
    .unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Ok);
    assert_eq!(status(&report, budget::INTERACTION_P99), Status::Ok);
    assert!(!report.failed());
}

#[test]
fn e3_one_nanosecond_over_fails_with_the_gate() {
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[run(1_000_001, 1)]),
        false,
    )
    .unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Over);
    assert!(report.failed());
    assert!(
        report
            .markdown()
            .contains("| Event p99 | 1000.0 | <= 1000 | over |"),
        "{}",
        report.markdown()
    );
}

#[test]
fn e4_over_without_the_gate_reports_only() {
    let report = budget::evaluate(
        &budgets(false, Some(1000), Some(1000)),
        &measured(&[run(1_000_001, 1)]),
        false,
    )
    .unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Over);
    assert!(!report.failed());
    let text = report.markdown();
    assert!(text.contains("over (report only)"), "{text}");
    assert!(text.contains("gate: off (report only)"), "{text}");
}

#[test]
fn e5_one_of_two_over_fails() {
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[run(10, 2_000_000)]),
        false,
    )
    .unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Ok);
    assert_eq!(status(&report, budget::INTERACTION_P99), Status::Over);
    assert!(report.failed());
}

#[test]
fn e6_median_of_three_ignores_one_outlier() {
    assert_eq!(budget::median3([900, 300, 5000]), 900);
    let runs = [
        run(900_000, 900_000),
        run(300_000, 300_000),
        run(5_000_000, 5_000_000),
    ];
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&runs),
        false,
    )
    .unwrap();
    let event = &report.rows[0];
    assert_eq!(event.measured_ns, Some(900_000));
    assert_eq!(event.status, Status::Ok);
    assert!(!report.failed());
}

#[test]
fn e7_median_of_equal_values() {
    assert_eq!(budget::median3([7, 7, 7]), 7);
    assert_eq!(budget::median(&[7, 7, 7]), Some(7));
    assert_eq!(budget::median(&[4]), Some(4));
    assert_eq!(budget::median(&[]), None);
}

#[test]
fn e9_too_few_samples_is_invalid() {
    let mut short = run(1, 1);
    short.interaction_p99_ns = None;
    let error = budget::check_run(&short, 1).unwrap_err();
    assert_eq!(
        error,
        RunError::TooFewSamples {
            metric: "interaction_p99_us",
            run: 2
        }
    );
    let error = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[run(1, 1), short]),
        false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("run 2"), "{error}");
}

#[test]
fn e10_rate_too_low_is_invalid() {
    let mut slow = run(1, 1);
    slow.achieved_per_s = 9_499;
    assert_eq!(
        budget::check_run(&slow, 0),
        Err(RunError::RateTooLow {
            achieved: 9_499,
            run: 1
        })
    );
    assert!(
        budget::evaluate(
            &budgets(true, Some(1000), Some(1000)),
            &measured(&[slow]),
            false
        )
        .is_err()
    );
}

#[test]
fn e11_rate_boundary_is_valid() {
    let mut edge = run(1, 1);
    edge.achieved_per_s = 9_500;
    assert_eq!(budget::check_run(&edge, 0), Ok(()));
}

#[test]
fn e14_full_mode_needs_bench() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    assert!(budget::full_mode(args(&["--bench"]).into_iter()));
    assert!(budget::full_mode(args(&["x", "--bench"]).into_iter()));
    assert!(!budget::full_mode(args(&[]).into_iter()));
    assert!(!budget::full_mode(args(&["--benchmark"]).into_iter()));
    assert!(!budget::full_mode(args(&["--test"]).into_iter()));
}

#[test]
fn e15_smoke_never_gates_and_notes_invalid_runs() {
    let mut weak = run(5_000_000, 5_000_000);
    weak.achieved_per_s = 8_000;
    weak.interaction_p99_ns = None;
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[weak]),
        true,
    )
    .unwrap();
    assert_eq!(status(&report, budget::EVENT_P99), Status::Over);
    assert!(!report.failed());
    let text = report.markdown();
    assert!(text.contains("smoke: not gated"), "{text}");
    assert!(
        text.contains("- smoke: run 1: fewer than 100 samples for interaction_p99_us"),
        "{text}"
    );
    assert!(
        text.contains("| Interaction p99 | n/a | <= 1000 | reported |"),
        "{text}"
    );
}

#[test]
fn markdown_golden() {
    let runs = [
        run(162_700, 281_400),
        run(150_000, 300_000),
        run(170_000, 250_000),
    ];
    let report = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&runs),
        false,
    )
    .unwrap();
    let expected = "\
## Dispatch latency (NFR-3)

AD-25 mix, 10 000 events/s, 4 workers, 4 producers, 3 × 5 s after 1 s warm-up; gated value: median of the runs' p99; gate: on (over-budget rows fail the run)

| Metric | Measured µs | Budget µs | Status |
|---|---|---|---|
| Event p99 | 162.7 | <= 1000 | ok |
| Interaction p99 | 281.4 | <= 1000 | ok |
| Event p50 | 20.0 | - | reported |
| Event p99.9 | 162.7 | - | reported |
| Interaction p50 | 90.0 | - | reported |
| Interaction p99.9 | n/a | - | reported |

Notes:
- Event p99: events
- Interaction p99: interactions
- Per-run p99 µs: events 162.7, 150.0, 170.0; interactions 281.4, 300.0, 250.0
- Achieved rate: lowest 10000 events/s (a valid run needs 9500); worst schedule lag 5000.0 µs
";
    assert_eq!(report.markdown(), expected);
}

// --- Exit code (the NFR-3 gate's process verdict) -------------------------------

fn exit_for(gate: bool, event_ns: u64, smoke: bool) -> u8 {
    let report = budget::evaluate(
        &budgets(gate, Some(1000), Some(1000)),
        &measured(&[run(event_ns, 1)]),
        smoke,
    );
    budget::exit_code(&report)
}

#[test]
fn exit_code_is_0_within_budget() {
    assert_eq!(exit_for(true, 1_000_000, false), 0);
}

#[test]
fn exit_code_is_1_for_a_gated_full_report_over_budget() {
    assert_eq!(exit_for(true, 1_000_001, false), 1);
}

#[test]
fn exit_code_is_0_over_budget_with_the_gate_off_or_in_smoke() {
    assert_eq!(exit_for(false, 1_000_001, false), 0);
    assert_eq!(exit_for(true, 1_000_001, true), 0);
}

#[test]
fn exit_code_is_2_for_an_error() {
    let error: Result<budget::Report, String> = Err("nothing measured".to_owned());
    assert_eq!(budget::exit_code(&error), 2);
    let no_runs = budget::evaluate(
        &budgets(true, Some(1000), Some(1000)),
        &measured(&[]),
        false,
    );
    assert_eq!(budget::exit_code(&no_runs), 2);
}
