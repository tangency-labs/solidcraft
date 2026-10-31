#!/usr/bin/env python3
"""doc_shots.py [--list] [SHOT...]: capture the user guide's screenshots and short clips from the
real app, so every figure can be made again with one command when the UI changes.

    python3 examples/demo/doc_shots.py                       # every shot
    python3 examples/demo/doc_shots.py ui-tour/window        # one shot
    python3 examples/demo/doc_shots.py --list

Each shot starts a fresh SolidCraft (its own settings folder, so your preferences never show) in
an Xvfb screen, drives it over the control channel the way a person would (enclosure_ui_mcp.py's
Demo: the pointer glides to a button and clicks it, values are typed), and writes
docs/book/src/media/<page>/<name>.png (a still, with numbered callouts where the shot asks for
them) or .mp4 (a clip, with the drawn pointer and click ripples of overlay.py, idle stretches cut by
trim_idle.py). Shots are the functions below; the page that shows a shot explains it.

Needs Xvfb, ffmpeg (x11grab) and Pillow, and a release build of the app (`--app`, default
$CARGO_TARGET_DIR/release/solidcraft, else target/release/solidcraft). Every new file needs an
ATTRIBUTION row (attribution/pending/).
"""

import argparse
import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
from enclosure_ui_mcp import Demo  # noqa: E402
from overlay import cursor_sprite, ripple_sprite  # noqa: E402

# The window every shot is taken in: the book column shows it at about 60 %.
SIZE = (1280, 800)
FPS = 30
MEDIA = ROOT / "docs/book/src/media"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
# Callout disc colours (an original blue with a white rim, legible on dark and light).
CALLOUT_FILL = (40, 120, 230)
CALLOUT_RIM = (255, 255, 255)

SHOTS = {}


def shot(name, kind, start="empty"):
    """Register a shot: `kind` is "still" or "clip"; `start` is "empty" (a new design) or
    "sample" (the built-in sample plate)."""

    def register(f):
        SHOTS[name] = (kind, start, f)
        return f

    return register


# -- the app ------------------------------------------------------------------------------------


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class App:
    """A fresh SolidCraft on DISPLAY with its own settings, the control channel on a free port."""

    def __init__(self, exe, display, start, work):
        self.port = free_port()
        self.token = secrets.token_hex(32)
        os.environ["SOLIDCRAFT_CONTROL_TOKEN"] = self.token
        settings = Path(work) / "settings"
        settings.mkdir(parents=True, exist_ok=True)
        env = dict(
            os.environ,
            DISPLAY=display,
            SOLIDCRAFT_SETTINGS_DIR=str(settings),
            XDG_DATA_HOME=str(Path(work) / "data"),
            XDG_CONFIG_HOME=str(Path(work) / "config"),
            XDG_CACHE_HOME=str(Path(work) / "cache"),
            SOLIDCRAFT_RENDERER="cpu",
            LANG="en_US.UTF-8",
        )
        argv = [exe, "--control", str(self.port)] + (["--sample"] if start == "sample" else [])
        self.log = open(Path(work) / "app.log", "w")
        self.p = subprocess.Popen(argv, env=env, stdout=self.log, stderr=subprocess.STDOUT)
        self.wait_ready()

    def wait_ready(self):
        from enclosure_mcp import control

        for _ in range(600):
            if self.p.poll() is not None:
                raise RuntimeError(f"the app exited ({self.p.returncode}); see {self.log.name}")
            try:
                r = control(self.port, "ui.inspect")
                if r.get("ok") and r.get("result", {}).get("viewport"):
                    return
            except OSError:
                pass
            time.sleep(0.1)
        raise RuntimeError("the app did not answer on its control port within 60 s")

    def close(self):
        from enclosure_mcp import control

        try:
            control(self.port, "app.quit", {"force": True})
            self.p.wait(10)
        except Exception:
            self.p.kill()
            self.p.wait()
        self.log.close()


