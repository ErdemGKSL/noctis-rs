#!/usr/bin/env bash
# Runs a release command and retries it while crates.io rate-limits new
# crates (5 at once, then 1 per 10 minutes; Story 14.7, docs/guides/release.md).
# usage: release-retry.sh <attempts> <wait-seconds> -- <command...>
# A failure without rate-limit text is not retried; the exit status is the
# last run's.
set -euo pipefail

name=release-retry

usage() {
    echo "$name: usage: release-retry.sh <attempts> <wait-seconds> -- <command...>" >&2
    exit 2
}

[[ $# -ge 4 && "$3" == "--" ]] || usage
attempts=$1
wait=$2
shift 3
[[ "$attempts" =~ ^[0-9]+$ ]] && ((10#$attempts >= 1)) || usage
[[ "$wait" =~ ^[0-9]+$ ]] || usage
attempts=$((10#$attempts))
wait=$((10#$wait))
[[ -n "$1" ]] || usage

log=$(mktemp)
trap 'rm -f "$log"' EXIT

attempt=1
while true; do
    status=0
    # The output is shown as it comes and kept to look for the rate limit.
    "$@" 2>&1 | tee "$log" || status=${PIPESTATUS[0]}
    if [[ $status -eq 0 ]]; then
        exit 0
    fi
    if ! grep -qiE '429|too many requests|too many new crates' "$log"; then
        exit "$status"
    fi
    if [[ $attempt -ge $attempts ]]; then
        echo "$name: still rate limited after $attempts attempts" >&2
        exit "$status"
    fi
    echo "$name: crates.io rate limit (attempt $attempt/$attempts); retrying in ${wait}s" >&2
    sleep "$wait"
    attempt=$((attempt + 1))
done
