#!/usr/bin/env python3
"""Author SELENE's Raspberry Pi 5 visual componentry through native commands.

The board outline and mounting pattern are from Raspberry Pi's public mechanical
drawing. Package envelopes and connector internals are an original, dimension-
informed visual reconstruction, not a production BOM, electrical design, or a
manufacturer CAD model. All coordinates are millimetres. No native document is
patched: this module only emits ordinary SolidCraft commands.

``commands()`` extends the pinned SELENE source. ``standalone_commands()`` makes
the same board fixture in a new, isolated document for inspection and testing.
The source board already sits at world (22, -28, 120); its early timeline was
translated twice. Components are therefore authored at the evaluated location.
"""

from __future__ import annotations

import argparse
import json
import math
from collections import defaultdict
from pathlib import Path

BOARD = "Raspberry Pi 5 PCB"
COMPONENT = "Raspberry Pi 5 componentry"
ORIGIN = (22.0, -28.0, 120.0)
THICKNESS = 1.6
CORNER_RADIUS = 3.0
GPIO_PITCH = 2.54
GPIO_FIRST = (7.5, 50.0)
MOUNTING = ((3.5, 3.5), (61.5, 3.5), (3.5, 52.5), (61.5, 52.5))
VIA_POSITIONS = ((16, 42), (19, 42), (35, 44), (38, 44), (42, 45), (45, 45),
                 (4, 42), (7, 44), (10, 44), (51, 8), (52.5, 8), (57, 46))
PLACEHOLDERS = ["CPU cooler base", *[f"CPU cooler fin {i}" for i in range(3)],
                *[f"Pi port envelope {i}" for i in range(3)], "GPIO envelope"]
SEMICONDUCTOR = "SELENE Pi5 semiconductor package proxy"
FR4 = "SELENE Pi5 FR4 proxy"

# Built-in physical materials are bulk proxies, independent of visual coatings.
ROLES = {
    "package": ((32, 35, 39), 0.0, 0.61, SEMICONDUCTOR),
    "socket": ((22, 27, 30), 0.0, 0.46, "ABS Plastic"),
    "usb_blue": ((30, 78, 155), 0.0, 0.40, "ABS Plastic"),
    "ceramic": ((173, 144, 95), 0.0, 0.62, SEMICONDUCTOR),
    "ferrite": ((76, 79, 80), 0.0, 0.70, SEMICONDUCTOR),
    "shield": ((182, 190, 198), 1.0, 0.31, "Steel"),
    "aluminum": ((184, 192, 201), 1.0, 0.27, "Aluminum"),
    "solder": ((174, 179, 185), 1.0, 0.39, "Steel"),
    "gold": ((215, 175, 79), 1.0, 0.24, "Brass"),
    "white": ((213, 218, 209), 0.0, 0.55, "ABS Plastic"),
    "led_green": ((74, 173, 70), 0.0, 0.22, "ABS Plastic"),
    "led_amber": ((229, 145, 34), 0.0, 0.24, "ABS Plastic"),
    "mask": ((31, 90, 51), 0.0, 0.44, FR4),
}


def command(command_id: str, **params) -> dict:
    return {"command": command_id, "params": params}


