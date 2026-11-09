#!/usr/bin/env python3
"""Start the installed SolidCraft snap on headless X11 and Wayland and model a box in it.

Runs the confined app (`snap install --dangerous` first) with its normal plugs, drives it over
the control channel from the host (snaps share the host's network namespace), checks a
30x20x10 box measures 6000 mm³ and that the GPU drew a non-blank frame, and copies each frame
to --out (default: the current directory) as snap-<backend>.png. Needs xvfb-run and weston.
Reuses the frame checks of flatpak-smoke.py.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import secrets
import tempfile
import time

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("flatpak_smoke", HERE / "flatpak-smoke.py")
flatpak_smoke = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(flatpak_smoke)

PORTS = {"x11": 18839, "wayland": 18840}
# The control channel's token (docs/control-protocol.md), given to the app so this client knows it.
TOKEN = secrets.token_hex(32)
REQUESTS = [
    {"id": 1, "method": "ui.inspect", "params": {}, "token": TOKEN},
    {"id": 2, "method": "engine.execute",
     "params": {"command": "solid.box", "params": {"length": 30, "width": 20, "height": 10}}},
    {"id": 3, "method": "document.inspect", "params": {"measure": True}},
]


def drive(app, port, frame):
    """Connect to the app's control port, model the box and capture a frame."""
    deadline = time.monotonic() + 60
    while True:
        if app.poll() is not None:
            raise RuntimeError(f"the app exited with {app.returncode} before listening")
        try:
            conn = socket.create_connection(("127.0.0.1", port), timeout=60)
            break
        except OSError:
            if time.monotonic() >= deadline:
                raise RuntimeError("the control port never opened") from None
            time.sleep(.2)
    with conn, conn.makefile("rw") as io:
        # Listening precedes the first frame, whose ui.inspect still has viewport:null.
        deadline = time.monotonic() + 60
        while True:
            if app.poll() is not None:
                raise RuntimeError(f"the app exited with {app.returncode} before creating a viewport")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RuntimeError("the viewport never appeared")
            conn.settimeout(remaining)
            io.write(json.dumps(REQUESTS[0]) + "\n")
            io.flush()
            try:
                line = io.readline()
            except TimeoutError as error:
                raise RuntimeError("the viewport never appeared before control timed out") from error
            if not line:
                raise RuntimeError("the control connection closed before creating a viewport")
            reply = json.loads(line)
            if not isinstance(reply, dict) or reply.get("id") != 1 or reply.get("ok") is not True:
                raise ValueError(f"ui.inspect failed: {reply}")
            ui = reply["result"]
            if ui["renderer"] not in {"gpu", "cpu"}:
                raise ValueError("ui.inspect returned an invalid renderer")
            viewport = ui["viewport"]
            if viewport is not None:
                if (not isinstance(viewport, list) or len(viewport) != 4
                        or not all(isinstance(value, (int, float)) and math.isfinite(value) for value in viewport)
                        or viewport[2] <= 0 or viewport[3] <= 0):
                    raise ValueError("ui.inspect returned an unusable viewport")
                break
            time.sleep(.25)
        conn.settimeout(60)
        replies = [line]
        for request in REQUESTS[1:] + [{"id": 4, "method": "ui.screenshot", "params": {"path": str(frame)}},
                                      {"id": 5, "method": "app.quit", "params": {"force": True}}]:
            io.write(json.dumps(request) + "\n")
            io.flush()
            replies.append(io.readline())
    renderer = flatpak_smoke.validate_replies("".join(replies[:3]))
    capture = json.loads(replies[3])
    if any(json.loads(reply).get("ok") is not True for reply in replies[4:]):
        raise ValueError(f"app.quit failed: {replies[4:]}")
    if capture.get("ok") is not True or renderer != "gpu":
        raise ValueError(f"GPU frame capture failed: renderer={renderer}, {capture}")
    return renderer, flatpak_smoke.frame_statistics(frame.read_bytes())


def run_backend(backend, out):
    runtime = Path(f"/run/user/{os.getuid()}")
    if not runtime.is_dir():
        raise RuntimeError(f"{runtime} is missing: snaps expect the login session's runtime directory")
    env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), SOLIDCRAFT_CONTROL_TOKEN=TOKEN)
    for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY", "SOLIDCRAFT_WAYLAND", "SOLVECRAFT_WAYLAND"):
        env.pop(name, None)
    # The snap can write to the (non-hidden) home folder, not to the host's /tmp.
    work = Path(tempfile.mkdtemp(prefix="solidcraft-snap-smoke-", dir=Path.home()))
    compositor = None
    log = (work / "app.log").open("wb")
    try:
        # No private dbus-run-session bus: snapd moves the app into its own systemd scope through
        # the session's systemd, which a private bus doesn't have.
        prefix = []
        if backend == "x11":
            prefix += ["xvfb-run", "-a", "-s", "-screen 0 1600x1000x24 -nolisten tcp"]
        else:
            # The wayland plug only allows sockets named wayland-N in the runtime directory.
            env["WAYLAND_DISPLAY"] = "wayland-77"
            compositor = subprocess.Popen(["weston", "--backend=headless", "--renderer=pixman", "--no-config",
                                           "--socket=wayland-77", "--width=1600", "--height=1000"],
                                          env=env, stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 20
            while not (runtime / "wayland-77").exists():
                if compositor.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError("headless Weston failed")
                time.sleep(.1)
        with socket.socket() as probe:
            if probe.connect_ex(("127.0.0.1", PORTS[backend])) == 0:
                raise RuntimeError(f"port {PORTS[backend]} is already in use (another SolidCraft?)")
        # Its own process group, so stopping it also stops the app under xvfb-run.
        app = subprocess.Popen(prefix + ["/snap/bin/solidcraft", "--control", str(PORTS[backend])],
                               env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            renderer, stats = drive(app, PORTS[backend], work / "frame.png")
            # app.quit exits cleanly, so the next run doesn't report a crash.
            app.wait(timeout=30)
        finally:
            if app.poll() is None:
                os.killpg(app.pid, signal.SIGTERM)
            try:
                app.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(app.pid, signal.SIGKILL)
                app.wait()
        shutil.copy(work / "frame.png", out / f"snap-{backend}.png")
        print(f"snap {backend}: PASS renderer={renderer}, box=6000 mm³, frame={stats}")
    except BaseException:
        log.flush()
        sys.stderr.write((work / "app.log").read_text(errors="replace")[-8000:])
        raise
    finally:
        log.close()
        if compositor:
            compositor.terminate()
            compositor.wait(timeout=10)
        shutil.rmtree(work, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=["x11", "wayland", "both"], default="both")
    parser.add_argument("--out", type=Path, default=Path.cwd())
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    for backend in (["x11", "wayland"] if args.backend == "both" else [args.backend]):
        run_backend(backend, args.out.resolve())


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"snap desktop smoke failed: {error}", file=sys.stderr)
        sys.exit(1)
