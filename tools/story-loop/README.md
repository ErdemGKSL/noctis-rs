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
