//! [`TestError`]: why a [`TestApp`](crate::TestApp) step failed.

use std::time::Duration;

/// Why building, driving or stopping a [`TestApp`](crate::TestApp) failed.
///
/// Every variant says what to change in the test. A run that fails before
/// dispatch (an unseeded id, an unknown command or option, a disabled
/// button, an invalid modal value) sends nothing to the App.
///
/// ```
/// use noctis_testkit::TestError;
///
/// let err = TestError::Unknown { kind: "user", id: 99 };
/// assert!(err.to_string().contains("TestApp::user"));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TestError {
    /// The current tokio runtime is not multi-threaded (or there is none).
    #[error(
        "TestApp needs a multi-thread tokio runtime: annotate the test with \
         #[tokio::test(flavor = \"multi_thread\")]"
    )]
    Runtime,
    /// A Discord fake could not start (no runtime, no loopback port).
    #[error("a local Discord fake could not start; check that 127.0.0.1 can bind a port")]
    Io(#[from] std::io::Error),
    /// The bot's builder failed `App::build`; the source lists every
    /// violation.
    #[error("the App failed to build; fix the violations it lists")]
    Build(#[source] noctis_core::BuildError),
    /// The App stopped while starting, or ended with an error.
    #[error("the App failed to run; see its start error")]
    Start(#[source] noctis_core::Error),
    /// The App has stopped: its shard has no gateway session.
    #[error("the App has stopped (no gateway session); build a new TestApp")]
    Stopped,
    /// A wait ran out.
    #[error(
        "timed out after {after:?} waiting for {what}; raise TestApp::timeout or make the handler \
         answer sooner"
    )]
    Timeout {
        /// What was awaited.
        what: &'static str,
        /// The bound that ran out.
        after: Duration,
    },
    /// A run references an id that was never seeded.
    #[error(
        "unknown {kind} {id}: seed it with TestApp::user / TestApp::guild (TestGuild::text_channel, TestGuild::role)"
    )]
    Unknown {
        /// `user`, `guild`, `channel` or `role`.
        kind: &'static str,
        /// The id.
        id: u64,
    },
    /// No mounted command (in scope for the run's guild) has this path.
    #[error(
        "no command `{path}` is registered for this run (check the path's words, \
         and `guilds = [..]` scoping)"
    )]
    UnknownCommand {
        /// The path as given.
        path: String,
    },
    /// The command has no option of this name.
    #[error("command `{path}` has no option `{option}`")]
    UnknownOption {
        /// The command path.
        path: String,
        /// The option name.
        option: String,
    },
    /// The value's type does not match the option's.
    #[error("option `{option}` is {expected}, but a {found} value was given")]
    OptionType {
        /// The option name.
        option: String,
        /// The declared type (`STRING`, `INTEGER`, …).
        expected: &'static str,
        /// The given value's type.
        found: &'static str,
    },
    /// Discord would reject the value before sending it.
    #[error("option `{option}`: {reason}")]
    InvalidOption {
        /// The option name.
        option: String,
        /// Why Discord rejects it.
        reason: &'static str,
    },
    /// A required option was not given.
    #[error("required option `{option}` is missing; add .option(\"{option}\", ..)")]
    MissingOption {
        /// The option name.
        option: String,
    },
    /// The value is not one of the option's `choices`.
    #[error("`{value}` is not a choice of option `{option}`")]
    NotAChoice {
        /// The option name.
        option: String,
        /// The given value.
        value: String,
    },
    /// The option is not flagged `autocomplete`.
    #[error("option `{option}` is not flagged `autocomplete`")]
    NotAutocomplete {
        /// The option name.
        option: String,
    },
    /// A finder found nothing.
    #[error("no {what} in the response; available: {available:?}")]
    NotFound {
        /// What was looked for, e.g. `button "Next"`.
        what: String,
        /// What the response has instead.
        available: Vec<String>,
    },
    /// The component is of another kind than the run needs.
    #[error(
        "expected a {expected}, found a {found}: use TestApp::click for buttons, TestApp::select for selects"
    )]
    WrongComponent {
        /// The kind the run needs.
        expected: &'static str,
        /// The component's kind.
        found: &'static str,
    },
    /// The component is disabled, so Discord's client cannot use it.
    #[error("component `{custom_id}` is disabled")]
    Disabled {
        /// Its `custom_id`.
        custom_id: String,
    },
    /// A link or premium button: it has no interaction.
    #[error("link and premium buttons send no interaction")]
    NotInteractive,
    /// The component belongs to an ephemeral message another user sees.
    #[error("user {user} cannot see this ephemeral message; run it .by() the user it was sent to")]
    NotVisible {
        /// The user who tried.
        user: u64,
    },
    /// The number of selected values is outside the select's bounds.
    #[error("the select takes {min}..={max} values, {got} given")]
    SelectValues {
        /// `min_values` (default 1).
        min: u8,
        /// `max_values` (default 1).
        max: u8,
        /// How many were given.
        got: usize,
    },
    /// A string-select value that is not among its options.
    #[error("`{value}` is not an option of this select")]
    NotAnOption {
        /// The value.
        value: String,
    },
    /// The modal has no input with this key.
    #[error("the modal has no input `{key}`")]
    UnknownInput {
        /// The key.
        key: String,
    },
    /// The setter does not fit the input's kind.
    #[error("input `{key}` is a {expected}; use the matching ModalForm setter")]
    WrongInput {
        /// The key.
        key: String,
        /// The input's kind.
        expected: &'static str,
    },
    /// Discord's client would refuse to submit this value.
    #[error("input `{key}`: {reason}")]
    InvalidInput {
        /// The key.
        key: String,
        /// Why.
        reason: String,
    },
    /// The testkit cannot simulate this yet.
    #[error("not supported by the testkit yet: {0}")]
    Unsupported(&'static str),
    /// A message could not be rendered to JSON.
    #[error("the message could not be rendered: {0}")]
    Render(String),
    /// [`assert_memory_flat`](crate::assert_memory_flat) needs at least 10
    /// runs.
    #[error("assert_memory_flat needs at least 10 runs, {n} given")]
    TooFewRuns {
        /// The runs requested.
        n: usize,
    },
    /// Another [`assert_memory_flat`](crate::assert_memory_flat) is
    /// running in this process.
    #[error(
        "another assert_memory_flat is running; the heap profiler is process-wide, so run one at a time"
    )]
    ProfilerBusy,
    /// The test binary does not use the dhat allocator.
    #[error(
        "assert_memory_flat needs the dhat allocator in this test binary: add \
         `#[global_allocator] static ALLOC: noctis::testkit::dhat::Alloc = noctis::testkit::dhat::Alloc;` \
         (one #[test] per binary)"
    )]
    AllocatorMissing,
    /// Live heap memory grew over the slack while the runs ran.
    #[error(
        "live heap grew from {before} to {after} bytes over {runs} runs (more than \
         MEMORY_FLAT_SLACK_BYTES): something keeps per-run state"
    )]
    MemoryGrew {
        /// Live bytes after the warm-up.
        before: u64,
        /// Live bytes after the runs.
        after: u64,
        /// The runs measured.
        runs: usize,
    },
}
