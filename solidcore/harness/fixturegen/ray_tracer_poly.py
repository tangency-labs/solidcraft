#!/usr/bin/env python3
"""Writes fixtures/intersect.ray_tracer/poly_breps.json: rays fired at poly B-reps.

Upstream `tests/prog_test/src/smTessTest.cpp` `my_test_tess` test 10 ("Test ray-firing for
poly brep") reads `pt_TestFiles/Tess/Extrusion.pbp` into an `SmPolyBrep`, makes a 20^3
`SmGrid` over the poly B-rep's box expanded by 1.0, an `SmRayTracer(ctx, grid,
SM_ZONE_TOL_3D, 1.0e-8)`, calls `AddPolyBrepToGrid(pPolyBrep, 0.001)` and fires one ray from
the box centre moved back 15 along the unit (1, 1, 1), with `dMaxRayParameter` 40 and
`m_bHitAnyThing` FALSE, asserting that it hits. Case `tess_test10_extrusion` is that test:
the file's one region and shell, its 296 faces in file order with their stored tolerances
and normals, each face's loops as the start vertices of their edges (`SmPolyEdgeData` holds
the edge's start vertex; a loop's edges are consecutive from its start edge, and a face's
loops from its first loop), and the vertices with their tolerances. The grid box is the box
of the vertex points expanded by 1.0 (upstream's `SmPolyBrep::CalculateBoundingBox` box is
not reproduced; the grid only places the voxels). Further rays of the case: the same ray
with hit-any-thing, the reverse ray, and rays along the axes through the centre.

The other cases are built here to reach the branches of `sm_FindPolyHit` and the polygon
branch of `FireRay`: nearest of stacked faces, a face behind the ray start, misses outside
the polygon and in its plane, a hole, a non-convex notch, coincident faces in two poly
B-reps (the first one tested keeps the hit), a big face over many small ones (objects on
inner tree nodes: the walk up the parents), a poly face against a surface, computed and
given normals, a degenerate face, expansion tolerances, and a render with shadows.

Run from the repo root: `python3 -I harness/fixturegen/ray_tracer_poly.py`.
"""

import json
import math
import os

UPSTREAM = "upstream/TestFiles/pt_TestFiles/Tess/Extrusion.pbp"
OUT = "fixtures/intersect.ray_tracer/poly_breps.json"
SM_ZONE_TOL_3D = 1e-05  # as the existing ray tracer fixtures use it
BIG = 1.0e20  # SM_BIG_DOUBLE


def read_pbp(path):
    """The faces, loops, edges and vertices of an ASCII .pbp file (SmPolyBrepData)."""
    with open(path) as f:
        lines = [ln.strip() for ln in f]
    at = 0

    def section(title):
        nonlocal at
        while not lines[at].startswith("//" + title):
            at += 1
        at += 1
        count = int(lines[at].split()[0])
        at += 1
        while lines[at].startswith("//"):
            at += 1
        return count

    def nums():
        nonlocal at
        vals = lines[at].split()
        at += 1
        return vals

    tol = float(next(ln for ln in lines if ln.endswith("PolyBrep Tolerance")).split()[0])

    count = section("PolyFace Data")
    faces = []
    for _ in range(count):
        idx, ftol, nloops, first = nums()[:4]
        n = [float(x) for x in nums()[:3]]
        nums()  # attributes
        faces.append((float(ftol), int(nloops), int(first), n))

    count = section("PolyLoop Data")
    loops = []
    for _ in range(count):
        idx, nedges, start = nums()[:3]
        nums()
        loops.append((int(nedges), int(start)))

    count = section("PolyEdge Data")
    edges = []
    for _ in range(count):
        vals = nums()
        edges.append(int(vals[2]))  # m_lVertex: the start vertex
        nums()

    count = section("PolyVertex Data")
    vertices = []
    for _ in range(count):
        vtol = float(nums()[1])
        p = [float(x) for x in nums()[:3]]
        nums()
        vertices.append(p + [vtol])

    out_faces = []
    for ftol, nloops, first, n in faces:
        fl = []
        for li in range(first, first + nloops):
            nedges, start = loops[li]
            fl.append([edges[e] for e in range(start, start + nedges)])
        out_faces.append({"tol": ftol, "normal": n, "loops": fl})
    return tol, vertices, out_faces


