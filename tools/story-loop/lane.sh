#!/bin/bash
# Parallel lanes: each lane is a git worktree on branch story-loop/<name>, whose finished stories merge into main.
# usage: lane.sh setup <name> [target-dir]   (from the main checkout) create or reuse the lane worktree at
#                               <lanes>/<name>/noctis-rs, print its path; target-dir (e.g. the idle main checkout's
#                               warm target/) goes into <lanes>/<name>/.cargo/config.toml. Lanes never share a target
#                               dir: trybuild's project dir is per crate name, so two lanes would overwrite each other.
#        lane.sh sync           (in a lane) fast-forward the lane to origin/main before a story starts
#        lane.sh merge          (in a lane) rebase the lane's commits onto origin/main; FAIL:conflict leaves the rebase open
#        lane.sh push-main      (in a lane) push the rebased lane to main, then to its own branch; releases the merge lock
#        lane.sh lock [owner]   take the cross-run merge lock (waits up to 9 min; WAIT:<holder> when still held)
#        lane.sh unlock [owner] release it if [owner] holds it. Owner defaults to the lane name (branch story-loop/<name>).
#                               Anyone pushing to main by hand takes it first (lane.sh lock manual), so no lane's
#                               merge is rejected halfway; a lock older than 4 h is stale and is broken.
source "$(dirname "$0")/env.sh"
LANES=${NOCTIS_LANES_DIR:-$(dirname "$ROOT")/lanes}
LOCK=$(cd "$(git rev-parse --git-common-dir)" && pwd)/story-loop-merge.lock
owner=${2:-$(git branch --show-current | sed 's|^story-loop/||')}
release(){ [ "$(cat "$LOCK/owner" 2>/dev/null)" = "$owner" ] && rm -rf "$LOCK"; return 0; }
case "$1" in
setup)
  name=$2; [ -n "$name" ] || { echo "FAIL: lane name missing"; exit 1; }
  wt=$LANES/$name/noctis-rs
  mkdir -p "$LANES/$name"
  if [ -n "$3" ]; then
    mkdir -p "$LANES/$name/.cargo"
    printf '[build]\ntarget-dir = "%s"\n' "$3" > "$LANES/$name/.cargo/config.toml"
  fi
  git fetch -q origin main || { echo "FAIL: fetch"; exit 1; }
  if [ ! -d "$wt" ]; then
    git worktree add -q -B "story-loop/$name" "$wt" origin/main || { echo "FAIL: worktree add"; exit 1; }
  fi
  mkdir -p "$wt/target/story-loop"
  [ -s "$OUT/trailers" ] && cp "$OUT/trailers" "$wt/target/story-loop/trailers"
  echo "$wt"
  ;;
sync)
  git fetch -q origin main || { echo "FAIL: fetch"; exit 1; }
  if [ -n "$(git status --porcelain)" ]; then
    # An interrupted story left work in the tree; keep it as it is (the story resumes). Its commits reach main
    # through lane.sh merge, which rebases them once the story is committed.
    echo "dirty tree kept"; git log --oneline -1; exit 0
  fi
  if git merge-base --is-ancestor HEAD origin/main; then git merge -q --ff-only origin/main
  elif ! git merge-base --is-ancestor origin/main HEAD; then
    git rebase -q origin/main || { echo "FAIL:conflict"; git diff --name-only --diff-filter=U; exit 1; }
  fi
  git log --oneline -1
  ;;
merge)
  git fetch -q origin main || { echo "FAIL: fetch"; exit 1; }
  if [ -d "$(git rev-parse --git-path rebase-merge)" ] || [ -d "$(git rev-parse --git-path rebase-apply)" ]; then
    echo "FAIL:conflict (rebase still in progress)"; git diff --name-only --diff-filter=U; exit 1
  fi
  [ -z "$(git status --porcelain)" ] || { echo "FAIL: dirty tree before merge"; git status --short | head; exit 1; }
  git rebase -q origin/main || { echo "FAIL:conflict"; git diff --name-only --diff-filter=U; exit 1; }
  git log --oneline -1
  ;;
push-main)
  b=$(git branch --show-current)
  git push -q origin HEAD:main || { echo "FAIL: main moved; merge again"; exit 1; }
  release
  git push -q -f origin "HEAD:$b" 2>/dev/null
  git log --oneline -1
  ;;
lock)
  end=$((SECONDS + 540))
  while :; do
    if mkdir "$LOCK" 2>/dev/null; then echo "$owner" > "$LOCK/owner"; echo "locked by $owner"; exit 0; fi
    [ "$(cat "$LOCK/owner" 2>/dev/null)" = "$owner" ] && { echo "locked by $owner"; exit 0; }
    [ -n "$(find "$LOCK" -maxdepth 0 -mmin +240 2>/dev/null)" ] && { rm -rf "$LOCK"; continue; }
    [ $SECONDS -ge $end ] && { echo "WAIT:$(cat "$LOCK/owner" 2>/dev/null)"; exit 1; }
    sleep 20
  done
  ;;
unlock) release; echo "unlocked" ;;
*) echo "FAIL: usage: lane.sh setup <name> | sync | merge | push-main | lock [owner] | unlock [owner]"; exit 1 ;;
esac
