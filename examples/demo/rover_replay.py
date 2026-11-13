#!/usr/bin/env python3
"""Replay a design's timeline in a running SolidCraft, for a screen recording (record_rover.sh).

    solidcraft --control 7878 &
    python3 examples/demo/rover_replay.py --port 7878 Rover-recorded.sc 

Opens the design, frames the finished model, rolls the timeline back to its start and then
forward in even steps (the History bar's roll-to, as `timeline.roll_to` over the control
channel) while the camera orbits the model at a constant rate; it ends a few seconds after the last step. Prints "start" and "end" lines with epoch times so the recorder can trim.
"""
import argparse
import math
import os
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_mcp import control  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("design")
    ap.add_argument("--port", type=int, default=7878)
    ap.add_argument("--rate", type=float, default=0.0133, help="orbit speed in radians per second of real time")
    ap.add_argument("--turn", type=float, default=4, help="seconds the orbit goes on after the last step")
    a = ap.parse_args()

    def ui(method, params=None):
        r = control(a.port, method, params)
        if not r.get("ok", False):
            raise RuntimeError(f"{method} {params}: {r.get('error')}")
        return r.get("result")

    ui("ui.prefs", {"language": "en", "theme": "dark", "button": "apply"})
    ui("ui.resize", {"width": 1920, "height": 1080})
    ui("ui.home", {"open": False})
    ui("ui.file", {"path": os.path.abspath(a.design)})
    time.sleep(1)
    while ui("ui.inspect").get("file_busy"):
        time.sleep(0.5)
    ui("ui.home", {"open": False})
    n = ui("engine.execute", {"command": "timeline.roll_to", "params": {}})["features"]
    joints = control(a.port, "engine.execute", {"command": "joint.inspect", "params": {}}).get("result", {}).get("joints", [])
    ui("ui.set", {"showSketches": False, "showOrigin": False, "hiddenJoints": [j["id"] for j in joints if "id" in j]})
    ui("ui.view", {"view": "iso"})
    ui("ui.camera", {"fit": True})
    time.sleep(1.5)
    cam = ui("ui.inspect")["camera"]
    yaw0 = cam["yaw"]
    # Keep the finished model in frame all through the turn.
    ui("ui.camera", {"distance": cam["distance"] * 0.82})
    time.sleep(0.5)

    def execute(params):
        r = control(a.port, "engine.execute", {"command": "timeline.roll_to", "params": params})
        if not r.get("ok", False):
            raise RuntimeError(r)

    execute({"position": 0})
    time.sleep(1.0)
    print("start", time.time(), flush=True)
    # The camera orbits at a constant rate for the whole replay (a thread, so it does not wait
    # for the timeline steps): yaw = yaw0 + rate * seconds since the start.
    t0 = time.time()
    stop = threading.Event()

    def orbit():
        while not stop.is_set():
            ui("ui.camera", {"yaw": yaw0 + a.rate * (time.time() - t0)})
            time.sleep(0.03)

    spin = threading.Thread(target=orbit, daemon=True)
    spin.start()
    steps = 90
    for i in range(1, steps + 1):
        pos = round(n * i / steps)
        execute({"position": pos} if pos < n else {})
    time.sleep(a.turn)
    stop.set()
    spin.join()
    print("end", time.time(), flush=True)


if __name__ == "__main__":
    main()
