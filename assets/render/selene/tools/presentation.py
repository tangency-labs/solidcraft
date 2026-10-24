#!/usr/bin/env python3
"""Saved native views, browser groups and closed sample-vial glass bodies."""

from __future__ import annotations

from collections import defaultdict


def c(command: str, **params: object) -> dict:
    return {"command": command, "params": params}


VIEWS = {
    "Whole rover": ([87, 0, 145], -.66, .53, 760),
    "Electronics": ([64.5, 0, 123], -1.05, .78, 132),
    "Shoulder bearing": ([177, -11, 165], -.30, .25, 47),
    "Elbow bearing": ([242, -7, 281], -.38, .28, 58),
    "Gripper": ([449, 0, 252], -.45, .85, 84),
    "Sensor head": ([-45, 0, 250], .20, .20, 160),
    "Sample laboratory": ([65, 0, 170], -.70, .80, 145),
    "Drivetrain": ([0, -85, 57], 0, .17, 370),
    "Suspension": ([-96, -55, 81], .10, .26, 114),
    "Solar cells": ([0, -187, 182], .15, .90, 240),
}


def camera(name: str) -> dict:
    target, yaw, pitch, distance = VIEWS[name]
    return {"target": target, "yaw": yaw, "pitch": pitch, "distance": distance, "fov": 0}


def body_owners(document: dict) -> dict[str, int]:
    owners = dict(document.get("body_components", {}))
    for feature in document["features"]:
        if feature.get("suppressed"):
            continue
        for name in feature.get("body_names", []):
            owners.setdefault(name, feature.get("component", 0))
        if feature["type"] == "body_transfer":
            for name in feature["bodies"]:
                owners[name] = feature["to"]
    return owners


def category(name: str) -> str:
    n = name.lower()
    if name.startswith("Pi5 "):
        if any(word in n for word in ("gpio", "mounting pad", "via")):
            return "GPIO, mounting pads and plated vias"
        if any(word in n for word in ("contact", "solder", "pad ")):
            return "Contacts and solder joints"
        if any(word in n for word in ("resistor", "capacitor", "passive", "inductor", "crystal")):
            return "Passive components and timing"
        if any(word in n for word in ("bcm", "rp1", "lpddr", "da9091", "phy", "wireless", "rf shield")):
            return "Processing, power and radio"
        return "Connector housings and interfaces"
    if any(word in n for word in ("screw", "shank", "washer", "locknut", "pin cap", "grub")):
        return "Fasteners and retention"
    if any(word in n for word in ("glass", "lens", "diffuser", " led", "reflector", "photovoltaic")):
        return "Optics and active surfaces"
    if any(word in n for word in ("cage", "ball ", "bearing", "shaft", "bushing", "hub", "piston")):
        return "Bearing and drive hardware"
    return "Structure and housings"


