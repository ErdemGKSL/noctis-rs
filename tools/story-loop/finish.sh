#!/bin/bash
# usage: finish.sh <sprint-key> <followup true|false> "<commit msg>" ; Auto Run Result text on stdin
D=$(dirname "$0"); source "$D/env.sh"
f=_bmad-output/implementation-artifacts/spec-$1.md
{ echo; echo "## Auto Run Result"; echo; cat; } >> "$f"
sed -i -E "0,/^status: .*/s//status: 'done'/; s/^followup_review_recommended: .*/followup_review_recommended: $2/" "$f"
y=_bmad-output/implementation-artifacts/sprint-status.yaml
sed -i -E "s/^(  $1): .*/\1: done/" "$y"
e=${1%%-*}; sed -i -E "s/^(  epic-$e): backlog/\1: in-progress/" "$y"
"$D/cp.sh" "$3"
