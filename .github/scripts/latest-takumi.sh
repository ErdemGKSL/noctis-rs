#!/usr/bin/env bash
# Builds and tests Noctis against the newest takumi and takumi-core
# (Story 10.11, NFR-8, AD-28). Run by main.yml's report-only `latest-takumi`
# job only: it rewrites the exact pins in the root Cargo.toml and updates
# Cargo.lock, so never run it in a working tree you want to keep.
#
# Every check's pass/fail goes to $GITHUB_STEP_SUMMARY (stdout without it);
# the script always exits 0.
set -euo pipefail

summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"

latest() {
  # `cargo search` prints `name = "x.y.z"    # description`.
  cargo search "$1" --limit 1 | sed -n "s/^$1 = \"\([^\"]*\)\".*/\1/p" | head -n 1
}

takumi="$(latest takumi || true)"
takumi_core="$(latest takumi-core || true)"
{
  echo "## Latest takumi"
  echo
  echo "- takumi: \`${takumi:-unknown}\`"
  echo "- takumi-core: \`${takumi_core:-unknown}\`"
  echo
} >>"$summary"
if [[ -z "$takumi" || -z "$takumi_core" ]]; then
  echo "- could not read the newest versions from crates.io" >>"$summary"
  exit 0
fi

sed -i -E \
  -e "s/^takumi = \{ version = \"=[^\"]+\"/takumi = { version = \"=${takumi}\"/" \
  -e "s/^takumi-core = \{ version = \"=[^\"]+\"/takumi-core = { version = \"=${takumi_core}\"/" \
  Cargo.toml
if ! grep -qF "takumi = { version = \"=${takumi}\"" Cargo.toml ||
  ! grep -qF "takumi-core = { version = \"=${takumi_core}\"" Cargo.toml; then
  echo "- FAIL: pin rewrite (Cargo.toml pin lines did not match)" >>"$summary"
  exit 0
fi

run() {
  local label="$1"
  shift
  if "$@"; then
    echo "- pass: \`${label}\`" >>"$summary"
  else
    echo "- FAIL: \`${label}\`" >>"$summary"
  fi
}

run "cargo update -p takumi -p takumi-core" cargo update -p takumi -p takumi-core
run "build noctis-image (render)" cargo build --release -p noctis-image --features render
run "tw-grammar --check" cargo run --release -p xtask -- tw-grammar --check
run "tw_crosscheck" cargo test --release -p noctis-image --features render --test tw_crosscheck
run "reference_card pixel snapshot" \
  cargo test --release -p noctis --features image,testkit --test reference_card

exit 0
