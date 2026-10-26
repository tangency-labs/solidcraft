#!/usr/bin/env bash
# Build and package SolidCraft for macOS:
#
#   $DIST/solidcraft-<version>-macos-<arch>.dmg        SolidCraft.app (with solidcraft-cli inside) on a
#                                                      drag-to-Applications DMG
#   $DIST/solidcraft-cli-<version>-macos-<arch>.zip    the headless CLI (and its solvecraft-cli
#                                                      compatibility alias) with the licence files
#
# Usage: packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build] [--sign]
#        packaging/macos/package.sh --build-only <rust-target>
#
# --build-only builds just that target (aarch64-apple-darwin or x86_64-apple-darwin) and stops,
# so the release workflow can build the two architectures on separate runners; later
# `--skip-build` runs package the binaries they find under $CARGO_TARGET_DIR/<target>/release/:
# joined for `--arch universal`, alone for `--arch aarch64` or `--arch x86_64`.
#
# Signing: with --sign, MACOS_SIGN_IDENTITY (a "Developer ID Application: ..." name or its SHA-1
# hash, found in the keychain) signs the app, the CLI and the DMG with the hardened runtime and a
# secure timestamp. With the App Store Connect API key as well (APPLE_API_KEY_PATH = the .p8 file,
# APPLE_API_KEY_ID, APPLE_API_ISSUER) the app, the DMG and the CLI zip are notarized; the app
# and the DMG are stapled. Without --sign the app is ad-hoc signed, not notarized: first launch needs
# right-click > Open.
# Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
# The same reverse-DNS id as the Linux APP_ID.
MACOS_BUNDLE_ID=ai.storyteller.solidcraft

# sign_code <path> [identifier] [entitlements]: Developer ID with the hardened runtime and a secure
# timestamp, or the ad-hoc signature (Apple silicon refuses unsigned code) without an identity.
sign_code() {
  local path="$1" identifier="${2:-}" entitlements="${3:-}"
  local args=(--force)
  if [ -n "$SIGN_IDENTITY" ]; then
    args+=(--sign "$SIGN_IDENTITY" --options runtime --timestamp)
  else
    args+=(--sign - --timestamp=none)
  fi
  if [ -n "$identifier" ]; then args+=(--identifier "$identifier"); fi
  if [ -n "$entitlements" ] && [ -n "$SIGN_IDENTITY" ]; then args+=(--entitlements "$entitlements"); fi
  codesign "${args[@]}" "$path"
}

# notarize <path>: submit to Apple and wait up to 30 minutes; a rejection or timeout fails the
# build, and a rejection prints Apple's log.
notarize() {
  local out
  # Not fatal here: the status check below is the only failure path, so the log is always fetched.
  out="$(xcrun notarytool submit "$1" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" \
    --issuer "$APPLE_API_ISSUER" --wait --timeout 30m --output-format json || true)"
  echo "$out"
  local id
  id="$(printf '%s' "$out" | /usr/bin/plutil -extract id raw -o - - 2>/dev/null || true)"
  if [ "$(printf '%s' "$out" | /usr/bin/plutil -extract status raw -o - - 2>/dev/null || true)" != Accepted ]; then
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" >&2 || true
    fi
    echo "notarization of $1 was not accepted" >&2
    return 1
  fi
}

# staple <path>: stapling can fail with error 65 for a while after Apple accepts a submission, so
# try a few times.
staple() {
  local i
  for i in 1 2 3 4 5 6; do
    if xcrun stapler staple "$1"; then return 0; fi
    echo "stapler staple failed (attempt $i), retrying" >&2
    sleep "${STAPLE_RETRY_SLEEP:-20}"
  done
  echo "could not staple $1" >&2
  return 1
}

ARCH=universal
SKIP_BUILD=0
BUILD_ONLY=
SIGN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --build-only) BUILD_ONLY="$2"; shift 2 ;;
    --sign) SIGN=1; shift ;;
    -h | --help) sed -n '2,23p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

if [ -n "$BUILD_ONLY" ] && [ "$SKIP_BUILD" = 1 ]; then
  echo "--skip-build and --build-only exclude each other" >&2; exit 2
fi
if [ -n "$BUILD_ONLY" ] && [ "$SIGN" = 1 ]; then
  echo "--build-only builds nothing to sign: drop --sign" >&2; exit 2