class BoardAuthor:
    """Small deterministic native-command emitter with unique body names."""

    def __init__(self):
        self.steps = []
        self.bodies = []
        self.roles = defaultdict(list)

    def add(self, command_id: str, **params):
        self.steps.append(command(command_id, **params))

    def point(self, x: float, y: float, z: float) -> list:
        return [round(ORIGIN[0] + x, 8), round(ORIGIN[1] + y, 8),
                round(ORIGIN[2] + THICKNESS + z, 8)]

    def body(self, name: str, role: str) -> str:
        name = "Pi5 " + name
        if name in self.bodies:
            raise ValueError(f"Duplicate body: {name}")
        self.bodies.append(name)
        self.roles[role].append(name)
        return name

    def box(self, name, xyz, size, role="socket"):
        name = self.body(name, role)
        self.add("solid.box", corner=self.point(*xyz), length=size[0],
                 width=size[1], height=size[2], operation="new",
                 name=name, body_name=name)
        return name

    def cut_box(self, target, label, xyz, size):
        self.add("solid.box", corner=self.point(*xyz), length=size[0],
                 width=size[1], height=size[2], operation="cut",
                 targets=[target], name="Pi5 " + label)

    def cylinder(self, name, xyz, radius, height, role="gold", axis=(0, 0, 1)):
        name = self.body(name, role)
        self.add("solid.cylinder", base=self.point(*xyz), radius=radius,
                 height=height, axis=list(axis), operation="new",
                 name=name, body_name=name)
        return name

    def bore(self, target, label, xyz, radius, height):
        self.add("solid.cylinder", base=self.point(*xyz), radius=radius,
                 height=height, axis=[0, 0, 1], operation="cut",
                 targets=[target], name="Pi5 " + label)

    def ring(self, name, xyz, outer, inner, height, role="solder"):
        n = self.cylinder(name, xyz, outer, height, role)
        self.bore(n, name + " bore", (xyz[0], xyz[1], xyz[2] - 0.1), inner, height + 0.2)
        return n

    def rotated_box(self, name, center, size, angle, role="package"):
        xyz = (center[0] - size[0] / 2, center[1] - size[1] / 2, center[2])
        n = self.box(name, xyz, size, role)
        pivot = self.point(*center)
        self.add("solid.move", bodies=[n], translate=[-v for v in pivot], name=n + " origin")
        self.add("solid.move", bodies=[n], axis=[0, 0, 1], angle=angle, name=n + " rotation")
        self.add("solid.move", bodies=[n], translate=pivot, name=n + " placement")
        return n


