#!/bin/bash
# Usage: prune-target.sh <target-dir> <minutes>
# Frees disk by deleting whole cargo build units (every deps/build file with the unit's hash plus its .fingerprint
# entry) whose newest file is older than <minutes>. Deleting single files instead leaves a fingerprint that cargo
# still trusts, and the next link fails on a missing .rlib. The pass holds cargo's build lock, so it never runs
# while cargo is building in that target dir; a busy target dir is skipped.
t=$1; min=${2:-360}; r=$t/release
[ -d "$r/.fingerprint" ] || exit 0
exec 9>>"$r/.cargo-lock"
flock -n 9 || { echo "skip $t (cargo is running)"; exit 0; }
n=0
for fp in "$r"/.fingerprint/*-*; do
  [ -d "$fp" ] || continue
  h=${fp##*-}
  [[ $h =~ ^[0-9a-f]{16}$ ]] || continue
  files=$(ls -d "$fp" "$r"/deps/*-"$h"* "$r"/build/*-"$h" 2>/dev/null)
  [ -n "$(find $files -newermt "$min minutes ago" -print -quit 2>/dev/null)" ] && continue
  rm -rf $files; n=$((n + 1))
done
echo "pruned $n units older than ${min} min in $t"
