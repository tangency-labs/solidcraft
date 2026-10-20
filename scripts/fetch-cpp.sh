#!/usr/bin/env bash
# Download the prebuilt C++ reference kernel (what harness/cpp/build.sh produces) from
# this repository's GitHub Releases, or build it from source when no prebuilt matches.
#
# Usage: scripts/fetch-cpp.sh [--no-build] [--force] [--jobs N]
#
#   --no-build  exit 3 instead of falling back to harness/cpp/build.sh
#   --force     download again even if the destination already holds this key
#   --jobs N    passed to build.sh when falling back
#
# Destination: $SOLIDCORE_CPP_DIR if set, else <repo>/bin/cpp (the same override
# solidcore-ffi and build.sh honour). Only lib/libSMLib.a, lib/libSM_API.a, ffi.env and
# the licence files are replaced; any CMake build tree already there is left alone.
#
# The release is tag cpp-ref-<key>, asset solidcore-cpp-ref-<key>-linux-x86_64.tar.zst,
# with <key> from scripts/cpp-ref-key.sh, published by .github/workflows/cpp-reference.yml.
# Needs curl, tar, zstd and sha256sum; no GitHub token (the repository is public).
#
# Compatibility: the libraries are built on Ubuntu 24.04 (x86_64, glibc 2.39, GCC 13,
# libstdc++ with the C++11 ABI), the image CI runs on. Linking them needs glibc >= 2.39
# and GCC/libstdc++ >= 13 (Ubuntu 24.04, Debian 13, Fedora 39 or newer). On other hosts
# (macOS, aarch64, older distributions) this script builds from source instead.
#
# You still need upstream/ cloned at the pinned commit: ffi.env points the shims at
# upstream/source/*/inc and difftest reads upstream/TestFiles.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${SOLIDCORE_CPP_DIR:-$ROOT/bin/cpp}"
REPO_URL="${SOLIDCORE_CPP_REF_REPO:-https://github.com/tangency-labs/solidcore}"
NO_BUILD=0
FORCE=0
BUILD_ARGS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) NO_BUILD=1 ;;
    --force) FORCE=1 ;;
    --jobs) BUILD_ARGS+=(--jobs "$2"); shift ;;
    -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

say() { echo "fetch-cpp: $*" >&2; }

fallback() {
  say "$1"
  if [ "$NO_BUILD" = 1 ]; then
    say "--no-build given; not building from source"
    exit 3
  fi
  say "building from source with harness/cpp/build.sh instead"
  # Bash 3.2 (macOS) treats an empty array as unset under set -u.
  exec "$ROOT/harness/cpp/build.sh" ${BUILD_ARGS[@]+"${BUILD_ARGS[@]}"}
}

KEY="$("$ROOT/scripts/cpp-ref-key.sh")"
COMMIT="${UPSTREAM_COMMIT:-$(sed -n 's/^PINNED=//p' "$ROOT/scripts/cpp-ref-key.sh")}"
NAME="solidcore-cpp-ref-$KEY-linux-x86_64.tar.zst"
URL="$REPO_URL/releases/download/cpp-ref-$KEY/$NAME"

if [ -d "$ROOT/upstream/.git" ] || [ -f "$ROOT/upstream/.git" ]; then
  have="$(git -C "$ROOT/upstream" rev-parse HEAD 2>/dev/null || true)"
  if [ -n "$have" ] && [ "$have" != "$COMMIT" ]; then
    say "warning: upstream/ is at $have, the pinned commit is $COMMIT"
  fi
else
  say "warning: upstream/ is missing; clone it at $COMMIT (see README) before building shims"
fi

if [ "$FORCE" = 0 ] && [ -f "$DEST/.cpp-ref-key" ] && [ "$(cat "$DEST/.cpp-ref-key")" = "$KEY" ] \
  && [ -f "$DEST/lib/libSMLib.a" ] && [ -f "$DEST/lib/libSM_API.a" ] && [ -f "$DEST/ffi.env" ]; then
  say "$DEST already holds prebuilt reference $KEY"
  exit 0
fi

# ---- host compatibility (see header) -------------------------------------------------
if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
  fallback "no prebuilt reference for $(uname -s) $(uname -m) (only Linux x86_64)"
fi
glibc="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')"
case "$glibc" in
  2.*) gminor="${glibc#2.}"; gminor="${gminor%%.*}" ;;
  *) gminor=0 ;;
esac
if [ "$gminor" -lt 39 ] 2>/dev/null; then
  fallback "host glibc '${glibc:-unknown}' is older than 2.39 (prebuilt is from Ubuntu 24.04)"
fi
if command -v g++ >/dev/null 2>&1; then
  gxx="$(g++ -dumpfullversion -dumpversion 2>/dev/null || echo 0)"
  if [ "${gxx%%.*}" -lt 13 ] 2>/dev/null; then
    fallback "g++ $gxx is older than 13 (prebuilt needs libstdc++ from GCC >= 13)"
  fi
fi
for tool in curl tar zstd; do
  command -v "$tool" >/dev/null 2>&1 || fallback "'$tool' is not installed"
done
if command -v sha256sum >/dev/null 2>&1; then
  SHA=(sha256sum)
else
  SHA=(shasum -a 256)
fi

# ---- download and verify --------------------------------------------------------------
mkdir -p "$DEST"
TMP="$(mktemp -d "$DEST/.fetch.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

say "downloading $URL"
start=$(date +%s)
code="$(curl -sSL --retry 3 --retry-delay 2 -o "$TMP/$NAME" -w '%{http_code}' "$URL" || true)"
if [ "$code" != 200 ]; then
  fallback "no prebuilt reference for key $KEY (HTTP ${code:-error} for $URL)"
fi
if ! curl -fsSL --retry 3 --retry-delay 2 -o "$TMP/$NAME.sha256" "$URL.sha256"; then
  fallback "could not download $URL.sha256"
fi
want="$(awk '{print $1; exit}' "$TMP/$NAME.sha256")"
got="$("${SHA[@]}" "$TMP/$NAME" | awk '{print $1}')"
if [ -z "$want" ] || [ "$want" != "$got" ]; then
  say "sha256 mismatch for $NAME: expected '$want', got '$got'"
  exit 1
fi

# ---- extract ------------------------------------------------------------------------------
mkdir "$TMP/x"
zstd -dc "$TMP/$NAME" | tar -xmf - -C "$TMP/x"   # -m: fresh mtimes, so cargo sees the change
for f in lib/libSMLib.a lib/libSM_API.a ffi.env; do
  if [ ! -f "$TMP/x/$f" ]; then
    say "archive $NAME has no $f"
    exit 1
  fi
done
mkdir -p "$DEST/lib"
mv -f "$TMP/x/lib/libSMLib.a" "$TMP/x/lib/libSM_API.a" "$DEST/lib/"
for f in ffi.env LICENSE NOTICE BUILDINFO; do
  if [ -f "$TMP/x/$f" ]; then mv -f "$TMP/x/$f" "$DEST/$f"; fi
done
printf '%s\n' "$KEY" > "$DEST/.cpp-ref-key"
end=$(date +%s)
say "prebuilt reference $KEY -> $DEST ($((end - start)) s)"
