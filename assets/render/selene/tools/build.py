#!/usr/bin/env python3
"""Build the enhanced native SELENE through reproducible SolidCraft commands.

Each stage runs in a fresh native process and is saved before the next stage.
This bounds undo-history memory on the large source model. Successful resume
requires unchanged input, executable, command recipe and checkpoint hashes.
The source archive is inspected read-only; only SolidCraft writes CAD state.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import zipfile

sys.dont_write_bytecode = True

import bearings
import electronics
import finishes
import mechanisms
import suspension
import yaw_supports
from prepare import inspect_native

PACKAGE = Path(__file__).resolve().parents[1]
SOURCE = PACKAGE / "source/SELENE-showcase.solidcraft"


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def json_digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def write(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def require(value: bool, message: str) -> None:
    if not value:
        raise ValueError(message)


def document(path: Path) -> dict:
    inspect_native(path)  # Bounded archive, path, count and relationship admission.
    with zipfile.ZipFile(path) as archive:
        return json.loads(archive.read("document.json"))


def health(inventory: dict) -> None:
    errors = [f for f in inventory.get("timeline", []) if f.get("error") or f.get("warning")]
    require(not errors, f"Native timeline has errors or warnings: {errors[:3]}")
    require(inventory.get("bodies") and len({b["name"] for b in inventory["bodies"]}) == len(inventory["bodies"]),
            "Native inventory has no bodies or duplicate names.")


def execute(cli: Path, arguments: list[str], log: Path, timeout: float) -> None:
    env = os.environ.copy()
    env.update(RAYON_NUM_THREADS="2", MALLOC_ARENA_MAX="1", SOLIDCRAFT_GPU="cpu",
               MALLOC_MMAP_THRESHOLD_="65536", MALLOC_TRIM_THRESHOLD_="65536")
    started = time.monotonic()
    with log.open("wb") as stream:
        process = subprocess.Popen([str(cli), *arguments], stdout=stream, stderr=subprocess.STDOUT, env=env)
        try:
            while process.poll() is None:
                elapsed = time.monotonic() - started
                if elapsed > timeout:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    raise TimeoutError(f"Native process exceeded {timeout:g}s; see {log}")
                if elapsed > 0 and int(elapsed) % 30 == 0:
                    print(f"  native process active: {elapsed:.0f}s", flush=True)
                time.sleep(1)
        except BaseException:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
            raise
    require(process.returncode == 0, f"Native process exited {process.returncode}; see {log}")


def expected_stages(inventory: dict, source: dict) -> list[tuple[str, list[dict]]]:
    result = [
        ("01-finishes", finishes.commands(inventory, source)),
        ("02-primary-mechanisms", mechanisms.primary_commands()),
        ("03-electronics", electronics.commands()),
    ]
    for index, steps in enumerate(bearings.stages(), 1):
        if isinstance(steps, tuple):
            steps = steps[-1]
        result.append((f"04-bearings-{index:02}", steps))
    suspension_stages = list(suspension.stages())
    for index, steps in enumerate(suspension_stages, 1):
        if isinstance(steps, tuple):
            steps = steps[-1]
        if index == len(suspension_stages):
            # Reparenting wheels recreates their joints. Rebind the inherited
            # study before this checkpoint, so it never retains deleted IDs.
            steps = [*steps, mechanisms.adapted_author_tour(source)]
        result.append((f"05-suspension-{index:02}", steps))
    for index, (_, steps) in enumerate(yaw_supports.stages(), 1):
        result.append((f"05b-yaw-supports-{index:02}", steps))
    return [(name, commands + [mechanisms.c("document.inspect", measure=False, sketches=False), mechanisms.c("joint.list")])
            for name, commands in result]


def run_stage(cli: Path, current: Path, work: Path, name: str, commands: list[dict], timeout: float) -> tuple[Path, dict]:
    require(bool(commands) and all(isinstance(x, dict) and "command" in x for x in commands), f"Bad stage {name}")
    recipe, result_path, checkpoint = (work / (name + suffix) for suffix in (".commands.json", ".results.json", ".solidcraft"))
    require(not checkpoint.exists(), f"Refusing to replace an unrecorded checkpoint: {checkpoint}")
    write(recipe, {"commands": commands})
    start = time.monotonic()
    print(f"Starting {name}: {len(commands)} native commands", flush=True)
    execute(cli, ["run", str(recipe), "--in", str(current), "--save", str(checkpoint),
                  "--quiet", "--results", "--timings", "--json-out", str(result_path)],
            work / (name + ".log"), timeout)
    result = json.loads(result_path.read_text(encoding="utf-8"))
    timing = result.get("timings", [])
    require(len(timing) == len(commands), f"{name}: incomplete native timing trace")
    require([t.get("index") for t in timing] == list(range(len(commands))), f"{name}: unordered native timing trace")
    require([t.get("command") for t in timing] == [c["command"] for c in commands], f"{name}: command trace mismatch")
    require(len(result.get("results", [])) == len(commands), f"{name}: missing native command results")
    inventory, graph = result["results"][-2:]
    health(inventory)
    for key in ("conflicts", "errors", "floating"):
        require(not graph.get(key), f"{name}: native joint graph has {key}: {graph[key]}")
    native = inspect_native(checkpoint)
    entry = {
        "name": name, "commands": len(commands), "recipe_sha256": json_digest(commands),
        "input_sha256": sha(current), "checkpoint": checkpoint.name, "checkpoint_sha256": sha(checkpoint),
        "result_file": result_path.name, "result_sha256": sha(result_path),
        "duration_seconds": round(time.monotonic() - start, 3), "format": native["format"],
        "counts": native["counts"], "native_exit_code": 0,
        "process_memory_policy": {"MALLOC_ARENA_MAX": "1", "MALLOC_MMAP_THRESHOLD_": "65536",
                                  "MALLOC_TRIM_THRESHOLD_": "65536", "RAYON_NUM_THREADS": "2"},
    }
    print(f"Saved {name}: {native['counts']['history_features']} features, {native['counts']['joints']} joints", flush=True)
    return checkpoint, entry


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--source", type=Path, default=SOURCE)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--until", help="Stop after a named native stage; preserves a resumable checkpoint.")
    parser.add_argument("--inventory", type=Path, help="Reuse an existing evaluated source inventory; only body names are used to generate finishes.")
    parser.add_argument("--stage-timeout", type=float, default=2400)
    parser.add_argument("--output", type=Path, default=PACKAGE / "models/SELENE-functional.solidcraft")
    args = parser.parse_args()
    cli, source, work = args.cli.resolve(), args.source.resolve(), args.work.resolve()
    require(cli.is_file(), "Native CLI is missing.")
    require(sha(source) == finishes.SOURCE_SHA256 and source.stat().st_size == 314452, "Source is not the pinned author-supplied file.")
    source_doc = document(source)
    version = subprocess.check_output([str(cli), "--version"], text=True).strip()
    work.mkdir(parents=True, exist_ok=True)
    history_file = work / "build-history.json"
    initial = {
        "schema_version": "1.0", "source_sha256": sha(source), "cli_sha256": sha(cli), "cli_version": version,
        "started_at": datetime.now(timezone.utc).isoformat(), "stages": [], "status": "in_progress",
    }
    if args.resume:
        history = json.loads(history_file.read_text(encoding="utf-8"))
        for key in ("source_sha256", "cli_sha256", "cli_version"):
            require(history[key] == initial[key], f"Resume fingerprint changed: {key}")
    else:
        require(not history_file.exists(), "Build history exists; use --resume or a new work directory.")
        history = initial
        write(history_file, history)
    baseline = work / "source-inventory.json"
    if not baseline.exists():
        if args.inventory:
            shutil.copyfile(args.inventory.resolve(), baseline)
            history["inventory_origin"] = "Supplied evaluated inventory; native stage replay still verifies the source geometry."
        else:
            execute(cli, ["eval", str(source), "--json-out", str(baseline)], work / "source-evaluation.log", args.stage_timeout)
            history["inventory_origin"] = "This executable evaluated the pinned source."
        write(history_file, history)
    inventory = json.loads(baseline.read_text(encoding="utf-8"))
    health(inventory)
    require(len(inventory["bodies"]) == 1418, "Unexpected source body inventory.")
    stages = expected_stages(inventory, source_doc)
    require(args.until is None or args.until in dict(stages) or args.until == "06-presentation", "Unknown --until stage.")
    current = source
    for number, (name, commands) in enumerate(stages):
        if number < len(history["stages"]):
            old = history["stages"][number]
            require(old["name"] == name and old["recipe_sha256"] == json_digest(commands), f"Completed recipe changed: {name}")
            require(old["input_sha256"] == sha(current), f"Checkpoint chain changed before {name}")
            current = work / old["checkpoint"]
            require(sha(current) == old["checkpoint_sha256"], f"Checkpoint changed: {name}")
            require(sha(work / old["result_file"]) == old["result_sha256"], f"Native results changed: {name}")
        else:
            try:
                current, entry = run_stage(cli, current, work, name, commands, args.stage_timeout)
            except (ValueError, OSError, subprocess.SubprocessError, TimeoutError) as error:
                history.setdefault("failed_attempts", []).append({
                    "stage": name, "recipe_sha256": json_digest(commands),
                    "input_sha256": sha(current), "error": str(error),
                    "recorded_at": datetime.now(timezone.utc).isoformat(),
                })
                write(history_file, history)
                raise
            history["stages"].append(entry)
            write(history_file, history)
        if args.until == name:
            print(f"Stopped at requested checkpoint: {current}", flush=True)
            return 0
    # Assembly-dependent groups and studies are authored after final native IDs exist.
    import presentation
    input_doc = document(current)
    prior_results = json.loads((work / history["stages"][len(stages)-1]["result_file"]).read_text(encoding="utf-8"))
    presentation_commands = presentation.commands(input_doc, prior_results["results"][-2])
    presentation_commands.append(mechanisms.adapted_author_tour(source_doc))
    presentation_commands.extend(mechanisms.study_commands(include_suspension=True))
    presentation_commands.extend([
        mechanisms.c("document.inspect", measure=False, sketches=False),
        mechanisms.c("joint.list"),
    ])
    name = "06-presentation"
    if len(history["stages"]) > len(stages):
        old = history["stages"][len(stages)]
        require(old["name"] == name and old["recipe_sha256"] == json_digest(presentation_commands), "Presentation recipe changed.")
        current = work / old["checkpoint"]
        require(sha(current) == old["checkpoint_sha256"], "Presentation checkpoint changed.")
        require(sha(work / old["result_file"]) == old["result_sha256"], "Presentation results changed.")
    else:
        try:
            current, entry = run_stage(cli, current, work, name, presentation_commands, args.stage_timeout)
        except (ValueError, OSError, subprocess.SubprocessError, TimeoutError) as error:
            history.setdefault("failed_attempts", []).append({
                "stage": name, "recipe_sha256": json_digest(presentation_commands),
                "input_sha256": sha(current), "error": str(error),
                "recorded_at": datetime.now(timezone.utc).isoformat(),
            })
            write(history_file, history)
            raise
        history["stages"].append(entry)
        write(history_file, history)
    final_results = json.loads((work / (name + ".results.json")).read_text(encoding="utf-8"))["results"]
    final_inventory, joints = final_results[-2:]
    health(final_inventory)
    for key in ("conflicts", "errors", "floating"):
        require(not joints.get(key), f"Final native joint graph has {key}: {joints[key]}")
    write(work / "final-inspect.json", final_inventory)
    write(work / "final-joints.json", joints)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists():
        require(sha(args.output) == sha(current), "Final output exists with another hash; choose a new --output.")
    else:
        shutil.copyfile(current, args.output)
    history.update(status="native_build_passed", final_sha256=sha(current),
                   final_body_count=len(final_inventory["bodies"]), output=str(args.output),
                   completed_at=datetime.now(timezone.utc).isoformat())
    write(history_file, history)
    print(f"Native master saved: {args.output} ({sha(current)})", flush=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.SubprocessError, TimeoutError) as error:
        print(f"SELENE build failed: {error}", file=sys.stderr)
        raise SystemExit(1)
