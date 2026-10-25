# Story loop

The unattended implementation loop for noctis-rs: for every story that is not done it
plans a spec, implements it, runs the local gates, reviews the diff in four parallel
layers, triages the findings and patches or re-specifies until the story is done. It
follows the `bmad-build-auto` workflow and the maintainer workflow in `AGENTS.md`.

## Files

| File | Role |
|---|---|
| `.claude/workflows/story-loop.js` | Workflow script (Claude Code `Workflow` tool, name `bmad-story-loop`) that drives every story; `story-loop.js` here is a symlink to it |
| `stories.py` | Prints the workflow's `stories` argument: stories not done, in sprint order |
| `planner.md`, `triage.md`, `rv-*.md` | Prompts for the planner, triage and the four review layers |
| `gates.sh` | Every local gate from `AGENTS.md`; prints `ALL-OK` or `FAIL:<step>` |
| `bg.sh` | Runs a long command (the gates, a workspace-wide test) detached; a caller cut off by a tool timeout reruns it and attaches |
| `readme-due.py` | Tells the loop whether README progress is due (3+ stories past the badge on origin/main, or an epic just closed) |
| `epic-last.py` | Tells `gates.sh` whether the gated story is the last open one of its epic (only then `cargo hack` runs) |
| `prune-target.sh` | Frees disk by deleting whole cargo build units under cargo's build lock (for a near-full disk only) |
| `cp.sh` | Commit everything and push to `main` (and the current branch) |
| `start.sh`, `finish.sh`, `diff.sh` | Mark a spec started or done, write the review diff |

Diffs, review reports, patch lists and gate logs go to `target/story-loop/` (ignored).

## Setup in a new container

1. Toolchains and tools (see `AGENTS.md` "Known pitfalls"):
   `rustup toolchain install nightly-2026-10-15 --profile minimal`,
   `rustup toolchain install 1.91 --profile minimal`,
   `cargo install cargo-public-api --version 0.52.0 --locked`,
   `cargo install cargo-hack --version ^0.6 --locked`,
   `cargo install cargo-deny --version ^0.20 --locked` (skip what is already installed).
2. Render the workflow and note the directory it prints (`W`, the directory of the
   printed `workflow.md`):
   `uv run --no-cache _bmad/scripts/render_skill.py --project-root "$PWD" --skill "$PWD/.claude/skills/bmad-build-auto"`
3. Commit trailers: write the session's own attribution lines (the `Co-Authored-By`
   and session lines your harness asks for) to `target/story-loop/trailers`, one per
   line. `cp.sh` always adds the maintainer as co-author before them.
4. If an older container still has the cargo queue shim installed as `cargo`, remove it:
   `ln -sf rustup ~/.cargo/bin/cargo`.
5. If the harness has a stop hook that demands a commit on every turn, the loop
   commits only finished steps; never make WIP commits to satisfy it.

## Run

```bash
python3 tools/story-loop/stories.py --epics 7-9   # or no flag for everything left
```

Then run the `Workflow` tool with `name: bmad-story-loop` (or `scriptPath: .claude/workflows/story-loop.js`) and
`args: { root: "<repo root>", w: "<W>", date: "<today>", stories: <that JSON> }`.
Run about one or two epics per invocation, then check the results, `git log` and
`sprint-status.yaml` before the next batch.

- A story whose spec is already `in-progress` resumes at implementation
  (`resumeAt: "implement"`). If its implementation was already committed and diffed,
  set `resumeReviewPass: <pass>` on it instead to resume at review. A story whose review
  patches are applied but not gated takes `resumeAt: "gates"` with `pass` and `tri` (the
  triage result); in a lane, a story done in the lane but not yet in main takes
  `resumeAt: "merge"`.
- `[Maintainer-gated]` stories (14.8, 14.9) are skipped and reported as waiting.
- After a restart, give each in-flight story `resumeAt: "auto"`: a recovery agent reads the
  spec, `git log` and the working tree and picks the resume point itself.
