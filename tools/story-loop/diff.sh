#!/bin/bash
# usage: diff.sh <sprint-key> <tag> — writes target/story-loop/diff-<tag>.patch: spec baseline to working tree, untracked files included
source "$(dirname "$0")/env.sh"
b=$(grep -oP "^baseline_revision: '\K[0-9a-f]+" "_bmad-output/implementation-artifacts/spec-$1.md")
git add -A -N . && git diff "$b" -- . ':(exclude)Cargo.lock' ':(exclude)_bmad-output/implementation-artifacts/sprint-status.yaml' > "$OUT/diff-$2.patch"
ls -la "$OUT/diff-$2.patch"