class Shot(Demo):
    """Demo, plus what a doc shot needs: engine commands, the crop and the callouts."""

    def __init__(self, port, log):
        super().__init__(port, None, log, speed=1.0, minimum_caption=0)
        self.crop = None  # (x, y, w, h) in window points; None: the whole window
        self.callouts = []  # [(x, y)] in window points, numbered from 1 in order

    def execute(self, command, params=None):
        self.settle_camera()
        r = self.ui("engine.execute", {"command": command, "params": params or {}})
        self.settle_camera()
        return r

    def handle(self, name):
        """The handle's screen point, or None when it isn't drawn."""
        return next(((h["x"], h["y"]) for h in self.ui("ui.handles") if h["name"] == name), None)

    def prepare(self):
        """The same starting window for every shot: SIZE, the default layout, no start page."""
        self.ui("ui.resize", {"width": SIZE[0], "height": SIZE[1]})
        for _ in range(100):
            vp = self.ui("ui.inspect").get("viewport") or [0, 0, 0, 0]
            if vp[0] + vp[2] >= SIZE[0] - 10:
                break
            time.sleep(0.1)
        if self.ui("ui.home").get("open"):
            self.ui("ui.home", {"open": False})
        self.pos = (SIZE[0] * 0.6, SIZE[1] * 0.55)
        self.settle_camera()


# -- capture ------------------------------------------------------------------------------------


def run_shot(name, exe, display, keep):
    kind, start, f = SHOTS[name]
    work = Path(tempfile.mkdtemp(prefix="doc-shot-"))
    out = MEDIA / f"{name}.{'png' if kind == 'still' else 'mp4'}"
    out.parent.mkdir(parents=True, exist_ok=True)
    app = App(exe, display, start, work)
    try:
        log = open(work / "steps.log", "w")
        d = Shot(app.port, log)
        d.prepare()
        if kind == "still":
            f(d)
            d.settle_camera()
            raw = work / "window.png"
            r = d.ui("ui.screenshot", {"path": str(raw)})
            if r and r.get("hidden"):
                raise RuntimeError("the window was not presented")
            still(raw, out, d.crop, d.callouts)
        else:
            clip(d, f, display, work, out)
    finally:
        app.close()
        if keep:
            print(f"  work files: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)
    print(f"{out.relative_to(ROOT)}: {out.stat().st_size // 1024} KB")


def still(raw, out, crop, callouts):
    from PIL import Image, ImageDraw, ImageFont

    img = Image.open(raw).convert("RGB")
    lo, hi = img.getextrema()[0]
    if hi - lo < 8:
        raise RuntimeError(f"{raw}: the screenshot is blank")
    draw = ImageDraw.Draw(img)
    font = ImageFont.truetype(FONT, 17)
    for i, (x, y) in enumerate(callouts, 1):
        r = 15
        draw.ellipse([x - r - 2, y - r - 2, x + r + 2, y + r + 2], fill=CALLOUT_RIM)
        draw.ellipse([x - r, y - r, x + r, y + r], fill=CALLOUT_FILL)
        draw.text((x, y), str(i), font=font, fill=CALLOUT_RIM, anchor="mm")
    if crop:
        x, y, w, h = crop
        img = img.crop((x, y, x + w, y + h))
    img.save(out, optimize=True)
    # Screen captures are mostly flat colour: a 256-colour palette keeps them sharp at a third of
    # the size. Keep it only when it is smaller.
    q = out.with_suffix(".q.png")
    img.quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(q, optimize=True)
    if q.stat().st_size < out.stat().st_size:
        q.replace(out)
    else:
        q.unlink()


def clip(d, f, display, work, out):
    raw = work / "raw.mp4"
    ff = subprocess.Popen(
        ["ffmpeg", "-loglevel", "info", "-nostats", "-y", "-f", "x11grab", "-draw_mouse", "0", "-video_size", f"{SIZE[0]}x{SIZE[1]}", "-framerate", str(FPS), "-i", display, "-c:v", "libx264", "-preset", "veryfast", "-crf", "12", "-pix_fmt", "yuv444p", str(raw)],
        stderr=open(work / "ffmpeg.log", "w"),
    )
    try:
        time.sleep(1.0)
        d.mark(start=True)
        f(d)
        d.settle_camera()
        time.sleep(0.8)
    finally:
        ff.send_signal(2)
        ff.wait(30)
    log = (work / "ffmpeg.log").read_text()
    start = float(log.split("start: ", 1)[1].split(",", 1)[0])
    composite(work, raw, start, d.crop, out)


def composite(work, raw, start, crop, out):
    """The drawn pointer and click ripples over the recording, cropped, idle cut, H.264."""
    marks = [json.loads(line) for line in open(work / "steps.log") if line.strip()]
    cursor_sprite(str(work / "cursor.png"))
    ripple_sprite(str(work / "ripple.png"))
    pts, clicks = [], []
    for i, m in enumerate(marks):
        t = m["t"] - start
        if "cursor" in m:
            pts.append((t, *m["cursor"]))
            if m.get("click"):
                clicks.append((t, *m["cursor"]))
        elif "drag_start" in m:
            end = next((n for n in marks[i + 1 :] if "drag_end" in n), None)
            if end:
                (x0, y0), (x1, y1), t1 = m["drag_start"], end["drag_end"], end["t"] - start
                n = max(2, int((t1 - t) * FPS))
                pts += [(t + (t1 - t) * k / n, x0 + (x1 - x0) * k / n, y0 + (y1 - y0) * k / n) for k in range(n + 1)]
    pts.sort()
    cmds = [f"{t:.3f} [enter] overlay@cur x {x:.1f}, [enter] overlay@cur y {y:.1f};" for t, x, y in pts]
    cmds += [f"{t:.3f} [enter] overlay@rip x {x - 22:.1f}, [enter] overlay@rip y {y - 22:.1f};" for t, x, y in clicks]
    (work / "cursor.cmd").write_text("\n".join(sorted(cmds, key=lambda c: float(c.split()[0]))) + "\n")
    ripple = "+".join(f"between(t,{t:.3f},{t + 0.28:.3f})" for t, _, _ in clicks) or "0"
    cx, cy = (pts[0][1], pts[0][2]) if pts else (-100, -100)
    x, y, w, h = crop or (0, 0, *SIZE)
    w, h = w // 2 * 2, h // 2 * 2
    graph = f"[0:v][2:v]overlay@rip=x=-100:y=-100:enable='{ripple}':shortest=1[v1];[v1][1:v]overlay@cur=x={cx}:y={cy}:shortest=1,sendcmd=f={work / 'cursor.cmd'},crop={w}:{h}:{x}:{y}"
    if labels(work, marks, start, w, h):
        graph += f",ass={work / 'labels.ass'}"
    graph += "[v]"
    full = work / "full.mp4"
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-i", str(raw), "-loop", "1", "-i", str(work / "cursor.png"), "-loop", "1", "-i", str(work / "ripple.png"), "-filter_complex", graph, "-map", "[v]", "-c:v", "libx264", "-crf", "14", "-pix_fmt", "yuv420p", str(full)],
        check=True,
    )
    subprocess.run([sys.executable, str(HERE / "trim_idle.py"), str(full), str(out), "--crf", "26"], check=True)


