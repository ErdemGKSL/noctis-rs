#!/bin/bash
# usage: bg.sh <command> [args...]   run a long command detached, so a caller's tool timeout does not kill it.
# Prints the command's output and exits with its status. A caller cut off by a timeout reruns the identical command
# (same directory, same working tree) and attaches to the running run, or gets the result of one that finished
# meanwhile. Job state: target/story-loop/bg/<key>/ (log, status).
dir=$(pwd -P)
root=$(git rev-parse --show-toplevel 2>/dev/null || echo "$dir")
idx=$(mktemp); cp "$(git rev-parse --path-format=absolute --git-path index 2>/dev/null)" "$idx" 2>/dev/null
tree=$(cd "$root" && GIT_INDEX_FILE=$idx git add -A >/dev/null 2>&1 && GIT_INDEX_FILE=$idx git write-tree 2>/dev/null)
rm -f "$idx"
key=$( { printf "%s\0" "$dir" "$tree" "$@"; env | grep -E "^(NOCTIS_|CARGO_|RUST)" | sort; } | md5sum | cut -c1-16)
J=$root/target/story-loop/bg/$key
mkdir -p "$root/target/story-loop/bg"
exec 8>>"$root/target/story-loop/bg/.lock"; flock 8
# A finished run is reused only for a caller that was cut off (within 30 min, same tree); otherwise run again.
if [ -f "$J/status" ] && [ -n "$(find "$J/status" -mmin +30)" ]; then rm -rf "$J"; fi
if [ ! -d "$J" ]; then
  mkdir -p "$J"; : > "$J/log"
  (cd "$dir" && setsid nohup bash -c '"$@" > "$0/log" 2>&1 < /dev/null; echo $? > "$0/status.tmp"; mv "$0/status.tmp" "$0/status"' "$J" "$@" &) 
else
  echo "bg.sh: attached to the same command on the same tree" >&2
fi
flock -u 8; exec 8>&-
tail -c +1 -f "$J/log" & tp=$!
while [ ! -f "$J/status" ]; do sleep 1; done
sleep 1; kill "$tp" 2>/dev/null; wait "$tp" 2>/dev/null
st=$(cat "$J/status")
# Consume the result: the next identical call (e.g. after a fix that left the tree identical) runs again.
mv "$J" "$J.done.$$" 2>/dev/null && rm -rf "$J.done.$$"
exit "$st"
