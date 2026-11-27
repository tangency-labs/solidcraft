"""Arm C: scx v15.1 exactly as shipped (docs/scx-hybrid/scx.py), with one change: a script may import
any module the environment has (numpy, scipy, pandas...), not only math and itertools, and the
instructions say so in one line. Everything else, the vocabulary and the replies, is unchanged.

    python scx_open.py --headless --text-only
"""

from __future__ import annotations

import builtins
import importlib.util
import sys
from pathlib import Path

SCX = Path(__file__).resolve().parents[2] / "docs" / "scx-hybrid" / "scx.py"
spec = importlib.util.spec_from_file_location("scx", SCX)
assert spec and spec.loader
scx = importlib.util.module_from_spec(spec)
sys.modules["scx"] = scx
spec.loader.exec_module(scx)

scx.SAFE_BUILTINS["__import__"] = builtins.__import__
old = "Vocabulary (math, pi, sqrt, sin, cos ready):"
assert old in scx.INSTRUCTIONS
scx.INSTRUCTIONS = scx.INSTRUCTIONS.replace(old, "Vocabulary (math, pi, sqrt, sin, cos ready; numpy, scipy, pandas can be imported):")

if __name__ == "__main__":
    scx.main()
