#!/bin/bash
# usage: start.sh <sprint-key> "<story label>" — commit the ready spec, record its baseline, mark it in-progress
D=$(dirname "$0"); source "$D/env.sh"
f=_bmad-output/implementation-artifacts/spec-$1.md
test -f "$f" || { echo "missing $f"; exit 1; }
grep -q "^status: '\?ready-for-dev" "$f" || { echo "not ready: $(grep ^status "$f")"; exit 1; }
"$D/cp.sh" "Plan $2" || exit 1
b=$(git rev-parse HEAD)
sed -i -E "0,/^status: .*/s//status: 'in-progress'\nbaseline_revision: '$b'/" "$f"
sed -i -E "s/^(  $1): .*/\1: in-progress/" _bmad-output/implementation-artifacts/sprint-status.yaml
"$D/cp.sh" "Start $2" || exit 1
echo "baseline $b"