def _geometry(a: BoardAuthor):
    # The preserved source PCB is drilled explicitly; pins do not disappear into
    # an unperforated slab. Via and GPIO bore sizes are illustrative clearances.
    header = a.box("GPIO insulator", (6.23, 48.73, 0), (50.8, 5.08, 2.5))
    for col in range(20):
        for row in range(2):
            pin = col * 2 + row + 1
            x, y = GPIO_FIRST[0] + GPIO_PITCH * col, GPIO_FIRST[1] + GPIO_PITCH * row
            a.cut_box(header, f"GPIO insulator bore {pin:02}", (x - .36, y - .36, -.1), (.72, .72, 2.7))
            a.box(f"GPIO pin {pin:02}", (x - .32, y - .32, -2.4), (.64, .64, 10.0), "gold")
            a.ring(f"GPIO top solder {pin:02}", (x, y, .01), .82, .50, .14)
            a.ring(f"GPIO underside solder {pin:02}", (x, y, -1.75), .82, .50, .15)

    for i, (x, y) in enumerate(MOUNTING, 1):
        a.ring(f"Mounting annular pad {i}", (x, y, .01), 2.55, 1.35, .055, "gold")

    # Visible packages and leads. Dimensions outside the public board interface
    # are a clearly documented visual approximation, not component CAD imports.
    a.box("BCM2712 substrate", (25.4, 19.1, .06), (15.5, 15.5, .55), "package")
    a.box("BCM2712 heat spreader", (26.35, 20.05, .61), (13.6, 13.6, .95), "aluminum")
    a.box("LPDDR4X package", (43.4, 29.0, .08), (10.0, 14.4, 1.0), "package")
    a.box("RP1 controller", (45.3, 14.2, .08), (12.0, 12.0, 1.18), "package")
    a.box("DA9091 PMIC", (15.2, 21.0, .08), (6.0, 6.0, 1.0), "package")
    for i, (x, y) in enumerate(((15.2, 28.2), (19.5, 28.2), (15.2, 32.8), (19.5, 16.5)), 1):
        a.box(f"Power inductor {i}", (x, y, .05), (3.1, 3.1, 1.9), "ferrite")
        a.box(f"Power inductor {i} terminal A", (x - .15, y + .3, .01), (.35, 2.5, .4), "solder")
        a.box(f"Power inductor {i} terminal B", (x + 2.9, y + .3, .01), (.35, 2.5, .4), "solder")
    a.rotated_box("BCM54213 Ethernet PHY", (64.0, 24.0, .08), (7.2, 7.2, 1.0), 45)
    a.box("CYW43455 wireless package", (6.0, 31.0, .08), (5.8, 5.8, .8), "package")
    # Five sheet bodies form a hollow RF cover, retaining a genuine underside.
    a.box("RF shield roof", (4.5, 29.5, 2.05), (10.0, 10.0, .22), "shield")
    a.box("RF shield west", (4.5, 29.5, 0), (.22, 10.0, 2.05), "shield")
    a.box("RF shield east", (14.28, 29.5, 0), (.22, 10.0, 2.05), "shield")
    a.box("RF shield south", (4.72, 29.5, 0), (9.56, .22, 2.05), "shield")
    a.box("RF shield north", (4.72, 39.28, 0), (9.56, .22, 2.05), "shield")

    # Stacked USB-A: open front cavities, two distinct tongues and their gold
    # contacts. The USB 3 additional contacts sit further back in each socket.
    for family, y, tongue_role in (("USB2", 37.8, "socket"), ("USB3", 19.8, "usb_blue")):
        shell = a.box(f"{family} stacked shell", (70.0, y, 0), (17.0, 14.2, 15.2), "shield")
        for level in range(2):
            z = .85 + level * 7.3
            a.cut_box(shell, f"{family} cavity {level + 1}", (71.0, y + .55, z), (16.4, 13.1, 5.75))
            a.box(f"{family} tongue {level + 1}", (72.1, y + 1.2, z + 2.45), (12.9, 11.8, 1.05), tongue_role)
            for pin in range(4):
                a.box(f"{family} level {level + 1} contact {pin + 1}",
                      (76.0, y + 3.1 + pin * 2.5, z + 2.27), (8.1, .85, .18), "gold")
            if family == "USB3":
                for pin in range(5):
                    a.box(f"{family} level {level + 1} SS contact {pin + 1}",
                          (73.1, y + 2.3 + pin * 2.0, z + 3.50), (2.0, .65, .14), "gold")
            for sign, sy in (("L", y + 2.5), ("R", y + 10.0)):
                a.box(f"{family} retention spring {level + 1}{sign}",
                      (82.0, sy, z + 5.38), (2.0, 1.15, .25), "shield")
        for t, yy in enumerate((y + .9, y + 12.5), 1):
            a.box(f"{family} mounting tab {t}", (71.1, yy, -1.9), (.65, .9, 2.0), "solder")

    # RJ45 is Pi 5's bottom right connector. Its socket is hollow, with a latch
    # recess, 8 contacts and green/amber indicator lenses in separate pockets.
    shell = a.box("RJ45 metal shell", (65.8, .8, 0), (21.2, 16.0, 13.5), "shield")
    a.cut_box(shell, "RJ45 socket cavity", (72.4, 2.0, 1.7), (15.0, 13.6, 10.2))
    a.cut_box(shell, "RJ45 latch recess", (80.0, 6.6, .6), (7.5, 4.4, 2.0))
    a.box("RJ45 internal contact carrier", (73.0, 2.5, 2.1), (3.0, 12.6, 6.2), "socket")
    for pin in range(8):
        a.box(f"RJ45 gold spring contact {pin + 1}", (75.0, 4.0 + pin * 1.27, 7.65), (8.8, .48, .20), "gold")
    for label, y, role in (("link", 1.5, "led_green"), ("activity", 14.2, "led_amber")):
        a.cut_box(shell, f"RJ45 {label} pocket", (85.3, y, 10.5), (2.0, 1.2, 1.8))
        a.box(f"RJ45 {label} lens", (85.4, y + .1, 10.6), (1.4, 1.0, 1.6), role)

    # USB-C and dual micro-HDMI face the -Y edge. Contact counts describe their
    # visible interfaces, and do not imply that every pin is electrically used.
    shell = a.box("USB-C shell", (6.5, -1.2, .05), (9.2, 7.6, 3.25), "shield")
    a.cut_box(shell, "USB-C opening", (6.9, -1.5, .47), (8.4, 6.6, 2.37))
    a.box("USB-C tongue", (7.35, -.75, 1.44), (7.5, 5.2, .55), "socket")
    for row, z in (("upper", 2.0), ("lower", 1.30)):
        for pin in range(12):
            a.box(f"USB-C {row} contact {pin + 1:02}", (8.25 + pin * .5, -.15, z), (.25, 3.2, .12), "gold")
    for port, x in enumerate((24.5, 38.0), 1):
        shell = a.box(f"micro-HDMI {port} shell", (x, -1.0, .05), (7.0, 6.9, 2.85), "shield")
        a.cut_box(shell, f"micro-HDMI {port} opening", (x + .35, -1.2, .42), (6.3, 5.5, 2.08))
        a.box(f"micro-HDMI {port} tongue", (x + .8, -.5, 1.13), (5.4, 4.8, .52), "socket")
        for pin in range(19):
            a.box(f"micro-HDMI {port} contact {pin + 1:02}",
                  (x + 1.0 + (pin // 2) * .5, -.15, 1.66 if pin % 2 else 1.0),
                  (.20, 2.6, .10), "gold")

    # Two 22-position MIPI FFC sockets and a 16-position PCIe FFC socket.
    for port, x in enumerate((45.8, 54.0), 1):
        # The long contact rows run along Y, leaving the real two bottom-edge
        # connectors visually readable between HDMI and Ethernet.
        n = a.box(f"MIPI {port} housing", (x, .5, 0), (4.8, 12.6, 2.15), "socket")
        a.cut_box(n, f"MIPI {port} cable slot", (x + .85, .4, .8), (3.1, 12.2, .65))
        a.box(f"MIPI {port} latch", (x + 3.85, 1.0, 1.60), (.8, 11.6, .65), "white")
        for pin in range(22):
            a.box(f"MIPI {port} contact {pin + 1:02}", (x + .45, 1.55 + pin * .5, .83), (3.1, .20, .16), "gold")
    n = a.box("PCIe FFC housing", (.8, 13.3, 0), (4.7, 9.7, 2.15), "socket")
    a.cut_box(n, "PCIe FFC cable slot", (.7, 14.0, .8), (4.2, 8.3, .65))
    a.box("PCIe FFC latch", (.65, 13.8, 1.6), (.85, 8.7, .65), "white")
    for pin in range(16):
        a.box(f"PCIe contact {pin + 1:02}", (1.0, 14.4 + pin * .5, .83), (3.2, .20, .16), "gold")

    # Small shrouded JST-style headers with separate pins, open at the top.
    for name, count, x, y, pitch in (("Fan", 4, 61.5, 41.0, 1.0),
                                    ("UART", 3, 18.0, 4.4, 1.0),
                                    ("RTC battery", 2, 10.4, 10.8, 1.0)):
        width = (count - 1) * pitch + 2.2
        n = a.box(name + " shroud", (x, y, 0), (width, 3.4, 3.1), "white")
        a.cut_box(n, name + " header well", (x + .48, y + .48, .7), (width - .96, 2.44, 2.7))
        for pin in range(count):
            a.box(f"{name} pin {pin + 1}", (x + 1.0 + pin * pitch, y + 1.5, .25), (.3, .3, 2.35), "gold")
            a.box(f"{name} solder toe {pin + 1}", (x + .9 + pin * pitch, y - .55, .01), (.5, .75, .12), "solder")
    a.box("PoE 2x2 insulator", (60.7, 33.8, 0), (5.08, 5.08, 2.5), "socket")
    for col in range(2):
        for row in range(2):
            a.box(f"PoE pin {col * 2 + row + 1}",
                  (61.65 + col * 2.54, 34.75 + row * 2.54, 2.5), (.64, .64, 4.0), "gold")

    # Underside microSD socket: open mouth, shell, insulating carrier and 8 pads.
    shell = a.box("microSD underside cage", (-.5, 21.0, -3.5), (14.8, 13.8, 1.75), "shield")
    a.cut_box(shell, "microSD cavity", (-.8, 21.45, -3.2), (14.2, 12.9, 1.15))
    a.box("microSD internal carrier", (9.3, 21.6, -3.18), (3.9, 12.6, .45), "socket")
    for pin in range(8):
        a.box(f"microSD contact {pin + 1}", (5.8, 23.0 + pin * 1.1, -2.68), (6.6, .55, .12), "gold")

    a.box("Power button frame", (-.5, 7.0, .1), (3.8, 5.2, 2.6), "shield")
    a.box("Power button plunger", (-1.1, 8.2, .6), (.9, 2.8, 1.6), "socket")
    a.box("Status LED base", (1.0, 46.0, .02), (1.8, 1.0, .35), "white")
    a.box("Status LED lens", (1.25, 46.12, .37), (1.3, .76, .35), "led_green")
    for index, (x, y) in enumerate(((37.8, 36.6), (55.6, 42.0)), 1):
        a.box(f"Crystal {index} ceramic base", (x, y, .03), (3.2, 2.5, .2), "ceramic")
        a.box(f"Crystal {index} metal lid", (x + .1, y + .1, .23), (3.0, 2.3, .6), "shield")

    # Small passives provide honest scale cues. These are representative package
    # positions, not an asserted electrical component map or hidden full netlist.
    passives = ((18.7, 34), (21.2, 34), (23.7, 34), (17, 37), (20, 37), (23, 37),
                (28, 37), (31, 37), (34, 37), (28, 40), (31, 40), (34, 40),
                (39, 16), (42, 16), (39, 12.5), (42, 12.5),
                (25, 8), (28, 8), (31, 8), (34, 8), (37, 8), (40, 8),
                (55.2, 28), (57.8, 30.5), (58, 32), (63, 30.5), (66, 30.5), (67, 34.5),
                (22, 43), (25, 43), (28, 43), (31, 43))
    for i, (x, y) in enumerate(passives, 1):
        a.box(f"Passive {i:02} ceramic", (x, y, .08), (1.2, .8, .52), "ceramic" if i % 3 else "package")
        a.box(f"Passive {i:02} termination A", (x - .25, y, .04), (.3, .8, .45), "solder")
        a.box(f"Passive {i:02} termination B", (x + 1.15, y, .04), (.3, .8, .45), "solder")

    for i, (x, y) in enumerate(VIA_POSITIONS, 1):
        a.ring(f"Plated via {i:02}", (x, y, -1.65), .35, .225, 1.75, "gold")


def commands(*, standalone: bool = False) -> list[dict]:
    """Return all native commands; no file IO and no inferred session state."""
    a = BoardAuthor()
    a.add("component.activate", component="root")
    if standalone:
        a.add("solid.box", corner=list(ORIGIN), length=85, width=56, height=THICKNESS,
              operation="new", name=BOARD, body_name=BOARD)
        for i, (x, y) in enumerate(MOUNTING, 1):
            a.bore(BOARD, f"Fixture mounting bore {i}", (x, y, -1.8), 1.35, 2.0)
        a.add("component.create", name="Avionics", parent="root", activate=False)
        a.add("component.move_bodies", bodies=[BOARD], component="Avionics")
        a.add("occurrence.ground", occurrence="Avionics:1", grounded=True)
    else:
        a.add("solid.remove", bodies=PLACEHOLDERS, name="Pi5 retire block placeholders")

    # Complete substrate machining in its own component before adding hundreds
    # of small parts. This also supports engines that scope cuts to the active
    # component even when the target body was explicitly named.
    a.add("component.activate", component="Avionics")
    a.add("solid.fillet", edges=[[ORIGIN[0] + x, ORIGIN[1] + y, ORIGIN[2] + THICKNESS / 2]
                                  for x, y in ((0, 0), (85, 0), (85, 56), (0, 56))],
          radius=CORNER_RADIUS, name="Pi5 PCB corner radii")
    for col in range(20):
        for row in range(2):
            pin = col * 2 + row + 1
            x, y = GPIO_FIRST[0] + GPIO_PITCH * col, GPIO_FIRST[1] + GPIO_PITCH * row
            a.bore(BOARD, f"GPIO substrate bore {pin:02}", (x, y, -1.8), .5, 2.0)
    for i, (x, y) in enumerate(VIA_POSITIONS, 1):
        a.bore(BOARD, f"Via substrate bore {i:02}", (x, y, -1.8), .35, 2.0)
    for family, y in (("USB2", 37.8), ("USB3", 19.8)):
        for tab, yy in enumerate((y + .9, y + 12.5), 1):
            a.cut_box(BOARD, f"{family} PCB anchor slot {tab}",
                      (71.0, yy - .1, -1.8), (.85, 1.1, 2.0))
    a.add("component.activate", component="root")

    a.add("material.create", name=SEMICONDUCTOR, density_g_cm3=2.33,
          color=[32, 35, 39])
    a.add("material.create", name=FR4, density_g_cm3=1.85, color=[31, 90, 51])
    _geometry(a)
    a.roles["mask"].append(BOARD)
    for role, bodies in a.roles.items():
        color, metallic, roughness, material = ROLES[role]
        a.add("material.assign", bodies=bodies, material=material)
        a.add("appearance.assign", bodies=bodies, color=list(color), opacity=1,
              pbr={"metallic": metallic, "roughness": roughness, "dielectric_ior": 1.5})
    a.add("component.create", name=COMPONENT, parent="root", activate=False)
    a.add("component.move_bodies", bodies=a.bodies, component=COMPONENT)
    a.add("joint.as_built", name="Pi5 componentry fixed to board", type="rigid",
          a="Avionics", b=COMPONENT, quiet=True)
    a.add("component.activate", component="root")
    return a.steps


def standalone_commands() -> list[dict]:
    return commands(standalone=True)


def manifest() -> dict:
    """Return independent analytic interfaces and new-body inventory."""
    a = BoardAuthor()
    _geometry(a)
    board_volume = (85 * 56 - 4 * CORNER_RADIUS ** 2 * (1 - math.pi / 4)) * THICKNESS - math.pi * THICKNESS * (
        4 * 1.35 ** 2 + 40 * .5 ** 2 + 12 * .35 ** 2) - 4 * .85 * 1.1 * THICKNESS
    header_volume = 50.8 * 5.08 * 2.5 - 40 * .72 * .72 * 2.5
    return {
        "scope": "Original visual reconstruction; not a complete BOM or electrical design",
        "board": {"body": BOARD, "origin_mm": list(ORIGIN), "size_mm": [85, 56, THICKNESS],
                  "corner_radius_mm": CORNER_RADIUS,
                  "usb_anchor_slots": {"count": 4, "size_mm": [.85, 1.1]},
                  "mounting_centres_local_mm": [list(p) for p in MOUNTING],
                  "mounting_diameter_mm": 2.7, "fixture_volume_mm3": board_volume},
        "gpio": {"count": 40, "pitch_mm": GPIO_PITCH,
                 "first_pin_local_mm": list(GPIO_FIRST), "pin_section_mm": [.64, .64],
                 "substrate_bore_mm": 1.0, "insulator_bore_mm": .72,
                 "insulator_volume_mm3": header_volume},
        "body_count": len(a.bodies), "body_names": a.bodies,
        "roles": dict(a.roles), "removed_placeholders": PLACEHOLDERS,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--standalone", action="store_true")
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({"commands": commands(standalone=args.standalone)}, indent=2) + "\n")
    if args.manifest:
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        args.manifest.write_text(json.dumps(manifest(), indent=2) + "\n")
    print(f"{len(commands(standalone=args.standalone))} commands; {manifest()['body_count']} new bodies")


if __name__ == "__main__":
    main()
