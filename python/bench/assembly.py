"""Open-ended assembly tasks and the facts the checker records about each design.

The prompt is a request as a person would make it ("design a full quadruped robot assembly using
high torque hobby servos, including mounting points for a Raspberry Pi 5"), with no dimensions or
part numbers: researching the parts, sizing the design and documenting it is the task.

Nothing about the result is fixed in advance, so the checker records facts instead of comparing
with an answer. Public interfaces are found wherever the model put them:
- a Raspberry Pi 5 mount: four holes of M2.5 size on the board's 58 x 49 mm rectangle;
- standard-size servo mounts: four holes on the 49.5 x 10 mm tab pattern;
- the parts, the bill of materials (`assembly.bom`), interference between parts, and which kinds
  of component the design names (servos, battery, regulator, fasteners, wiring).
These facts feed the core checklist below and the AI judge (judge.py), which scores the design on
a rubric from its renders, its facts and the model's own summary.

    python bench/assembly.py <design.sc>
"""

from __future__ import annotations

import json
import math
import re
import sys
from pathlib import Path

import solidcraft as sc

TASKS = {
    "quadruped": {
        "prompt": "Design a full quadruped robot assembly using high torque hobby servos, including mounting points for a Raspberry Pi 5.",
    },
    # Research: a real, current servo whose case is not the standard 40 x 20 mm and which runs on
    # 12 V; a model designing from memory gets its size and its power supply wrong. The case size
    # is from public listings (45.22 x 24.72 x 35 mm, two sellers and a servo database agree); no
    # public drawing of its mounting holes was found, so only the case is checked.
    "quadruped_sts3250": {
        "prompt": "Design a full quadruped robot assembly using Feetech STS3250 serial bus servos, including mounting points for a Raspberry Pi 5.",
        "servo_case": (24.72, 35.0, 45.22),
    },
    # Finishing: the first prompt with the bar stated, to see whether wiring, fasteners and a
    # full bill of materials appear when asked for.
    "quadruped_buildready": {
        "prompt": "Design a full quadruped robot assembly using high torque hobby servos, including mounting points for a Raspberry Pi 5. "
        "It should be ready to build: everything to buy in a bill of materials, wiring routed and secured, and fasteners modelled.",
    },
}

KINDS = {
    "servo": r"\bservos?\b",
    "compute": r"raspberry|\brpi\b|\bpi ?5\b|controller board|\bsbc\b",
    "battery": r"\bbatter(y|ies)\b|\blipo\b|li-?ion|\b18650\b|\b21700\b|\b[23]s\b",
    "regulator": r"regulat|\bbuck\b|\bu?bec\b|step.?down|dc.?dc|power (board|distribution)|\bpdb\b",
    "fastener": r"\bscrews?\b|\bbolts?\b|\bnuts?\b|\bm[2-5](x\d+)?\b|standoff|heat.?set|\binserts?\b|\bwashers?\b",
    "wiring": r"\bwir(e|es|ing)\b|\bcables?\b|harness|\bclips?\b|zip.?ties?|cable.?ties?|\bloom\b|connector|wire (channel|guide)",
    "leg": r"\blegs?\b|femur|tibia|coxa|\bhips?\b|\bknees?\b|\bshin\b|\bthigh\b|\bfoot\b|\bfeet\b",
    "servo horn": r"\bhorns?\b|\bbrackets?\b",
}


def _strings(v) -> list[str]:
    """Every string value in a reply (never its keys)."""
    if isinstance(v, str):
        return [v]
    if isinstance(v, dict):
        return [s for x in v.values() for s in _strings(x)]
    if isinstance(v, list):
        return [s for x in v for s in _strings(x)]
    return []


def _unit(v):
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / n for x in v]


def _rect(holes: list[dict], a_len: float, b_len: float, tol: float) -> list[list[dict]]:
    """Groups of four parallel holes whose centres form an a_len x b_len rectangle (± tol), in any
    orientation and on any body."""
    groups: list[list[dict]] = []
    for h in holes:
        for g in groups:
            if abs(abs(sum(p * q for p, q in zip(g[0]["axis"], h["axis"]))) - 1) < 1e-3:
                g.append(h)
                break
        else:
            groups.append([h])
    found: list[list[dict]] = []
    seen: set = set()
    for g in groups:
        if len(g) < 4 or len(g) > 400:
            continue
        ax = g[0]["axis"]
        pts = []
        for h in g:  # each centre flattened onto a plane across the axis
            t = sum(a * b for a, b in zip(h["origin"], ax))
            pts.append([a - t * b for a, b in zip(h["origin"], ax)])
        n = len(pts)
        for i in range(n):
            for j in range(n):
                if j == i or abs(math.dist(pts[i], pts[j]) - a_len) > tol:
                    continue
                for k in range(n):
                    if k in (i, j) or abs(math.dist(pts[i], pts[k]) - b_len) > tol:
                        continue
                    u = [x - y for x, y in zip(pts[j], pts[i])]
                    v = [x - y for x, y in zip(pts[k], pts[i])]
                    if abs(sum(p * q for p, q in zip(u, v))) > tol * max(a_len, b_len):
                        continue
                    corner = [pts[j][m] + pts[k][m] - pts[i][m] for m in range(3)]
                    for m in range(n):
                        if m not in (i, j, k) and math.dist(pts[m], corner) <= tol:
                            # one rectangle per four hole positions (a hole is often two faces)
                            key = tuple(sorted(tuple(round(c, 1) for c in pts[q]) for q in (i, j, k, m)))
                            if key not in seen:
                                seen.add(key)
                                found.append([g[q] for q in (i, j, k, m)])
    return found


