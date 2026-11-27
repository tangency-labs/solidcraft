"""Arm E: scx v15.1's vocabulary (box, cylinder, hole, sketch, hollow, part, ...) on top of the
package's session, unchanged: scx's own functions, with its bridge replaced by `solidcraft.session()`.

    from scx_vocab import *      # the vocabulary, plus `d`, the session (d.cmd..., d.measure(), ...)

Also used by the harness to build the edit tasks' starting designs from their scx setup scripts.
"""

from __future__ import annotations

import atexit
import importlib.util
import json
import math
import sys
from pathlib import Path

import solidcraft as sc

SCX = Path(__file__).resolve().parents[2] / "docs" / "scx-hybrid" / "scx.py"
_spec = importlib.util.spec_from_file_location("scx", SCX)
assert _spec and _spec.loader
scx = importlib.util.module_from_spec(_spec)
_argv, sys.argv = sys.argv, [sys.argv[0]]  # scx reads its own flags from argv at import
_spec.loader.exec_module(scx)
sys.argv = _argv
scx.TEXT_ONLY = True

d = sc.session()


class _Bridge(scx.Bridge):
    """scx's bridge over the package session: the same tool calls, without a second process."""

    def __init__(self) -> None:  # noqa: D107 - no child process
        self.n = 0

    def text(self, name, args):
        try:
            r = d.tool_call(name, args, {})
        except sc.SolidCraftError as e:
            raise scx.SCError(str(e).split(": ", 1)[-1]) from None
        return r if isinstance(r, str) else json.dumps(r)

    def tool(self, name, args):
        return [], False


scx.B = _Bridge()


@atexit.register
def _log() -> None:
    for line in scx.LOG:
        print(line)


globals().update(scx.VOCAB)
pi, sqrt, sin, cos, tan, radians, degrees = math.pi, math.sqrt, math.sin, math.cos, math.tan, math.radians, math.degrees
__all__ = [*scx.VOCAB, "d", "sc", "math", "pi", "sqrt", "sin", "cos", "tan", "radians", "degrees"]
