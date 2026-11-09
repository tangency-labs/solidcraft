#!/usr/bin/env python3
"""Writes fixtures/surface.surface_p1/upstream_sil_adv.json from upstream's silhouette regression.

Upstream `tests/prog_test/src/smAdvancedTest.cpp` `my_test_sil_adv` runs 34 blocks: each
reads a single-face `pt_TestFiles/*.smb` B-rep, translates it (`SmAxis2Placement::Translate`
then `SmBrep::Transform`) and calls (`smTopoTest.cpp`) `my_test_tsurf_silhouette(brep,
1.0e-3, vec, bPerspective, lNumFound)`, which for every face calls
`SmSurface::CreateSilhouetteCurves(ctx, face UV domain, vec, bPerspective, &1e-3,
&SM_DEG2RAD(20.0), ..)` on the face's `SmBSplineSurface`, then trims the curves to the face
(`SmFace::CreateCurvesByTrimming`) and counts the trimmed 3D curves.

Each block becomes one `sil` case on the face surface over the face's stored UV domain
(`SmFaceData` reads `UMin UMax VMin VMax`; the fixture domain is `[umin, vmin, umax, vmax]`)
with the block's translation applied by `Transform` on both sides. The op stops at
`CreateSilhouetteCurves` (no trimming), so upstream's asserted counts are quoted in
`upstream_ref` and not compared. The surface is free (no owning face), so both sides build its
plain surface cache where upstream's face surface gets a trimmed surface cache.

`.smb` files hold one B-rep in the `SmBrepData` DB layout of `.smp` part files (as
`ssi_adv.py` reads them), without version line and without attribute index lines. Every face
surface is an `SmBSplineSurface` (type 15002); the control net is copied as stored
(`wx wy wz w` when rational).

Run from the repo root: `python3 -I harness/fixturegen/sil_adv.py`.
"""

import json
import math
import os
import re
import sys

ROOT = os.getcwd()
TEST = os.path.join(ROOT, "upstream/tests/prog_test/src/smAdvancedTest.cpp")
FILES = os.path.join(ROOT, "upstream/TestFiles/pt_TestFiles")
OUT = os.path.join(ROOT, "fixtures/surface.surface_p1/upstream_sil_adv.json")
APPROX_TOL = 1.0e-3  # my_test_tsurf_silhouette(pBrep1, 1.0e-3, ...)
ANG_TOL = 20.0 * math.pi / 180.0  # SM_DEG2RAD(20.0) in my_test_tsurf_silhouette

FUNC = re.compile(r"PT_EXPORT SmStatus my_test_sil_adv\(void\)")
BLOCK = re.compile(r'Entered my_test_sil_adv (\d+)"')
READ = re.compile(r'ReadFromFile\(sContext, _T\("\.\./\.\./TestFiles/pt_TestFiles/([A-Za-z0-9_]+\.smb)"\)')
MOVE = re.compile(r"sA2P\.Translate\(SmVector3d\(([^)]*)\)\)")
PERSP = re.compile(r"SmBoolean bPerspective = (TRUE|FALSE);")
VEC = re.compile(r"SmVector3d sVec\(([^)]*)\);")
CALL = re.compile(r"my_test_tsurf_silhouette\(pBrep1, ([0-9.eE+-]+), sVec, bPerspective, lNumFound\)")
ASSERT = re.compile(r"SM_ASSERT\((lNumFound == [^;]*)\);")
END = re.compile(r"end my_test_sil_adv")


def floats(s):
    return [float(x) for x in s.split(",")]


def blocks():
    """The blocks of my_test_sil_adv in source order, with the call's line."""
    out = []
    cur = None
    inside = False
    with open(TEST, encoding="latin-1") as f:
        for no, line in enumerate(f, 1):
            code = line.split("//", 1)[0]
            if FUNC.search(code):
                inside = True
                continue
            if not inside:
                continue
            if END.search(line):
                break
            m = BLOCK.search(code)
            if m:
                cur = {"block": int(m.group(1))}
                out.append(cur)
                continue
            if cur is None:
                continue
            if (m := READ.search(code)):
                cur["file"] = m.group(1)
            elif (m := MOVE.search(code)):
                cur["translate"] = floats(m.group(1))
            elif (m := PERSP.search(code)):
                cur["perspective"] = m.group(1) == "TRUE"
            elif (m := VEC.search(code)):
                cur["eye"] = floats(m.group(1))
            elif (m := CALL.search(code)):
                cur["line"] = no
                cur["tol"] = float(m.group(1))
            elif (m := ASSERT.search(code)):
                cur["expect"] = m.group(1)
    for b in out:
        missing = {"file", "translate", "perspective", "eye", "line", "tol", "expect"} - set(b)
        if missing:
            raise ValueError(f"block {b['block']}: missing {sorted(missing)}")
        if b["tol"] != APPROX_TOL:
            raise ValueError(f"block {b['block']}: tolerance {b['tol']}")
    return out


