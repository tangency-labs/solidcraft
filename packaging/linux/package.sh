#!/usr/bin/env bash
# Build and package SolidCraft for Linux (<arch> is x86_64 or aarch64):
#
#   $DIST/solidcraft-<version>-linux-<arch>.AppImage        any distro with glibc >= the build host's
#   $DIST/solidcraft-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/solvecraft-<version>-linux-<arch>.AppImage.zsync  the same, for pre-rename AppImages
#   $DIST/solidcraft-<version>-linux-<arch>.deb             Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/solidcraft-<version>-linux-<arch>.rpm             Fedora, openSUSE, RHEL, ...
#   $DIST/solidcraft-<version>-linux-<arch>.tar.gz          plain FHS-style tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com); appimagetool for the AppImage
# (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu 22.04,
# glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate, appstreamcli,
# zsyncmake (the zsync package) for the AppImage's .zsync.
#
# Cross builds (CI: riscv64; deb, rpm and tar.gz): set CROSS_ARCH (riscv64), CROSS_TARGET (the
# Rust triple), CROSS_COMPILE (binutils prefix, for strip), GLIBC_FLOOR (the build host's glibc,
# which the deb's libc6 dependency declares; 2.35 by default) and optionally EMULATOR (e.g.
# qemu-riscv64) to smoke-test the CLI.
# Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="${CROSS_ARCH:-$(uname -m)}"
case "$ARCH" in
  riscv64) DEB_ARCH=riscv64 ;;
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
# The deb's libc6 floor (nfpm.yaml): the glibc of the host the binaries were built on.
export GLIBC_FLOOR="${GLIBC_FLOOR:-2.35}"
export SOLIDCRAFT_MAINTAINER="${SOLIDCRAFT_MAINTAINER:-${SOLVECRAFT_MAINTAINER:-SolidCraft contributors <https://github.com/bherbruck/solidcraft>}}"  # SOLVECRAFT_*: compatibility shim, see docs/compatibility.md
BASENAME="solidcraft-$VERSION-linux-$ARCH"

echo "==> SolidCraft $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  if [ -n "${CROSS_TARGET:-}" ]; then
    (cd "$ROOT" && cargo build --release --locked -p solidcraft -p solidcraft-cli --target "$CROSS_TARGET")
    BIN="$CARGO_TARGET_DIR/$CROSS_TARGET/release"
  else
    (cd "$ROOT" && cargo build --release --locked -p solidcraft -p solidcraft-cli)
    BIN="$CARGO_TARGET_DIR/release"
  fi
else
  if [ -n "${CROSS_TARGET:-}" ]; then
    BIN="$CARGO_TARGET_DIR/$CROSS_TARGET/release"
  else
    BIN="$CARGO_TARGET_DIR/release"
  fi
fi
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/solidcraft" "$STAGE/usr/bin/solidcraft"
install -Dm755 "$BIN/solidcraft-cli" "$STAGE/usr/bin/solidcraft-cli"
# Compatibility shim (docs/compatibility.md): the old CLI name forwards to solidcraft-cli.
install -Dm755 "$BIN/solvecraft-cli" "$STAGE/usr/bin/solvecraft-cli"
"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/solidcraft" "$STAGE/usr/bin/solidcraft-cli" "$STAGE/usr/bin/solvecraft-cli" 2>/dev/null || true
install -Dm644 "$ROOT/assets/platforms/linux/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$ROOT/assets/platforms/linux/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$SOLIDCRAFT_BUILD_DATE/g" \
  "$ROOT/assets/platforms/linux/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
make_icons "$STAGE/usr/bin/solidcraft-cli" "$WORK/icons"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$WORK/icons/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/solidcraft"
copy_docs "$STAGE/usr/share/doc/solidcraft"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm --------------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src`.
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/SolidCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  mv "$APPDIR/usr/share/doc" "$WORK/doc-unused"
  # AppRun is a script, not a symlink: it installs the .desktop entry and icons into
  # $XDG_DATA_HOME before launching, so Wayland compositors can resolve the window's app_id to
  # our icon.
  install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
  cp "$ROOT/assets/platforms/linux/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$WORK/icons/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  OUT="$DIST/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate and the like fetch only the changed blocks of a newer
  # release through the .zsync published next to each AppImage. `latest` is the newest published
  # release that is not a pre-release.
  REPO="${GITHUB_REPOSITORY:-bherbruck/solidcraft}"
  UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|solidcraft-*-linux-$ARCH.AppImage.zsync"
  # Extract-and-run: works without FUSE (containers, CI). With zsyncmake on the host appimagetool
  # also writes the .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream -u "$UPDATE_INFO" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [ -s "$OUT.zsync" ]; then
    echo "wrote $OUT.zsync"
    # Compatibility shim (docs/compatibility.md): AppImages from before the rename look for
    # `solvecraft-*-linux-$ARCH.AppImage.zsync` in the latest release. The same .zsync under that
    # name (its URL is the new AppImage's file name, relative to the release) updates them to
    # SolidCraft.
    cp "$OUT.zsync" "$DIST/solvecraft-$VERSION-linux-$ARCH.AppImage.zsync"
    echo "wrote $DIST/solvecraft-$VERSION-linux-$ARCH.AppImage.zsync (legacy update alias)"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

if [ -z "${CROSS_TARGET:-}" ]; then
  "$STAGE/usr/bin/solidcraft-cli" --version
elif [ -n "${EMULATOR:-}" ]; then
  "$EMULATOR" "$STAGE/usr/bin/solidcraft-cli" --version
fi
echo "==> done"
ls -lh "$DIST"
