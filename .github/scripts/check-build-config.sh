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

exit "$status"
