#!/usr/bin/env python3
"""trim_idle.py IN OUT [--rate R] [--crf N] [--hold S]: speed a real-time screen recording up R
times and cut the idle stretches (nothing on screen changes: the pointer is still and the app is waiting).

A run of near-identical frames that would play for more than 0.15 s at the new speed is cut down
to 0.1 s (--hold S keeps S seconds instead, and cuts only runs longer than 1.5 S); the last run
(the held finished model) is kept for 0.5 s, or S if longer. Frames are compared at 320 px
wide, so a moving pointer, a drifting camera or a changing caption all count as activity.
Then every R-th remaining frame is kept at 30 fps and the result is encoded like the other
recordings (H.264 CRF 20, yuv420p, faststart). Pure Python and ffmpeg; no other dependencies."""
import argparse
import json
import subprocess

ap = argparse.ArgumentParser()
ap.add_argument("inp")
ap.add_argument("out")
ap.add_argument("--rate", type=float, default=1.0)
ap.add_argument("--crf", default="20")
ap.add_argument("--threshold", type=int, default=4, help="grey levels a pixel must change by")
ap.add_argument("--hold", type=float, default=0.1, help="seconds of each idle stretch kept, at the new speed")
a = ap.parse_args()
FPS = 30
info = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "stream=width,height", "-of", "json", a.inp]))["streams"][0]
W, H = info["width"], info["height"]
AW = 320
AH = int(AW * H / W) // 2 * 2


def frames(vf, size):
    cmd = ["ffmpeg", "-v", "error", "-i", a.inp] + (["-vf", vf] if vf else []) + ["-f", "rawvideo", "-pix_fmt", "gray" if vf else "rgb24", "-"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    while True:
        b = p.stdout.read(size)
        if len(b) < size:
            break
        yield b


active, prev = [], None
for f in frames(f"fps={FPS},scale={AW}:{AH}", AW * AH):
    active.append(True if prev is None else sum(1 for x, y in zip(f, prev) if abs(x - y) > a.threshold) >= 2)
    prev = f
n = len(active)
longest, keep_run, end_keep = int(max(0.15, a.hold * 1.5) * a.rate * FPS), int(a.hold * a.rate * FPS), int(max(0.5, a.hold) * a.rate * FPS)
keep = [True] * n
i = cut = 0
while i < n:
    if active[i]:
        i += 1
        continue
    j = i
    while j < n and not active[j]:
        j += 1
    k = end_keep if j == n else keep_run
    if j - i > max(longest, k):
        for t in range(i + k, j):
            keep[t] = False
        cut += j - i - k
    i = j
kept = sum(keep)
print(f"{a.inp}: {n / FPS:.1f}s real time, {cut / FPS:.1f}s idle cut, {kept / FPS / a.rate:.1f}s at {a.rate}x")
enc = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-", "-an", "-c:v", "libx264", "-preset", "slow", "-crf", a.crf, "-pix_fmt", "yuv420p", "-movflags", "+faststart", a.out], stdin=subprocess.PIPE)
j = 0
for idx, f in enumerate(frames(None, W * H * 3)):
    if idx < n and keep[idx]:
        if int(j / a.rate) != int((j - 1) / a.rate) or j == 0:
            enc.stdin.write(f)
        j += 1
enc.stdin.close()
enc.wait()
