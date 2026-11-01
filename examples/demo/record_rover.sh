#!/bin/bash
# record_rover.sh DISPLAY PORT DESIGN OUTDIR [PLAYBACK_RATE]: record rover_replay.py replaying DESIGN's
# timeline in the app on X display DISPLAY (1920x1080, started with --control PORT) to
# OUTDIR/rover-build.mp4 (H.264) and .gif, accelerated PLAYBACK_RATE times (default 15) with the idle
# stretches cut (trim_idle.py).
# SOLIDCRAFT_DEMO_GIF_WIDTH and SOLIDCRAFT_DEMO_GIF_FPS size the GIF (default 800 px, 10 fps).
set -e
DISP=${1:-:0}; PORT=${2:-7878}; DESIGN=$3; OUT=$(cd "${4:-.}" && pwd); RATE=${5:-15}
HERE=$(cd "$(dirname "$0")" && pwd)
NAME=${SOLIDCRAFT_DEMO_NAME:-rover-build}
GIF_WIDTH=${SOLIDCRAFT_DEMO_GIF_WIDTH:-800}
GIF_FPS=${SOLIDCRAFT_DEMO_GIF_FPS:-10}
ffmpeg -loglevel info -nostats -y -f x11grab -draw_mouse 0 -video_size 1920x1080 -framerate 30 -i "$DISP" \
  -c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p "$OUT/raw.mp4" 2> "$OUT/ffmpeg.log" &
FF=$!
trap 'kill -INT $FF 2>/dev/null || true; wait $FF 2>/dev/null || true' EXIT
sleep 1
python3 "$HERE/rover_replay.py" --port "$PORT" "$DESIGN" | tee "$OUT/replay.log"
sleep 3
kill -INT $FF; wait $FF || true
FSTART=$(grep -o "start: [0-9.]*" "$OUT/ffmpeg.log" | head -1 | cut -d" " -f2)
A=$(awk '/^start/{print $2}' "$OUT/replay.log"); B=$(awk '/^end/{print $2}' "$OUT/replay.log")
SS=$(python3 -c "print(max(0,$A-$FSTART-0.5))"); TO=$(python3 -c "print($B-$FSTART+1.5)")
ffmpeg -loglevel error -y -ss "$SS" -to "$TO" -i "$OUT/raw.mp4" -an -c:v libx264 -preset veryfast -crf 14 -pix_fmt yuv420p "$OUT/real-time.mp4"
python3 "$HERE/trim_idle.py" "$OUT/real-time.mp4" "$OUT/$NAME.mp4" --rate "$RATE" --crf 23
ffmpeg -loglevel error -y -i "$OUT/$NAME.mp4" \
  -vf "fps=$GIF_FPS,scale=$GIF_WIDTH:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
  "$OUT/$NAME.gif"
ls -la "$OUT/$NAME.mp4" "$OUT/$NAME.gif"