def unit(v):
    n = math.sqrt(sum(x * x for x in v))
    return [x / n for x in v]


def fire(p, v, max_t=BIG, hit_any=False):
    return {"op": "fire", "p": p, "v": v, "max_t": max_t, "hit_any": hit_any, "skip": None}


def poly(vertices, faces, tol=1e-5, expansion=1e-3):
    return {"op": "add_poly_brep", "tol": tol, "vertices": vertices, "faces": faces,
            "expansion": expansion}


def face(loops, tol=1e-5, normal=None):
    f = {"tol": tol, "loops": loops}
    if normal is not None:
        f["normal"] = normal
    return f


def rect(x0, y0, x1, y1, z, tol=1e-5):
    """Counter-clockwise from above."""
    return [[x0, y0, z, tol], [x1, y0, z, tol], [x1, y1, z, tol], [x0, y1, z, tol]]


def plane_surface():
    return {"kind": "plane", "o": [0, 0, 0], "x": [1, 0, 0], "y": [0, 1, 0], "scale": [1, 1],
            "domain": [0, 0, 4, 4]}


def grid(bbox, n=4, ray_tol=SM_ZONE_TOL_3D):
    return {"bbox": bbox, "size": [n, n, n], "ray_tol": ray_tol, "ray_acc": 1e-08}


def case(name, ref, ops, bbox, n=4, surfaces=None):
    return {"name": name, "upstream_ref": ref,
            "inputs": {"surfaces": surfaces or [plane_surface()], "grid": grid(bbox, n),
                       "ops": ops}}


def tess_test10():
    tol, vertices, faces = read_pbp(UPSTREAM)
    lo = [min(v[i] for v in vertices) - 1.0 for i in range(3)]
    hi = [max(v[i] for v in vertices) + 1.0 for i in range(3)]
    c = [0.5 * (lo[i] + hi[i]) for i in range(3)]
    d = unit([1.0, 1.0, 1.0])
    start = [c[i] - d[i] * 15.0 for i in range(3)]
    ops = [poly(vertices, faces, tol=tol, expansion=0.001),
           fire(start, d, 40.0),
           fire(start, d, 40.0, hit_any=True),
           fire([c[i] + d[i] * 15.0 for i in range(3)], [-x for x in d], 40.0)]
    for axis in range(3):
        for sign in (1.0, -1.0):
            v = [0.0, 0.0, 0.0]
            v[axis] = sign
            p = list(c)
            p[axis] -= sign * 30.0
            ops.append(fire(p, v))
    return case("tess_test10_extrusion",
                "tests/prog_test/src/smTessTest.cpp:2716 (test 10: Extrusion.pbp, grid 20^3 over "
                "the box expanded by 1.0, SM_ZONE_TOL_3D, 1e-8, AddPolyBrepToGrid 0.001, ray from "
                "the centre - 15 (1,1,1)/sqrt 3 to 40; plus hit-any, reverse and axis rays)",
                ops, lo + hi, n=20)


def stacked():
    v = rect(2, 2, 8, 8, 2.0) + rect(2, 2, 8, 8, 6.0)
    ops = [poly(v, [face([[0, 1, 2, 3]]), face([[4, 5, 6, 7]])]),
           fire([5, 5, 9.5], [0, 0, -1]),                 # nearest: z = 6
           fire([5, 5, 9.5], [0, 0, -1], hit_any=True),
           fire([5, 5, 4.0], [0, 0, -1]),                 # z = 6 behind the start
           fire([5, 5, -0.5], [0, 0, 1]),                 # from below: z = 2
           fire([9.0, 5, 9.5], [0, 0, -1]),               # outside the squares
           fire([0.5, 5, 4.0], [1, 0, 0]),                # parallel, between them
           fire([5, 5, 9.5], [0, 0, -1], max_t=3.0),      # stops before z = 6
           fire([5, 5, 6.0], [0, 0, -1]),                 # starts on the face (t = 0)
           fire([2.0, 2.0, 9.5], [0, 0, -1]),             # through a corner vertex
           fire([5, 2.0, 9.5], [0, 0, -1]),               # through an edge
           fire([0.5, 0.5, 9.5], unit([1, 1, -1]))]       # oblique
    return case("poly_stacked_squares", "sm_FindPolyHit: nearest, behind, miss, in-plane, "
                "short ray, on-face start, vertex and edge hits", ops, [0, 0, 0, 10, 10, 10])


