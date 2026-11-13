#!/usr/bin/env python3
"""Writes fixtures/surface.surface_p2/ssi_adv.json from upstream's advanced SSI regression.

Upstream `tests/prog_test/src/smAdvancedTest.cpp` `my_test_ssi_adv` calls
`my_test_one_ssi(file, translate, expected)` for `pt_TestFiles/ssi_*.smp` part files:
it reads the two B-reps, translates both by `translate` and (`smTopoTest.cpp`
`my_test_ssi`) calls `SmSurface::GlobalSurfaceIntersect` on every (face of B-rep 0,
face of B-rep 1) pair with `SmApproxTol3d 1e-3`, summing the 3D curve counts.

Every face in these files is an `SmBSplineSurface` (type 15002), so each pair becomes one
`gssi` case on the face surfaces over the faces' stored UV domains (`SmFaceData` reads
`UMin UMax VMin VMax`; the fixture domain is `[umin, vmin, umax, vmax]`). The control net
is copied as stored (`wx wy wz w` when rational), as `.sms` files are read.

Run from the repo root: `python3 -I harness/fixturegen/ssi_adv.py`.
"""

import json
import os
import re
import sys

ROOT = os.getcwd()
TEST = os.path.join(ROOT, "upstream/tests/prog_test/src/smAdvancedTest.cpp")
FILES = os.path.join(ROOT, "upstream/TestFiles/pt_TestFiles")
OUT = os.path.join(ROOT, "fixtures/surface.surface_p2/ssi_adv.json")
APPROX_TOL = 1.0e-3  # my_test_one_ssi -> my_test_ssi(pBrep1, pBrep2, 1.0e-3, ...)


CALL = re.compile(
    r'my_test_one_ssi\(_T\("\.\./\.\./TestFiles/pt_TestFiles/([A-Za-z0-9_]+\.smp)"\),'
    r"\s*SmVector3d\(([^)]*)\),\s*([0-9]+)\)"
)


def calls():
    """(line, file, translate, expected) of every live call, in source order."""
    out = []
    with open(TEST, encoding="latin-1") as f:
        for no, line in enumerate(f, 1):
            code = line.split("//", 1)[0]
            m = CALL.search(code)
            if m:
                t = [float(x) for x in m.group(2).split(",")]
                out.append((no, m.group(1), t, int(m.group(3))))
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
    """An `SmBSplineSurface` DB record (version <= 32) starting at line `i`."""
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


def brep(lines, start):
    """The faces of the B-rep block from `start`: `(surface, [umin, vmin, umax, vmax])`."""
    i = section(lines, "//Face Data", start)
    nf = count(lines[i], "Faces In Brep")
    i += 3  # count, two column comments
    faces = []
    for _ in range(nf):
        toks = lines[i].split()
        srf = int(toks[-4])  # ... Surface, Outer Loop, # of Loops, Naturally Trimmed
        (u0, u1, v0, v1), i = nums(lines, i + 1, 4)
        attrs = lines[i].split()  # indexed attributes: count, then the indices
        if not attrs or not all(a.isdigit() for a in attrs) or int(attrs[0]) != len(attrs) - 1:
            raise ValueError(f"line {i + 1}: expected the face's attribute indices")
        i += 1
        faces.append((srf, [u0, v0, u1, v1]))
    i = section(lines, "//Surface Data", i)
    ns = count(lines[i], "Surfaces In Brep")
    i += 1
    surfaces = []
    for k in range(ns):
        if lines[i].split()[:3] != [str(k), "-", "Surface"]:
            raise ValueError(f"line {i + 1}: expected surface {k}")
        typ = lines[i + 1].split()
        if typ != ["Surface", "Type", "15002"]:
            raise ValueError(f"line {i + 2}: not an SmBSplineSurface: {typ}")
        s, i = surface(lines, i + 2)
        surfaces.append(s)
        attrs = lines[i].split()  # the surface's attribute indices
        if not attrs or not all(a.isdigit() for a in attrs):
            raise ValueError(f"line {i + 1}: expected the surface's attribute indices")
        i += 1
        while i < len(lines) and not lines[i].strip():
            i += 1
    return [(surfaces[s], d) for s, d in faces]


def part(name):
    with open(os.path.join(FILES, name), encoding="latin-1") as f:
        lines = f.read().split("\n")
    version = count(lines[1].replace("//   (1)", ""), "Curves") // 1000000
    if version > 32:
        raise ValueError(f"{name}: version {version} records carry knot type and form")
    starts = [i for i, l in enumerate(lines) if l.startswith("[Brep #")]
    if len(starts) < 2:
        raise ValueError(f"{name}: fewer than two B-reps")
    return brep(lines, starts[0]), brep(lines, starts[1])


def main():
    cases = []
    seen = set()
    for line, name, t, expected in calls():
        key = (name, tuple(t))
        if key in seen:
            continue  # the same call again (upstream repeats some under other flags)
        seen.add(key)
        b0, b1 = part(name)
        stem = name[: -len(".smp")]
        for i, (s0, d0) in enumerate(b0):
            for j, (s1, d1) in enumerate(b1):
                cases.append(
                    {
                        "name": f"ssi_adv_{stem}_f{i}_f{j}",
                        "upstream_ref": (
                            f"tests/prog_test/src/smAdvancedTest.cpp:{line} my_test_one_ssi "
                            f"{name} faces {i}x{j} of {len(b0)}x{len(b1)}; upstream expects "
                            f"{expected} curves over all pairs"
                        ),
                        "inputs": {
                            "this": {"srf": s0},
                            "op": "gssi",
                            "dom": d0,
                            "other": {"srf": s1},
                            "other_dom": d1,
                            "tol": APPROX_TOL,
                            "translate": t,
                        },
                    }
                )
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f, indent=1)
        f.write("\n")
    print(f"{len(cases)} cases from {len(seen)} calls -> {os.path.relpath(OUT, ROOT)}")


if __name__ == "__main__":
    sys.exit(main())
