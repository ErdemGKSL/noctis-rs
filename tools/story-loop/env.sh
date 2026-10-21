# Sourced by the story-loop scripts: repository root, scratch dir for logs/diffs/reviews.
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
OUT=$ROOT/target/story-loop
mkdir -p "$OUT"
cd "$ROOT" || exit 1
