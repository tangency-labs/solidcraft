#!/usr/bin/env python3
"""Exercise desktop startup and modelling in the installed Flatpak, without images.

Uses the app's normal sandbox permissions. The control client runs inside that
sandbox, so this does not grant host network access or save Flatpak overrides.
Each headless backend has private runtime/configuration directories and a bus.
"""
import argparse
import base64
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib

APP_ID = "io.tangencylabs.solidcraft"
MAX_PIXELS = 16_000_000
MAX_PNG_BYTES = 70_000_000
SANDBOX_PROBE = r'''
set -euo pipefail
work=$(mktemp -d)
export XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data" XDG_CACHE_HOME="$work/cache"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
unset WGPU_BACKEND WGPU_BACKENDS
# The control channel's token (docs/control-protocol.md), set here so this client knows it.
SOLIDCRAFT_CONTROL_TOKEN=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
export SOLIDCRAFT_CONTROL_TOKEN
solidcraft --control 18837 >"$work/app.log" 2>&1 &
app=$!
cleanup() {
  kill "$app" 2>/dev/null || true
  wait "$app" 2>/dev/null || true
  cat "$work/app.log" >&2
  rm -rf -- "$work"
}
trap cleanup EXIT
ready=0
for ((attempt=0; attempt<300; attempt++)); do
  if ! kill -0 "$app" 2>/dev/null; then exit 1; fi
  if { exec 3<>/dev/tcp/127.0.0.1/18837; } 2>/dev/null; then ready=1; break; fi
  sleep .1
done
test "$ready" = 1
# The control port opens before the first frame, so ui.inspect has no viewport yet; poll until it does.
inspect="{\"id\":1,\"method\":\"ui.inspect\",\"params\":{},\"token\":\"$SOLIDCRAFT_CONTROL_TOKEN\"}"
deadline=$((SECONDS + 60))
until printf '%s\n' "$inspect" >&3 && IFS= read -r -t 30 reply <&3 && [[ "$reply" =~ \"viewport\":\ *\[ ]]; do
  if ! kill -0 "$app" 2>/dev/null || ((SECONDS >= deadline)); then
    echo "viewport never appeared: ${reply:-no reply}" >&2
    exit 1
  fi
  sleep .25
done
printf '%s\n' "$reply"
for request in '{"id":2,"method":"engine.execute","params":{"command":"solid.box","params":{"length":30,"width":20,"height":10}}}' '{"id":3,"method":"document.inspect","params":{"measure":true}}'; do
  printf '%s\n' "$request" >&3
  IFS= read -r -t 30 reply <&3
  printf '%s\n' "$reply"
done
printf '{"id":4,"method":"ui.screenshot","params":{"path":"%s/frame.png"}}\n' "$work" >&3
IFS= read -r -t 30 reply <&3
printf '%s\n' "$reply"
base64 -w0 "$work/frame.png"
printf '\n'
'''


def validate_replies(stdout):
    replies = [json.loads(line) for line in stdout.splitlines()]
    if len(replies) != 3 or any(reply.get("id") != i or reply.get("ok") is not True
                                for i, reply in enumerate(replies, 1)):
        raise ValueError(f"desktop control failed: {replies}")
    ui = replies[0]["result"]
    viewport = ui["viewport"]
    if (ui["renderer"] not in {"gpu", "cpu"} or len(viewport) != 4
            or not all(math.isfinite(value) for value in viewport)
            or viewport[2] <= 0 or viewport[3] <= 0):
        raise ValueError("desktop did not create a usable viewport")
    document = replies[2]["result"]
    bodies = document["bodies"]
    if (len(bodies) != 1 or len(document["timeline"]) != 1
            or document["timeline"][0]["error"] is not None
            or not math.isclose(bodies[0]["volume_mm3"], 6000, rel_tol=0, abs_tol=1e-7)
            or bodies[0]["bbox"] != {"min": [0, 0, 0], "max": [30, 20, 10]}):
        raise ValueError(f"desktop modelling failed: {document}")
    return ui["renderer"]