def commands(document: dict, inventory: dict) -> list[dict]:
    alive = {body["name"] for body in inventory["bodies"]}
    result = [
        c("occurrence.ground", occurrence="Rock sample:1", grounded=True),
        c("material.assign", bodies=["Battery management board"], material="SELENE FR4 proxy"),
        c("appearance.assign", bodies=["Battery management board"], color=[39, 104, 72], opacity=1,
          pbr={"metallic": 0, "roughness": .44, "dielectric_ior": 1.5}),
        c("component.activate", component="Sample laboratory"),
    ]
    # The original touching wall/bottom union fails in the native kernel. Rebuild
    # one blind-bored cup with the same exterior and an integral 1 mm bottom.
    # Source Move9 baked +213 X/+14 Z into these bodies; their occurrence is identity.
    vials = []
    for x in (-160, -136):
        for y in (-22, 0, 22):
            wall, bottom = f"Glass sample vial ({x}, {y})", f"Glass vial bottom ({x}, {y})"
            if wall not in alive or bottom not in alive:
                raise ValueError(f"Missing evaluated vial bodies: {wall}, {bottom}")
            result.extend([
                c("solid.remove", bodies=[wall, bottom], name=f"SELENE retire split vial ({x}, {y})"),
                c("solid.cylinder", base=[x + 213, y, 152], radius=5.8, height=27,
                  operation="new", body_name=wall, name=f"SELENE closed vial envelope ({x}, {y})"),
                c("solid.cylinder", base=[x + 213, y, 153], radius=5, height=27,
                  operation="cut", targets=[wall], name=f"SELENE closed vial blind bore ({x}, {y})"),
            ])
            alive.remove(bottom)
            vials.append(wall)
    result.extend([
        c("material.assign", bodies=vials, material="Glass"),
        c("appearance.assign", bodies=vials, color=[198, 231, 232], opacity=.30),
    ])
    result.append(c("component.activate", component="root"))
    owners = body_owners(document)
    missing = sorted(alive - owners.keys())
    if missing:
        raise ValueError(f"Evaluated bodies have unresolved component ownership: {missing[:5]}")
    grouped = defaultdict(list)
    for name in sorted(alive):
        grouped[(owners.get(name, 0), category(name))].append(name)
    for (component, group), names in sorted(grouped.items()):
        if len(names) >= 3:
            result.append(c("browser.group", component=component, folder="bodies", name=group, items=names))
    for name in VIEWS:
        result.append(c("view.save", name="SELENE - " + name, camera=camera(name)))
        result.append(c("render.settings.set", name="SELENE - " + name, set={
            "camera": camera(name), "width": 1600, "height": 1200,
            "background": [232, 236, 242], "environment_intensity": 1.0,
        }))
    metadata = dict(document.get("metadata", {}).get("io.tangency.selene", {}))
    metadata.update({
        "revision": "1.0",
        "saved_pose": "Neutral: primary independent drivers at zero, including sensor elevation.",
        "bearing_model": "Ten open radial bearings; grooved steel races, eight balls each, brass cages. Ideal zero-contact-angle rolling relations; no load/contact/friction solver.",
        "suspension_model": "Six hinged trailing carriers with closed-loop telescoping dampers; -4 to +1.5 degrees. Rigid spring and lead geometry are neutral-pose display details.",
        "gripper_model": "Two independently represented pivoting jaws linked 1:-1, with plain sleeves and elastomer pads.",
        "render_scope": "Ten common camera/setup records and opaque PBR appearances. Transparent viewport looks require separately admitted runtime dielectrics for physical glass rendering.",
        "moving_detail_policy": "Hide rigid suspension springs and motor leads in motion close-ups; source rigid hydraulic display cylinders were removed from the arm.",
        "sample_rock_policy": "The source rock is grounded as demonstration scenery; it is not a simulated grasped or released body.",
        "components": [{"name": x["name"], "part_number": "SELENE-" + str(x["id"])} for x in document["components"]],
    })
    result.append(c("document.metadata.set", key="io.tangency.selene", value=metadata))
    for folder in ("sketches", "joints", "origin"):
        result.append(c("browser.visibility", folder=folder, visible=False))
    result.append(c("view.apply", view="SELENE - Whole rover"))
    result.append(c("component.activate", component="root"))
    return result


def visibility_commands(inventory: dict, profile: str) -> list[dict]:
    names = [body["name"] for body in inventory["bodies"]]
    if profile == "electronics":
        selected = [name for name in names if name.startswith(("Pi5 ", "Pi M2.5")) or name == "Raspberry Pi 5 PCB"]
    elif profile == "bearing":
        selected = [name for name in names if name.startswith("Bearing Elbow port")]
    elif profile == "motion":
        selected = [name for name in names if not name.startswith(("Track ", "Suspension spring ", "Motor lead "))
                    and "flexible hose" not in name.lower()]
    elif profile == "whole":
        selected = [name for name in names if not name.startswith("Track ") and "flexible hose" not in name.lower()]
    else:
        raise ValueError("Unknown visibility profile.")
    return [c("browser.visibility", folder="bodies", visible=False),
            c("browser.visibility", items=selected, visible=True)]
