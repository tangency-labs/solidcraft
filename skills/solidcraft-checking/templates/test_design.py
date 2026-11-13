"""Regression test for a SolidCraft design. One session walks the parameters through the cases in
order, as edits in the app do, and compares a body after each case with frozen values.
Copy next to the design, then fill in DESIGN, BODY and CASES. Needs solidcraft-cli on PATH."""

import json
import subprocess
from math import dist
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import NamedTuple

import pytest

DESIGN = Path(__file__).parent / "design.json"
BODY = "Body"
# SolidCraft integrates volume and area on B-rep surfaces and measures mesh bodies from their
# triangles. Choose REL for the accuracy the design needs. Check volume against the analytic
# value or the OCCT volume from compare_step.py on the design's STEP export compared with itself.
REL = 2e-4


class Measures(NamedTuple):
    volume: float
    area: float
    center_of_mass: tuple[float, float, float]
    bbox_min: tuple[float, float, float]
    bbox_max: tuple[float, float, float]


# Parameter values of each case, in the order the session applies them, and the body's expected
# measures: from an exact reference when porting, from hand calculation for simple shapes, or a
# frozen build the user has reviewed. A case changes only the parameters it names; the others keep
# the values of the cases before it.
CASES = {
    # (("rows", "7"), ("cells", "4")): Measures(volume=..., area=..., center_of_mass=(...), bbox_min=(...), bbox_max=(...)),
}


@pytest.fixture(scope="module")
def inspections() -> dict:
    """document.inspect after each case, all in one session. A fixture, so that a failed session
    is reported to every test instead of rerun for each."""
    commands, at, current = [], [], {}
    for case in CASES:
        for name, value in case:
            if current.get(name) != value:
                commands.append({"command": "parameters.change", "params": {"name": name, "expression": value}})
                current[name] = value
        at.append(len(commands))
        commands.append({"command": "document.inspect", "params": {"measure": True}})
    with TemporaryDirectory() as tmp:
        script = Path(tmp) / "script.json"
        script.write_text(json.dumps({"commands": commands}))
        done = subprocess.run(
            ["solidcraft-cli", "run", script, "--in", DESIGN, "--results"], capture_output=True, encoding="utf-8"
        )
    assert done.returncode == 0, done.stderr
    results = json.loads(done.stdout)
    return {case: results[index] for case, index in zip(CASES, at)}


def body(inspection: dict) -> Measures:
    found = {b["name"]: b for b in inspection["bodies"]}[BODY]
    bbox = found["bbox"]
    return Measures(found["volume_mm3"], found["area_mm2"], tuple(found["center_of_mass"]),
                    tuple(bbox["min"]), tuple(bbox["max"]))


def assert_matches(actual: Measures, expected: Measures) -> None:
    assert actual.volume == pytest.approx(expected.volume, rel=REL)
    assert actual.area == pytest.approx(expected.area, rel=REL)
    # Set the position tolerance for the design. This default scales with the body size.
    tolerance = max(1e-3, 1.5e-4 * dist(expected.bbox_min, expected.bbox_max))
    for point in ("center_of_mass", "bbox_min", "bbox_max"):
        assert getattr(actual, point) == pytest.approx(getattr(expected, point), abs=tolerance), point


@pytest.mark.parametrize("case", CASES)
def test_body_matches(inspections, case):
    assert_matches(body(inspections[case]), CASES[case])