- A story that stops (planning blocked, a halt, a failed sync, start or merge) does not end
  the lane. A recovery agent (`recover <num>`) diagnoses it: it fixes a tooling or environment
  fault (in `tools/story-loop/` too, so it does not recur), amends a bad spec, or decides an
  intent question and logs it as a `(decision)` line, sets a `blocked` status back to
  `in-progress`, and names the resume point; the story is retried in place, at most twice.
  Only then does the lane stop.
- An agent that dies (no structured output) is retried once and then counts as a failed
  step; it no longer ends its lane silently.
- Every 3-4 merged stories and at the end of each epic (`readme-due.py`: done stories exceed origin/main's README badge by 3, or the story closed its epic) the script refreshes `README.md`; in a lane it commits the refresh before the merge, so it lands on main with the story under the merge lock.
- Before the Epic 14 launch stories, move `.github/workflows-disabled/*` back to
  `.github/workflows/` and fix every CI failure.

Gates between implementation and review rounds are light (`NOCTIS_GATES_LIGHT=1`: no
`cargo hack`, no MSRV check); the full gates run once before a story is finalized, skipping
every step that already passed on the same tree. `cargo hack` itself runs only for the last story of an epic
(the in-progress story's epic has no other story left that is not done in `sprint-status.yaml`).

## Parallel lanes

Independent stories can run at the same time in separate git worktrees. Pass `lanes`
instead of `stories`:

```js
args: { root, w, date, lanes: [
  { name: 'a', stories: <stories.py --epics 10-10> },
  { name: 'b', stories: <stories.py --epics 11-13> },
] }
```

- `lane.sh setup <name> [target-dir]` creates the worktree `../lanes/<name>/noctis-rs` on
  branch `story-loop/<name>`; with a target dir it writes `../lanes/<name>/.cargo/config.toml`.
  Run one lane per machine: a lane builds in place with its own `target/`. Leave the main
  checkout idle while the lane runs.
- Each lane runs its stories in order. Before a story it fast-forwards to `origin/main`
  (`lane.sh sync`); its commits go only to its own branch (`cp.sh` detects `story-loop/*`).
- After a story's finalize, its lane rebases onto `origin/main` (`lane.sh merge`), an agent
  resolves conflicts (sprint-status union, `public-api.txt` re-bless and the Usage Skill hash,
  both sides' docs), the gates run on the merged tree, and `lane.sh push-main` lands it.
  These post-merge gates skip `cargo hack` and the MSRV check: both sides already passed them,
  and the next story's full gates on main run them again.
  Merges are serialized across lanes, also across separate workflow runs: `mergeIntoMain`
  takes the merge lock `lane.sh lock` (the remote branch `story-loop/merge-lock`, so lanes on
  different machines share it; `push-main` releases it, a lock older than 4 h is stale). Push to main by hand only while holding it:
  `tools/story-loop/lane.sh lock manual`, push, `tools/story-loop/lane.sh unlock manual`.
- Run each lane in its own workflow run (`lanes: [<one lane>]`): a run caps concurrent agents
  at CPUs - 2 (2 on a 4-CPU container), so three lanes in one run advance only two at a time.
  The merge lock serializes their merges across runs and machines; a lane may also run in its
  own clone on another machine (lane names must be unique).
- Pick lanes from story dependencies: stories in one lane may depend on each other; stories
  in different lanes must not (they may touch the same files; the merge step handles that).
- After a restart, rerun with the same lanes: `lane.sh setup` reuses the worktrees, and an
  interrupted story keeps its dirty tree (`stories.py` run inside the lane gives `resumeAt`).

## Cargo

Each lane runs on its own machine (maintainer rule, 2026-10-24), so there is no shared build
queue: agents run `cargo check` and `cargo test` themselves, scoped to the crates they touch,
and wrap anything that may outlast a 10-minute tool timeout in `bg.sh` (a rerun of the identical
command on the same tree attaches to the running one). The operator runs the gates through
`bg.sh` too. `CARGO_INCREMENTAL=0` always (`env.sh`; the incremental cache filled the disk
before). `cargo hack` runs only in `gates.sh`, and only while the last open story of an epic is
gated (`epic-last.py`, which also counts stories other lanes merged into `origin/main`).

