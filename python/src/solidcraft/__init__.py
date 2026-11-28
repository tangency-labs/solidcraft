"""Drive SolidCraft from Python: every engine command and MCP tool, typed, generated from the build.

    import solidcraft as sc
    d = sc.session()                     # the session daemon's design, or a headless one
    d.cmd.solid.box(length=60, width=40, height=8, body_name="Plate")
    d.cmd.solid.hole(position=[30, 20, 8], diameter=6)
    print(d.changes())
    print(sc.find("fillet"))             # command ids, labels and parameters

Design notes: docs/python-api.md in the SolidCraft repository.
"""

from ._encode import EncodeError
from ._mcp import SolidCraftError
from ._session import Session, connect, find, headless, session

try:
    from ._generated import VERSION as __version__
except ImportError:  # pragma: no cover
    __version__ = "0"

__all__ = ["EncodeError", "Session", "SolidCraftError", "connect", "find", "headless", "session", "__version__"]