def labels(work, marks, start, w, h):
    """The shot's d.caption() labels (which button does what) in the clip's top-left corner,
    each until the next one. False when the shot has none."""
    caps = [m for m in marks if "caption" in m]
    if not caps:
        return False

    def ts(t):
        t = max(t, 0.0)
        return f"{int(t // 3600)}:{int(t % 3600 // 60):02}:{t % 60:05.2f}"

    head = f"""[Script Info]
ScriptType: v4.00+
PlayResX: {w}
PlayResY: {h}

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Label,DejaVu Sans,24,&H00FFFFFF,&H00FFFFFF,&H40000000,&H40000000,1,0,0,0,100,100,0,0,3,8,0,7,24,24,24,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
    end = marks[-1]["t"] - start + 5
    lines = []
    for i, m in enumerate(caps):
        a = m["t"] - start
        b = caps[i + 1]["t"] - start if i + 1 < len(caps) else end
        if m["caption"]:
            lines.append(f"Dialogue: 0,{ts(a)},{ts(b)},Label,,0,0,0,,{m['caption']}")
    (work / "labels.ass").write_text(head + "\n".join(lines) + "\n")
    return True


# -- the shots ----------------------------------------------------------------------------------
# ui-tour.md


@shot("ui-tour/window", "still", start="sample")
def ui_tour_window(d):
    """The window, numbered in the order ui-tour.md describes it: 1 title bar, 2 toolbar,
    3 browser, 4 viewport, 5 axis view, 6 navigation bar, 7 timeline, 8 command search."""
    d.key("Escape")
    d.ui("ui.move", {"x": 5, "y": 400})
    names = {h["name"]: (h["x"], h["y"]) for h in d.ui("ui.handles")}
    vp = d.ui("ui.inspect")["viewport"]
    # Each callout sits in empty space beside what it names.
    want = [
        ("titlebar:assistant", (-46, 0)),
        ("ribbon:add_tab", (36, 0)),
        ("browser:header", (0, 0)),
        (None, (vp[0] + vp[2] * 0.33, vp[1] + vp[3] * 0.8)),
        ("axis_view:-x", (-44, 0)),
        ("nav:slot:orbit", (-40, 0)),
        ("history:play", (0, -36)),
        ("ribbon:search", (-100, 0)),
    ]
    for name, (dx, dy) in want:
        if name is None:
            d.callouts.append((dx, dy))
        elif name in names:
            x, y = names[name]
            d.callouts.append((x + dx, y + dy))
        else:
            raise RuntimeError(f"no handle {name}; the window's handles changed")


@shot("ui-tour/navigate", "clip", start="sample")
def ui_tour_navigate(d):
    """Orbit (right drag), pan (middle drag), zoom (wheel) and back with the axis view's Home."""
    vp = d.ui("ui.inspect")["viewport"]
    # The viewport right of the browser, down to the navigation bar.
    d.crop = (262, vp[1] + 126, SIZE[0] - 262, 590)
    c = (vp[0] + vp[2] * 0.62, vp[1] + vp[3] * 0.5)
    d.glide(c, 0.6)
    d.caption("Right-drag: orbit")
    d.orbit(240, 0, steps=70)
    d.orbit(0, 40, steps=30)
    d.wait(0.6)
    d.caption("Middle-drag: pan")
    d.drag(d.pos, (d.pos[0] - 160, d.pos[1] - 40), button="middle", steps=50)
    d.wait(0.6)
    d.caption("Wheel: zoom at the pointer")
    for delta in (40, 40, 40, -40, -40, -40):
        d.ui("ui.scroll", {"x": d.pos[0], "y": d.pos[1], "delta": delta})
        time.sleep(0.25)
    d.settle_camera()
    d.wait(0.6)
    d.caption("Home: back to the starting view")
    d.click(d.at(handle="axis_view:home"), dur=0.6, settle=0.3)
    d.ui("ui.view", {"view": "fit"})
    d.settle_camera()
    d.wait(1.0)


