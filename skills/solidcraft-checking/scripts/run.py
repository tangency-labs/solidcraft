#!/usr/bin/env python3
"""Run a SolidCraft command script and print a short report: a failure, feature warnings,
sketches that are not fully constrained, and body measurements. A document.inspect inside the
script reports that state too, its lines prefixed with its step; the end state comes last."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

INSPECT = {"command": "document.inspect", "params": {"measure": True}}
WARNING_LENGTH = 160


class SolidCraftError(RuntimeError):
    pass


def run_commands(commands: list, design: Path | None = None, save: Path | None = None) -> list:
    """Results of every command, followed by a final document.inspect."""
    with tempfile.TemporaryDirectory() as tmp:
        script = Path(tmp) / "script.json"
        script.write_text(json.dumps({"commands": [*commands, INSPECT]}))
        args = ["solidcraft-cli", "run", str(script), "--results"]
        if design:
            args += ["--in", str(design)]
        if save:
            args += ["--save", str(save)]
        done = subprocess.run(args, capture_output=True, encoding="utf-8")
    if done.returncode:
        raise SolidCraftError(done.stderr.strip().removeprefix("error: "))
    return json.loads(done.stdout)


def execute(commands: list, design: Path | None = None, save: Path | None = None) -> dict:
    """The document.inspect of the design after the commands."""
    return run_commands(commands, design, save)[-1]


def number(value: float) -> str:
    return f"{round(value, 3) + 0.0:.3f}".rstrip("0").rstrip(".")


def point(values: list) -> str:
    return "(" + ", ".join(map(number, values)) + ")"


def report(inspection: dict) -> list[str]:
    """Warnings come from the timeline: a rebuild after a parameter change reports them only there."""
    lines = [f'WARN "{feature["name"]}": {feature["warning"][:WARNING_LENGTH]}'
             for feature in inspection["timeline"] if feature.get("warning")]
    for sketch in inspection.get("sketches", []):
        if sketch["dof"] or sketch["status"] != "solved":
            lines.append(f'SKETCH "{sketch["name"]}": dof {sketch["dof"]}, {sketch["status"]}')
    lines += map(body_line, inspection["bodies"])
    if not inspection["bodies"]:
        lines.append("no bodies")
    return lines


def body_line(body: dict) -> str:
    if "volume_mm3" not in body:  # inspected without measure
        return f'BODY {body["name"]}'
    bbox = body["bbox"]
    return (f'BODY {body["name"]}: volume {number(body["volume_mm3"])} mm3, area {number(body["area_mm2"])} mm2, '
            f'bbox {point(bbox["min"])}..{point(bbox["max"])}, com {point(body["center_of_mass"])}')


def script_report(commands: list, results: list) -> list[str]:
    lines = [f"step {step} {line}"
             for step, (command, result) in enumerate(zip(commands, results), 1)
             if command["command"] == "document.inspect"
             for line in report(result)]
    return lines + report(results[-1])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("script", type=Path, help='command script: {"commands": [...]}')
    parser.add_argument("--in", dest="design", type=Path, help="design to run the script on")
    parser.add_argument("--save", type=Path, help="save the resulting design; a .json name keeps it JSON")
    args = parser.parse_args()

    commands = json.loads(args.script.read_text())["commands"]
    try:
        results = run_commands(commands, args.design, args.save)
    except SolidCraftError as error:
        print(f"FAIL {error}")
        sys.exit(1)
    print("\n".join(script_report(commands, results)))


if __name__ == "__main__":
    main()
