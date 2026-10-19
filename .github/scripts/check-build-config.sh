#!/usr/bin/env bash
# Asserts the pinned toolchain and the release-equivalent dev profile (AD-28).
set -euo pipefail

cd "$(dirname "$0")/../.."

status=0

require() {
    local file=$1 line=$2
    if [[ ! -f "$file" ]] || ! grep -qxF -- "$line" "$file"; then
        echo "missing line in $file: $line" >&2
        status=1
    fi
}

require rust-toolchain.toml 'channel = "1.97.0"'
require rust-toolchain.toml 'components = ["rustfmt", "clippy"]'

require .cargo/config.toml '[profile.dev]'
require .cargo/config.toml 'opt-level = 3'
require .cargo/config.toml 'debug = false'
require .cargo/config.toml 'debug-assertions = false'
require .cargo/config.toml 'overflow-checks = false'
require .cargo/config.toml 'incremental = false'

# `.cargo/config.toml` holds only the six required lines (plus blanks and
# comments), so no extra table, dotted key or duplicate key can weaken them.
allowed=(
    '[profile.dev]'
    'opt-level = 3'
    'debug = false'
    'debug-assertions = false'
    'overflow-checks = false'
    'incremental = false'
)
if [[ -f .cargo/config.toml ]]; then
    lineno=0
    declare -A seen=()
    while IFS= read -r line || [[ -n "$line" ]]; do
        lineno=$((lineno + 1))
        [[ "$line" =~ ^[[:space:]]*(#|$) ]] && continue
        ok=0
        for a in "${allowed[@]}"; do
            if [[ "$line" == "$a" ]]; then
                ok=1
                break
            fi
        done
        if [[ $ok -eq 0 || -n "${seen[$line]:-}" ]]; then
            echo "unexpected line in .cargo/config.toml:$lineno: $line" >&2
            status=1
        fi
        seen[$line]=1
    done < .cargo/config.toml
fi

# The root `Cargo.toml` must not override any profile.
if offending=$(grep -nE '^\s*\[+\s*profile|^\s*profile\.' Cargo.toml); then
    while IFS= read -r hit; do
        echo "profile override in Cargo.toml:$hit" >&2
    done <<< "$offending"
    status=1
fi

exit "$status"
