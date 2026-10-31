#!/bin/bash
# record_ui.sh DISPLAY PORT OUTDIR [STORY] [PLAYBACK_RATE]: record enclosure_ui_mcp.py --story STORY (v2, v3,
# pi or pi-0.6, default pi) driving the app on X display DISPLAY (1920x1080, started with --control PORT) to
# OUTDIR/enclosure-demo-STORY.mp4 (H.264) and .gif, with captions and a drawn pointer (the app
# gets synthetic pointer events, so the screen's own pointer never moves). Needs ffmpeg with
# x11grab and libass.
# PLAYBACK_RATE defaults to 1; label accelerated previews with their rate when publishing. Idle stretches
# (nothing on screen changes) are cut by trim_idle.py.
# SOLIDCRAFT_DEMO_NAME overrides the filename stem; PAUSE scales UI pauses, CAPTION is the
# minimum real-time caption duration, and HOLD keeps the live completed model on screen.
# For a readable 4x preview use SOLIDCRAFT_DEMO_CAPTION=4 and SOLIDCRAFT_DEMO_HOLD=8.
# SOLIDCRAFT_DEMO_GIF_WIDTH and SOLIDCRAFT_DEMO_GIF_FPS size the README GIF (default 800 px, 10 fps).
# The README demo: SOLIDCRAFT_DEMO_NAME=demo SOLIDCRAFT_DEMO_PAUSE=0.4 SOLIDCRAFT_DEMO_CAPTION=9
#   SOLIDCRAFT_DEMO_HOLD=10 record_ui.sh :74 7974 OUT pi 9
set -e
DISP=${1:-:0}; PORT=${2:-7878}; OUT=$(cd "${3:-.}" && pwd); STORY=${4:-pi}; RATE=${5:-1}
HERE=$(cd "$(dirname "$0")" && pwd)
CLI=${SOLIDCRAFT_CLI:-solidcraft-cli}
NAME=${SOLIDCRAFT_DEMO_NAME:-enclosure-demo-$STORY}
PAUSE=${SOLIDCRAFT_DEMO_PAUSE:-0.55}
CAPTION=${SOLIDCRAFT_DEMO_CAPTION:-1.4}
HOLD=${SOLIDCRAFT_DEMO_HOLD:-1}
GIF_WIDTH=${SOLIDCRAFT_DEMO_GIF_WIDTH:-800}
GIF_FPS=${SOLIDCRAFT_DEMO_GIF_FPS:-10}
python3 -c 'import math,sys; r=float(sys.argv[1]); assert math.isfinite(r) and 0<r<=16' "$RATE"
# Frame the actual app at capture resolution before the first recorded blank viewport.
python3 - "$HERE" "$PORT" <<'PY'
import sys,time
sys.path.insert(0,sys.argv[1])
from enclosure_mcp import control
r=control(int(sys.argv[2]),"ui.resize",{"width":1920,"height":1080})
if not r.get("ok"): raise RuntimeError(r)
for _ in range(100):
    vp=control(int(sys.argv[2]),"ui.inspect").get("result",{}).get("viewport",[])
    if len(vp)==4 and vp[0]+vp[2]>=1910 and vp[1]+vp[3]>=1000: break
    time.sleep(.1)
else: raise RuntimeError("the capture window did not resize")
PY
# x11grab stamps frames with the wall clock: its "start:" is when the first frame was taken.
ffmpeg -loglevel info -nostats -y -f x11grab -draw_mouse 0 -video_size 1920x1080 -framerate 30 -i "$DISP" \
  -c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p "$OUT/raw.mp4" 2> "$OUT/ffmpeg.log" &
FF=$!
# Finalize the recording even when the modelling script refuses an unhealthy take.
# Waiting lets ffmpeg write the MP4 index so the failed take remains inspectable.
trap 'kill -INT $FF 2>/dev/null || true; wait $FF 2>/dev/null || true' EXIT
sleep 1
python3 "$HERE/enclosure_ui_mcp.py" --port "$PORT" --cli "$CLI" --story "$STORY" --speed "$PAUSE" --minimum-caption "$CAPTION" --log "$OUT/steps.log" --save "$OUT/enclosure-$STORY.sc"
sleep "$HOLD"
kill -INT $FF; wait $FF || true
START=$(grep -o "start: [0-9.]*" "$OUT/ffmpeg.log" | head -1 | cut -d" " -f2)
python3 "$HERE/overlay.py" "$OUT/steps.log" "$START" "$OUT"
read CX CY < "$OUT/cursor.start"
ffmpeg -loglevel error -y -i "$OUT/raw.mp4" -loop 1 -i "$OUT/cursor.png" -loop 1 -i "$OUT/ripple.png" -filter_complex \
  "[0:v]ass=$OUT/captions.ass[v0];[v0][2:v]overlay@rip=x=-100:y=-100:enable='$(cat "$OUT/ripple.enable")':shortest=1[v1];[v1][1:v]overlay@cur=x=$CX:y=$CY:shortest=1,sendcmd=f=$OUT/cursor.cmd[v]" \
  -map "[v]" -c:v libx264 -preset slow -crf 20 -pix_fmt yuv420p -movflags +faststart "$OUT/real-time.mp4"
# Composite first, then omit the marked private proof interval. The burned-in cursor,
# ripples and captions remain attached to their actual frames when playback time closes.
python3 - "$OUT/steps.log" "$START" "$RATE" "$OUT/public.filter" <<'PY'
import json,math,pathlib,sys
log,start,rate,destination=sys.argv[1:]
start,rate=float(start),float(rate)
p=pathlib.Path(log)
if p.stat().st_size>128*1024*1024: raise RuntimeError("pointer log exceeds capture budget")
markers=[]
with p.open() as stream:
    for i,line in enumerate(stream):
        if i>=500000: raise RuntimeError("pointer log exceeds event budget")
        event=json.loads(line)
        for kind in ("private_start","private_end"):
            if event.get(kind): markers.append((kind,float(event["t"])-start))
if markers:
    if len(markers)!=2 or [k for k,_ in markers]!=["private_start","private_end"]:
        raise RuntimeError("expected one ordered private capture interval")
    a,b=(t for _,t in markers)
    if not all(math.isfinite(t) for t in (a,b)) or not 0<=a<b<=86400:
        raise RuntimeError("invalid private capture interval")
    select=f"select='not(between(t,{a:.6f},{b:.6f}))',"
else:
    select="" # Partial rehearsals have no health/save/reopen interval.
pathlib.Path(destination).write_text(select+f"setpts=N/(FRAME_RATE*TB*{rate})")
PY
# Speed the public composite up RATE times and cut the idle stretches (cursor, ripples and captions
# stay aligned with the UI). A take with a private proof interval drops it first.
SRC="$OUT/real-time.mp4"
if grep -q select "$OUT/public.filter"; then
  ffmpeg -loglevel error -y -i "$SRC" -vf "$(sed 's/,setpts.*//' "$OUT/public.filter"),setpts=N/(FRAME_RATE*TB)" -an -c:v libx264 -preset slow -crf 16 -pix_fmt yuv420p "$OUT/public-source.mp4"
  SRC="$OUT/public-source.mp4"
fi
python3 "$HERE/trim_idle.py" "$SRC" "$OUT/$NAME.mp4" --rate "$RATE"
ffmpeg -loglevel error -y -i "$OUT/$NAME.mp4" \
  -vf "fps=$GIF_FPS,scale=$GIF_WIDTH:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
  "$OUT/$NAME.gif"
ls -la "$OUT/$NAME.mp4" "$OUT/$NAME.gif"