def frame_statistics(png):
    """Bounded PNG readback check for the app's non-interlaced RGBA8 output."""
    if len(png) > MAX_PNG_BYTES or not png.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("invalid or oversized presented-frame PNG")
    offset, width, height, ended = 8, 0, 0, False
    compressed = bytearray()
    while offset < len(png):
        if offset + 12 > len(png):
            raise ValueError("truncated PNG chunk")
        length = struct.unpack_from(">I", png, offset)[0]
        kind = png[offset + 4:offset + 8]
        end = offset + 12 + length
        if end > len(png):
            raise ValueError("truncated PNG data")
        data = memoryview(png)[offset + 8:offset + 8 + length]
        crc = zlib.crc32(data, zlib.crc32(kind)) & 0xffffffff
        if crc != struct.unpack_from(">I", png, end - 4)[0]:
            raise ValueError("PNG checksum mismatch")
        if kind == b"IHDR":
            if offset != 8 or length != 13:
                raise ValueError("invalid PNG header")
            width, height, depth, colour, compression, filtering, interlace = struct.unpack(">IIBBBBB", data)
            if (width == 0 or height == 0 or width * height > MAX_PIXELS
                    or (depth, colour, compression, filtering, interlace) != (8, 6, 0, 0, 0)):
                raise ValueError("unsupported PNG dimensions or format")
        elif kind == b"IDAT":
            if not width or len(compressed) + length > MAX_PNG_BYTES:
                raise ValueError("invalid PNG image data")
            compressed.extend(data)
        elif kind == b"IEND":
            if length or end != len(png):
                raise ValueError("invalid PNG end")
            ended = True
            break
        elif not (kind[0] & 32):
            raise ValueError("unsupported critical PNG chunk")
        offset = end
    if not ended or not width or not compressed:
        raise ValueError("incomplete PNG")
    stride = width * 4
    expected = height * (stride + 1)
    inflater = zlib.decompressobj()
    try:
        raw = inflater.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise ValueError("invalid compressed PNG data") from error
    if (len(raw) != expected or not inflater.eof or inflater.unconsumed_tail or inflater.unused_data):
        raise ValueError("PNG decompressed size mismatch")
    previous = bytearray(stride)
    first = None
    nonblack, varied = 0, False
    for y in range(height):
        start = y * (stride + 1)
        method = raw[start]
        if method > 4:
            raise ValueError("unsupported PNG row filter")
        row = bytearray(raw[start + 1:start + 1 + stride])
        if method:
            for i in range(stride):
                left = row[i - 4] if i >= 4 else 0
                above = previous[i]
                corner = previous[i - 4] if i >= 4 else 0
                if method == 1:
                    prediction = left
                elif method == 2:
                    prediction = above
                elif method == 3:
                    prediction = (left + above) // 2
                else:
                    p = left + above - corner
                    a, b, c = abs(p - left), abs(p - above), abs(p - corner)
                    prediction = left if a <= b and a <= c else (above if b <= c else corner)
                row[i] = (row[i] + prediction) & 255
        if first is None:
            first = tuple(row[:3])
        for i in range(0, stride, 4):
            nonblack += row[i + 3] > 8 and (row[i] > 8 or row[i + 1] > 8 or row[i + 2] > 8)
            varied |= row[i] != first[0] or row[i + 1] != first[1] or row[i + 2] != first[2]
        previous = row
    if not varied or nonblack <= width * height // 10:
        raise ValueError(f"blank presented frame: {nonblack}/{width * height} nonblack pixels, varied={varied}")
    return {"width": width, "height": height, "nonblack": nonblack, "pixels": width * height}


