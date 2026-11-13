#!/usr/bin/env python3
"""Propose which triangles of a mesh belong to one face, for SolidCraft's `mesh_to_cad`.

Reads an STL, samples points over its surface, runs Point2Cyl (an open-source network that
decomposes a point cloud into extrusion cylinders) over those points, and turns the per-point
labels back into one group number per triangle, in the STL's own triangle order. That array is
what `mesh_to_cad` takes as `groups`.

The groups are a proposal and nothing more: SolidCraft fits and certifies every surface itself
and splits a group that will not certify, so a poor proposal costs recognition, never
correctness. Running this at all is optional -- `mesh_to_cad` groups the triangles by its own
region growing when no proposal is given.

This is a separate program. It is not part of SolidCraft's build and nothing it downloads is
shipped with SolidCraft. See README.md.

Usage:
    python segment.py --fetch                                  # download the model, once
    python segment.py --mesh part.stl --out groups.json
    python segment.py --self-test                              # no model and no torch needed
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import struct
import sys
import urllib.request

# Point2Cyl (MIT), pinned to the commit the hashes below were taken from.
REPO = "https://raw.githubusercontent.com/mikacuy/point2cyl"
COMMIT = "main"
FILES = {
    "models/pointnet_extrusion.py": "59660ecd8d7f4548349b7eefd2611245c4ed93889273201e29952178733ceef4",
    "models/pointnet_util.py": "b8a090bcdb393df366aab08cae3f60cfef9c35e0911189442468cee1fca498e0",
    "results/Point2Cyl_without_sketch/model.pth": "11d4092a98918968c690214045f3f4df3e536c7646f07743fd23e2e88896785d",
}
WEIGHTS = "results/Point2Cyl_without_sketch/model.pth"
# The network as that checkpoint was trained: 8192 points, at most 8 extrusions, two channels per
# extrusion (its barrel and its base, interleaved).
POINTS = 8192
EXTRUSIONS = 8


def fetch(into: pathlib.Path) -> None:
    """Download the model files and check them against their hashes."""
    for name, digest in FILES.items():
        target = into / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() and sha256(target) == digest:
            print("have %s" % name)
            continue
        url = "%s/%s/%s" % (REPO, COMMIT, name)
        print("fetching %s" % url)
        with urllib.request.urlopen(url, timeout=120) as response:  # noqa: S310 (a pinned https URL)
            target.write_bytes(response.read())
        got = sha256(target)
        if got != digest:
            raise SystemExit("%s hashes to %s, expected %s: refusing it" % (name, got, digest))
    (into / "models" / "__init__.py").write_text("", encoding="utf-8")
    print("model ready in %s" % into)


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_stl(path: pathlib.Path):
    """The triangles of a binary or ASCII STL, as a list of three (x, y, z) corners each."""
    raw = path.read_bytes()
    if len(raw) < 84:
        raise SystemExit("%s is too short to be an STL" % path)
    # A binary STL is 84 + 50 per triangle. An ASCII one starting with "solid" can be that long by
    # accident, so the length decides and the keyword only breaks the tie.
    (count,) = struct.unpack_from("<I", raw, 80)
    if len(raw) == 84 + 50 * count:
        return [
            [struct.unpack_from("<3f", raw, 84 + 50 * i + 12 + 12 * c) for c in range(3)]
            for i in range(count)
        ]
    return read_ascii_stl(raw.decode("utf-8", errors="replace"))


def read_ascii_stl(text: str):
    triangles, corners = [], []
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 4 and parts[0] == "vertex":
            corners.append(tuple(float(v) for v in parts[1:]))
            if len(corners) == 3:
                triangles.append(corners)
                corners = []
    if not triangles:
        raise SystemExit("no triangles in the STL")
    return triangles


def sample_surface(triangles, count, seed):
    """`count` points spread over the surface, and the triangle each came from.

    Spread by area, so a large triangle carries more points than a small one, which is what the
    network was trained on. Every triangle keeps at least its own middle, so no face goes
    unlabelled however the areas fall.
    """
    import numpy as np

    corners = np.asarray(triangles, dtype=np.float64)
    first, second, third = corners[:, 0], corners[:, 1], corners[:, 2]
    areas = 0.5 * np.linalg.norm(np.cross(second - first, third - first), axis=1)
    middles = (first + second + third) / 3.0
    if not np.isfinite(areas).all() or areas.sum() <= 0:
        raise SystemExit("the mesh has no area")
    owners = np.arange(len(corners))
    points = middles
    extra = count - len(corners)
    if extra > 0:
        rng = np.random.default_rng(seed)
        drawn = rng.choice(len(corners), size=extra, p=areas / areas.sum())
        # A point inside a triangle, evenly spread over it.
        u, v = rng.random(extra), rng.random(extra)
        fold = u + v > 1.0
        u[fold], v[fold] = 1.0 - u[fold], 1.0 - v[fold]
        inside = (
            first[drawn]
            + (second[drawn] - first[drawn]) * u[:, None]
            + (third[drawn] - first[drawn]) * v[:, None]
        )
        points = np.concatenate([middles, inside])
        owners = np.concatenate([owners, drawn])
    return points[:count], owners[:count]


def normalize(points, how):
    """The points as the network expects them: centred, and scaled to about unit size."""
    import numpy as np

    points = np.asarray(points, dtype=np.float32)
    if how == "sphere":
        centre = points.mean(axis=0)
        moved = points - centre
        scale = np.linalg.norm(moved, axis=1).max()
    else:
        box = (points.min(axis=0) + points.max(axis=0)) / 2.0
        moved = points - box
        scale = np.abs(moved).max()
    if not scale > 0:
        raise SystemExit("every point of the mesh is in the same place")
    return (moved / scale).astype(np.float32)


def run_model(points, model_dir):
    """Per-point extrusion numbers from Point2Cyl."""
    import numpy as np
    import torch

    sys.path.insert(0, str(model_dir))
    from models.pointnet_extrusion import backbone

    net = backbone(output_sizes=[3, 2 * EXTRUSIONS])
    state = torch.load(model_dir / WEIGHTS, map_location="cpu", weights_only=True)
    net.load_state_dict(state["model"], strict=True)
    net.eval()
    with torch.no_grad():
        _normals, logits = net(torch.from_numpy(points[None, :, :]))
    # The channels are the extrusions interleaved with their bases, so two channels per
    # extrusion; which of the two a point won on is the base/barrel label, which this does not
    # need.
    return (logits.argmax(dim=2) // 2)[0].numpy().astype(np.int64)


def self_test():
    """Check the mesh reading and the label transfer, which need no model and no torch."""
    import numpy as np

    # A unit cube as 12 triangles, written as an ASCII STL and read back.
    corners = [
        [(0, 0, 0), (1, 1, 0), (1, 0, 0)],
        [(0, 0, 0), (0, 1, 0), (1, 1, 0)],
        [(0, 0, 1), (1, 0, 1), (1, 1, 1)],
        [(0, 0, 1), (1, 1, 1), (0, 1, 1)],
    ]
    text = ["solid cube"]
    for triangle in corners:
        text.append(" facet normal 0 0 0\n  outer loop")
        text += ["   vertex %g %g %g" % c for c in triangle]
        text.append("  endloop\n endfacet")
    text.append("endsolid cube")
    read = read_ascii_stl("\n".join(text))
    assert len(read) == len(corners), read
    assert np.allclose(read, corners), read

    binary = struct.pack("<80sI", b"cube", len(corners))
    for triangle in corners:
        binary += struct.pack("<3f", 0, 0, 0)
        for corner in triangle:
            binary += struct.pack("<3f", *corner)
        binary += struct.pack("<H", 0)
    path = pathlib.Path("self-test.stl")
    path.write_bytes(binary)
    try:
        assert np.allclose(read_stl(path), corners), "binary and ASCII disagree"
    finally:
        path.unlink()

    # Sampling gives every triangle its middle first, so every triangle can be labelled.
    points, owners = sample_surface(corners, 64, seed=1)
    assert len(points) == 64 and len(owners) == 64
    assert sorted(set(owners[: len(corners)].tolist())) == list(range(len(corners))), owners
    # Points on the Z 1 face are labelled 1, the rest 0: the groups follow the majority.
    labels = np.where(points[:, 2] > 0.5, 1, 0)
    groups = majority(labels, owners, len(corners))
    assert groups.tolist() == [0, 0, 1, 1], groups.tolist()
    # Normalizing puts the cube in the unit box, centred.
    unit = normalize(points, "box")
    assert abs(unit).max() <= 1.0 + 1e-6 and abs(unit.mean()) < 0.5, unit.mean()
    print("self-test passed")


def majority(labels, owners, triangle_count):
    """One group per triangle: whichever label most of its points were given."""
    import numpy as np

    votes = np.zeros((triangle_count, EXTRUSIONS), dtype=np.int32)
    np.add.at(votes, (owners, labels), 1)
    return votes.argmax(axis=1)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mesh", type=pathlib.Path, help="STL to read (as `file.save_mesh` writes one)")
    parser.add_argument("--out", type=pathlib.Path, help="Where to write the groups JSON (default: stdout)")
    parser.add_argument("--model", type=pathlib.Path, default=pathlib.Path(__file__).parent / "model", help="Where the model files are")
    parser.add_argument("--points", type=int, default=POINTS, help="Points to sample (default %d, what the checkpoint was trained on)" % POINTS)
    parser.add_argument("--normalize", choices=["box", "sphere"], default="box", help="How to scale the points before the network sees them")
    parser.add_argument("--seed", type=int, default=0, help="Seed for the surface sampling, so a run repeats")
    parser.add_argument("--fetch", action="store_true", help="Download the model files and check their hashes")
    parser.add_argument("--self-test", action="store_true", help="Check the mesh reading and label transfer (no model, no torch)")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if args.fetch:
        fetch(args.model)
        return
    if not args.mesh:
        parser.error("give --mesh, --fetch or --self-test")

    missing = [name for name in FILES if not (args.model / name).exists()]
    if missing:
        raise SystemExit("the model is not here yet (%s missing). Run: python segment.py --fetch" % len(missing))

    triangles = read_stl(args.mesh)
    points, owners = sample_surface(triangles, args.points, args.seed)
    labels = run_model(normalize(points, args.normalize), args.model)
    groups = majority(labels, owners, len(triangles))
    reply = {
        "triangles": len(triangles),
        "points": len(points),
        "model": "point2cyl-without-sketch",
        "groups": [int(g) for g in groups],
    }
    text = json.dumps(reply)
    if args.out:
        args.out.write_text(text, encoding="utf-8")
        print("%d triangles in %d groups -> %s" % (len(triangles), len(set(reply["groups"])), args.out))
    else:
        print(text)


if __name__ == "__main__":
    main()
