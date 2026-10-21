#!/bin/bash
# The local gates from AGENTS.md; prints FAIL:<step> plus the log tail, or ALL-OK. Logs: target/story-loop/gate-<step>.log
source "$(dirname "$0")/env.sh"
run(){ local n=$1; shift; "$@" > "$OUT/gate-$n.log" 2>&1 || { echo "FAIL:$n"; tail -30 "$OUT/gate-$n.log"; exit 1; }; }
run fmt cargo fmt --all --check
run clippy cargo clippy --release --workspace --all-targets --all-features -- -D warnings
run test cargo test --release --workspace --all-features
run hack cargo hack test --release --workspace --each-feature
run msrv cargo +1.91 check --release --workspace
run deny cargo deny check
run cfg bash .github/scripts/check-build-config.sh
for t in deps-check rules-catalog api-snapshot skill-check; do run "$t" cargo run --release -q -p xtask -- "$t"; done
test ! -d target/debug || { echo "FAIL:debug-dir"; exit 1; }
echo ALL-OK
