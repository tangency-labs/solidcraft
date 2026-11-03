#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Baseline overlay must first be built by build_setback_executive.sh.
set -eu
baseline=${1:?usage: build_setback_streamed.sh baseline-overlay streamed-overlay fork-source fork-libraries}
stream=${2:?}
fork=${3:?}
libs=${4:?}
flags=(-std=c++17 -O2 -ffp-contract=off -fno-fast-math -fPIC -w
       -DNDEBUG -DNOMINMAX -DSM_GRAPHICS_CALLBACKS=1 -D_GLIBCXX_USE_CXX11_ABI=1
       "-I$stream" "-I$baseline" "-I$fork/source/SMLib/inc" "-I$fork/source/SM_API/inc")
# Fill grew work counters: recompile callers that allocate and delete it as well
# (a sized delete must use the updated structure size).
nice -n 10 c++ "${flags[@]}" -c "$stream/SmBlendSetback.cpp" -o "$stream/SmBlendSetback.o"
nice -n 10 c++ "${flags[@]}" -c "$baseline/SmBlendCornerSetback.cpp" -o "$stream/SmBlendCornerSetback.o"
nice -n 10 c++ "${flags[@]}" -c "$baseline/SmBlendCorner.cpp" -o "$stream/SmBlendCorner.o"
objects=()
for obj in "$baseline"/Sm*.o; do
    case "$obj" in *SmBlendSetback.o|*SmBlendCornerSetback.o|*SmBlendCorner.o) continue;; esac
    objects+=("$obj")
done
nice -n 10 c++ "${flags[@]}" harness/cpp/tests/blend_setback_executive.cpp     "${objects[@]}" "$stream/SmBlendSetback.o" "$stream/SmBlendCornerSetback.o" "$stream/SmBlendCorner.o"     "-L$libs" -lSM_API -lSMLib -lpthread -o "$stream/executive"
nice -n 10 c++ "${flags[@]}" harness/cpp/tests/blend_setback_streamed_counts.cpp     "$stream/SmBlendSetback.o" "-L$libs" -lSM_API -lSMLib -lpthread -o "$stream/counts-gate"
# The row-order gate includes the allowed source to reach its private row provider.
nice -n 10 c++ "${flags[@]}" harness/cpp/tests/blend_setback_streamed_row_order.cpp     "-L$libs" -lSM_API -lSMLib -lpthread -o "$stream/order"
nice -n 10 "$stream/order"
nice -n 10 "$stream/counts-gate"