def holed():
    v = rect(1, 1, 9, 9, 5.0) + [[4, 4, 5.0, 1e-5], [4, 6, 5.0, 1e-5], [6, 6, 5.0, 1e-5],
                                 [6, 4, 5.0, 1e-5]] + rect(1, 1, 9, 9, 1.0)
    ops = [poly(v, [face([[0, 1, 2, 3], [4, 5, 6, 7]]), face([[8, 9, 10, 11]])]),
           fire([5, 5, 9.5], [0, 0, -1]),     # through the hole: z = 1
           fire([2, 2, 9.5], [0, 0, -1]),     # the holed face
           fire([4, 5, 9.5], [0, 0, -1]),     # on the hole's edge
           fire([5, 5, 9.5], [0, 0, -1], hit_any=True)]
    return case("poly_face_with_hole", "PointInPolygon with an inner loop", ops,
                [0, 0, 0, 10, 10, 10])


def notch():
    # an L: the notch [5, 8] x [5, 8] is outside
    pts = [[2, 2], [8, 2], [8, 5], [5, 5], [5, 8], [2, 8]]
    v = [[x, y, 4.0, 1e-5] for x, y in pts]
    ops = [poly(v, [face([[0, 1, 2, 3, 4, 5]])]),
           fire([6.5, 6.5, 9.5], [0, 0, -1]),  # in the notch: miss
           fire([3.5, 6.5, 9.5], [0, 0, -1]),
           fire([6.5, 3.5, 9.5], [0, 0, -1]),
           fire([6.5, 6.5, 9.5], unit([-1, -1, -1]))]
    return case("poly_nonconvex_notch", "PointInPolygon on a non-convex face", ops,
                [0, 0, 0, 10, 10, 10])


def coincident():
    a = rect(2, 2, 8, 8, 5.0)
    ops = [poly(a, [face([[0, 1, 2, 3]])]),
           poly(a, [face([[0, 1, 2, 3]])]),
           fire([5, 5, 9.5], [0, 0, -1]),
           fire([5, 5, 0.5], [0, 0, 1]),
           fire([3, 7, 9.5], unit([0.2, -0.1, -1]))]
    return case("poly_coincident_breps", "two poly B-reps with the same face: equal ray "
                "parameters, the first tested keeps the hit (strict <)", ops, [0, 0, 0, 10, 10, 10])


def big_over_small():
    v = rect(0.5, 0.5, 9.5, 9.5, 9.0)
    faces = [face([[0, 1, 2, 3]])]
    for i in range(8):
        for j in range(8):
            base = len(v)
            x, y = 1 + i, 1 + j
            z = 1.0 + 0.25 * ((i * 8 + j) % 7)
            v += rect(x, y, x + 0.9, y + 0.9, z)
            faces.append(face([[base, base + 1, base + 2, base + 3]]))
    ops = [poly(v, faces)]
    for x, y in [(1.45, 1.45), (4.5, 6.5), (8.95, 8.95), (5.0, 5.0), (3.2, 8.4), (7.99, 1.01)]:
        ops.append(fire([x, y, 0.2], [0, 0, 1]))
        ops.append(fire([x, y, 9.8], [0, 0, -1]))
    ops.append(fire([0.2, 0.3, 0.2], unit([1, 1, 0.3])))
    ops.append(fire([9.8, 9.7, 9.5], unit([-1, -0.9, -0.6]), hit_any=True))
    return case("poly_big_face_over_small_faces", "a face spanning the tree on an inner node: "
                "sm_FindPolyHit walks up the parents", ops, [0, 0, 0, 10, 10, 10], n=6)


