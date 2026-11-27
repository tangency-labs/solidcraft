"""Arguments as JSON: NumPy arrays and scalars, pandas Series, tuples and anything else that
offers ``tolist()`` or ``__float__`` become plain JSON values. The client imports none of those
libraries."""

from __future__ import annotations

import math
from typing import Any


class EncodeError(ValueError):
    """An argument that cannot be sent: a NaN or an infinity, or a value with no JSON form."""


def plain(value: Any, where: str = "argument") -> Any:
    """``value`` as JSON-ready Python data; ``where`` names it in an error."""
    if value is None or isinstance(value, (bool, str)):
        return value
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        if not math.isfinite(value):
            raise EncodeError(f"{where} is {value}; SolidCraft takes finite numbers only")
        return value
    if isinstance(value, dict):
        return {str(k): plain(v, f"{where}[{k!r}]") for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [plain(v, f"{where}[{i}]") for i, v in enumerate(value)]
    # A feature or body object of this package passes its name.
    name = getattr(value, "__solidcraft_name__", None)
    if isinstance(name, str):
        return name
    tolist = getattr(value, "tolist", None)
    if callable(tolist):  # NumPy arrays and scalars, pandas Series
        return plain(tolist(), where)
    if hasattr(value, "__index__"):
        return int(value.__index__())
    if hasattr(value, "__float__"):
        return plain(float(value), where)
    if hasattr(value, "__iter__"):
        return [plain(v, f"{where}[{i}]") for i, v in enumerate(value)]
    raise EncodeError(f"{where}: a {type(value).__name__} cannot be sent to SolidCraft")
