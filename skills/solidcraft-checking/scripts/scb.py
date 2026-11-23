"""SolidCraft builder: write command scripts whose sketches come out fully constrained.

Each sketch helper makes one sketch and dimensions its geometry from the sketch origin. `at` is
where the geometry starts: put it near the solved position, on the side of the origin it must
stay on, because a distance dimension keeps the side it starts on.
"""

from __future__ import annotations

import json
from pathlib import Path


FEATURE_COMMANDS = ("solid.", "construct.", "sheet.", "surface.", "mesh.", "file.insert_", "sketch.create")


class Script:
    def __init__(self) -> None:
        self.commands: list[dict] = []
        self.names: set[str] = set()
        self.body_names: set[str] = set()

    def cmd(self, command: str, /, **params) -> None:
        if "name" in params and command.startswith(FEATURE_COMMANDS) and not command.endswith(".manage_rules"):
            self._claim(params["name"], self.names, "a sketch or feature")
        if "body_name" in params:
            self._claim(params["body_name"], self.body_names, "a body")
        self.commands.append({"command": command, "params": params})

    @staticmethod
    def _claim(name: str, taken: set[str], kind: str) -> None:
        """SolidCraft resolves a sketch or feature name to its first match and renames a repeated
        body name, so either repeat silently points at the wrong thing."""
        if name in taken:
            raise ValueError(f"{name}: {kind} already has this name")
        taken.add(name)

    def parameter(self, name: str, expression: str, unit: str | None = None, comment: str = "") -> None:
        """A user parameter; unit "" makes a unitless one such as a count."""
        params = {"name": name, "expression": expression}
        if unit is not None:
            params["unit"] = unit
        if comment:
            params["comment"] = comment
        self.cmd("parameters.add", **params)

    @staticmethod
    def _check(point: str, at: tuple, x: str | None, y: str | None) -> None:
        if x is None and y is None and tuple(at) != (0, 0):
            raise ValueError(f"{point}: give x or y, or start it at the origin")
        for axis, start, distance in (("x", at[0], x), ("y", at[1], y)):
            if distance is not None and start == 0:
                raise ValueError(f"{point}: the {axis} distance starts at 0 and would lose its side")

    def place(self, point: str, at: tuple, x: str | None, y: str | None) -> None:
        """Distances of `point` from the origin; None puts it on that axis."""
        self._check(point, at, x, y)
        if x is None and y is None:
            return  # a point created on the origin is made coincident with it
        if x is None:
            self.cmd("sketch.constraint.horizontal_vertical", points=["origin", point], mode="vertical")
        else:
            self.cmd("sketch.dimension", entities=["origin", point], type="horizontal", value=x)
        if y is None:
            self.cmd("sketch.constraint.horizontal_vertical", points=["origin", point], mode="horizontal")
        else:
            self.cmd("sketch.dimension", entities=["origin", point], type="vertical", value=y)

    def _sketch(self, name: str, offset: str | None) -> None:
        self.cmd("sketch.create", plane="XY", name=name, **({"offset": offset} if offset else {}))

    def hexagon(self, sketch: str, across_flats: str, at: tuple, x: str | None = None, y: str | None = None,
                offset: str | None = None, approx: float = 20) -> None:
        """Regular hexagon with vertices up and down and flat sides facing ±x, on the XY plane
        raised by `offset`. `approx` is a rough across-flats size for the solver to start from."""
        self._check(sketch, at, x, y)
        self._sketch(sketch, offset)
        self.cmd("sketch.polygon.circumscribed", center=list(at), radius=approx / 2, sides=6, angle=0)
        self.cmd("sketch.dimension", entities=["c1"], type="diameter", value=across_flats)
        # Record an explicit regular-polygon constraint in the generated script.
        self.cmd("sketch.constraint.polygon", lines=["l1", "l2", "l3", "l4", "l5", "l6"])
        self.cmd("sketch.constraint.horizontal_vertical", line="l3", mode="vertical")
        self.place("c1.center", at, x, y)
        self.cmd("sketch.finish")

    def circle(self, sketch: str, diameter: str, at: tuple, x: str | None = None, y: str | None = None,
               offset: str | None = None, approx: float = 20) -> None:
        self._check(sketch, at, x, y)
        self._sketch(sketch, offset)
        self.cmd("sketch.circle.center", center=list(at), radius=approx / 2)
        self.cmd("sketch.dimension", entities=["c1"], type="diameter", value=diameter)
        self.place("c1.center", at, x, y)
        self.cmd("sketch.finish")

    def rounded_rectangle(self, sketch: str, width: str, height: str, bottom: str, corner_radius: str,
                          at: tuple, offset: str | None = None) -> None:
        """Rectangle symmetric about the y axis with its bottom edge `bottom` from the origin.
        `at` is (left, bottom, right, top) to start from."""
        left, low, right, top = at
        if low == 0:
            raise ValueError(f"{sketch}: the bottom distance starts at 0 and would lose its side")
        self._sketch(sketch, offset)
        self.cmd("sketch.rectangle.two_point", p0=[left, low], p1=[right, top])
        # Sides: l1 bottom, l2 right, l3 top, l4 left; corners p1..p4 from bottom left, anticlockwise
        self.cmd("sketch.constraint.symmetry", a="p1", b="p2", line="y")
        self.cmd("sketch.dimension", entities=["l4", "l2"], type="distance", value=width)
        self.cmd("sketch.dimension", entities=["origin", "l1"], type="distance", value=bottom)
        self.cmd("sketch.dimension", entities=["l4"], type="length", value=height)
        self.cmd("sketch.fillet", points=["p1", "p2", "p3", "p4"], radius=corner_radius)
        self.cmd("sketch.finish")

    def save(self, path: Path) -> None:
        Path(path).write_text(json.dumps({"commands": self.commands}, indent=1))
