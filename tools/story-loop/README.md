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
| `cargo-queue.sh`, `tree-sync.py` | The cargo queue: every building cargo command of the main checkout and the lanes runs in one shared build runner, one at a time |
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
4. Install the cargo queue: `tools/story-loop/cargo-queue.sh --install` (it replaces
   `$CARGO_HOME/bin/cargo` with a shim; undo with `ln -sf rustup ~/.cargo/bin/cargo`).
   Repeat after every container restart.
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
- A blocked story stops the batch; read its spec's `## Auto Run Result`, fix the cause
  or amend the spec, and run again.
- Every third finished epic (3, 6, 9, 12, 15) the script refreshes `README.md`.
- Before the Epic 14 launch stories, move `.github/workflows-disabled/*` back to
  `.github/workflows/` and fix every CI failure.

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
  Lanes do not build in place: the cargo queue (below) builds for all of them. Leave the main
  checkout idle while lanes run.
- Each lane runs its stories in order. Before a story it fast-forwards to `origin/main`
  (`lane.sh sync`); its commits go only to its own branch (`cp.sh` detects `story-loop/*`).
- After a story's finalize, its lane rebases onto `origin/main` (`lane.sh merge`), an agent
  resolves conflicts (sprint-status union, `public-api.txt` re-bless and the Usage Skill hash,
  both sides' docs), the gates run on the merged tree, and `lane.sh push-main` lands it.
  These post-merge gates skip `cargo hack` and the MSRV check: both sides already passed them,
  and the next story's full gates on main run them again.
  Merges are serialized across lanes.
- Pick lanes from story dependencies: stories in one lane may depend on each other; stories
  in different lanes must not (they may touch the same files; the merge step handles that).
- After a restart, rerun with the same lanes: `lane.sh setup` reuses the worktrees, and an
  interrupted story keeps its dirty tree (`stories.py` run inside the lane gives `resumeAt`).

## Cargo queue

Lanes code in parallel, but their cargo commands never run in parallel: with `cargo-queue.sh`
installed as `cargo`, a building command (`check`, `build`, `test`, `clippy`, `fmt`, `run`,
`hack`, ...) started in the main checkout or a lane is queued. The runner takes one command at
a time, oldest first:

1. It copies the caller's working tree (tracked and untracked, non-ignored files; no `target/`, no
   `.git`) into a work dir of that tree, `/home/claude/cargo-queue/work/<hash>/noctis-rs`, so each lane
   keeps its own warm workspace artifacts while registry dependencies are shared. A file is rewritten only when its
   content differs, so cargo's mtime-based freshness never sees a stale file.
2. It runs the command there with `CARGO_TARGET_DIR` set to the main checkout's `target/`. That is
   one warm target dir for everyone, so there is no per-lane target dir and no trybuild clash.
3. It copies back the files the command changed (`cargo fmt`, `--bless`, `Cargo.lock`).

The caller sees the output live, with work-dir paths mapped back to its own tree, and gets the
command's exit code. A caller cut off by a timeout reruns the identical command and attaches to
the queued or running run. A command that already succeeded on the identical tree replays its
output. Commands that do not build (`metadata`, `tree`, `--version`, ...) and the runner's own
nested cargo calls (`NOCTIS_CARGO_DIRECT=1`) run directly. The queue state lives in
`/home/claude/cargo-queue/` (`runner.log`, `jobs/<key>/log`).

Expensive agents do not run cargo themselves: the workflow tells them to hand cargo commands to
a Haiku subagent, which waits in the queue and reports only exit codes, errors and failing
tests.
