#!/usr/bin/env bash
set -euo pipefail
here=$(dirname "$(realpath "$0")")
root=$(realpath "$here/../..")
: "${SDKROOT:?Run nix develop ./packaging/ios before building}"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.4}"
export CC_aarch64_apple_ios="${CC_aarch64_apple_ios:-clang}"
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="${CARGO_TARGET_AARCH64_APPLE_IOS_LINKER:-ld64.lld}"
# Bound memory/disk usage by default. These do not change the CAD feature set.
export CARGO_TARGET_DIR="$root/target"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_OPT_LEVEL=2
python3 "$here/make-icons.py"
cargo rustc --manifest-path "$root/Cargo.toml" -p solidcraft-ios --lib --release --locked --target aarch64-apple-ios -- --print native-static-libs
mkdir -p "$here/Libs"
cp "$root/target/aarch64-apple-ios/release/libsolidcraft_ios.a" "$here/Libs/"
# Keep cwd inside this subshell; xtool resolves Package.swift relative to it.
# SwiftPM must select the SDK's clang, not mkShell's Linux CC/CXX.
(cd "$here" && env -u SDKROOT -u CC -u CXX xtool dev build --configuration release --ipa)
# xtool stores ZIP entries uncompressed. Deflate them without changing any
# bundle bytes, permissions or metadata, to reduce the sideload download.
python3 - "$here/xtool/SolidCraft.ipa" <<'PY'
import pathlib, sys, zipfile
path = pathlib.Path(sys.argv[1])
compressed = path.with_suffix(".compressed.ipa")
with zipfile.ZipFile(path) as source, zipfile.ZipFile(compressed, "w") as destination:
    for entry in source.infolist():
        destination.writestr(entry, source.read(entry), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
compressed.replace(path)
PY
python3 "$here/verify_ipa.py" "$here/xtool/SolidCraft.ipa"
