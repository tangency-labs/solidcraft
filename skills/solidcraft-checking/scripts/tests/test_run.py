import json
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))
import run  # noqa: E402

BOX = [{"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "body_name": "Cube"}}]
CAVITY_WARNING = 'WARN "Pocket": the cut leaves a closed cavity'


def cavity_commands(depth):
    return [
        *BOX,
        {"command": "parameters.add", "params": {"name": "depth", "expression": depth}},
        {"command": "solid.cylinder", "params": {
            "radius": 2, "height": "depth", "base": [5, 5, 4],
            "operation": "cut", "targets": ["Cube"], "name": "Pocket",
        }},
    ]


def report(tmp_path, commands, *args):
    script = tmp_path / "script.json"
    script.write_text(json.dumps({"commands": commands}))
    return subprocess.run(
        [sys.executable, SCRIPTS / "run.py", script, *args], capture_output=True, text=True
    )


def test_reports_bodies(tmp_path):
    result = report(tmp_path, BOX)
    assert result.returncode == 0, result.stderr
    assert "BODY Cube: volume 1000 mm3, area 600 mm2, bbox (0, 0, 0)..(10, 10, 10), com (5, 5, 5)" in result.stdout


def test_flags_underconstrained_sketch(tmp_path):
    result = report(tmp_path, [
        {"command": "sketch.create", "params": {"plane": "XY", "name": "Loose"}},
        {"command": "sketch.circle.center", "params": {"center": [5, 5], "radius": 5}},
        {"command": "sketch.finish", "params": {}},
    ])
    assert result.returncode == 0, result.stderr
    assert 'SKETCH "Loose": dof 3, solved' in result.stdout


def test_reports_step_warnings(tmp_path):
    result = report(tmp_path, cavity_commands("4 mm"))
    assert result.returncode == 0, result.stderr
    assert CAVITY_WARNING in result.stdout


def test_fails_on_a_bad_step(tmp_path):
    result = report(tmp_path, [{"command": "solid.box", "params": {"width": 10}}])
    assert result.returncode == 1
    assert result.stdout.startswith("FAIL") and "step 1 (solid.box)" in result.stdout


def test_applies_a_script_to_a_saved_design(tmp_path):
    design = tmp_path / "cube.json"
    built = report(tmp_path, [
        {"command": "parameters.add", "params": {"name": "size", "expression": "10 mm"}},
        {"command": "solid.box", "params": {"length": "size", "width": "size", "height": "size", "body_name": "Cube"}},
    ], "--save", design)
    assert built.returncode == 0, built.stderr

    changed = report(tmp_path, [
        {"command": "parameters.change", "params": {"name": "size", "expression": "20 mm"}},
    ], "--in", design)
    assert changed.returncode == 0, changed.stderr
    assert "BODY Cube: volume 8000 mm3" in changed.stdout


def test_execute_returns_the_final_inspection():
    inspection = run.execute(BOX)
    assert [b["name"] for b in inspection["bodies"]] == ["Cube"]


def test_execute_raises_on_failure():
    with pytest.raises(run.SolidCraftError, match="step 1"):
        run.execute([{"command": "solid.box", "params": {"width": 10}}])


@pytest.fixture
def open_pocket_design(tmp_path):
    design = tmp_path / "pocket.json"
    built = report(tmp_path, cavity_commands("7 mm"), "--save", design)
    assert built.returncode == 0, built.stdout + built.stderr
    assert "WARN" not in built.stdout
    return design


def change_depth(value: str) -> dict:
    return {"command": "parameters.change", "params": {"name": "depth", "expression": value}}


def test_reports_warnings_after_a_parameter_change(tmp_path, open_pocket_design):
    changed = report(tmp_path, [change_depth("4 mm")], "--in", open_pocket_design)
    assert changed.returncode == 0, changed.stderr
    assert CAVITY_WARNING in changed.stdout


def test_reports_every_inspection_in_the_script(tmp_path, open_pocket_design):
    walk = report(tmp_path, [
        change_depth("4 mm"),
        {"command": "document.inspect", "params": {"measure": True}},
        change_depth("7 mm"),
        {"command": "document.inspect", "params": {"measure": True}},
    ], "--in", open_pocket_design)
    assert walk.returncode == 0, walk.stderr
    lines = walk.stdout.splitlines()
    assert [line.split(":", 1)[0] for line in lines] == [
        'step 2 WARN "Pocket"', "step 2 BODY Cube", "step 4 BODY Cube", "BODY Cube",
    ]
    assert lines[0].startswith(f"step 2 {CAVITY_WARNING}")


def test_reports_bodies_inspected_without_measurements(tmp_path):
    result = report(tmp_path, [*BOX, {"command": "document.inspect", "params": {}}])
    assert "step 2 BODY Cube\n" in result.stdout
    assert "BODY Cube: volume 1000 mm3" in result.stdout


def test_says_so_when_there_is_nothing_to_report(tmp_path):
    result = report(tmp_path, [{"command": "parameters.add", "params": {"name": "size", "expression": "10 mm"}}])
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "no bodies"
