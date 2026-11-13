#!/usr/bin/env python3
"""Native SELENE joints, gripper geometry and inspection studies.

All public command angles are degrees and all lengths are millimetres. The
author's native document remains the input; this module never edits its JSON.
"""

from __future__ import annotations

import math


def c(command: str, **params: object) -> dict:
    return {"command": command, "params": params}


WHEEL_JOINTS = [f"Wheel axis {side}:{x}" for side in (-1, 1) for x in (-90, 0, 90)]
DRIVERS = [
    WHEEL_JOINTS[0], "Port solar tilt articulation",
    "Turret azimuth articulation", "Turret elevation articulation",
    "Arm azimuth articulation", "Shoulder articulation", "Elbow articulation",
    "Wrist articulation", "Tool tilt articulation", "Dish azimuth articulation",
    "Dish elevation articulation", "Vial cap thread",
]
LIMITS = {
    "Turret azimuth articulation": (-135, 135),
    "Turret elevation articulation": (-25, 45),
    "Dish azimuth articulation": (-120, 120),
    "Dish elevation articulation": (-35, 40),
    "Arm azimuth articulation": (-45, 45),
    "Shoulder articulation": (-35, 15),
    "Elbow articulation": (-30, 55),
    "Wrist articulation": (-45, 45),
    "Tool tilt articulation": (-45, 45),
    "Port solar tilt articulation": (0, 30),
    "Starboard solar tilt articulation": (-30, 0),
    "Vial cap thread": (0, 1440),
}
GRIPPER_DRIVER = "SELENE gripper jaw -1 articulation"
GRIPPER_FOLLOWER = "SELENE gripper jaw 1 articulation"


def ring(name: str, point: list, axis: list, outer: float, inner: float, height: float) -> list[dict]:
    """An annular profile avoids a nearly coincident post-extrusion bore."""
    if axis == [0, 0, 1]:
        xd, yd = [1, 0, 0], [0, 1, 0]
    elif axis == [0, 1, 0]:
        xd, yd = [1, 0, 0], [0, 0, -1]
    elif axis == [0, -1, 0]:
        xd, yd = [1, 0, 0], [0, 0, 1]
    else:
        raise ValueError("Only the fixture's axial directions are supported.")
    return [
        c("sketch.create", name=name + " profile", plane={"origin": point, "x_dir": xd, "y_dir": yd}),
        c("sketch.circle.center", center=[0, 0], radius=outer),
        c("sketch.circle.center", center=[0, 0], radius=inner),
        c("sketch.constraint.fix", entities=["c1", "c2"], fixed=True),
        c("sketch.finish"),
        c("solid.extrude", sketch=name + " profile", profiles=[{"point": [(inner + outer) / 2, 0]}],
          distance=height, operation="new", name=name + " extrusion", body_name=name),
    ]


def polygon(name: str, points: list, z: float, height: float) -> list[dict]:
    return [
        c("sketch.create", name=name + " profile", plane="XY", offset=z),
        c("sketch.line", points=points, closed=True, infer=False),
        c("sketch.constraint.fix", entities=[f"l{i+1}" for i in range(len(points))], fixed=True),
        c("sketch.finish"),
        c("solid.extrude", sketch=name + " profile", distance=height, operation="new",
          name=name + " extrusion", body_name=name),
    ]