def section(lines, header, start):
    """Index of the line after `header` (searching from `start`)."""
    for i in range(start, len(lines)):
        if lines[i].strip() == header:
            return i + 1
    raise ValueError(f"missing {header!r}")


def count(line, what):
    m = re.match(rf"\s*(\d+) {what}", line)
    if not m:
        raise ValueError(f"expected count of {what!r}, got {line!r}")
    return int(m.group(1))


def nums(lines, i, k):
    """`k` numbers from line `i` on; returns them and the next line index."""
    out = []
    while len(out) < k:
        out.extend(float(t) for t in lines[i].split())
        i += 1
    if len(out) != k:
        raise ValueError(f"line {i}: record does not end at a line break")
    return out, i


def surface(lines, i):
    """An `SmBSplineSurface` DB record (no knot type / form) starting at line `i`."""
    (n, m), i = nums(lines, i, 2)
    (p, q), i = nums(lines, i, 2)
    (rat,), i = nums(lines, i, 1)
    n, m, p, q = int(n), int(m), int(p), int(q)
    net = []
    for _ in range(n + 1):
        row = []
        for _ in range(m + 1):
            if rat:
                pt, i = nums(lines, i, 4)
            else:
                pt, i = nums(lines, i, 3)
                pt.append(-1.7976931348623157e308)  # NOW: not rational
            row.append(pt)
        net.append(row)
    u, i = nums(lines, i, n + p + 2)
    v, i = nums(lines, i, m + q + 2)
    return {"net": net, "p": p, "q": q, "u": u, "v": v}, i


def brep(name):
    """The faces of the `.smb` B-rep: `(surface, [umin, vmin, umax, vmax])`."""
    with open(os.path.join(FILES, name), encoding="latin-1") as f:
        lines = f.read().split("\n")
    if lines[0].strip() != "//Brep Starts":
        raise ValueError(f"{name}: not a single B-rep file")
    i = section(lines, "//Face Data", 0)
    nf = count(lines[i], "Faces In Brep")
    i += 3  # count, two column comments
    faces = []
    for _ in range(nf):
        toks = lines[i].split()
        srf = int(toks[-4])  # ... Surface, Outer Loop, # of Loops, Naturally Trimmed
        (u0, u1, v0, v1), i = nums(lines, i + 1, 4)
        faces.append((srf, [u0, v0, u1, v1]))
    i = section(lines, "//Surface Data", i)
    ns = count(lines[i], "Surfaces In Brep")
    i += 1
    surfaces = []
    for k in range(ns):
        if lines[i].split()[:3] != [str(k), "-", "Surface"]:
            raise ValueError(f"{name} line {i + 1}: expected surface {k}")
        typ = lines[i + 1].split()
        if typ != ["Surface", "Type", "15002"]:
            raise ValueError(f"{name} line {i + 2}: not an SmBSplineSurface: {typ}")
        s, i = surface(lines, i + 2)
        surfaces.append(s)
        while i < len(lines) and not lines[i].strip():
            i += 1
    return [(surfaces[s], d) for s, d in faces]


def main():
    cases = []
    bs = blocks()
    for b in bs:
        faces = brep(b["file"])
        stem = b["file"][: -len(".smb")]
        for j, (s, d) in enumerate(faces):
            cases.append(
                {
                    "name": f"sil_adv_{b['block']:02d}_{stem}_f{j}",
                    "upstream_ref": (
                        f"tests/prog_test/src/smAdvancedTest.cpp:{b['line']} my_test_sil_adv "
                        f"block {b['block']}: {b['file']} face {j} of {len(faces)} "
                        f"(smTopoTest.cpp my_test_tsurf_silhouette, tol 1e-3, 20 deg); "
                        f"upstream asserts {b['expect']} after SmFace::CreateCurvesByTrimming "
                        f"(topology.face_p2, not run by this op)"
                    ),
                    "inputs": {
                        "surf": {"bspline": {"srf": s}},
                        "op": "sil",
                        "dom": d,
                        "eye": b["eye"],
                        "perspective": b["perspective"],
                        "tol": APPROX_TOL,
                        "ang": ANG_TOL,
                        "translate": b["translate"],
                    },
                }
            )
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f, indent=1)
        f.write("\n")
    print(f"{len(cases)} cases from {len(bs)} blocks -> {os.path.relpath(OUT, ROOT)}")


if __name__ == "__main__":
    sys.exit(main())
