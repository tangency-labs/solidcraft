#!/usr/bin/env bash
# The Python wheel of the `solidcraft` client (python/, docs/python-api.md) and the CLI archives it
# downloads (python/src/solidcraft/_download.py): each platform's released solidcraft-cli on its
# own, the same bytes as in the packages (signed where the release is), with the licence files.
#
#   VERSION=<version> packaging/python/wheels.sh [ARTIFACTS] [OUT]
#
# ARTIFACTS holds the release packages (release.yml downloads them there):
#   solidcraft-<version>-linux-<arch>.tar.gz           -> solidcraft-cli-<version>-linux-<arch>.tar.gz
#   solidcraft-<version>-freebsd-x86_64.tar.gz         -> solidcraft-cli-<version>-freebsd-x86_64.tar.gz
#   solidcraft-<version>-windows-<arch>-portable.zip   -> solidcraft-cli-<version>-windows-<arch>.zip
# macOS needs none: solidcraft-cli-<version>-macos-universal.zip is already the notarized CLI alone.
# OUT gets the archives and one py3-none-any wheel, which carries no binary. The bindings are
# generated from the Linux x86_64 binary, which runs here; every binary of one release has the
# same commands.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IN="$(cd "${1:-artifacts}" && pwd)"
OUT="${2:-$ROOT/dist/python}"
VERSION="${VERSION:?VERSION is the release version, as in Cargo.toml}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

xtask() { (cd "$ROOT" && cargo run -q --release -p xtask -- "$@"); }

find_one() { # <dir> <file name>: the one file of that name below dir
  local found
  found="$(find "$1" -type f -name "$2" | head -n 1)"
  [ -n "$found" ] || { echo "wheels.sh: no $2 in $1" >&2; exit 1; }
  printf '%s\n' "$found"
}

# cli_dir <name> <binary>: $WORK/<name>/ with the binary and the licence files, as the macOS CLI zip has.
cli_dir() {
  mkdir -p "$WORK/$1"
  cp "$2" "$WORK/$1/"
  chmod +x "$WORK/$1/$(basename "$2")"
  cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/NOTICE" "$ROOT/THIRD-PARTY-LICENSES.txt" "$WORK/$1/"
}

# Linux first: its x86_64 binary generates the bindings the wheel carries.
generated=false
for pkg in linux-x86_64 linux-aarch64 freebsd-x86_64; do
  tarball="$IN/solidcraft-$VERSION-$pkg.tar.gz"
  [ -f "$tarball" ] || { echo "wheels.sh: missing $tarball" >&2; exit 1; }
  mkdir -p "$WORK/pkg-$pkg"
  tar -xzf "$tarball" -C "$WORK/pkg-$pkg"
  cli="$(find_one "$WORK/pkg-$pkg" solidcraft-cli)"
  chmod +x "$cli"
  if [ "$pkg" = linux-x86_64 ]; then
    xtask python --cli "$cli"
    generated=true
  fi
  name="solidcraft-cli-$VERSION-$pkg"
  cli_dir "$name" "$cli"
  tar -C "$WORK" -czf "$OUT/$name.tar.gz" "$name"
done
$generated || { echo "wheels.sh: the bindings were not generated" >&2; exit 1; }

for arch in x64 x86 arm64; do
  zip="$IN/solidcraft-$VERSION-windows-$arch-portable.zip"
  [ -f "$zip" ] || { echo "wheels.sh: missing $zip" >&2; exit 1; }
  mkdir -p "$WORK/pkg-windows-$arch"
  unzip -q "$zip" -d "$WORK/pkg-windows-$arch"
  name="solidcraft-cli-$VERSION-windows-$arch"
  cli_dir "$name" "$(find_one "$WORK/pkg-windows-$arch" solidcraft-cli.exe)"
  (cd "$WORK" && zip -qr -9 "$OUT/$name.zip" "$name")
done

xtask python wheel --platform any --version "$VERSION" --out "$OUT"
ls -l "$OUT"
