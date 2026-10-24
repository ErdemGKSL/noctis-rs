//! The `[memory_per_guild]` table of `benches/budgets.toml` and the report
//! the bench prints against it (NFR-1, AD-28). Pure except [`load`].

use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The largest guild count a run accepts.
pub(crate) const MAX_GUILDS: u32 = 100_000;

/// The note suffix of a negative value.
const NOISE: &str = "(below measurement noise)";

/// Why the budgets cannot be read or a measurement cannot be evaluated.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BudgetError {
    /// The budgets file cannot be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The budgets file is not valid TOML or breaks the schema.
    #[error("cannot parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    /// A guild count of 0 measures nothing per guild.
    #[error("the guild count must be at least 1")]
    ZeroGuilds,
    /// More guilds than [`MAX_GUILDS`].
    #[error("the guild count {guilds} exceeds {MAX_GUILDS}")]
    TooManyGuilds { guilds: u64 },
}

/// The whole budgets file: only `[memory_per_guild]` is read, every other
/// top-level table (another bench's) is ignored.
#[derive(Debug, Deserialize)]
struct BudgetsFile {
    memory_per_guild: MemoryBudgets,
}

/// The `[memory_per_guild]` table.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryBudgets {
    /// The requirement id (`NFR-1`).
    pub(crate) nfr: String,
    /// The default guild count of a run.
    pub(crate) guilds: u64,
    /// Whether an over-budget metric fails the run.
    pub(crate) gate: bool,
    pub(crate) framework_bytes_per_guild: MetricBudget,
    pub(crate) discord_cache_bytes_per_guild: MetricBudget,
    pub(crate) render_cache_bytes: MetricBudget,
}

impl MemoryBudgets {
    /// The validated default guild count.
    pub(crate) fn guilds(&self) -> u32 {
        // `parse` checked the range.
        u32::try_from(self.guilds).unwrap_or(MAX_GUILDS)
    }
}

/// One metric's budget.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetricBudget {
    /// The budget; `None` reports the value only.
    pub(crate) max: Option<u64>,
    /// Shown in the report's notes.
    pub(crate) note: String,
}

/// The live-heap deltas of the bench's runs and the Renderer cache usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Measurement {
    /// How many guilds the `none` and `default` runs loaded.
    pub(crate) guilds: u32,
    /// The empty App's delta (0 guilds, Discord cache off).
    pub(crate) baseline: i64,
    /// The delta with `guilds` guilds and `DiscordCacheConfig::none()`.
    pub(crate) none: i64,
    /// The delta with `guilds` guilds and `DiscordCacheConfig::default()`.
    pub(crate) default: i64,
    /// The App's `MemoryRenderCache` weigher bytes.
    pub(crate) render_cache_bytes: u64,
}

/// Reads the budgets file at `path`.
///
/// # Errors
///
/// [`BudgetError::Read`], or [`parse`]'s errors.
pub(crate) fn load(path: &Path) -> Result<MemoryBudgets, BudgetError> {
    let text = std::fs::read_to_string(path).map_err(|source| BudgetError::Read {
        path: path.to_owned(),
        source,
    })?;
    parse(&text, path)
}

/// Parses the budgets file `text` (read from `path`).
///
/// # Errors
///
/// [`BudgetError::Parse`] for invalid TOML or schema,
/// [`BudgetError::ZeroGuilds`] / [`BudgetError::TooManyGuilds`] for a guild
/// count outside `1..=100_000`.
pub(crate) fn parse(text: &str, path: &Path) -> Result<MemoryBudgets, BudgetError> {
    let file: BudgetsFile = toml::from_str(text).map_err(|source| BudgetError::Parse {
        path: path.to_owned(),
        source,
    })?;
    check_guilds(file.memory_per_guild.guilds)?;
    Ok(file.memory_per_guild)
}

/// Accepts `1..=MAX_GUILDS`.
pub(crate) fn check_guilds(guilds: u64) -> Result<(), BudgetError> {
    match guilds {
        0 => Err(BudgetError::ZeroGuilds),
        n if n > u64::from(MAX_GUILDS) => Err(BudgetError::TooManyGuilds { guilds: n }),
        _ => Ok(()),
    }
}

/// How a metric compares with its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Within its `max` (a negative value always is).
    Ok,
    /// Above its `max`.
    Over,
    /// No `max`: reported only.
    Reported,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Over => "over",
            Self::Reported => "reported",
        }
    }
}

/// A measured value: per guild (one decimal) or a byte total (integer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Value {
    PerGuild(f64),
    Total(i128),
}

impl Value {
    fn is_negative(self) -> bool {
        match self {
            Self::PerGuild(v) => v < 0.0,
            Self::Total(v) => v < 0,
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a budget is compared at f64 precision"
    )]
    fn exceeds(self, max: u64) -> bool {
        match self {
            Self::PerGuild(v) => v > max as f64,
            Self::Total(v) => v > i128::from(max),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PerGuild(v) => write!(f, "{v:.1}"),
            Self::Total(v) => write!(f, "{v}"),
        }
    }
}

