#!/usr/bin/env python3
"""Native SELENE trailing-arm suspension and closed-loop coilovers.

The original six Drive modules become limited revolute assemblies. Each wheel
is reparented to its module, and each coilover is split into a barrel and rod.
The model is an ideal kinematic mechanism, not an elastic spring or force model.
Rigid spring and motor-lead geometry is retained for the neutral render pose;
motion profiles must hide it. All command angles are degrees and lengths mm.
Original piston and suspension-ring body labels are retained: late renames force
expensive source-history rebuilds. Their physical roles are stated in manifest().

``stages()`` bounds geometry batches to two wheel modules each, followed by
finishes and assembly. Twelve component anchors are collected in one from_bodies
command, avoiding a full-document rebuild per component. Recreate motion studies
after the assembly stage: deleting the old wheel joints retires their study keys.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

POSITIONS = [(side, x) for side in (-1, 1) for x in (-90, 0, 90)]
SOURCE_DRIVE_OCCURRENCES = [2109, 2192, 2275, 2358, 2441, 2524]
HINGE_LIMITS = (-4.0, 1.5)
STROKE_LIMITS = (-3.0, 1.0)


def c(command_id: str, **params) -> dict:
    return {"command": command_id, "params": params}


def names(side: int, x: int) -> dict[str, str]:
    suffix = f"{side}:{x}"
    return {
        "suffix": suffix, "drive": f"Drive module {suffix}", "wheel": f"Wheel {suffix}",
        "barrel": f"SELENE suspension barrel {suffix}", "rod": f"Damper piston {suffix}",
        "hinge": f"SELENE suspension hinge {suffix}", "root": f"SELENE suspension root pin {suffix}",
        "stroke": f"SELENE suspension stroke {suffix}", "tip": f"SELENE suspension tip pin {suffix}",
        "bushing": f"Suspension bearing {suffix}:{x - 20}",
        "spacer": f"Suspension bearing {suffix}:{x}",
    }


def stroke(angle_deg: float) -> float:
    """Independent pin-distance relation: L(theta) - neutral length 22 mm."""
    t = math.radians(angle_deg)
    return math.hypot(35 * math.cos(t) - 20 * math.sin(t) - 35,
                      -35 * math.sin(t) - 20 * math.cos(t) - 2) - 22


def ring(name, point, axis, outer, inner, length):
    if axis == [0, 0, 1]:
        xd, yd = [1, 0, 0], [0, 1, 0]
    elif axis == [0, 1, 0]:
        xd, yd = [1, 0, 0], [0, 0, -1]
    else:
        raise ValueError("The suspension only needs +Z and +Y annular profiles.")
    return [
        c("sketch.create", name=name + " profile", plane={"origin": point, "x_dir": xd, "y_dir": yd}),
        c("sketch.circle.center", center=[0, 0], radius=outer),
        c("sketch.circle.center", center=[0, 0], radius=inner),
        c("sketch.constraint.fix", entities=["c1", "c2"], fixed=True),
        c("sketch.finish"),
        c("solid.extrude", sketch=name + " profile", profiles=[{"point": [(outer + inner) / 2, 0]}],
          distance=length, operation="new", body_name=name, name=name + " extrusion"),
    ]


def barrel_commands(side, x):
    n = names(side, x)
    result = ring(n["barrel"], [x + 15, side * 60, 85], [0, 0, 1], 3.4, 1.8, 10)
    # The source neck ended at the upper pin centre and intersected the pin.
    # A short closed end below the eye bore gives a real barrel-to-eye bridge.
    result.append(c("solid.cylinder", base=[x + 15, side * 60, 94], radius=2.1, height=2,
                    operation="join", targets=[n["barrel"]], name=n["barrel"] + " upper neck"))
    return result


def gusset_commands(side, x):
    result = []
    y = side * 54 + (1 if side == 1 else -4)
    # Both ribs sit outboard of the carrier's M3 heads, bridge the arm to its
    # fixed carrier and clear the retained carrier axle. Their interfaces are
    # welded visual overlaps, not claimed press-fit or fastener tolerance data.
    for hand in (-1, 1):
        name = f"SELENE suspension carrier gusset {side}:{x}:{hand}"
        points = [[hand * 10, -67.9], [hand * 3.2, -67.9], [hand * 7, -74.0]]
        result.extend([
            c("sketch.create", name=name + " profile",
              plane={"origin": [x, y, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}),
            c("sketch.line", points=points, closed=True, infer=False),
            c("sketch.constraint.fix", entities=["l1", "l2", "l3"], fixed=True),
            c("sketch.finish"),
            c("solid.extrude", sketch=name + " profile", distance=3, operation="new",
              body_name=name, name=name + " extrusion"),
        ])
    return result


def geometry_commands_for_positions(positions) -> list[dict]:
    result = []
    for side, x in positions:
        n = names(side, x)
        suffix = n["suffix"]
        result.append(c("component.activate", component="Structure"))
        result.append(c("solid.cylinder", base=[x - 20, side * 54 - 3, 96], axis=[0, 1, 0],
                        radius=9.2, height=6, operation="cut", targets=[f"Suspension chassis shoe {suffix}"],
                        name=f"SELENE trailing arm shoe clearance {suffix}"))
        result.append(c("solid.cylinder", base=[x + 15, side * 60 - 3.1, 98], axis=[0, 1, 0],
                        radius=3.15, height=6.2, operation="cut", targets=[f"Coilover upper foot {suffix}"],
                        name=f"SELENE coilover eye foot clearance {suffix}"))
        result.append(c("component.activate", component=n["drive"]))
        result.append(c("solid.remove", bodies=[f"Gold suspension damper {suffix}",
                                                 f"Damper upper neck {suffix}"],
                        name=f"SELENE retire solid damper {suffix}"))
        # Give the piston a genuine overlap with its eye, below the eye's bore.
        result.append(c("solid.cylinder", base=[x + 15, side * 60, 77.8], radius=1.7, height=1.4,
                        operation="join", targets=[f"Damper piston {suffix}"],
                        name=f"SELENE piston eye bridge {suffix}"))
        # The barrel neck is now Ø4.2 rather than the source's Ø3.4. Open the
        # spring-seat clearance to Ø4.3 without changing its outer envelope.
        result.append(c("solid.cylinder", base=[x + 15, side * 60, 94.8], radius=2.15, height=2,
                        operation="cut", targets=[f"Spring upper seat {suffix}"],
                        name=f"SELENE barrel neck seat clearance {suffix}"))
        result.append(c("material.assign", bodies=[n["bushing"]], material="Brass"))
        result.append(c("appearance.assign", bodies=[n["bushing"]], color=[189, 148, 73],
                        pbr={"metallic": 1, "roughness": .3, "dielectric_ior": 1.5}))
        result.append(c("material.assign", bodies=[n["spacer"]], material="Aluminum"))
        result.append(c("component.activate", component="root"))
        result.extend(barrel_commands(side, x))
        result.extend(gusset_commands(side, x))
    return result


def finishing_commands() -> list[dict]:
    barrels = [names(side, x)["barrel"] for side, x in POSITIONS]
    rods = [names(side, x)["rod"] for side, x in POSITIONS]
    gussets = [f"SELENE suspension carrier gusset {side}:{x}:{hand}"
               for side, x in POSITIONS for hand in (-1, 1)]
    return [
        c("material.assign", bodies=barrels + gussets, material="Aluminum"),
        c("appearance.assign", bodies=barrels, color=[175, 128, 46],
          pbr={"metallic": 1, "roughness": .28, "dielectric_ior": 1.5}),
        c("appearance.assign", bodies=gussets, color=[89, 102, 116],
          pbr={"metallic": 1, "roughness": .32, "dielectric_ior": 1.5}),
        c("material.assign", bodies=rods, material="Steel"),
        c("appearance.assign", bodies=rods, color=[180, 190, 201],
          pbr={"metallic": 1, "roughness": .18, "dielectric_ior": 1.5}),
        c("component.activate", component="root"),
    ]


def geometry_commands() -> list[dict]:
    return ([c("component.activate", component="root")]
            + geometry_commands_for_positions(POSITIONS) + finishing_commands())


def loop_commands(side, x) -> list[dict]:
    n = names(side, x)
    a, root, tip = [x - 20, side * 54, 96], [x + 15, side * 60, 98], [x + 15, side * 60, 76]
    result = [c("joint.as_built", type="revolute", a="Structure", b=n["drive"],
                at={"occurrence": "Structure", "point": a, "z": [0, 1, 0]},
                name=n["hinge"], limits=[list(HINGE_LIMITS)], quiet=True)]
    for kind, name, left, right, point, axis, limits in (
        ("revolute", n["root"], "Structure", n["barrel"], root, [0, 1, 0], None),
        ("slider", n["stroke"], n["barrel"], n["rod"], root, [0, 0, -1], [list(STROKE_LIMITS)]),
        ("revolute", n["tip"], n["rod"], n["drive"], tip, [0, 1, 0], None),
    ):
        params = {"type": kind, "name": name, "flip": True, "quiet": True,
                  "a": {"occurrence": left, "point": point, "z": axis},
                  "b": {"occurrence": right, "point": point, "z": axis}}
        if limits is not None:
            params["limits"] = limits
        result.append(c("joint.create", **params))
    return result


def assembly_commands() -> list[dict]:
    result = [c("component.activate", component="root")]
    anchors = [names(side, x)[part] for side, x in POSITIONS for part in ("barrel", "rod")]
    result.append(c("component.from_bodies", bodies=anchors))
    for side, x in POSITIONS:
        n, suffix = names(side, x), f"{side}:{x}"
        result.append(c("component.move_bodies", component=n["barrel"], bodies=[
            f"Coilover upper eye {suffix}", f"Spring upper seat {suffix}",
        ]))
        result.append(c("component.move_bodies", component=n["rod"], bodies=[
            f"Coilover lower eye {suffix}", f"Spring lower seat {suffix}",
        ]))
        upper_fixed = [f"Suspension shaft {suffix}:{x - 20}",
                       f"Suspension retaining washer {suffix}:{x - 20}",
                       f"Coilover upper pin {suffix}"]
        upper_fixed += [f"Coilover upper pin cap {suffix}:{y:g}" for y in (side * 60 - 7.8, side * 60 + 6)]
        upper_fixed += [f"Motor lead ({side}, {x}, {offset}) floor gland" for offset in (-3, 3)]
        result.append(c("component.move_bodies", component="Structure", bodies=upper_fixed))
        result.append(c("component.move_bodies", component=n["drive"], bodies=[
            f"SELENE suspension carrier gusset {suffix}:{hand}" for hand in (-1, 1)
        ]))

    # These exact original joint names belong to the admitted native source.
    # Deletion removes their old dependent links and study keys. Recreate the
    # inspection studies after this module using joint names, not saved ids.
    for occurrence in SOURCE_DRIVE_OCCURRENCES:
        result.append(c("joint.delete", joint=f"Rigid Group 1930:{occurrence}"))
    for side, x in POSITIONS:
        result.append(c("joint.delete", joint=f"Wheel axis {side}:{x}"))
    for side, x in POSITIONS:
        n = names(side, x)
        result.extend(loop_commands(side, x))
        result.append(c("joint.as_built", type="revolute", a=n["drive"], b=n["wheel"],
                        at={"occurrence": n["drive"], "point": [x, side * 86, 40], "z": [0, 1, 0]},
                        name=f"Wheel axis {side}:{x}", quiet=True))
    wheels = [f"Wheel axis {side}:{x}" for side, x in POSITIONS]
    for first, second in zip(wheels, wheels[1:]):
        result.append(c("joint.motion_link", a=first, b=second, ratio=1, offset=0, quiet=True))
    result.append(c("joint.rack_pinion", a=wheels[0], b="Track slide", radius=40, reverse=True, quiet=True))
    result.append(c("component.activate", component="root"))
    return result


def stages() -> list[tuple[str, list[dict]]]:
    result = []
    for index, start in enumerate(range(0, len(POSITIONS), 2), 1):
        batch = geometry_commands_for_positions(POSITIONS[start:start + 2])
        if index == 1:
            batch.insert(0, c("component.activate", component="root"))
        result.append((f"suspension-geometry-{index}", batch))
    result.append(("suspension-finishes", finishing_commands()))
    result.append(("suspension-assembly", assembly_commands()))
    return result


def commands() -> list[dict]:
    return geometry_commands() + assembly_commands()


def fixture_commands(side: int = -1, x: int = -90) -> list[dict]:
    """Four-component analytic fixture with genuine barrel, eyes and pins."""
    n, sy = names(side, x), side * 60
    result = [c("component.activate", component="root"),
              c("solid.box", corner=[x - 29, side * 50 - 11, 101], length=47, width=22, height=4,
                operation="new", body_name="Structure", name="Fixture chassis shoe"),
              c("solid.cylinder", base=[x - 20, side * 54 - 3, 96], axis=[0, 1, 0],
                radius=9.2, height=6, operation="cut", targets=["Structure"], name="Fixture arm clearance"),
              c("solid.box", corner=[x + 10, sy - 6, 99], length=10, width=12, height=3,
                operation="new", body_name="Fixture upper mounting foot", name="Fixture upper mounting foot"),
              c("solid.cylinder", base=[x + 15, sy - 3.1, 98], axis=[0, 1, 0],
                radius=3.15, height=6.2, operation="cut", targets=["Fixture upper mounting foot"],
                name="Fixture eye clearance"),
              c("sketch.create", name="Fixture trailing arm profile",
                plane={"origin": [0, side * 54 - 2, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}),
              c("sketch.slot.center_to_center", p0=[x - 20, -96], p1=[x, -78], width=17),
              c("sketch.circle.center", center=[x - 20, -96], radius=6.1),
              c("sketch.circle.center", center=[x, -78], radius=6.1),
              c("sketch.constraint.fix", entities=["l1", "a1", "l2", "a2", "c1", "c2"], fixed=True),
              c("sketch.finish"),
              c("solid.extrude", sketch="Fixture trailing arm profile", profiles=[{"point": [x - 10, -87]}],
                distance=4, operation="new", body_name=n["drive"], name="Fixture trailing arm extrusion")]
    result += barrel_commands(side, x)
    result.append(c("solid.cylinder", base=[x + 15, sy, 79], radius=1.7, height=8,
                    operation="new", body_name=n["rod"], name="Fixture piston"))
    result.append(c("solid.cylinder", base=[x + 15, sy, 77.8], radius=1.7, height=1.4,
                    operation="join", targets=[n["rod"]], name="Fixture piston eye bridge"))
    result += ring("Fixture upper eye", [x + 15, sy - 3, 98], [0, 1, 0], 3, 1.75, 6)
    result += ring("Fixture lower eye", [x + 15, sy - 3, 76], [0, 1, 0], 3, 1.75, 6)
    result += ring("Fixture plain bushing", [x - 20, side * 54 - 2, 96], [0, 1, 0], 6, 3.05, 4)
    result += ring("Fixture upper spring seat", [x + 15, sy, 95], [0, 0, 1], 6.2, 1.75, 1.5)
    result.append(c("solid.cylinder", base=[x + 15, sy, 94.8], radius=2.15, height=2,
                    operation="cut", targets=["Fixture upper spring seat"], name="Fixture neck seat clearance"))
    result.append(c("solid.box", corner=[x - 17, side * 54 - 12, 65], length=34, width=24, height=3,
                    operation="new", body_name="Fixture motor carrier", name="Fixture motor carrier"))
    result += gusset_commands(side, x)
    for name, point, radius, length in (
        ("Fixture upper pin", [x + 15, sy - 7, 98], 1.7, 14),
        ("Fixture lower pin", [x + 15, sy - 7, 76], 1.7, 14),
        ("Fixture pivot axle", [x - 20, side * 54 - 7.5, 96], 3, 15),
    ):
        result.append(c("solid.cylinder", base=point, axis=[0, 1, 0], radius=radius,
                        height=length, operation="new", body_name=name, name=name))
    result.extend([
        c("component.from_bodies", bodies=["Structure", n["drive"], n["barrel"], n["rod"]]),
        c("component.move_bodies", component="Structure", bodies=["Fixture upper pin", "Fixture pivot axle", "Fixture upper mounting foot"]),
        c("component.move_bodies", component=n["drive"], bodies=["Fixture lower pin", "Fixture plain bushing", "Fixture motor carrier"]
          + [f"SELENE suspension carrier gusset {side}:{x}:{hand}" for hand in (-1, 1)]),
        c("component.move_bodies", component=n["barrel"], bodies=["Fixture upper eye", "Fixture upper spring seat"]),
        c("component.move_bodies", component=n["rod"], bodies=["Fixture lower eye"]),
        c("occurrence.ground", occurrence="Structure:1", grounded=True),
    ])
    result += loop_commands(side, x)
    result.append(c("component.activate", component="root"))
    return result


def manifest() -> dict:
    return {
        "scope": "Ideal kinematics; no spring force, elastic coils, contact dynamics or cable deformation",
        "source_body_labels_retained": True,
        "label_policy": "Original Damper piston and Suspension bearing body labels are retained to avoid expensive late source-history rebuilds. The rod uses its original piston label; the pivot ring is a brass plain bushing and the carrier ring is a fixed spacer.",
        "new_components": 12, "net_new_joints": 18,
        "bodies_added": 18, "bodies_removed": 12, "net_bodies_added": 6,
        "neutral_damper_pin_spacing_mm": 22, "barrel_outer_diameter_mm": 6.8,
        "barrel_inner_diameter_mm": 3.6, "piston_diameter_mm": 3.4,
        "barrel_with_neck_volume_mm3": math.pi * ((3.4 ** 2 - 1.8 ** 2) * 10 + 2.1 ** 2 * 2 - (2.1 ** 2 - 1.8 ** 2)),
        "piston_volume_mm3": math.pi * 1.7 ** 2 * 9.2,
        "gusset_volume_mm3": .5 * 6.8 * 6.1 * 3,
        "support_clearances_mm": {"arm_shoe_radius": 9.2, "arm_outer_radius": 8.5,
                                  "upper_eye_foot_radius": 3.15, "upper_eye_outer_radius": 3.0},
        "hinge_limits_deg": list(HINGE_LIMITS), "slider_limits_mm": list(STROKE_LIMITS),
        "endpoint_strokes_mm": {str(angle): stroke(angle) for angle in HINGE_LIMITS},
        "modules": [{**names(side, x), "hinge_point_mm": [x - 20, side * 54, 96],
                     "root_point_mm": [x + 15, side * 60, 98], "tip_point_mm": [x + 15, side * 60, 76]}
                    for side, x in POSITIONS],
        "hide_for_motion": [f"Suspension spring {side}:{x}" for side, x in POSITIONS]
                           + [f"Motor lead ({side}, {x}, {offset})" for side, x in POSITIONS for offset in (-3, 3)],
        "rebuild_studies_after_stage": True,
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
