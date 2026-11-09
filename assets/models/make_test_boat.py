#!/usr/bin/env python3
"""Packs the boat for the 3D mouse test model in Preferences into assets/models/test_boat.bin, from a
binary STL of the low-poly boat (about 850 triangles). Needs numpy.

    python3 assets/models/make_test_boat.py SOURCE.stl assets/models/test_boat.bin

No reduction happens here: the vertices are welded, and each welded vertex is split by the angle
between the faces that meet there (more than 40 degrees apart is a crease), so smooth surfaces share
a vertex normal and edges stay sharp. The model is centred and scaled into a unit box, then
quantised. File layout, little-endian: u32 vertex count, u32 triangle count, f32 centre (x, y, z),
f32 half size of the longest side; then per vertex its position as i16 x 3 (32767 is the half size)
and its normal as i8 x 3 (127 is a unit); then per triangle three u16 vertex indices. The model
sits with z up.
"""
import struct
import sys

import numpy as np

CREASE_DEGREES = 40.0


def main() -> None:
    source, target = sys.argv[1], sys.argv[2]
    data = open(source, "rb").read()
    count = struct.unpack_from("<I", data, 80)[0]
    record = np.dtype([("n", "<f4", 3), ("v", "<f4", (3, 3)), ("attr", "<u2")])
    tris = np.frombuffer(data, dtype=record, count=count, offset=84)["v"].astype(np.float64)
    points, inverse = np.unique(np.round(tris.reshape(-1, 3), 3), axis=0, return_inverse=True)
    faces = inverse.reshape(-1, 3)
    keep = (faces[:, 0] != faces[:, 1]) & (faces[:, 1] != faces[:, 2]) & (faces[:, 0] != faces[:, 2])
    faces = faces[keep]
    a, b, c = points[faces[:, 0]], points[faces[:, 1]], points[faces[:, 2]]
    cross = np.cross(b - a, c - a)
    area = np.linalg.norm(cross, axis=1)
    ok = area > 1e-9
    faces, cross, area = faces[ok], cross[ok], area[ok]
    face_normal = cross / area[:, None]
    # Split each welded vertex into one vertex per group of faces that agree (within the crease angle).
    cos_limit = np.cos(np.radians(CREASE_DEGREES))
    corners = {}
    for f, tri in enumerate(faces):
        for k in range(3):
            corners.setdefault(int(tri[k]), []).append((f, k))
    out_points, out_normals, index = [], [], np.zeros(faces.shape, dtype=np.int64)
    for v, incident in corners.items():
        groups = []  # each: [summed normal, [(face, corner)]]
        for f, k in incident:
            n = face_normal[f]
            for g in groups:
                mean = g[0] / np.linalg.norm(g[0])
                if mean @ n > cos_limit:
                    g[0] = g[0] + n * area[f]
                    g[1].append((f, k))
                    break
            else:
                groups.append([n * area[f], [(f, k)]])
        for total, members in groups:
            length = np.linalg.norm(total)
            out_normals.append(total / length if length > 0 else np.array([0.0, 0.0, 1.0]))
            out_points.append(points[v])
            for f, k in members:
                index[f, k] = len(out_points) - 1
    out_points, out_normals = np.array(out_points), np.array(out_normals)
    low, high = out_points.min(axis=0), out_points.max(axis=0)
    centre, half = (low + high) / 2.0, float((high - low).max() / 2.0)
    positions = np.clip(np.round((out_points - centre) / half * 32767.0), -32767, 32767).astype("<i2")
    normals = np.clip(np.round(out_normals * 127.0), -127, 127).astype("i1")
    assert len(out_points) <= 65535
    with open(target, "wb") as out:
        out.write(struct.pack("<II3ff", len(out_points), len(index), *centre.tolist(), half))
        for p, n in zip(positions, normals):
            out.write(p.tobytes() + n.tobytes())
        out.write(index.astype("<u2").tobytes())
    print(f"{len(out_points)} vertices, {len(index)} triangles, centre {centre}, half size {half}")


if __name__ == "__main__":
    main()
