#!/bin/bash
set -xe

# Copies the native, prefab-src for GameActivity + GameTextInput from the
# upstream, android-games-sdk, including our android-activity integration
# changes.
#
# This code is maintained out-of-tree, based on a fork of Google's AGDK repo, so
# it's more practical to try and upstream changes we make, or to rebase on new
# versions.

if [ $# -ne 1 ]; then
    echo "Usage: $0 <android-games-sdk dir>"
    exit 1
fi

SOURCE_DIR="$1"
TOP_DIR=$(git rev-parse --show-toplevel)
DEST_DIR="$TOP_DIR/android-activity/android-games-sdk"

if [ ! -d "$SOURCE_DIR" ]; then
    echo "Error: Source directory '$SOURCE_DIR' does not exist."
    exit 1
fi

if [ ! -d "$DEST_DIR" ]; then
    echo "Error: expected find destination directory $DEST_DIR"
    exit 1
fi

rm -fr "$DEST_DIR/game-activity"
rm -fr "$DEST_DIR/game-text-input"
rm -fr "$DEST_DIR/src/common"
rm -fr "$DEST_DIR/include/common"

mkdir -p "$DEST_DIR/game-activity"
mkdir -p "$DEST_DIR/game-text-input"
mkdir -p "$DEST_DIR/include/common"
mkdir -p "$DEST_DIR/src/common"

cp -av "$SOURCE_DIR/game-activity/prefab-src" "$DEST_DIR/game-activity"
cp -av "$SOURCE_DIR/game-text-input/prefab-src" "$DEST_DIR/game-text-input"
cp -av "$SOURCE_DIR/include/common/gamesdk_common.h" "$DEST_DIR/include/common"
cp -av "$SOURCE_DIR/src/common/system_utils.h" "$DEST_DIR/src/common"
cp -av "$SOURCE_DIR/src/common/system_utils.cpp" "$DEST_DIR/src/common"

# Remove symlinks so the android-activity crate is easily buildable
# from Git on Windows.

rm "$DEST_DIR/game-activity/prefab-src/modules/game-activity/include/common"
rm -fr "$DEST_DIR/game-activity/prefab-src/modules/game-activity/include/game-text-input"
rm -fr "$DEST_DIR/game-activity/prefab-src/modules/game-activity/src/common"
rm -fr "$DEST_DIR/game-activity/prefab-src/modules/game-activity/src/game-text-input"

rm "$DEST_DIR/game-text-input/prefab-src/modules/game-text-input/include/common"
