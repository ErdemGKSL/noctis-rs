#!/bin/bash
# usage: incremental-guard.sh   (detached, e.g. setsid nohup ... &; again after a container restart)
# Every 15 minutes: logs the size of the cargo queue's incremental cache and the free disk. When less than
# NOCTIS_MIN_FREE_GIB (default 4) is free, it turns incremental release builds off for the cargo queue
# (touches $Q/no-incremental) and deletes the cache while holding cargo's build lock. rm the flag to turn it back on.
Q=${NOCTIS_CARGO_QUEUE:-/home/claude/cargo-queue}
T=${NOCTIS_CARGO_TARGET:-/home/claude/noctis-rs/target}
min=$(( ${NOCTIS_MIN_FREE_GIB:-4} * 1048576 ))
exec 7>>"$Q/incremental-guard.lock"
flock -n 7 || exit 0
while true; do
  free=$(df --output=avail -k "$T" | tail -1)
  size=$(du -sk "$T/release/incremental" 2>/dev/null | cut -f1)
  echo "$(date -u +%FT%TZ) incremental ${size:-0} KiB, free $free KiB$([ -f "$Q/no-incremental" ] && echo ', incremental off')"
  if [ "$free" -lt "$min" ] && [ ! -f "$Q/no-incremental" ]; then
    touch "$Q/no-incremental"
    flock "$T/release/.cargo-lock" rm -rf "$T/release/incremental"
    echo "$(date -u +%FT%TZ) disk low: incremental off, cache deleted, free $(df --output=avail -k "$T" | tail -1) KiB"
  fi
  sleep 900
done
