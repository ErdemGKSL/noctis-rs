#!/bin/bash
# usage: cp.sh "message" — commit everything, push to main (and the current branch if it is not main); prints FAIL: if the push never lands.
# Trailers: the maintainer as co-author, then the lines in target/story-loop/trailers (the session's own attribution, written by the orchestrator).
source "$(dirname "$0")/env.sh"
git add -A
if ! git diff --cached --quiet; then
  msg="$1

Co-Authored-By: Erdem <74366943+ErdemGKSL@users.noreply.github.com>"
  [ -s "$OUT/trailers" ] && msg="$msg
$(cat "$OUT/trailers")"
  git commit -q -m "$msg" || exit 1
else
  echo "nothing to commit"
fi
b=$(git branch --show-current)
for i in 1 2 3 4; do
  git push -q origin HEAD:main 2>/dev/null && { [ -z "$b" ] || [ "$b" = main ] || git push -q origin "HEAD:$b" 2>/dev/null; } && break
  sleep $((2**i))
done
git fetch -q origin main 2>/dev/null
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then echo "FAIL: HEAD $(git rev-parse --short HEAD) is not on origin/main"; exit 1; fi
git log --oneline -1
