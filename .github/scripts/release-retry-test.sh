#!/usr/bin/env bash
# Tests release-retry.sh (Story 14.7, rows T1-T7) with stub commands that
# count their runs in a file; the wait is 0, so nothing sleeps.
set -euo pipefail

name=release-retry-test
here=$(cd "$(dirname "$0")" && pwd)
retry=$here/release-retry.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

failures=0

fail() {
    echo "$name: $*" >&2
    failures=$((failures + 1))
}

# stub <fail-runs> <fail-text>: fails its first <fail-runs> runs printing
# <fail-text>, then prints "published" and succeeds. Runs are counted in
# $tmp/count.
cat > "$tmp/stub.sh" <<'STUB'
#!/usr/bin/env bash
count_file=$1 fail_runs=$2 text=$3
n=$(( $(cat "$count_file" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$count_file"
if (( n <= fail_runs )); then
    echo "$text" >&2
    exit 3
fi
echo published
STUB
chmod +x "$tmp/stub.sh"

# check <case> <expected-status> <expected-runs> <stderr-pattern|-> <retry args...>
check() {
    local case=$1 want_status=$2 want_runs=$3 pattern=$4
    shift 4
    rm -f "$tmp/count"
    local status=0
    "$retry" "$@" > "$tmp/out" 2> "$tmp/err" || status=$?
    local runs
    runs=$(cat "$tmp/count" 2>/dev/null || echo 0)
    [[ $status -eq $want_status ]] || fail "$case: exit $status, expected $want_status"
    [[ $runs -eq $want_runs ]] || fail "$case: $runs runs, expected $want_runs"
    if [[ "$pattern" != - ]] && ! grep -qF -- "$pattern" "$tmp/out" "$tmp/err"; then
        fail "$case: output lacks: $pattern"
    fi
}

limit='error: failed to publish: the remote server responded with an error (status 429 Too Many Requests): You have published too many new crates in a short period of time.'
usage='release-retry: usage: release-retry.sh <attempts> <wait-seconds> -- <command...>'

# T1: success on the first try, output passed through.
check T1 0 1 published 3 0 -- "$tmp/stub.sh" "$tmp/count" 0 ''
# T2/T5: rate limited twice, then published.
check T2 0 3 'release-retry: crates.io rate limit (attempt 2/5); retrying in 0s' \
    5 0 -- "$tmp/stub.sh" "$tmp/count" 2 "$limit"
check T5 0 2 published 5 0 -- "$tmp/stub.sh" "$tmp/count" 1 'Too Many Requests'
check T5-new 0 2 published 5 0 -- "$tmp/stub.sh" "$tmp/count" 1 'too many new crates'
# T3: rate limited on every attempt.
check T3 3 3 'release-retry: still rate limited after 3 attempts' \
    3 0 -- "$tmp/stub.sh" "$tmp/count" 9 "$limit"
# T4: another failure is not retried.
check T4 3 1 'error: verify failed' 5 0 -- "$tmp/stub.sh" "$tmp/count" 9 'error: verify failed'
check T4-line-429 3 1 'src/lib.rs:429:5: error' 5 0 -- "$tmp/stub.sh" "$tmp/count" 9 'src/lib.rs:429:5: error'
# T6: usage errors.
check T6-attempts-0 2 0 "$usage" 0 0 -- true
check T6-attempts-neg 2 0 "$usage" -1 0 -- true
check T6-attempts-x 2 0 "$usage" x 0 -- true
check T6-attempts-empty 2 0 "$usage" '' 0 -- true
check T6-wait-neg 2 0 "$usage" 3 -1 -- true
check T6-wait-x 2 0 "$usage" 3 x -- true
check T6-wait-empty 2 0 "$usage" 3 '' -- true
check T6-no-dashes 2 0 "$usage" 3 0 true
check T6-no-command 2 0 "$usage" 3 0 --
check T6-empty-command 2 0 "$usage" 3 0 -- ''
check T6-no-args 2 0 "$usage"
# T7: a missing command is a plain failure (status 127), never retried.
check T7 127 0 - 3 0 -- "$tmp/no-such-command"

if [[ $failures -gt 0 ]]; then
    echo "$name: $failures failures" >&2
    exit 1
fi
echo "$name: ok"
