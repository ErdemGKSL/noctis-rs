#!/usr/bin/env bash
# Asserts that `noctis` with default features pulls no optional subsystem:
# no image renderer (takumi), no voice driver (Songbird), no prefix commands
# and no test kit (AD-15, AD-30, NFR-7; Story 14.7).
set -euo pipefail

name=check-default-graph

cd "$(dirname "$0")/../.."

# Crate names (or `takumi*` prefixes) the default graph must not contain.
forbidden=(noctis-image noctis-voice noctis-prefix noctis-testkit songbird takumi)

# Prints the crate names of `cargo tree` output, one per line.
names() {
    awk '{ print $1 }' | sort -u
}

default_graph=$(cargo tree -p noctis -e normal --prefix none --locked)
default_names=$(names <<< "$default_graph")

if ! grep -qxF noctis-core <<< "$default_names"; then
    echo "$name: cargo tree output has no noctis-core" >&2
    exit 1
fi

status=0
for crate in "${forbidden[@]}"; do
    if [[ $crate == takumi ]]; then
        hits=$(grep -E '^takumi' <<< "$default_names" || true)
    else
        hits=$(grep -xF "$crate" <<< "$default_names" || true)
    fi
    while IFS= read -r hit; do
        [[ -n "$hit" ]] || continue
        echo "$name: $hit is in noctis's default dependency graph" >&2
        status=1
    done <<< "$hits"
done

# The list is only meaningful while `image` really pulls noctis-image and takumi.
image_names=$(cargo tree -p noctis -e normal --prefix none --locked --features image | names)
if ! grep -qxF noctis-image <<< "$image_names" || ! grep -qE '^takumi' <<< "$image_names"; then
    echo "$name: --features image does not pull noctis-image and takumi; the forbidden list is stale" >&2
    status=1
fi

if [[ $status -eq 0 ]]; then
    echo "$name: ok" >&2
fi
exit "$status"
