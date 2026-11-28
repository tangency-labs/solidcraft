"""Scores a design against a real-world task's checks (realworld.py), on SolidCraft itself.

The design arrives as a STEP file (exported from the run's final state), is inserted into a fresh
headless session, and each check runs there: holes from `body_topology`'s cylindrical faces,
empty and full regions as the overlap (`inspect.interference`) between the design and a probe
solid built for the check, the wing's sections as the overlap with the spec's own wing.

    python bench/checker.py <task> <design.step>
    python bench/realworld.py --selftest          # reference builds pass, wrong builds fail
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path
from typing import Any

import solidcraft as sc

sys.path.insert(0, str(Path(__file__).parent))
import realworld  # noqa: E402

AXES = {"X": [1.0, 0.0, 0.0], "Y": [0.0, 1.0, 0.0], "Z": [0.0, 0.0, 1.0]}


class Unverified(Exception):
    """A check the kernel could not evaluate: reported as unverified, not as failed by the design."""


class Checker:
    def __init__(self, d: sc.Session, design_bodies: list[str]) -> None:
        self.d = d
        self.bodies = design_bodies
        self.n = 0
        self._cyl: list[dict] | None = None

    # -- probes ----------------------------------------------------------------------------

    def _name(self) -> str:
        self.n += 1
        return f"__probe{self.n}"

    def probe(self, p: dict) -> str:
        name = self._name()
        if "box" in p:
            lo, hi = p["box"]
            self.d.cmd.solid.box(length=hi[0] - lo[0], width=hi[1] - lo[1], height=hi[2] - lo[2], corner=lo, body_name=name)
        elif "cyl" in p:
            c = p["cyl"]
            self.d.cmd.solid.cylinder(height=c["h"], diameter=c["d"], base=c["base"], axis=c["axis"], body_name=name)
        elif "ring" in p:
            c = p["ring"]
            self.d.cmd.solid.cylinder(height=c["h"], diameter=c["d_out"], base=c["base"], axis=c["axis"], body_name=name)
            self.d.cmd.solid.cylinder(height=c["h"], diameter=c["d_in"], base=c["base"], axis=c["axis"], operation="cut", targets=[name])
        else:
            raise ValueError(f"unknown probe {p}")
        if p.get("rotate_z"):
            self.d.cmd.solid.move(bodies=[name], axis=[0, 0, 1], angle=f"{p['rotate_z']} deg")
        return name

    def volume(self, body: str) -> float:
        for b in self.d.measure(summary=True)["bodies"]:
            if b["name"] == body:
                return float(b.get("volume_mm3") or 0.0)
        return 0.0

    def overlap(self, probe: str, with_bodies: list[str] | None = None) -> float:
        others = with_bodies or self.bodies
        r = self.d.cmd.inspect.interference(bodies=[probe, *others])
        for u in r.get("unchecked", []):
            if probe in (u.get("a"), u.get("b")):
                raise Unverified(f"the kernel could not evaluate this probe: {str(u.get('reason'))[:160]}")
        return sum(i["volume_mm3"] for i in r.get("interferences", []) if probe in (i["a"], i["b"]) and ({i["a"], i["b"]} - {probe}) <= set(others))

    def remove(self, *names: str) -> None:
        self.d.cmd.solid.remove(bodies=list(names))

    # -- checks ----------------------------------------------------------------------------

    def cylinders(self) -> list[dict]:
        if self._cyl is None:
            self._cyl = []
            for b in self.bodies:
                for f in self.d.body_topology(body=b).get("faces", []):
                    if f.get("cylinder"):
                        self._cyl.append(f["cylinder"])
        return self._cyl

    def hole(self, point: list[float], axis: str, dia: float) -> tuple[bool, str]:
        a = AXES[axis]
        best = None
        for c in self.cylinders():
            ax = c["axis"]
            n = math.sqrt(sum(x * x for x in ax)) or 1.0
            ax = [x / n for x in ax]
            if abs(abs(sum(p * q for p, q in zip(ax, a))) - 1) > 1e-3:
                continue
            v = [p - o for p, o in zip(point, c["origin"])]
            t = sum(p * q for p, q in zip(v, ax))
            off = math.sqrt(max(0.0, sum(x * x for x in v) - t * t))
            r_ok = abs(2 * c["radius"] - dia) <= 0.1
            if r_ok and off <= 0.1:
                return True, "ok"
            if best is None or off < best[0]:
                best = (off, 2 * c["radius"])
        return False, f"no {dia} mm hole along {axis} through {point}" + (f" (nearest: {best[1]:.2f} mm, {best[0]:.2f} mm off)" if best else "")

    def region(self, p: dict, want_full: bool) -> tuple[bool, str]:
        name = self.probe(p)
        try:
            vol = self.volume(name)
            ov = self.overlap(name)
        finally:
            self.remove(name)
        frac = ov / vol if vol else 0.0
        if want_full:
            return frac >= 0.98, f"{frac:.1%} material"
        return ov <= max(1.0, 0.01 * vol), f"{ov:.1f} mm³ of material ({frac:.1%})"

    def section(self, y: float, tol: float) -> tuple[bool, str]:
        upper, lower = realworld.wing_section(y)
        x0, x1 = upper[0][0], upper[-1][0]
        inside, outside = [], []
        for f in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9):
            x = x0 + f * (x1 - x0)
            zu, zl = realworld.surface_z(upper, x), realworld.surface_z(lower, x)
            if zu - zl > 2 * tol + 0.5:
                inside.append(realworld.box([x - 0.25, y, zl + tol], [x + 0.25, y + 1, zu - tol]))
            outside.append(realworld.box([x - 0.25, y, zu + tol], [x + 0.25, y + 1, zu + tol + 2]))
            outside.append(realworld.box([x - 0.25, y, zl - tol - 2], [x + 0.25, y + 1, zl - tol]))
        names_in = [self.probe(p) for p in inside]
        names_out = [self.probe(p) for p in outside]
        try:
            v_in = sum(self.volume(n) for n in names_in)
            v_out = sum(self.volume(n) for n in names_out)
            o_in = sum(self.overlap(n) for n in names_in)
            o_out = sum(self.overlap(n) for n in names_out)
        finally:
            self.remove(*names_in, *names_out)
        f_in, f_out = (o_in / v_in if v_in else 0.0), (o_out / v_out if v_out else 1.0)
        return f_in >= 0.98 and f_out <= 0.02, f"inside the spec's surfaces {f_in:.1%} material, outside {f_out:.1%} (±{tol} mm)"


def run_checks(d: sc.Session, checks: list) -> list[dict]:
    m = d.measure(summary=True)["bodies"]
    design = [b["name"] for b in m]
    ck = Checker(d, design)
    out = []
    for c in checks:
        kind = c[0]
        try:
            if kind == "bodies":
                ok, det = len(design) == c[1], f"{len(design)} bodies"
            elif kind in ("bbox_within", "bbox_spans"):
                boxes = [b["bbox"] for b in m if b.get("bbox")]
                lo = [min(b["min"][i] for b in boxes) for i in range(3)] if boxes else [0, 0, 0]
                hi = [max(b["max"][i] for b in boxes) for i in range(3)] if boxes else [0, 0, 0]
                if kind == "bbox_within":
                    ok = bool(boxes) and all(lo[i] >= c[1][0][i] and hi[i] <= c[1][1][i] for i in range(3))
                else:
                    i = "xyz".index(c[1])
                    ok = bool(boxes) and abs(lo[i] - c[2]) <= c[4] and abs(hi[i] - c[3]) <= c[4]
                det = f"bbox {[round(v, 2) for v in lo]}..{[round(v, 2) for v in hi]}"
            elif kind == "volume_max":
                tot = sum(b.get("volume_mm3") or 0 for b in m)
                ok, det = tot <= c[1], f"volume {tot:.0f} mm³ (max {c[1]:.0f})"
            elif kind == "hole":
                ok, det = ck.hole(*c[1:])
            elif kind in ("empty", "full"):
                ok, det = ck.region(c[1], kind == "full")
            elif kind == "section":
                ok, det = ck.section(c[1], c[2])
            else:
                ok, det = False, f"unknown check {kind}"
        except Unverified as e:
            out.append({"check": json.dumps(c, default=str)[:160], "ok": False, "unverified": True, "detail": str(e)})
            continue
        except sc.SolidCraftError as e:
            ok, det = False, f"check failed to run: {str(e)[:200]}"
        out.append({"check": json.dumps(c, default=str)[:160], "ok": ok, "detail": det})
    return out


def check_step(task: str, step: str, allow: list[str] | None = None) -> dict:
    t = realworld.TASKS[task]
    with sc.headless(allow_paths=[str(Path(step).parent), *(allow or [])]) as d:
        try:
            d.cmd.file.insert_step(path=step)
        except sc.SolidCraftError as e:
            return {"pass": False, "score": 0.0, "results": [], "error": str(e)[:300]}
        results = run_checks(d, t["checks"]())
    passed = sum(r["ok"] for r in results)
    return {"pass": passed == len(results), "score": passed / len(results) if results else 0.0, "passed": passed, "total": len(results), "results": results}


if __name__ == "__main__":
    r = check_step(sys.argv[1], sys.argv[2])
    for x in r["results"]:
        print(("PASS " if x["ok"] else "FAIL ") + x["check"] + " -> " + x["detail"])
    print(f"{r.get('passed', 0)}/{r.get('total', 0)} checks", r.get("error", ""))
