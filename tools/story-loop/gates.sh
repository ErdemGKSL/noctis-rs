#!/bin/bash
# The local gates from AGENTS.md; prints FAIL:<step> plus the log tail, or ALL-OK. Logs: target/story-loop/gate-<step>.log
# A step that already passed on the identical working tree (same tracked + untracked, non-ignored content) is skipped,
# so a run cut off by a caller's timeout resumes where it stopped. Stamps: target/story-loop/gate-<step>.ok
source "$(dirname "$0")/env.sh"
# Disk guard: lanes each have a cargo target dir and the session disk is small. Below 8 GiB free, hardlink identical
# files across the lane target dirs (registry dependencies build to the same bytes in every lane). Nothing is pruned
# here: an old artifact is usually still fresh, so deleting it only forces a rebuild; prune-target.sh is for a
# near-full disk.
if [ "$(df --output=avail -k "$ROOT" | tail -1)" -lt 8388608 ]; then
  main=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
  command -v hardlink >/dev/null && hardlink -q "$main/target/release/deps" "$(dirname "$main")"/lanes/*/noctis-rs/target/release/deps 2>/dev/null
fi
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
run hack cargo hack test --release --workspace --each-feature
run msrv cargo +1.91 check --release --workspace
run deny cargo deny check
run cfg bash .github/scripts/check-build-config.sh
for t in deps-check rules-catalog api-snapshot skill-check; do run "$t" cargo run --release -q -p xtask -- "$t"; done
run tw-grammar cargo run --release -q -p xtask -- tw-grammar --check
test ! -d target/debug || { echo "FAIL:debug-dir"; exit 1; }
echo ALL-OK
