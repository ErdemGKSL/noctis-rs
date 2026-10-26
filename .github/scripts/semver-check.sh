#!/usr/bin/env bash
# Runs cargo-semver-checks on the facade with every feature (AD-28, NFR-8;
# Story 14.7). Without arguments it compares against the latest `noctis` on
# crates.io and skips while nothing is published yet; arguments (such as
# `--baseline-rev HEAD`) go to cargo-semver-checks and skip the probe.
# usage: semver-check.sh [cargo-semver-checks args...]
set -euo pipefail

name=semver-check

cd "$(dirname "$0")/../.."

if [[ $# -eq 0 ]]; then
    # The sparse index path of `noctis`: crates of 4+ chars live under
    # <first two>/<next two>/<name>.
    code=$(curl -sS -o /dev/null -w '%{http_code}' https://index.crates.io/no/ct/noctis 2>/dev/null) || true
    case "$code" in
        404)
            echo "$name: noctis is not on crates.io yet (first release); nothing to compare" >&2
            exit 0
            ;;
        200) ;;
        *)
            echo "$name: cannot query the crates.io index (HTTP ${code:-000})" >&2
            exit 1
            ;;
    esac
fi

if ! cargo semver-checks --version > /dev/null 2>&1; then
    echo "$name: cargo-semver-checks is not installed" >&2
    exit 1
fi

exec cargo semver-checks check-release -p noctis --all-features "$@"
