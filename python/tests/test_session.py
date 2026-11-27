"""The client against a real headless SolidCraft (`SOLIDCRAFT_CLI`, or `solidcraft-cli` on PATH)."""

from __future__ import annotations

import math
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

import solidcraft as sc
from solidcraft._generated.tools import TOOL_NAMES
from solidcraft._mcp import Client


@pytest.fixture()
def d():
    with sc.headless() as s:
        yield s


def test_every_mcp_tool_has_a_method():
    """The bindings follow the server: a tool added to MCP without regenerating fails here."""
    c = Client(["--headless"])
    try:
        listed = {t["name"] for t in c.list_tools()}
    finally:
        c.close()
    assert listed == set(TOOL_NAMES), f"regenerate with `cargo xtask python`: {sorted(listed ^ set(TOOL_NAMES))}"
    for name in listed:
        assert callable(getattr(sc.Session, name)), name


def test_box_and_holes_report_their_volume(d):
    d.cmd.solid.box(length=60, width=40, height=8, corner=[0, 0, 0], body_name="Plate")
    for x, y in [(8, 8), (52, 8), (8, 32), (52, 32)]:
        d.cmd.solid.hole(position=(x, y, 8), diameter=6)
    m = d.measure(summary=True)
    assert m["bodies"][0]["volume_mm3"] == pytest.approx(60 * 40 * 8 - 4 * math.pi * 9 * 8, rel=1e-9)
    report = d.changes()
    assert report.startswith("+ Plate: vol 18295.2213  bbox [0, 0, 0]..[60, 40, 8]  faces 10"), report


def test_numpy_values_are_sent_as_numbers(d):
    np = pytest.importorskip("numpy")
    d.cmd.solid.box(length=np.float64(10), width=np.int64(20), height=np.array(30.0), corner=np.zeros(3))
    bb = d.measure(summary=True)["bodies"][0]["bbox"]
    assert bb == {"min": [0.0, 0.0, 0.0], "max": [10.0, 20.0, 30.0]}


def test_mistakes_fail_where_they_were_written(d):
    with pytest.raises(TypeError, match="bogus"):
        d.cmd.solid.box(length=1, width=1, height=1, bogus=3)
    with pytest.raises(sc.EncodeError, match="length is nan"):
        d.cmd.solid.box(length=float("nan"), width=1, height=1)
    with pytest.raises(sc.SolidCraftError, match="no_such"):
        d.call("solid.no_such_command")


def test_changes_are_relative_to_the_mark(d):
    d.cmd.solid.box(length=10, width=10, height=10, body_name="A")
    d.mark()
    d.cmd.solid.box(length=10, width=10, height=5, corner=[0, 0, 10], operation="join")
    assert d.changes().startswith("~ A: vol 1000 -> 1500"), d.changes()


def test_find_lists_ids_with_their_parameters():
    found = sc.find("solid.box")
    assert found.splitlines()[0].startswith("solid.box (Box): length, width, height")
    assert sc.find("no-such-word-anywhere") == "no command matches 'no-such-word-anywhere'"


def test_the_daemon_keeps_one_design_across_scripts(tmp_path: Path):
    env = {**os.environ, "SOLIDCRAFT_SESSION": str(tmp_path / "session.json"), "SOLIDCRAFT_REPORT": "1"}
    serve = subprocess.Popen([sys.executable, "-m", "solidcraft", "serve", "--allow-path", str(tmp_path)], env=env, cwd=tmp_path)
    try:
        for _ in range(200):
            if (tmp_path / "session.json").exists():
                break
            time.sleep(0.05)
        first = "import solidcraft as sc\nd = sc.session()\nd.cmd.solid.box(length=10, width=10, height=10, body_name='A')\n"
        out1 = subprocess.run([sys.executable, "-c", first], env=env, cwd=tmp_path, capture_output=True, text=True, check=True).stdout
        assert "[SolidCraft] + A: vol 1000" in out1, out1
        second = "import solidcraft as sc\nd = sc.session()\nprint(sorted(b['name'] for b in d.measure(summary=True)['bodies']))\n"
        out2 = subprocess.run([sys.executable, "-c", second], env=env, cwd=tmp_path, capture_output=True, text=True, check=True).stdout
        assert "['A']" in out2 and "no geometry changed" in out2, out2
    finally:
        subprocess.run([sys.executable, "-m", "solidcraft", "stop"], env=env, cwd=tmp_path, check=False)
        serve.wait(timeout=30)
