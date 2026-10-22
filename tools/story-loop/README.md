# Story loop

The unattended implementation loop for noctis-rs: for every story that is not done it
plans a spec, implements it, runs the local gates, reviews the diff in four parallel
layers, triages the findings and patches or re-specifies until the story is done. It
follows the `bmad-build-auto` workflow and the maintainer workflow in `AGENTS.md`.

## Files

| File | Role |
|---|---|
| `story-loop.js` | Workflow script (Claude Code `Workflow` tool) that drives every story |
| `stories.py` | Prints the workflow's `stories` argument: stories not done, in sprint order |
| `planner.md`, `triage.md`, `rv-*.md` | Prompts for the planner, triage and the four review layers |
| `gates.sh` | Every local gate from `AGENTS.md`; prints `ALL-OK` or `FAIL:<step>` |
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
4. If the harness has a stop hook that demands a commit on every turn, the loop
   commits only finished steps; never make WIP commits to satisfy it.

## Run

```bash
python3 tools/story-loop/stories.py --epics 7-9   # or no flag for everything left
```

Then run the `Workflow` tool with `scriptPath: tools/story-loop/story-loop.js` and
`args: { root: "<repo root>", w: "<W>", date: "<today>", stories: <that JSON> }`.
Run about one or two epics per invocation, then check the results, `git log` and
`sprint-status.yaml` before the next batch.

- A story whose spec is already `in-progress` resumes at implementation
  (`resumeAt: "implement"`). If its implementation was already committed and diffed,
  set `resumeReviewPass: <pass>` on it instead to resume at review.
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
  { name: 'a', target: '<root>/target', stories: <stories.py --epics 10-10> },  // reuses the idle main checkout's warm target
  { name: 'b', stories: <stories.py --epics 11-13> },                            // its own target/ inside the worktree
] }
```

- `lane.sh setup <name> [target-dir]` creates the worktree `../lanes/<name>/noctis-rs` on
  branch `story-loop/<name>`; with a target dir it writes `../lanes/<name>/.cargo/config.toml`.
  Lanes never share a target dir (trybuild's project dir is per crate name). Leave the main
  checkout idle while lanes run.
- Each lane runs its stories in order. Before a story it fast-forwards to `origin/main`
  (`lane.sh sync`); its commits go only to its own branch (`cp.sh` detects `story-loop/*`).
- After a story's finalize, its lane rebases onto `origin/main` (`lane.sh merge`), an agent
  resolves conflicts (sprint-status union, `public-api.txt` re-bless and the Usage Skill hash,
  both sides' docs), the gates run on the merged tree, and `lane.sh push-main` lands it.
  Merges are serialized across lanes.
- Pick lanes from story dependencies: stories in one lane may depend on each other; stories
  in different lanes must not (they may touch the same files; the merge step handles that).
- After a restart, rerun with the same lanes: `lane.sh setup` reuses the worktrees, and an
  interrupted story keeps its dirty tree (`stories.py` run inside the lane gives `resumeAt`).