# -- main ---------------------------------------------------------------------------------------


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("shots", nargs="*", help="shot names, or a page (ui-tour); default every shot")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--app", default=str(Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target") / "release/solidcraft"))
    ap.add_argument("--display", default=":97", help="the Xvfb display to start (or use, with --no-xvfb)")
    ap.add_argument("--no-xvfb", action="store_true", help="use a display that is already running")
    ap.add_argument("--keep", action="store_true", help="keep each shot's work folder")
    a = ap.parse_args()
    if a.list:
        for name, (kind, start, f) in SHOTS.items():
            print(f"{name:32} {kind:5} {start:6} {(f.__doc__ or '').split(chr(10))[0]}")
        return
    names = [n for n in SHOTS if not a.shots or n in a.shots or n.split("/")[0] in a.shots]
    if a.shots and not names:
        sys.exit(f"no such shot: {' '.join(a.shots)} (--list)")
    xvfb = None
    if not a.no_xvfb:
        xvfb = subprocess.Popen(["Xvfb", a.display, "-screen", "0", f"{SIZE[0]}x{SIZE[1]}x24", "-nolisten", "tcp"], stderr=subprocess.DEVNULL)
        time.sleep(1.0)
    try:
        for n in names:
            print(f"{n} ...", file=sys.stderr)
            run_shot(n, a.app, a.display, a.keep)
    finally:
        if xvfb:
            xvfb.terminate()


if __name__ == "__main__":
    main()