fi
SIGN_IDENTITY=
NOTARIZE=0
if [ "$SIGN" = 1 ]; then
  SIGN_IDENTITY="${MACOS_SIGN_IDENTITY:-}"
  if [ -z "$SIGN_IDENTITY" ]; then echo "--sign needs MACOS_SIGN_IDENTITY" >&2; exit 2; fi
  if [ -n "${APPLE_API_KEY_PATH:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
    NOTARIZE=1
  else
    echo "==> Developer ID signing without notarization: the APPLE_API_* settings are incomplete"
  fi
else
  echo "==> no --sign: ad-hoc signing, no notarization"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Whether this Mac can run what it packages: an Intel-only build needs Rosetta on Apple silicon,
# an Apple silicon build doesn't run on Intel. Without it, the checks that start the programs are
# skipped; the rest still run.
RUNS_HERE=1
case "$ARCH:$(uname -m)" in
  x86_64:arm64) arch -x86_64 /usr/bin/true 2>/dev/null || RUNS_HERE=0 ;;
  aarch64:x86_64) RUNS_HERE=0 ;;
esac
if [ "$RUNS_HERE" = 0 ]; then warn "this Mac can't run $ARCH programs: their --version checks are skipped"; fi

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/SolidCraft.app"
DMG="$DIST/solidcraft-$VERSION-macos-$ARCH.dmg"
CLI_ZIP="$DIST/solidcraft-cli-$VERSION-macos-$ARCH.zip"

if [ -n "$BUILD_ONLY" ]; then
  case "$BUILD_ONLY" in
    aarch64-apple-darwin | x86_64-apple-darwin) ;;
    *) echo "unknown --build-only target $BUILD_ONLY" >&2; exit 2 ;;
  esac
  echo "==> SolidCraft $VERSION for macOS ($BUILD_ONLY, build only)"
  (cd "$ROOT" && cargo build --release --locked -p solidcraft -p solidcraft-cli --target "$BUILD_ONLY")
  exit 0
fi

echo "==> SolidCraft $VERSION for macOS ($ARCH)"
if [ "$SKIP_BUILD" = 0 ]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p solidcraft -p solidcraft-cli "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for bin in solidcraft solidcraft-cli solvecraft-cli; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$bin"); done
  lipo -create -output "$WORK/bin/$bin" "${inputs[@]}"
  # -S drops only the debug map: entries naming the runner's object files, which hold the line
  # tables and never ship. Function names stay, so crash backtraces read the same (about 27 MB off
  # the app). It can leave these copies' signatures stale, so only the signed copies below are run.
  strip -S "$WORK/bin/$bin"
  lipo -info "$WORK/bin/$bin"
done

# ---- SolidCraft.app ----------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$WORK/bin/solidcraft" "$APP/Contents/MacOS/SolidCraft"
# solidcraft-cli next to the app, as every other package has it: the agent chat starts it as the
# agent's MCP server (`solidcraft-cli mcp --connect`) and looks for it beside the running program.
cp "$WORK/bin/solidcraft-cli" "$APP/Contents/MacOS/solidcraft-cli"
# Apple-compiled layered icon and older-macOS fallback (regeneration: assets/README.md).
# Keep compiled resources so packaging also works on runners with older Xcode versions.
cp "$ROOT/assets/icons/app/macos/exports/Assets.car" "$APP/Contents/Resources/Assets.car"
cp "$ROOT/assets/icons/app/macos/exports/icon.icns" "$APP/Contents/Resources/SolidCraft.icns"
mkdir -p "$APP/Contents/Resources/Licenses"
copy_docs "$APP/Contents/Resources/Licenses"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" "$ROOT/assets/platforms/macos/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
# Signature over the whole bundle. Nested code is signed first: the bundle signature refuses an
# unsigned helper in Contents/MacOS. The entitlements go on the bundle (they apply to the main
# executable): the 3D mouse driver is loaded by the app, not by the CLI.
sign_code "$APP/Contents/MacOS/solidcraft-cli" "$MACOS_BUNDLE_ID-cli"
sign_code "$APP" "" "$SCRIPT_DIR/entitlements.plist"
codesign --verify --strict --deep --verbose=2 "$APP"
if [ -n "$SIGN_IDENTITY" ]; then
  # The hardened runtime and the library-validation exception must be on the signed app.
  # Match a captured string, not a pipe: grep -q closing the pipe can SIGPIPE codesign under pipefail.
  info="$(codesign --display --verbose=2 "$APP" 2>&1)"
  grep -q 'flags=.*runtime' <<<"$info"
  info="$(codesign --display --entitlements - --xml "$APP" 2>/dev/null)"
  grep -q 'com.apple.security.cs.disable-library-validation' <<<"$info"
fi
test -x "$APP/Contents/MacOS/solidcraft-cli" || { echo "solidcraft-cli is missing from $APP" >&2; exit 1; }
if [ "$NOTARIZE" = 1 ]; then
  # Staple the app itself, so a copy dragged out of the DMG opens offline too.
  echo "==> notarizing $APP"
  ditto -c -k --keepParent "$APP" "$WORK/SolidCraft-app.zip"
  notarize "$WORK/SolidCraft-app.zip"
  staple "$APP"
  xcrun stapler validate "$APP"
