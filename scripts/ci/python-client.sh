#!/usr/bin/env bash
# The Python client's checks against a built ci-profile solidcraft-cli (ci.yml, Select checks'
# `python`): regenerate the bindings from it (a command or tool name the generator refuses fails
# here), then run the client's tests in a clean environment. pytest comes from
# python/tests/requirements.txt, exact versions with their hashes.
# Usage: scripts/ci/python-client.sh <path to solidcraft-cli>
set -euo pipefail
cli=$(realpath "$1")
cargo xtask python --cli "$cli"
venv="${RUNNER_TEMP:-/tmp}/python-client"
python3 -m venv "$venv"
"$venv/bin/pip" install --quiet --disable-pip-version-check --require-hashes --only-binary :all: -r python/tests/requirements.txt
SOLIDCRAFT_CLI="$cli" PYTHONPATH="$PWD/python/src" "$venv/bin/python" -m pytest -q -p no:cacheprovider python/tests
