# Sourced by the story-loop scripts: repository root, scratch dir for logs/diffs/reviews.
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
OUT=$ROOT/target/story-loop
mkdir -p "$OUT"
# Never incremental: its cache fills the small cloud disk (maintainer rule).
export CARGO_INCREMENTAL=0
cd "$ROOT" || exit 1
