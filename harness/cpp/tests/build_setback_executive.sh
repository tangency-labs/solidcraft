#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Build only the setback reference overlay; never touch the shared pinned C++ build.
set -eu
overlay=${1:?usage: build_setback_executive.sh overlay fork-source fork-libraries output}
fork=${2:?}
libs=${3:?}
output=${4:?}
flags=(-std=c++17 -O2 -ffp-contract=off -fno-fast-math -fPIC -w
       -DNDEBUG -DNOMINMAX -DSM_GRAPHICS_CALLBACKS=1 -D_GLIBCXX_USE_CXX11_ABI=1
       "-I$overlay" "-I$fork/source/SMLib/inc" "-I$fork/source/SM_API/inc")
objects=()
# All blend/fillet users of the changed class layouts must use the new headers.
for old in "$fork"/source/SMLib/src/SmBlend*.cpp "$fork"/source/SMLib/src/SmFillet*.cpp            "$fork/source/SMLib/src/SmBrep.cpp" "$fork/source/SMLib/src/SmProceduralBlend.cpp"; do
    name=${old##*/}
    src=$old
    if [ -f "$overlay/$name" ]; then src=$overlay/$name; fi
    obj="$overlay/${name%.cpp}.o"
    nice -n 10 c++ "${flags[@]}" -c "$src" -o "$obj"
    objects+=("$obj")
done
# SmBlendCornerSetback is new in #104 and absent from the old fork source.
nice -n 10 c++ "${flags[@]}" -c "$overlay/SmBlendCornerSetback.cpp" -o "$overlay/SmBlendCornerSetback.o"
objects+=("$overlay/SmBlendCornerSetback.o")
nice -n 10 c++ "${flags[@]}" harness/cpp/tests/blend_setback_executive.cpp     "${objects[@]}" "-L$libs" -lSM_API -lSMLib -lpthread -o "$output"
