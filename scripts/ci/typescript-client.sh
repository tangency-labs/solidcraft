#!/usr/bin/env bash
# The TypeScript client's checks against a built ci-profile solidcraft-cli (ci.yml, Select checks'
# `typescript`): regenerate the bindings from it (a command the generator refuses fails here),
# type-check the package with them, then run its tests, which build the examples into designs,
# reopen them and drive their parameters. Needs bun on PATH.
# Usage: scripts/ci/typescript-client.sh <path to solidcraft-cli>
set -euo pipefail
cli=$(realpath "$1")
cargo xtask typescript --cli "$cli"
cd typescript
bun install --frozen-lockfile
bun run typecheck
SOLIDCRAFT_CLI="$cli" SOLIDCRAFT_NO_DOWNLOAD=1 bun test
