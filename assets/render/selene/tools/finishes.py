#!/usr/bin/env python3
"""Assign physical proxies and opaque PBR finishes through native commands.

The author's palette is retained. Density describes an explicitly simplified
part material, not a verified rover mass budget. Optical glass remains separate
from the opaque PBR contract. No serialized design state is rewritten.
"""

from __future__ import annotations

from collections import defaultdict

SOURCE_SHA256 = "1b0df486f30da157db27a33e0b5ed67f13d527d4dfdc6c334717a1df4e5b8677"


def command(command_id: str, **params: object) -> dict:
    return {"command": command_id, "params": params}


CUSTOM_MATERIALS = {
    "SELENE elastomer proxy": (1.20, [30, 35, 41]),
    "SELENE FR4 proxy": (1.85, [39, 104, 72]),
    "SELENE silicon proxy": (2.33, [29, 66, 107]),
    "SELENE rock proxy": (2.80, [122, 118, 110]),
}


def material_for(name: str) -> str:
    n = name.lower()
    if any(s in n for s in ("glass sample", "glass vial", "lens glass", "searchlight glass")):
        return "Glass"
    if any(s in n for s in ("tpu wheel", "tread grouser", "flexible hose", "battery strap", "bumper")):
        return "SELENE elastomer proxy"
    if any(s in n for s in ("raspberry pi", "management pcb", "driver and power pcb")):
        return "SELENE FR4 proxy"
    if "photovoltaic tile" in n:
        return "SELENE silicon proxy"
    if any(s in n for s in ("basalt", "sample material", "track ")):
        return "SELENE rock proxy"
    if any(s in n for s in ("contact", "terminal", "rf connector", "retention sleeve")):
        return "Brass"
    if any(s in n for s in ("shank", "screw", "locknut", "washer", "shaft", "bearing", "spring", "pin ", "pin cap", "grub", "rod ", "piston", "whip")):
        return "Steel"
    if any(s in n for s in ("diffuser", "led lens", "led element", "led package", "envelope", "end cap", "cell wrapper", "cradle", "tray", "motor lead", "battery retainer", "light pipe")):
        return "ABS Plastic"
    return "Aluminum"


def optical_for(name: str, material: str, original: dict) -> tuple:
    n = name.lower()
    rgb = tuple(original.get("color", [146, 159, 174]))
    opacity = 1.0
    if any(s in n for s in ("glass sample", "glass vial")):
        return rgb, 0.30, None
    if any(s in n for s in ("lens glass", "searchlight glass", "underbody led diffuser")):
        return rgb, 0.46, None
    metallic, roughness = 0.0, 0.48
    if material == "SELENE elastomer proxy":
        roughness = 0.79
    elif material == "SELENE rock proxy":
        roughness = 0.93
    elif material == "Steel":
        metallic, roughness = 1.0, 0.28
    elif material == "Brass":
        metallic, roughness = 1.0, 0.30
    elif material == "Aluminum":
        metallic, roughness = 1.0, 0.32
    elif material == "SELENE silicon proxy":
        metallic, roughness = 0.14, 0.22
    elif material == "SELENE FR4 proxy":
        roughness = 0.44
    if rgb == (233, 137, 53):
        metallic, roughness = 0.0, 0.39
    elif rgb == (38, 49, 61) and material in ("Aluminum", "Steel"):
        metallic, roughness = 0.22, 0.49
    if "led" in n or "lamp" in n:
        roughness = 0.25 if material == "ABS Plastic" else roughness
    return rgb, opacity, (metallic, roughness, 1.5)


def commands(inventory: dict, source_document: dict) -> list[dict]:
    bodies = inventory["bodies"]
    names = [body["name"] for body in bodies]
    if len(names) != len(set(names)):
        raise ValueError("Evaluated body names must be unique.")
    original = source_document.get("appearances", {}).get("bodies", {})
    result = [command("component.activate", component="root")]
    for name, (density, color) in CUSTOM_MATERIALS.items():
        result.append(command("material.create", name=name, density_g_cm3=density, color=color))
    physical: dict[str, list[str]] = defaultdict(list)
    optical: dict[tuple, list[str]] = defaultdict(list)
    for name in names:
        material = material_for(name)
        physical[material].append(name)
        # Pattern copies inherit their seed's established tyre/grouser identity.
        look = original.get(name, {})
        if not look and name.startswith("Tread grouser"):
            look = {"color": [32, 41, 52]}
        optical[optical_for(name, material, look)].append(name)
    for material, targets in sorted(physical.items()):
        result.append(command("material.assign", bodies=targets, material=material))
    for (color, opacity, pbr), targets in optical.items():
        params = {"bodies": targets, "color": list(color), "opacity": opacity}
        if pbr is not None:
            params["pbr"] = dict(zip(("metallic", "roughness", "dielectric_ior"), pbr))
        result.append(command("appearance.assign", **params))
    hidden = [name for name in names if name.startswith("Track ") or "flexible hose" in name.lower()]
    result.append(command("browser.visibility", items=hidden, visible=False))
    for folder in ("sketches", "joints", "origin"):
        result.append(command("browser.visibility", folder=folder, visible=False))
    result.append(command("document.metadata.set", values={
        "project": "SELENE render and mechanism showcase",
        "part_number": "SELENE-FUNCTIONAL",
        "revision": "1.0",
        "description": "Enhanced native assembly based on the author-supplied SELENE showcase. See repository validation and scope notes.",
        "io.tangency.selene": {
            "source_sha256": SOURCE_SHA256,
            "source_commit": "32ec1720447e3fcc6bb6cc53a63fb5b39cfa21b9",
            "units": "mm", "up_axis": "+Z",
            "physical_scope": "Nominal material-density proxies; not a verified manufacturing BOM or mass budget.",
            "electronics_scope": "Visual mechanical reconstruction; not an electrically functional PCB.",
        },
    }))
    return result
