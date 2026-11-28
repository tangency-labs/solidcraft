#!/usr/bin/env python3
"""Closed-form expected values for the frame (structural member) oracle cases.

Run from the worktree root:  python3 tools/oracle-analytic/cases_frames.py [case ...]
Writes tests/oracle/<case>/expected.json with "source": "analytic".

Every member is a straight bar of a rectangular section, so each is a box: its volume, area and
centre follow from its section and its length after trimming. Nothing here is measured from a CAD
kernel. Stdlib only.
"""
import json
import os
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "tests", "oracle")


def c96_frame_tower():
    """The T-slot tower of #4003 in 45 x 45 bar on a 600 x 600 footprint, 1500 high. Every line runs
    through its section's centre, 22.5 in from the outside. Four uprights run through at the
    corners. Rails in rings at bottoms z = 0, 700 and 1455 (centres 22.5, 722.5, 1477.5) are butted
    against the uprights' inner faces: 600 - 2 x 45 = 510 long. Two cross members in the middle
    ring, at x = 200 and x = 400, are butted against the inner faces of the two rails along X:
    510 long."""
    s, w, h = 45.0, 600.0, 1500.0
    inner = w - 2 * s
    ring = [s / 2, 700 + s / 2, 1455 + s / 2]
    # (length, centre) of every member.
    members = [(h, (x, y, h / 2)) for x in (s / 2, w - s / 2) for y in (s / 2, w - s / 2)]
    for z in ring:
        members += [(inner, (w / 2, y, z)) for y in (s / 2, w - s / 2)]
        members += [(inner, (x, w / 2, z)) for x in (s / 2, w - s / 2)]
    members += [(inner, (x, w / 2, ring[1])) for x in (200.0, 400.0)]
    vol = sum(s * s * length for length, _ in members)
    com = [sum(s * s * length * c[k] for length, c in members) / vol for k in range(3)]
    return {
        "source": "analytic",
        "method": "18 separate 45 x 45 bars (boxes): 4 uprights of 1500, 12 rails and 2 cross members of 600 - 2 x 45 = 510; "
        "V = 2025 (4 x 1500 + 14 x 510); area = sum of 4 x 45 L + 2 x 2025 per bar; centre of mass by moments "
        "(rails centred at z = 22.5, 722.5, 1477.5, cross members at x = 200 and 400 in the middle ring)",
        "body_count": len(members),
        "volume_mm3": vol,
        "area_mm2": sum(4 * s * length + 2 * s * s for length, _ in members),
        "center_of_mass": com,
        "bbox": {"min": [0.0, 0.0, 0.0], "max": [w, w, h]},
        "faces": 6 * len(members),
    }


CASES = {
    "96-frame-tower": c96_frame_tower,
}


def main(argv):
    names = argv[1:] or list(CASES)
    for n in names:
        d = CASES[n]()
        path = os.path.join(ROOT, n, "expected.json")
        with open(path, "w") as f:
            json.dump(d, f, indent=2)
            f.write("\n")
        print(f"{n}: V={d['volume_mm3']!r} A={d['area_mm2']!r} com={d['center_of_mass']}")


if __name__ == "__main__":
    main(sys.argv)