def with_surface():
    # the plane z = 0 over [0, 4]^2 and a poly square at z = 1 and z = -1
    v = rect(0.5, 0.5, 3.5, 3.5, 1.0) + rect(0.5, 0.5, 3.5, 3.5, -1.0)
    ops = [{"op": "add_surface", "surface": 0},
           poly(v, [face([[0, 1, 2, 3]]), face([[4, 5, 6, 7]])]),
           fire([2, 2, 3], [0, 0, -1]),
           fire([2, 2, 0.5], [0, 0, -1]),
           fire([0.2, 0.2, 3], [0, 0, -1]),
           fire([2, 2, -3], [0, 0, 1])]
    return case("poly_and_surface", "a poly face and a surface leaf in one grid", ops,
                [-1, -1, -2, 5, 5, 2])


def normals():
    v = rect(2, 2, 8, 8, 3.0) + rect(2, 2, 8, 8, 7.0)
    # the first wound clockwise (normal -z), the second given a tilted normal
    ops = [poly(v, [face([[3, 2, 1, 0]]), face([[4, 5, 6, 7]], normal=unit([0, 0.6, 0.8]))]),
           fire([5, 5, 9.5], [0, 0, -1]),
           fire([5, 5, 5.0], [0, 0, -1]),
           fire([5, 5, 0.5], [0, 0, 1])]
    return case("poly_face_normals", "computed (clockwise) and given face normals in the "
                "solution and in the plane intersection", ops, [0, 0, 0, 10, 10, 10])


def degenerate():
    v = [[2, 2, 5, 1e-5], [5, 5, 5, 1e-5], [8, 8, 5, 1e-5], [2, 2, 2, 1e-5], [8, 2, 2, 1e-5],
         [5, 8, 2, 1e-5]]
    ops = [poly(v, [face([[0, 1, 2]]), face([[3, 4, 5]])]),
           fire([5, 5, 9.5], [0, 0, -1]),
           fire([5, 4, 9.5], [0, 0, -1]),
           fire([4, 4, 9.5], unit([1, 1, -1]))]
    return case("poly_degenerate_face", "a collinear face (degenerate normal) above a triangle",
                ops, [0, 0, 0, 10, 10, 10])


def expansions():
    out = []
    for e, name in [(0.0, "zero"), (2.5, "large")]:
        v = rect(2, 2, 8, 8, 5.0)
        ops = [poly(v, [face([[0, 1, 2, 3]])], expansion=e),
               fire([5, 5, 9.5], [0, 0, -1]),
               fire([8.5, 5, 9.5], [0, 0, -1])]
        out.append(case(f"poly_expansion_{name}", "AddPolyBrepToGrid expansion tolerance", ops,
                        [0, 0, 0, 10, 10, 10]))
    return out


def render():
    v = rect(1, 1, 3, 3, 1.0) + rect(0, 0, 4, 4, -1.0)
    ops = [poly(v, [face([[0, 1, 2, 3]]), face([[4, 5, 6, 7]])]),
           {"op": "renderer", "origin": [0, 0, 0], "x": [1, 0, 0], "y": [0, 1, 0],
            "view": [0.5, 0.5, 4.0, 3.5, 3.5, 4.0], "nx": 3, "ny": 3, "rate": 2},
           {"op": "light", "v": unit([0.3, 0.2, 1]), "point": False},
           {"op": "light", "v": [2, 2, 3.5], "point": True},
           {"op": "do_render", "shadows": True, "background": [0.1, 0.2, 0.3],
            "object": [0.8, 0.5, 0.2]},
           {"op": "render_pixel", "x": 1, "y": 1},
           {"op": "render_ray", "p": [2, 2, 3.5], "v": [0, 0, -1]},
           {"op": "render_ray", "p": [3.5, 3.5, 3.5], "v": [0, 0, -1]}]
    return case("poly_render_shadows", "SmRayRenderer over poly faces with shadow rays "
                "(hit-any-thing)", ops, [-1, -1, -2, 5, 5, 5])


def main():
    cases = [tess_test10(), stacked(), holed(), notch(), coincident(), big_over_small(),
             with_surface(), normals(), degenerate(), *expansions(), render()]
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        json.dump({"cases": cases}, f, indent=1)
        f.write("\n")
    print(f"wrote {OUT}: {len(cases)} cases")


if __name__ == "__main__":
    main()
