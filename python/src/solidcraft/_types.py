"""Type aliases the generated bindings use, named after the engine's shared parameter types."""

from __future__ import annotations

from typing import Any, Sequence, Union

#: A length: a number in mm, or an expression such as ``"w / 2"`` or ``"30 mm"``.
Length = Union[float, str]
#: An angle: a number in degrees, or an expression such as ``"45 deg"``.
Angle = Union[float, str]
#: A point ``[x, y, z]`` (any sequence or NumPy array of three) or ``{"x", "y", "z"}``.
Point3 = Union[Sequence[Length], dict]
#: A point ``[u, v]`` on a sketch plane.
Point2 = Union[Sequence[Length], dict]
#: A direction ``[x, y, z]``.
Vec3 = Sequence[float]
#: A name, or a list of names.
Names = Union[str, Sequence[str]]


class _Node:
    """A namespace of commands under one id prefix (``d.cmd.solid``)."""

    _send: Any

    def __repr__(self) -> str:
        names = [n for n in dir(self) if not n.startswith("_")]
        return f"<commands: {', '.join(names)}>"
