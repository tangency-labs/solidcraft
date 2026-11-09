#!/usr/bin/env python3
"""Native plain journal and thrust supports for the three SELENE yaw axes.

The admitted source uses stacked solid disks. This bounded reconstruction keeps
their outer envelopes, component ownership, joint origins and mounting heights.
Each fixed housing receives a flanged brass bushing. Its existing rotating disk
gains an integral hollow steel journal and a recessed, stationary retaining pin.
The pin head captures the rotor below the unchanged mast/clevis attachment plane.

These are illustrative geometric fits and ideal kinematics. Nominal cylindrical
retainer seats stand in for unmodelled thread detail; no tolerance class, preload,
lubrication, wear, structural load or manufacturing qualification is claimed.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


SUPPORTS = [
    {"key": "arm", "center": [177, 0], "base": 132, "interface": 140, "top": 146,
     "fixed_radius": 21, "rotor_radius": 18, "bushing_radius": 8,
     "journal_radius": 6, "pin_radius": 2.5, "head_radius": 4.8,
     "thrust_radius": 17.5, "fixed_component": "Structure", "rotor_component": "Arm turret",
     "housing": "Arm slewing base", "rotor": "Arm slewing bearing",
     "joint": "Arm azimuth articulation", "limit": 160},
    {"key": "dish", "center": [-177, 0], "base": 130, "interface": 135, "top": 139,
     "fixed_radius": 10, "rotor_radius": 9, "bushing_radius": 4.5,
     "journal_radius": 3, "pin_radius": 1.2, "head_radius": 2.6,
     "thrust_radius": 8.5, "fixed_component": "Dish pedestal", "rotor_component": "Dish yaw mount",
     "housing": "Dish yaw fixed bearing", "rotor": "Dish yaw rotating ring",
     "joint": "Dish azimuth articulation", "limit": 150},
    {"key": "turret", "center": [-45, 0], "base": 150, "interface": 157, "top": 163,
     "fixed_radius": 14, "rotor_radius": 13, "bushing_radius": 6,
     "journal_radius": 4, "pin_radius": 1.8, "head_radius": 3.3,
     "thrust_radius": 12.5, "fixed_component": "Turret pedestal", "rotor_component": "Turret yaw",
     "housing": "Turret fixed yaw bearing", "rotor": "Turret rotating collar",
     "joint": "Turret azimuth articulation", "limit": 160},
]

FLOOR = 2.0
THRUST_HEIGHT = .3
RADIAL_CLEARANCE = .1
PIN_CLEARANCE = .15
RECESS_DEPTH = 1.4
HEAD_HEIGHT = 1.0
HEAD_END_CLEARANCE = .3
SOCKET_DEPTH = .45


def c(command: str, **params) -> dict:
    return {"command": command, "params": params}


def names(support: dict) -> dict[str, str]:
    prefix = f"SELENE {support['key']} yaw"
    return {"prefix": prefix, "bushing": prefix + " flanged plain bushing",
            "retainer": prefix + " axial retaining pin"}


def cylinder(name, center, z, radius, height, operation="new", target=None):
    params = {"base": [*center, z], "radius": radius, "height": height,
              "operation": operation, "name": name}
    if target is None:
        params["body_name"] = name
    else:
        params["targets"] = [target]
    return c("solid.cylinder", **params)


def socket_commands(support: dict) -> list[dict]:
    n = names(support)
    name = n["prefix"] + " retaining socket"
    top = support["top"] - HEAD_END_CLEARANCE
    radius = support["head_radius"] * .35
    points = [[radius * math.cos(i * math.tau / 6), radius * math.sin(i * math.tau / 6)]
              for i in range(6)]
    return [
        c("sketch.create", name=name + " profile",
          plane={"origin": [*support["center"], top - SOCKET_DEPTH],
                 "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}),
        c("sketch.line", points=points, closed=True, infer=False),
        c("sketch.constraint.fix", entities=[f"l{i}" for i in range(1, 7)], fixed=True),
        c("sketch.finish"),
        c("solid.extrude", sketch=name + " profile", distance=SOCKET_DEPTH + .1,
          operation="cut", targets=[n["retainer"]], name=name),
    ]


def support_commands(support: dict) -> list[dict]:
    s, n = support, names(support)
    xy, base, interface, top = s["center"], s["base"], s["interface"], s["top"]
    bushing_floor = base + FLOOR
    journal_base = bushing_floor + .3
    head_top = top - HEAD_END_CLEARANCE
    result = [
        c("component.activate", component=s["fixed_component"]),
        cylinder(n["prefix"] + " housing bushing seat", xy, bushing_floor,
                 s["bushing_radius"], interface - bushing_floor + .4, "cut", s["housing"]),
        cylinder(n["prefix"] + " retaining pin seat", xy, base - .1,
                 s["pin_radius"], FLOOR + .2, "cut", s["housing"]),
        cylinder(n["bushing"], xy, bushing_floor, s["bushing_radius"],
                 interface - bushing_floor + THRUST_HEIGHT),
        cylinder(n["prefix"] + " thrust flange", xy, interface, s["thrust_radius"],
                 THRUST_HEIGHT, "join", n["bushing"]),
        cylinder(n["prefix"] + " plain journal clearance", xy, bushing_floor - .1,
                 s["journal_radius"] + RADIAL_CLEARANCE,
                 interface - bushing_floor + THRUST_HEIGHT + .2, "cut", n["bushing"]),
        cylinder(n["retainer"], xy, base, s["pin_radius"], head_top - base),
        cylinder(n["prefix"] + " axial retaining head", xy, head_top - HEAD_HEIGHT,
                 s["head_radius"], HEAD_HEIGHT, "join", n["retainer"]),
    ]
    result += socket_commands(s)
    if s["key"] == "dish":
        # The original Ø20 housing overlaps the four actual mounting heads by
        # about 21.33 mm³ each. Local scallops clear them without moving bolts.
        for dx in (-7, 7):
            for dy in (-8, 8):
                result.append(cylinder(f"{n['prefix']} mounting head clearance {dx}:{dy}",
                                       [xy[0] + dx, xy[1] + dy], base - .1, 2.9, 3.3,
                                       "cut", s["housing"]))
    result.extend([
        c("component.activate", component=s["rotor_component"]),
        cylinder(n["prefix"] + " thrust seat allowance", xy, interface - .1,
                 s["rotor_radius"] + .1, THRUST_HEIGHT + .1, "cut", s["rotor"]),
        cylinder(n["prefix"] + " integral rotating journal", xy, journal_base,
                 s["journal_radius"], interface + .8 - journal_base, "join", s["rotor"]),
        cylinder(n["prefix"] + " stationary pin clearance", xy, journal_base - .1,
                 s["pin_radius"] + PIN_CLEARANCE, top - journal_base + .2, "cut", s["rotor"]),
        cylinder(n["prefix"] + " retaining head recess", xy, top - RECESS_DEPTH,
                 s["head_radius"] + RADIAL_CLEARANCE, RECESS_DEPTH + .1, "cut", s["rotor"]),
    ])
    return result


def commands() -> list[dict]:
    result = []
    for support in SUPPORTS:
        result += support_commands(support)
    bushings = [names(s)["bushing"] for s in SUPPORTS]
    retainers = [names(s)["retainer"] for s in SUPPORTS]
    rotors = [s["rotor"] for s in SUPPORTS]
    housings = [s["housing"] for s in SUPPORTS]
    result.extend([
        c("material.assign", bodies=bushings, material="Brass"),
        c("appearance.assign", bodies=bushings, color=[187, 147, 73],
          pbr={"metallic": 1, "roughness": .3, "dielectric_ior": 1.5}),
        c("material.assign", bodies=retainers + rotors, material="Steel"),
        c("appearance.assign", bodies=retainers + rotors, color=[169, 181, 195],
          pbr={"metallic": 1, "roughness": .26, "dielectric_ior": 1.5}),
        c("material.assign", bodies=housings, material="Aluminum"),
        c("appearance.assign", bodies=housings, color=[77, 92, 109],
          pbr={"metallic": 1, "roughness": .34, "dielectric_ior": 1.5}),
        c("component.activate", component="root"),
    ])
    return result


def stages() -> list[tuple[str, list[dict]]]:
    return [("yaw-supports", commands())]


def fixture_commands() -> list[dict]:
    """Small six-component fixture with exact admitted support envelopes."""
    result = [c("component.activate", component="root")]
    anchors = []
    for s in SUPPORTS:
        for name, z, radius, height in (
            (s["housing"], s["base"], s["fixed_radius"], s["interface"] - s["base"]),
            (s["rotor"], s["interface"], s["rotor_radius"], s["top"] - s["interface"]),
        ):
            result.append(cylinder(name, s["center"], z, radius, height))
            anchors.append(name)
    result.append(c("component.from_bodies", bodies=anchors))
    for s in SUPPORTS:
        result.extend([
            c("component.rename", component=s["housing"], name=s["fixed_component"]),
            c("component.rename", component=s["rotor"], name=s["rotor_component"]),
            c("occurrence.ground", occurrence=s["fixed_component"] + ":1", grounded=True),
            c("joint.as_built", type="revolute", a=s["fixed_component"], b=s["rotor_component"],
              at={"occurrence": s["fixed_component"], "point": [*s["center"], s["interface"]],
                  "z": [0, 0, 1]}, name=s["joint"], limits=[[-s["limit"], s["limit"]]], quiet=True),
        ])
    result.append(c("component.activate", component="Dish pedestal"))
    for dx in (-7, 7):
        for dy in (-8, 8):
            name = f"Dish mounting foot M3 ({float(dx)}, {float(dy)}) head"
            xy = [-177 + dx, dy]
            result.extend([
                cylinder(name, xy, 130, 2.76, 3),
                cylinder(name + " bore", xy, 129.9, 1.2, 3.2, "cut", name),
            ])
    # The source's nearby attachment solids are reproduced so a hidden retaining
    # head cannot be accepted merely because an obstructing mast was omitted.
    result.append(c("component.activate", component="Arm turret"))
    for y in (-13, 9):
        result.append(c("solid.box", corner=[164, y, 146], length=26, width=4, height=28,
                        operation="new", body_name=f"Shoulder clevis {y}", name=f"Shoulder clevis {y}"))
    result.extend([
        c("component.activate", component="Dish yaw mount"),
        cylinder("Dish mast", [-177, 0], 139, 5, 46),
        c("component.activate", component="Turret yaw"),
        c("solid.box", corner=[-53, -8, 163], length=16, width=16, height=52,
          operation="new", body_name="Central hollow mast", name="Central hollow mast"),
    ])
    result += commands()
    return result


def expected_volumes(s: dict) -> dict[str, float]:
    """Independent analytic volumes; housing scallops are checked separately."""
    n = names(s)
    journal, inner = s["journal_radius"], s["pin_radius"] + PIN_CLEARANCE
    h = s["interface"] - s["base"]
    rotor_h = s["top"] - s["interface"]
    bushing = math.pi * ((s["bushing_radius"] ** 2 - (journal + RADIAL_CLEARANCE) ** 2) * (h - FLOOR)
                         + (s["thrust_radius"] ** 2 - (journal + RADIAL_CLEARANCE) ** 2) * THRUST_HEIGHT)
    rotor = math.pi * (s["rotor_radius"] ** 2 * (rotor_h - THRUST_HEIGHT)
                       + journal ** 2 * (h - FLOOR)
                       - inner ** 2 * (s["top"] - (s["base"] + FLOOR + .3))
                       - ((s["head_radius"] + RADIAL_CLEARANCE) ** 2 - inner ** 2) * RECESS_DEPTH)
    retainer = math.pi * (s["pin_radius"] ** 2 * (s["top"] - HEAD_END_CLEARANCE - s["base"])
                          + (s["head_radius"] ** 2 - s["pin_radius"] ** 2) * HEAD_HEIGHT)
    retainer -= 3 * math.sqrt(3) / 2 * (s["head_radius"] * .35) ** 2 * SOCKET_DEPTH
    return {n["bushing"]: bushing, s["rotor"]: rotor, n["retainer"]: retainer}


def manifest() -> dict:
    return {
        "scope": "Three plain journal/thrust supports with recessed axial retention; ideal kinematics and illustrative fits",
        "new_bodies": 6, "new_components": 0, "new_joints": 0,
        "joint_frames_changed": False, "mast_and_clevis_attachment_heights_changed": False,
        "radial_journal_clearance_mm": RADIAL_CLEARANCE,
        "pin_to_rotor_radial_clearance_mm": PIN_CLEARANCE,
        "head_to_recess_radial_clearance_mm": RADIAL_CLEARANCE,
        "axial_retention_play_mm": RECESS_DEPTH - HEAD_HEIGHT - HEAD_END_CLEARANCE,
        "journal_end_clearance_mm": .3, "housing_floor_mm": FLOOR,
        "thrust_flange_mm": THRUST_HEIGHT,
        "mounting_head_clearance": {"source_head_radius_mm": 2.76, "relief_radius_mm": 2.9,
                                    "source_overlap_mm3_approx_each": 21.33, "count": 4},
        "supports": [{**s, **names(s), "expected_volumes_mm3": expected_volumes(s)} for s in SUPPORTS],
        "limitations": ["Retainer threads are represented by nominal cylindrical seats.",
                        "No friction, wear, preload, structural strength or fit class is certified.",
                        "This stage does not change or add joint degrees of freedom."],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fixture", action="store_true")
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    steps = fixture_commands() if args.fixture else commands()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({"commands": steps}, indent=2) + "\n")
    if args.manifest:
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        args.manifest.write_text(json.dumps(manifest(), indent=2) + "\n")
    print(f"{len(steps)} native commands")


if __name__ == "__main__":
    main()