fi

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/SolidCraft.app"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/assets/platforms/macos/README.txt" "$STAGE/README.txt"
rm -f "$DMG" "$WORK/raw.dmg"
# makehybrid + convert builds the image without attaching a device, unlike `create -srcfolder`,
# which is flaky on CI runners ("Resource busy").
hdiutil makehybrid -hfs -hfs-volume-name "SolidCraft" -hfs-openfolder "$STAGE" -o "$WORK/raw.dmg" "$STAGE"
hdiutil convert "$WORK/raw.dmg" -format UDZO -imagekey zlib-level=9 -o "$DMG"
rm -f "$WORK/raw.dmg"

# Check the image as a user gets it: mount it and look inside.
MNT="$(mktemp -d)"
hdiutil attach -readonly -nobrowse -mountpoint "$MNT" "$DMG"
ls -la "$MNT" "$MNT/SolidCraft.app/Contents" "$MNT/SolidCraft.app/Contents/MacOS" "$MNT/SolidCraft.app/Contents/Resources"
test -L "$MNT/Applications" && test "$(readlink "$MNT/Applications")" = /Applications
plutil -lint "$MNT/SolidCraft.app/Contents/Info.plist"
# Check the names Finder/LaunchServices actually use, and the bundle identity.
for key in CFBundleName CFBundleDisplayName CFBundleExecutable CFBundleIconFile CFBundleIconName; do
  test "$(/usr/libexec/PlistBuddy -c "Print :$key" "$MNT/SolidCraft.app/Contents/Info.plist")" = SolidCraft
done
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$MNT/SolidCraft.app/Contents/Info.plist")" = "$MACOS_BUNDLE_ID"
test -s "$MNT/SolidCraft.app/Contents/Resources/SolidCraft.icns"
cmp "$ROOT/assets/icons/app/macos/exports/icon.icns" "$MNT/SolidCraft.app/Contents/Resources/SolidCraft.icns"
cmp "$ROOT/assets/icons/app/macos/exports/Assets.car" "$MNT/SolidCraft.app/Contents/Resources/Assets.car"
cmp "$ROOT/assets/platforms/macos/README.txt" "$MNT/README.txt"
case "$ARCH" in universal) WANT=(arm64 x86_64) ;; aarch64) WANT=(arm64) ;; *) WANT=(x86_64) ;; esac
for arch in "${WANT[@]}"; do
  lipo "$MNT/SolidCraft.app/Contents/MacOS/SolidCraft" -verify_arch "$arch"
done
test -s "$MNT/SolidCraft.app/Contents/Resources/Licenses/THIRD-PARTY-LICENSES.txt"
# Not --strict: the mounted HFS volume adds Finder info, which strict verification rejects.
codesign --verify --deep --verbose=2 "$MNT/SolidCraft.app"
if [ "$RUNS_HERE" = 1 ]; then "$MNT/SolidCraft.app/Contents/MacOS/SolidCraft" --version; fi
hdiutil detach "$MNT"

if [ -n "$SIGN_IDENTITY" ]; then
  sign_code "$DMG"
  codesign --verify --verbose=2 "$DMG"
fi
if [ "$NOTARIZE" = 1 ]; then
  echo "==> notarizing $DMG"
  notarize "$DMG"
  staple "$DMG"
  xcrun stapler validate "$DMG"
  # Gatekeeper's verdict on the DMG and on the app as a user gets it.
  spctl -a -t open --context context:primary-signature -vv "$DMG"
  MNT="$(mktemp -d)"
  hdiutil attach -readonly -nobrowse -mountpoint "$MNT" "$DMG"
  spctl --assess --type execute --verbose=4 "$MNT/SolidCraft.app"
  hdiutil detach "$MNT"
fi

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/solidcraft-cli-$VERSION-macos-$ARCH"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/solidcraft-cli" "$CLI_DIR/"
sign_code "$CLI_DIR/solidcraft-cli" "$MACOS_BUNDLE_ID-cli"
# Compatibility shim (docs/compatibility.md): the old CLI name forwards to solidcraft-cli.
cp "$WORK/bin/solvecraft-cli" "$CLI_DIR/"
sign_code "$CLI_DIR/solvecraft-cli" "$MACOS_BUNDLE_ID-cli-alias"
copy_docs "$CLI_DIR"
rm -f "$CLI_ZIP"
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
if [ "$NOTARIZE" = 1 ]; then
  # A zip of bare executables can't be stapled; Gatekeeper finds the ticket online.
  echo "==> notarizing $CLI_ZIP"
  notarize "$CLI_ZIP"
fi

if [ "$RUNS_HERE" = 1 ]; then "$CLI_DIR/solidcraft-cli" --version; fi
echo "==> done"
ls -lh "$DMG" "$CLI_ZIP"
