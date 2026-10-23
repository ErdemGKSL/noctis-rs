#!/bin/bash
# The local gates from AGENTS.md; prints FAIL:<step> plus the log tail, or ALL-OK. Logs: target/story-loop/gate-<step>.log
# A step that already passed on the identical working tree (same tracked + untracked, non-ignored content) is skipped,
# so a run cut off by a caller's timeout resumes where it stopped. Stamps: target/story-loop/gate-<step>.ok
# Every cargo command here goes through the cargo queue (cargo-queue.sh) when it is installed.
# In a lane after its finished story was rebased onto main, hack and msrv are skipped (see below).
source "$(dirname "$0")/env.sh"
idx=$(mktemp); trap 'rm -f "$idx"' EXIT
cp "$(git rev-parse --git-path index)" "$idx" 2>/dev/null
TREE=$(GIT_INDEX_FILE=$idx git add -A >/dev/null 2>&1 && GIT_INDEX_FILE=$idx git write-tree)
run(){
  local n=$1; shift
  [ -n "$TREE" ] && [ "$(cat "$OUT/gate-$n.ok" 2>/dev/null)" = "$TREE" ] && return 0
  rm -f "$OUT/gate-$n.ok"
  "$@" > "$OUT/gate-$n.log" 2>&1 || { echo "FAIL:$n"; tail -30 "$OUT/gate-$n.log"; exit 1; }
  [ -n "$TREE" ] && echo "$TREE" > "$OUT/gate-$n.ok"
}
run fmt cargo fmt --all --check
run clippy cargo clippy --release --workspace --all-targets --all-features -- -D warnings
run test cargo test --release --workspace --all-features
# After a finished lane story is rebased onto main (a "mark done" commit is not yet in origin/main), both sides
# already passed every gate; the slow per-feature and MSRV builds are left to the next story's full gates on main.
merged=
git rev-parse --abbrev-ref HEAD | grep -q '^story-loop/' && git log --format=%s origin/main..HEAD 2>/dev/null | grep -q 'review patches; mark done' && merged=1
[ -z "$merged" ] && run hack cargo hack test --release --workspace --each-feature
[ -z "$merged" ] && run msrv cargo +1.91 check --release --workspace
run deny cargo deny check
run cfg bash .github/scripts/check-build-config.sh
for t in deps-check rules-catalog api-snapshot skill-check; do run "$t" cargo run --release -q -p xtask -- "$t"; done
run tw-grammar cargo run --release -q -p xtask -- tw-grammar --check
test ! -d target/debug || { echo "FAIL:debug-dir"; exit 1; }
echo ALL-OK