/// One report row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) metric: &'static str,
    pub(crate) value: Value,
    pub(crate) max: Option<u64>,
    pub(crate) status: Status,
    /// The notes line; `None` shows no line.
    pub(crate) note: Option<String>,
}

impl Row {
    fn new(metric: &'static str, value: Value, max: Option<u64>, note: Option<&str>) -> Self {
        let status = match max {
            None => Status::Reported,
            Some(max) if value.exceeds(max) => Status::Over,
            Some(_) => Status::Ok,
        };
        let note = note.map(|note| {
            if value.is_negative() {
                format!("{note} {NOISE}")
            } else {
                note.to_owned()
            }
        });
        Self {
            metric,
            value,
            max,
            status,
            note,
        }
    }
}

/// The evaluated measurement.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Report {
    pub(crate) nfr: String,
    pub(crate) guilds: u32,
    pub(crate) gate: bool,
    pub(crate) rows: Vec<Row>,
}

/// Derives the metrics of `measured` and compares them with `budgets`:
/// framework bytes per guild = `(none − baseline) / guilds`, Discord cache
/// bytes = `default − none` (total and per guild), Renderer cache bytes as
/// measured.
///
/// # Errors
///
/// [`BudgetError::ZeroGuilds`] / [`BudgetError::TooManyGuilds`] for a guild
/// count outside `1..=100_000`.
#[expect(
    clippy::cast_precision_loss,
    reason = "per-guild values are reported at f64 precision"
)]
pub(crate) fn evaluate(
    budgets: &MemoryBudgets,
    measured: &Measurement,
) -> Result<Report, BudgetError> {
    check_guilds(u64::from(measured.guilds))?;
    let guilds = f64::from(measured.guilds);
    let framework = i128::from(measured.none) - i128::from(measured.baseline);
    let discord = i128::from(measured.default) - i128::from(measured.none);
    let framework_budget = &budgets.framework_bytes_per_guild;
    let discord_budget = &budgets.discord_cache_bytes_per_guild;
    let render_budget = &budgets.render_cache_bytes;
    let rows = vec![
        Row::new(
            "Framework bytes per guild",
            Value::PerGuild(framework as f64 / guilds),
            framework_budget.max,
            Some(&framework_budget.note),
        ),
        Row::new(
            "Discord cache bytes per guild",
            Value::PerGuild(discord as f64 / guilds),
            discord_budget.max,
            Some(&discord_budget.note),
        ),
        Row::new(
            "Discord cache bytes (total)",
            Value::Total(discord),
            None,
            None,
        ),
        Row::new(
            "Renderer cache bytes",
            Value::Total(i128::from(measured.render_cache_bytes)),
            render_budget.max,
            Some(&render_budget.note),
        ),
    ];
    Ok(Report {
        nfr: budgets.nfr.clone(),
        guilds: measured.guilds,
        gate: budgets.gate,
        rows,
    })
}

impl Report {
    /// Whether the run fails: gating is on and a metric is over budget.
    pub(crate) fn failed(&self) -> bool {
        self.gate && self.rows.iter().any(|row| row.status == Status::Over)
    }

    /// The Markdown report (also the GitHub step summary); ends with one
    /// newline.
    pub(crate) fn markdown(&self) -> String {
        let gate = if self.gate {
            "on (over-budget rows fail the run)"
        } else {
            "off (reporting until Story 14.3)"
        };
        let mut out = String::new();
        // Writing to a String cannot fail.
        let _ = writeln!(out, "## Memory per idle guild ({})\n", self.nfr);
        let _ = writeln!(
            out,
            "{} idle guilds through FakeGateway; Discord cache: {}; gate: {gate}\n",
            self.guilds,
            default_cache()
        );
        out.push_str("| Metric | Measured | Budget | Status |\n|---|---|---|---|\n");
        for row in &self.rows {
            let budget = row
                .max
                .map_or_else(|| "-".to_owned(), |max| format!("<= {max}"));
            let _ = writeln!(
                out,
                "| {} | {} | {budget} | {} |",
                row.metric,
                row.value,
                row.status.label()
            );
        }
        out.push_str("\nNotes:\n");
        for row in &self.rows {
            if let Some(note) = &row.note {
                let _ = writeln!(out, "- {}: {note}", row.metric);
            }
        }
        out
    }
}

/// The default Discord cache configuration's resources, plus which of the
/// growing resources it leaves out: `GUILD | CHANNEL | ROLE (no members, no
/// messages)`.
fn default_cache() -> String {
    use noctis::{DiscordCacheConfig, DiscordCacheResources};

    let resources = DiscordCacheConfig::default().resources();
    let absent: Vec<&str> = [
        (DiscordCacheResources::MEMBER, "no members"),
        (DiscordCacheResources::MESSAGE, "no messages"),
    ]
    .into_iter()
    .filter(|(resource, _)| !resources.contains(*resource))
    .map(|(_, label)| label)
    .collect();
    if absent.is_empty() {
        resources.to_string()
    } else {
        format!("{resources} ({})", absent.join(", "))
    }
}