def facts(design: str, task: str = "quadruped") -> dict:
    out: dict = {}
    with sc.headless(design=design, allow_paths=[str(Path(design).parent)]) as d:
        bodies = d.measure(summary=True).get("bodies", [])
        out["bodies"] = len(bodies)
        out["volume_mm3"] = round(sum(b.get("volume_mm3") or 0 for b in bodies), 1)
        boxes = [b["bbox"] for b in bodies if b.get("bbox")]
        if boxes:
            lo = [min(b["min"][i] for b in boxes) for i in range(3)]
            hi = [max(b["max"][i] for b in boxes) for i in range(3)]
            out["size_mm"] = [round(hi[i] - lo[i], 1) for i in range(3)]
        try:
            comps = d.cmd.component.list(compact=True, bodies=True).get("components", [])
        except sc.SolidCraftError:
            comps = []
        parts = [c for c in comps if c.get("parent") is not None]
        out["parts"] = len(parts)
        names = [b["name"] for b in bodies] + [c["name"] for c in parts]
        try:
            bom = d.call("assembly.bom")
            items = bom.get("items") or bom.get("rows") or bom.get("parts") or []
        except (sc.SolidCraftError, AttributeError):
            items = []
        out["bom_items"] = len(items)
        bom_text = " ".join(_strings(items)).lower()
        text = (" ".join(names) + " " + bom_text).lower()
        out["kinds"] = {k: bool(re.search(p, text)) for k, p in KINDS.items()}
        out["names_sample"] = names[:40]
        # Servo-sized bodies: a standard servo case is about 40 x 20 x 37-43 mm.
        servo_like = 0
        for b in bodies:
            if b.get("bbox"):
                s = sorted(b["bbox"]["max"][i] - b["bbox"]["min"][i] for i in range(3))
                if 17 <= s[0] <= 23 and 35 <= s[1] <= 46 and 36 <= s[2] <= 62:
                    servo_like += 1
        out["servo_sized_bodies"] = servo_like
        case = TASKS.get(task, {}).get("servo_case")
        if case:  # bodies at the named servo's datasheet case size (±2 mm on each side)
            out["named_servo_bodies"] = sum(
                1
                for b in bodies
                if b.get("bbox") and all(abs(a - c) <= 2.0 for a, c in zip(sorted(b["bbox"]["max"][i] - b["bbox"]["min"][i] for i in range(3)), case))
            )
        # Holes, for the public mounting patterns.
        holes = []
        for b in bodies[:300]:
            try:
                for f in d.body_topology(body=b["name"]).get("faces", []):
                    c = f.get("cylinder")
                    if c:
                        holes.append({"body": b["name"], "origin": c["origin"], "axis": _unit(c["axis"]), "d": 2 * c["radius"]})
            except sc.SolidCraftError:
                pass
        out["cylindrical_faces"] = len(holes)
        m25 = [h for h in holes if 2.0 <= h["d"] <= 3.4]
        pi = _rect(m25, 58.0, 49.0, 0.5)
        out["rpi5_mount"] = bool(pi)
        out["rpi5_mount_on"] = sorted({h["body"] for h in pi[0]}) if pi else []
        tabs = [h for h in holes if 2.0 <= h["d"] <= 5.5]
        servo = _rect(tabs, 49.5, 10.0, 1.0)
        out["servo_tab_patterns"] = len(servo)
        try:
            r = d.cmd.inspect.interference(budget_s=180)
            # Overlaps between different parts only: bodies of one part may overlap by design.
            ints = [i for i in r.get("interferences", []) if i.get("volume_mm3", 0) > 1.0 and (i.get("a_part") or {}).get("path") != (i.get("b_part") or {}).get("path")]
            out["interferences"] = len(ints)
            out["interference_mm3"] = round(sum(i["volume_mm3"] for i in ints), 1)
            out["interference_unchecked"] = len(r.get("unchecked", []))
        except sc.SolidCraftError as e:
            out["interferences"] = None
            out["interference_error"] = str(e)[:200]
    k = out["kinds"]
    out["core"] = {
        "an assembly (8+ parts)": out["parts"] >= 8,
        "Raspberry Pi 5 mount (58 x 49 mm, M2.5)": out["rpi5_mount"],
        "8+ servos mounted or modelled": max(out["servo_tab_patterns"], out["servo_sized_bodies"], out.get("named_servo_bodies", 0)) >= 8,
        "battery": k["battery"],
        "voltage regulation": k["regulator"],
        "fasteners modelled": k["fastener"],
        "wiring routed or secured": k["wiring"],
        "bill of materials": out["bom_items"] > 0,
        "parts do not interfere (< 100 mm³)": out.get("interferences") is not None and out.get("interference_mm3", 0) < 100,
    }
    if "named_servo_bodies" in out:
        out["core"]["8+ servos at the named servo's case size"] = out["named_servo_bodies"] >= 8
    out["core_score"] = round(sum(out["core"].values()) / len(out["core"]), 3)
    return out


if __name__ == "__main__":
    print(json.dumps(facts(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else "quadruped"), indent=1))