def primary_commands() -> list[dict]:
    result = [c("component.activate", component="root")]
    for joint in DRIVERS:
        result.append(c("joint.drive", joint=joint, value=0, quiet=True))
    # Independent sensor pointing: the original tour's illustrative gear is not a drivetrain.
    result.append(c("joint.delete", link=7))
    result.append(c("joint.rack_pinion", a=WHEEL_JOINTS[0], b="Track slide", radius=40, reverse=True, link=6, quiet=True))
    for joint, (low, high) in LIMITS.items():
        result.append(c("joint.limits", joint=joint, min=low, max=high))
    # These solid pieces transmit wheel torque, so they belong to the rotating wheel.
    for side in (-1, 1):
        for x in (-90, 0, 90):
            suffix = f"{side}:{x}"
            result.append(c("component.move_bodies", component=f"Wheel {suffix}", bodies=[
                f"4mm D output shaft {suffix}", f"Clamping hub barrel {suffix}",
                f"Hub grub screw {suffix}", f"Hub flange {suffix}",
            ]))
    # Each jaw becomes an actual moving component. The fixed M3 pins stay in the crosshead.
    result.append(c("component.from_bodies", bodies=["Gripper jaw -1", "Gripper jaw 1"]))
    for side in (-1, 1):
        component = f"Gripper jaw {side}"
        result.append(c("component.activate", component=component))
        sleeve = f"SELENE gripper plain sleeve {side}"
        result.extend(ring(sleeve, [443, side * 9, 247], [0, 0, 1], 1.70, 1.55, 5))
        # Rubber pad on the sloped inner gripping surface, inset one millimetre at each end.
        a, b = [459.0, 17 - 6 / 11], [468.0, 11 + 6 / 11]
        norm = math.hypot(11, 6)
        delta = [-0.75 * 6 / norm, -0.75 * 11 / norm]
        pts = [a, b, [b[0] + delta[0], b[1] + delta[1]], [a[0] + delta[0], a[1] + delta[1]]]
        pts = [[x, side * y] for x, y in pts]
        pad = f"SELENE gripper contact pad {side}"
        result.extend(polygon(pad, pts, 248, 3))
        result.extend([
            c("material.assign", bodies=[sleeve], material="Brass"),
            c("appearance.assign", bodies=[sleeve], color=[195, 157, 80], pbr={"metallic": 1, "roughness": .30, "dielectric_ior": 1.5}),
            c("material.assign", bodies=[pad], material="SELENE elastomer proxy"),
            c("appearance.assign", bodies=[pad], color=[26, 30, 35], pbr={"metallic": 0, "roughness": .78, "dielectric_ior": 1.5}),
            c("joint.as_built", type="revolute", a="Gripper tool", b=component,
              at={"occurrence": "Gripper tool", "point": [443, side * 9, 249.5], "z": [0, 0, 1]},
              name=f"SELENE gripper jaw {side} articulation", limits=[[-15, 18] if side == -1 else [-18, 15]], quiet=True),
        ])
    result.extend([
        c("joint.motion_link", a=GRIPPER_DRIVER, b=GRIPPER_FOLLOWER, ratio=-1, offset=0, quiet=True),
        c("component.activate", component="root"),
    ])
    return result


def study_commands(include_suspension: bool = False) -> list[dict]:
    drivers = DRIVERS + [GRIPPER_DRIVER]
    hinges = [f"SELENE suspension hinge {side}:{x}" for side in (-1, 1) for x in (-90, 0, 90)]
    if include_suspension:
        drivers += hinges
    poses = {
        "Drivetrain inspection": {WHEEL_JOINTS[0]: 360},
        "Solar deployment": {"Port solar tilt articulation": 30},
        "Arm articulation": {"Arm azimuth articulation": 18, "Shoulder articulation": -18,
                             "Elbow articulation": 30, "Wrist articulation": -10, "Tool tilt articulation": 15},
        "Gripper inspection": {GRIPPER_DRIVER: 18},
        "Independent sensor pointing": {"Turret azimuth articulation": 30, "Turret elevation articulation": 15,
                                         "Dish azimuth articulation": -25, "Dish elevation articulation": 20},
        "Sample cap inspection": {"Vial cap thread": 720},
    }
    if include_suspension:
        poses["Suspension travel inspection"] = {name: -4 for name in hinges}
    result = []
    for name, goals in poses.items():
        keys = [{"joint": joint, "points": [[0, 0], [30, 0], [80, goals.get(joint, 0)], [100, goals.get(joint, 0)], [120, 0]]}
                for joint in drivers]
        result.append(c("motion.study", name="SELENE - " + name, steps=120, keys=keys))
    return result


def adapted_author_tour(source_document: dict) -> dict:
    study = source_document["assembly"]["studies"][0]
    joints = {j["id"]: j for j in source_document["assembly"]["joints"]}
    keys = []
    for key in study["keys"]:
        joint = joints[key["joint"]]
        angular = joint["kind"] in ("revolute", "cylindrical") and key.get("index", 0) == 0
        points = [[step, math.degrees(value) if angular else value] for step, value in key["points"]]
        if joint["name"] in LIMITS:
            low, high = LIMITS[joint["name"]]
            points = [[step, min(high, max(low, value))] for step, value in points]
        keys.append({"joint": joint["name"], "index": key.get("index", 0), "points": points})
    return c("motion.study", study=study["id"], name="SELENE - Legacy author tour (adapted limits)", steps=120, keys=keys)


def suspension_stroke(angle_deg: float) -> float:
    t = math.radians(angle_deg)
    return math.hypot(35 * math.cos(t) - 20 * math.sin(t) - 35,
                      -35 * math.sin(t) - 20 * math.cos(t) - 2) - 22