def probe(gl_saved_four=False):
    shell = SANDBOX_PROBE
    if gl_saved_four:
        shell = shell.replace('solidcraft --control',
                              'mkdir -p "$XDG_DATA_HOME/solidcraft"\n'
                              'printf \'{"msaa":4}\' >"$XDG_DATA_HOME/solidcraft/graphics.json"\n'
                              'export WGPU_BACKEND=gl\nsolidcraft --control')
    result = subprocess.run(["flatpak", "run", "--user", "--command=bash", APP_ID, "-c", shell],
                            text=True, capture_output=True, timeout=125)
    if result.returncode:
        raise RuntimeError(f"Flatpak desktop exited {result.returncode}\n{result.stderr[-8000:]}")
    lines = result.stdout.splitlines()
    if len(lines) != 5 or len(lines[4]) > ((MAX_PNG_BYTES + 2) // 3) * 4:
        raise ValueError("invalid presented-frame response")
    renderer = validate_replies("\n".join(lines[:3]))
    capture = json.loads(lines[3])
    if capture.get("id") != 4 or capture.get("ok") is not True or renderer != "gpu":
        raise ValueError("GPU presented-frame capture failed")
    statistics = frame_statistics(base64.b64decode(lines[4], validate=True))
    if (capture["result"]["width"], capture["result"]["height"]) != (statistics["width"], statistics["height"]):
        raise ValueError("presented-frame dimensions disagree")
    if gl_saved_four and "SolidCraft graphics: Gl, MSAA 1x (requested 4x)" not in result.stderr:
        raise ValueError("forced GL did not apply the safe sample count")
    mode = "GL saved 4x" if gl_saved_four else "default backend"
    print(f"Flatpak desktop PASS: {mode}, renderer={renderer}, box=6000 mm³, frame={statistics}")


def run_backend(backend):
    tools = ["flatpak", "dbus-run-session", "xvfb-run"] if backend == "x11" else ["flatpak", "dbus-run-session", "weston"]
    for tool in tools:
        if not shutil.which(tool):
            raise RuntimeError(f"{tool} missing: install flatpak, dbus-x11, xvfb, xauth and weston")
    with tempfile.TemporaryDirectory(prefix="solidcraft-flatpak-smoke-") as temporary:
        runtime = Path(temporary) / "runtime"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime))
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("WAYLAND_SOCKET", None)
        env.pop("DISPLAY", None)
        env.pop("SOLIDCRAFT_WAYLAND", None)
        env.pop("SOLVECRAFT_WAYLAND", None)  # the legacy name is still read
        command = ["dbus-run-session", "--"]
        compositor = None
        with (Path(temporary) / "weston.log").open("wb") as log:
            try:
                if backend == "x11":
                    command += ["xvfb-run", "-a", "-s", "-screen 0 1600x1000x24 -nolisten tcp"]
                else:
                    env["WAYLAND_DISPLAY"] = "solidcraft-smoke"
                    compositor = subprocess.Popen(["weston", "--backend=headless", "--renderer=pixman", "--no-config",
                                                   "--socket=solidcraft-smoke", "--width=1600", "--height=1000"],
                                                  env=env, stdout=log, stderr=subprocess.STDOUT)
                    deadline = time.monotonic() + 20
                    while not (runtime / "solidcraft-smoke").exists():
                        if compositor.poll() is not None or time.monotonic() >= deadline:
                            raise RuntimeError("headless Weston failed: " + (Path(temporary) / "weston.log").read_text())
                        time.sleep(.1)
                command += [sys.executable, str(Path(__file__).resolve()), "--probe"]
                subprocess.run(command, env=env, check=True, timeout=150)
                if backend == "wayland":
                    subprocess.run(command + ["--gl-saved-four"], env=env, check=True, timeout=150)
                print(f"{backend}: default sandbox desktop startup and modelling PASS")
            finally:
                if compositor:
                    compositor.terminate()
                    try:
                        compositor.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        compositor.kill()
                        compositor.wait()
                # The isolated document portal can leave a disconnected FUSE
                # mount after its private session bus exits. Unmount only our
                # temporary runtime's portal before deleting that directory.
                if shutil.which("fusermount3"):
                    subprocess.run(["fusermount3", "-u", str(runtime / "doc")],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=["x11", "wayland", "both"], default="both")
    parser.add_argument("--probe", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--gl-saved-four", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.probe:
        probe(args.gl_saved_four)
    else:
        for backend in (["x11", "wayland"] if args.backend == "both" else [args.backend]):
            run_backend(backend)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Flatpak desktop smoke failed: {error}", file=sys.stderr)
        sys.exit(1)
