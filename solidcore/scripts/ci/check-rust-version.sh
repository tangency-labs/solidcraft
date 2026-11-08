#!/usr/bin/env bash
# Builds every workspace package that declares a `rust-version` with that Rust release, so
# the declared minimum is one the code actually compiles on. rust-toolchain.toml pins a newer
# release for everything else; this check is the only one that runs an older compiler.
# Packages declaring the same version are checked together (`cargo +<version> check -p ...`).
# Scope is the library and binaries only, on purpose: no `--all-targets`. A rust-version is a
# promise about what a consumer compiles; tests and benches would pull dev-dependencies that
# may need a newer compiler than the package itself.
# Needs rustup (installs each version's minimal profile) and jq.
# Usage: check-rust-version.sh
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
fail=0
# "<version> <package>" for each package with a declared rust-version, grouped by version.
pairs=$(cargo metadata --no-deps --format-version 1 --locked |
  jq -r '.packages[] | select(.rust_version != null) | "\(.rust_version) \(.name)"' | sort -V)
# An empty list would skip the loop and pass with nothing checked; fail instead.
[ -n "$pairs" ] || { echo "::error::no package declares rust-version"; exit 1; }
for version in $(cut -d' ' -f1 <<<"$pairs" | uniq); do
  packages=$(awk -v v="$version" '$1 == v { printf " -p %s", $2 }' <<<"$pairs")
  echo "::group::Rust $version:$packages"
  rustup toolchain install "$version" --profile minimal --no-self-update
  # shellcheck disable=SC2086 # one -p per package
  if ! cargo "+$version" check --locked $packages; then
    echo "::error::A package declares rust-version $version but does not build on it:$packages. Either the code or a dependency needs a newer release (cargo's \"requires rustc\" line above names the dependency). Make the code build on $version, hold that dependency at an older version, or raise the package's rust-version to the oldest release it builds on."
    fail=1
  fi
  echo "::endgroup::"
done
exit $fail
