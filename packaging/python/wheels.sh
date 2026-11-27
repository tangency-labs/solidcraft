#!/usr/bin/env bash
# Python wheels of the `solidcraft` client (python/, docs/python-api.md), each carrying the
# release's own solidcraft-cli: the bytes SHA256SUMS.txt lists, signed where the release is.
#
#   VERSION=<version> packaging/python/wheels.sh [ARTIFACTS] [OUT]
#
# ARTIFACTS holds the release packages (release.yml downloads them there):
#   solidcraft-<version>-linux-<arch>.tar.gz           -> manylinux_2_<glibc>_<arch>
#   solidcraft-cli-<version>-macos-universal.zip       -> macosx_11_0_universal2
#   solidcraft-<version>-windows-<arch>-portable.zip   -> win_amd64, win32, win_arm64
# and a py3-none-any wheel without a binary, for every other platform (it finds solidcraft-cli on
# PATH or through SOLIDCRAFT_CLI). The bindings are generated from the Linux x86_64 binary, which
# runs here; every binary of one release has the same commands.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IN="$(cd "${1:-artifacts}" && pwd)"
OUT="${2:-$ROOT/dist/python}"
VERSION="${VERSION:?VERSION is the release version, as in Cargo.toml}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$OUT"

xtask() { (cd "$ROOT" && cargo run -q --release -p xtask -- "$@"); }

wheel() { # <platform tag> [binary]
  if [ -n "${2:-}" ]; then
    xtask python wheel --platform "$1" --version "$VERSION" --cli "$2" --out "$OUT"
  else
    xtask python wheel --platform "$1" --version "$VERSION" --out "$OUT"
  fi
}

find_one() { # <dir> <file name>: the one file of that name below dir
  local found
  found="$(find "$1" -type f -name "$2" | head -n 1)"
  [ -n "$found" ] || { echo "wheels.sh: no $2 in $1" >&2; exit 1; }
  printf '%s\n' "$found"
}

# Linux first: its x86_64 binary generates the bindings every wheel carries.
generated=false
for arch in x86_64 aarch64; do
  tarball="$IN/solidcraft-$VERSION-linux-$arch.tar.gz"
  [ -f "$tarball" ] || { echo "wheels.sh: missing $tarball" >&2; exit 1; }
  mkdir -p "$WORK/linux-$arch"
  tar -xzf "$tarball" -C "$WORK/linux-$arch"
  cli="$(find_one "$WORK/linux-$arch" solidcraft-cli)"
  chmod +x "$cli"
  if [ "$arch" = x86_64 ]; then
    xtask python --cli "$cli"
    generated=true
  fi
  # The newest glibc symbol the binary needs sets its manylinux tag (2.35 from the Ubuntu 22.04 build).
  glibc="$(objdump -T "$cli" | grep -o 'GLIBC_2\.[0-9]*' | sort -t. -k2 -n -u | tail -n 1 | cut -d. -f2)"
  [ -n "$glibc" ] || { echo "wheels.sh: no glibc version in $cli" >&2; exit 1; }
  wheel "manylinux_2_${glibc}_${arch}" "$cli"
done
$generated || { echo "wheels.sh: the bindings were not generated" >&2; exit 1; }

zip="$IN/solidcraft-cli-$VERSION-macos-universal.zip"
[ -f "$zip" ] || { echo "wheels.sh: missing $zip" >&2; exit 1; }
mkdir -p "$WORK/macos"
unzip -q "$zip" -d "$WORK/macos"
wheel macosx_11_0_universal2 "$(find_one "$WORK/macos" solidcraft-cli)"

for pair in x64:win_amd64 x86:win32 arm64:win_arm64; do
  arch="${pair%%:*}"
  zip="$IN/solidcraft-$VERSION-windows-$arch-portable.zip"
  [ -f "$zip" ] || { echo "wheels.sh: missing $zip" >&2; exit 1; }
  mkdir -p "$WORK/windows-$arch"
  unzip -q "$zip" -d "$WORK/windows-$arch"
  wheel "${pair#*:}" "$(find_one "$WORK/windows-$arch" solidcraft-cli.exe)"
done

wheel any
ls -l "$OUT"
