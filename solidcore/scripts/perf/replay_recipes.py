#!/usr/bin/env python3
"""Replay recipes against prebuilt SolidCraft CLIs, then export numeric evidence.

Run on Linux with GNU time and nice. Build separately, stop competing workloads,
and supply neutral public IDs in the config; paths resolve beside that config.
Commands: run CONFIG OUTPUT; analyze OUTPUT; export OUTPUT PUBLIC_JSON.
Raw subprocess output stays in OUTPUT/private and is never read by the exporter.
The report describes completed evaluations, not certified geometry correctness.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import statistics
import subprocess
import sys
import time
import threading

SCHEMA = 1
STATUSES = {"ok", "exit_error", "timeout", "interrupted", "spawn_error", "invalid_output", "invalid_resources", "descendants_remaining"}
ABORT_REASONS = {"load_exceeded", "invalid_sample", "io_error"}
STATES = {"running", "complete", "warmup_failed", "interrupted", "input_changed", "aborted"}
COUNTS = ("bodies", "faces", "edges", "vertices", "errors", "warnings", "suppressed", "rolled_back", "open_bodies_skipped")
VALUES = ("volume", "area", "feature_seconds")
GEOMETRY = ("bodies", "faces", "edges", "vertices", "volume", "area", "closed", "bounds")


class Invalid(ValueError):
    """Input does not satisfy the measurement contract."""


class LoadExceeded(Invalid):
    """The configured preflight load limit was exceeded."""


def require(condition):
    """Reject invalid data without echoing arbitrary input or private paths."""
    if not condition:
        raise Invalid("invalid configuration or measurement data")


def finite(value, minimum=None):
    """Recognize finite JSON numbers, excluding booleans."""
    return type(value) in (int, float) and math.isfinite(value) and (minimum is None or value >= minimum)


def integer(value, minimum=0):
    """Recognize a bounded-sign integer, excluding booleans."""
    return type(value) is int and value >= minimum


def identifier(value):
    """Validate deliberately public case/variant IDs, never derived from paths."""
    require(isinstance(value, str) and re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,79}", value))
    return value


def hex_digest(value, length=64):
    """Validate content hashes and exact source revisions."""
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{" + str(length) + r"}", value))
    return value


def read_json(path):
    """Read strict JSON; later validators enforce its typed shape."""
    return json.loads(Path(path).read_text(encoding="utf-8"))


def write_json(path, value):
    """Replace a structured artifact atomically, rejecting NaN and infinity."""
    path = Path(path)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def digest(path):
    """Hash files in chunks, without loading executables into memory."""
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def public_variant(value):
    """Copy only declared public provenance fields, never executable paths."""
    out = {"id": identifier(value["id"]), "binary_sha256": hex_digest(value["binary_sha256"])}
    for key in ("app_revision", "kernel_revision"):
        if value.get(key) is not None:
            out[key] = hex_digest(value[key], 40)
    profile = value.get("build_profile", "unknown")
    compatibility = value.get("compatibility", "unknown")
    require(profile in ("release", "ci", "difftest", "unknown"))
    require(compatibility in ("shipped", "experiment", "unknown"))
    out.update(build_profile=profile, compatibility=compatibility)
    if value.get("adapter_sha256") is not None:
        out["adapter_sha256"] = hex_digest(value["adapter_sha256"])
    return out


def public_manifest(value):
    """Enforce an allowlist rather than redacting arbitrary manifest text."""
    require(type(value["schema_version"]) is int and value["schema_version"] == SCHEMA and value["state"] in STATES)
    variants = [public_variant(v) for v in value["variants"]]
    require(variants and len({v["id"] for v in variants}) == len(variants))
    cases = []
    for case in value["cases"]:
        item = {"id": identifier(case["id"]), "sha256": hex_digest(case["sha256"])}
        if case.get("expected_volume") is not None:
            require(finite(case["expected_volume"]))
            item["expected_volume"] = case["expected_volume"]
        cases.append(item)
    require(cases and len({c["id"] for c in cases}) == len(cases))
    require(integer(value["rounds"], 1) and integer(value["threads"], 1))
    require(finite(value["timeout_seconds"], 0.001))
    require(integer(value["niceness"]) and value["niceness"] <= 19)
    require(value["warmup_case"] in {c["id"] for c in cases})
    require(value.get("max_load") is None or finite(value["max_load"], 0))
    out = {key: value[key] for key in ("schema_version", "state", "rounds", "threads", "timeout_seconds", "niceness", "warmup_case", "max_load")}
    if value.get("abort_reason") is not None:
        require(value["abort_reason"] in ABORT_REASONS)
        out["abort_reason"] = value["abort_reason"]
    out.update(variants=variants, cases=cases, ordering="case then round; rotate variants by round", clock="fresh-process elapsed seconds", warmup="one excluded evaluation per variant")
    if value.get("recipe_archive_sha256") is not None:
        out["recipe_archive_sha256"] = hex_digest(value["recipe_archive_sha256"])
    if value.get("runner_sha256") is not None:
        out["runner_sha256"] = hex_digest(value["runner_sha256"])
    return out


def prepare(config_path):
    """Resolve private inputs separately from the path-free manifest."""
    config_path = Path(config_path).resolve()
    cfg = read_json(config_path)
    require(type(cfg["schema_version"]) is int and cfg["schema_version"] == SCHEMA)
    inputs = {}
    manifest = {key: cfg[key] for key in ("rounds", "threads", "timeout_seconds", "warmup_case")}
    manifest.update(schema_version=SCHEMA, state="running", niceness=cfg.get("niceness", 10), max_load=cfg.get("max_load"), variants=[], cases=[], runner_sha256=digest(__file__))
    for group, field in (("variants", "binary"), ("cases", "recipe")):
        for item in cfg[group]:
            name = identifier(item["id"])
            require((group, name) not in inputs)
            path = (config_path.parent / item[field]).resolve()
            require(path.is_file())
            inputs[group, name] = path
            entry = dict(item)
            entry["binary_sha256" if group == "variants" else "sha256"] = digest(path)
            manifest[group].append(entry)
    if "recipe_archive_sha256" in cfg:
        manifest["recipe_archive_sha256"] = cfg["recipe_archive_sha256"]
    return public_manifest(manifest), inputs


def clean_figures(value):
    """Validate and copy only numeric geometry fields and explicit booleans."""
    require(isinstance(value, dict))
    out = {}
    for key in COUNTS:
        require(integer(value[key]))
        out[key] = value[key]
    for key in VALUES:
        require(finite(value[key], 0 if key != "volume" else None))
        out[key] = value[key]
    require(isinstance(value["closed"], list) and len(value["closed"]) == out["bodies"])
    require(all(type(v) is bool for v in value["closed"]))
    out["closed"] = value["closed"].copy()
    require(isinstance(value["bounds"], list) and len(value["bounds"]) == out["bodies"])
    out["bounds"] = []
    for bounds in value["bounds"]:
        require(isinstance(bounds, list) and len(bounds) == 6 and all(finite(v) for v in bounds))
        require(all(bounds[i] <= bounds[i + 3] for i in range(3)))
        out["bounds"].append(bounds.copy())
    return out


def figures(path):
    """Validate eval JSON, retaining no messages, names or arbitrary dictionaries."""
    try:
        data = read_json(path)
        total, bodies, timeline = data["total"], data["bodies"], data["timeline"]
        require(isinstance(bodies, list) and isinstance(timeline, list))
        require(integer(data["body_count"]) and data["body_count"] == len(bodies))
        out = {"bodies": data["body_count"], "volume": total["volume_mm3"], "area": total["area_mm2"], "feature_seconds": 0.0}
        out.update({k: total[k] for k in ("faces", "edges", "vertices")})
        out["open_bodies_skipped"] = total.get("open_bodies_skipped", 0)
        for target in ("errors", "warnings", "suppressed", "rolled_back"):
            key = {"errors": "error", "warnings": "warning"}.get(target, target)
            out[target] = sum(bool(item.get(key)) for item in timeline)
        for item in timeline:
            ms = item.get("ms")
            require(ms is None or finite(ms, 0))
            out["feature_seconds"] += (ms or 0) / 1000
        out["closed"] = [body["closed"] for body in bodies]
        out["bounds"] = []
        for body in bodies:
            bounds = body["bbox"]
            # SolidCraft eval emits min/max XYZ arrays.
            require(isinstance(bounds["min"], list) and isinstance(bounds["max"], list))
            require(len(bounds["min"]) == len(bounds["max"]) == 3)
            out["bounds"].append(bounds["min"] + bounds["max"])
        return clean_figures(out)
    except (OSError, UnicodeError, ValueError, KeyError, TypeError, AttributeError, OverflowError):
        return None


def resources(path):
    """Parse GNU time's final record, rejecting missing or nonfinite metrics."""
    try:
        values = [float(v) for v in Path(path).read_text().splitlines()[-1].split()]
        require(len(values) == 3 and all(finite(v, 0) for v in values) and values[2].is_integer())
        return dict(cpu_user_seconds=values[0], cpu_system_seconds=values[1], max_rss_kib=int(values[2]))
    except (OSError, UnicodeError, ValueError, IndexError):
        return None


def stop_process(process):
    """Terminate only the process group this runner created, then reap it."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    # A leader can exit before descendants which ignore SIGTERM. Kill the owned
    # group even after wait succeeds; never leave those descendants running.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def process_group_exists(process):
    """Check only the process group created for this evaluation."""
    try:
        os.killpg(process.pid, 0)
        return True
    except ProcessLookupError:
        return False


def execute(command, environment, prefix, timeout):
    """Run a fresh process with logs streamed to disk and explicit failure states."""
    started = time.perf_counter()
    result = {"status": "spawn_error", "exit": None}
    with Path(str(prefix) + ".stdout").open("wb") as stdout, Path(str(prefix) + ".stderr").open("wb") as stderr:
        finished, expired = threading.Event(), threading.Event()
        cleanup_lock = threading.Lock()
        cleaned = False
        process = None

        def cleanup():
            """Serialize watchdog and interruption cleanup for this group."""
            nonlocal cleaned
            with cleanup_lock:
                if process is not None and not cleaned:
                    stop_process(process)
                    cleaned = True

        def watchdog():
            """Enforce the deadline without polling in the measured wait."""
            if not finished.wait(timeout):
                expired.set()
                cleanup()

        watcher = threading.Thread(target=watchdog, daemon=True)
        try:
            try:
                process = subprocess.Popen(command, env=environment, stdout=stdout, stderr=stderr, start_new_session=True)
            except OSError:
                result["seconds"] = time.perf_counter() - started
                return result
            watcher.start()
            try:
                # A timeout argument makes POSIX Popen.wait poll with sleeps,
                # biasing short recipes. Only the watchdog waits on a clock.
                code = process.wait()
                ended = time.perf_counter()
                result.update(exit=code, status="ok" if code == 0 else "exit_error")
            finally:
                finished.set()
                watcher.join()
            # A successful leader exit does not imply every child has stopped.
            # An evaluator must not leave background work to contaminate timing.
            if not cleaned and process_group_exists(process):
                cleanup()
                result["status"] = "descendants_remaining"
        except KeyboardInterrupt:
            cleanup()
            ended = time.perf_counter()
            result.update(status="interrupted", exit=process.returncode if process is not None else None)
        except BaseException:
            # Startup can fail after we own the evaluator but before wait begins.
            cleanup()
            raise
        finally:
            finished.set()
            if watcher.ident is not None:
                watcher.join()
        if expired.is_set() and result["status"] != "interrupted":
            result.update(status="timeout", exit=process.returncode)
    result["seconds"] = ended - started
    return result


def clean_run(value, manifest):
    """Validate samples and strip every field outside the public schema."""
    require(value["case"] in {c["id"] for c in manifest["cases"]})
    require(value["variant"] in {v["id"] for v in manifest["variants"]})
    require(value["phase"] in ("warmup", "measured") and value["status"] in STATUSES)
    require(integer(value["round"]) and value["round"] <= manifest["rounds"])
    require((value["phase"] == "warmup") == (value["round"] == 0))
    require(finite(value["seconds"], 0))
    require(value["exit"] is None or type(value["exit"]) is int)
    out = {k: value[k] for k in ("case", "variant", "phase", "round", "status", "seconds", "exit")}
    for key in ("cpu_user_seconds", "cpu_system_seconds", "max_rss_kib"):
        require(value.get(key) is None or finite(value[key], 0))
        out[key] = value.get(key)
    out["figures"] = clean_figures(value["figures"]) if value.get("figures") is not None else None
    return out


def successful(run):
    """Identify complete evaluations; this does not grade geometric accuracy."""
    f = run["figures"]
    return (run["status"] == "ok" and run["exit"] == 0 and f is not None
            and f["bodies"] > 0
            and all(finite(run.get(k), 0) for k in ("cpu_user_seconds", "cpu_system_seconds"))
            and integer(run.get("max_rss_kib"))
            and all(f[k] == 0 for k in ("errors", "suppressed", "rolled_back", "open_bodies_skipped"))
            and all(f["closed"]))


def summarize(manifest, runs):
    """Keep failed durations separate from medians of complete evaluations."""
    rows = []
    for case in manifest["cases"]:
        for variant in manifest["variants"]:
            samples = [r for r in runs if r["phase"] == "measured" and r["case"] == case["id"] and r["variant"] == variant["id"]]
            good = [r for r in samples if successful(r)]
            eligible = manifest["state"] == "complete"
            times = [r["seconds"] for r in good] if eligible else []
            geom = good[0]["figures"] if good else None
            rows.append({"case": case["id"], "variant": variant["id"], "samples": len(samples), "successful_samples": len(good),
                         "not_attempted": manifest["rounds"] - len(samples), "timing_eligible": eligible,
                         "failures": [{"round": r["round"], "status": r["status"] if r["status"] != "ok" else "incomplete", "seconds": r["seconds"]} for r in samples if not successful(r)],
                         "median_seconds": statistics.median(times) if times else None,
                         "minimum_seconds": min(times) if times else None, "maximum_seconds": max(times) if times else None,
                         "geometry": {k: geom[k] for k in GEOMETRY} if geom else None,
                         "recorded_geometry_repeatable": all(all(r["figures"][k] == geom[k] for k in GEOMETRY) for r in good) if good else None})
    return {"schema_version": SCHEMA, "manifest": manifest, "summary": rows, "samples": runs}


def load_report(output):
    """Recompute an export from validated manifest and samples, never raw logs."""
    manifest = public_manifest(read_json(output / "manifest.json"))
    runs = [clean_run(json.loads(line), manifest) for line in (output / "runs.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
    keys = [(r["case"], r["variant"], r["phase"], r["round"]) for r in runs]
    require(len(keys) == len(set(keys)))
    return summarize(manifest, runs)


def run(config, output):
    """Time only prebuilt inputs; preserve every finished sample on interruption."""
    require(sys.platform.startswith("linux") and shutil.which("nice") and Path("/usr/bin/time").is_file())
    manifest, inputs = prepare(config)
    output.mkdir(parents=True, exist_ok=False)
    private = output / "private"
    private.mkdir(mode=0o700)
    environment = {k: v for k, v in os.environ.items() if not k.startswith(("SOLIDCRAFT_", "SOLVECRAFT_"))}
    environment["RAYON_NUM_THREADS"] = str(manifest["threads"])
    variants = [v["id"] for v in manifest["variants"]]
    environments = {}
    for variant in variants:
        env = environment.copy()
        for variable in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME"):
            folder = private / variant / variable.lower()
            folder.mkdir(parents=True)
            env[variable] = str(folder.resolve())
        environments[variant] = env
    write_json(output / "manifest.json", manifest)
    runs = []

    def sample(case, variant, index, phase, log):
        """Measure one owned evaluation and persist its sanitized result."""
        if manifest["max_load"] is not None and os.getloadavg()[0] > manifest["max_load"]:
            raise LoadExceeded()
        prefix = private / f"{phase}-{case}-{variant}-{index}"
        measurement, resource = Path(str(prefix) + ".json"), Path(str(prefix) + ".resources")
        command = ["/usr/bin/time", "-f", "%U %S %M", "-o", str(resource.resolve()), "nice", "-n", str(manifest["niceness"]), str(inputs["variants", variant]), "eval", str(inputs["cases", case]), "--json-out", str(measurement.resolve())]
        result = execute(command, environments[variant], prefix, manifest["timeout_seconds"])
        result.update(case=case, variant=variant, round=index, phase=phase, figures=figures(measurement))
        metrics = resources(resource)
        if metrics:
            result.update(metrics)
        if result["status"] == "ok":
            if result["figures"] is None:
                result["status"] = "invalid_output"
            elif metrics is None:
                result["status"] = "invalid_resources"
        result = clean_run(result, manifest)
        runs.append(result)
        log.write(json.dumps(result, allow_nan=False) + "\n")
        log.flush()
        write_json(output / "summary.json", summarize(manifest, runs))
        return result

    previous_term = signal.getsignal(signal.SIGTERM)

    def interrupted(signum, frame):
        """Handle catchable termination through the normal interruption path."""
        # A repeated SIGTERM must not interrupt cleanup or artifact finalization.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    try:
        with (output / "runs.jsonl").open("w", encoding="utf-8") as log:
            for variant in variants:
                warmup = sample(manifest["warmup_case"], variant, 0, "warmup", log)
                if warmup["status"] == "interrupted":
                    manifest["state"] = "interrupted"
                    return 2
                if not successful(warmup):
                    manifest["state"] = "warmup_failed"
                    return 2
            for case in manifest["cases"]:
                timed_out = set()
                for index in range(manifest["rounds"]):
                    offset = index % len(variants)
                    for variant in variants[offset:] + variants[:offset]:
                        if variant in timed_out:
                            continue
                        result = sample(case["id"], variant, index + 1, "measured", log)
                        if result["status"] == "timeout":
                            timed_out.add(variant)
                        if result["status"] == "interrupted":
                            manifest["state"] = "interrupted"
                            return 2
            manifest["state"] = "complete"
    except KeyboardInterrupt:
        manifest["state"] = "interrupted"
        return 2
    except (OSError, Invalid) as error:
        reason = "load_exceeded" if isinstance(error, LoadExceeded) else "io_error" if isinstance(error, OSError) else "invalid_sample"
        manifest.update(state="aborted", abort_reason=reason)
        print("Recipe replay aborted: " + reason, file=sys.stderr)
        return 2
    finally:
        # Finish durable records before restoring the caller's termination policy.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        try:
            for group, key in (("variants", "binary_sha256"), ("cases", "sha256")):
                for item in manifest[group]:
                    try:
                        unchanged = digest(inputs[group, item["id"]]) == item[key]
                    except OSError:
                        unchanged = False
                    if not unchanged:
                        manifest["state"] = "input_changed"
            write_json(output / "manifest.json", manifest)
            write_json(output / "summary.json", summarize(manifest, runs))
        finally:
            signal.signal(signal.SIGTERM, previous_term)
    return 0 if manifest["state"] == "complete" else 2


def main():
    """Dispatch private measurement and explicitly requested public export."""
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    measure = commands.add_parser("run")
    measure.add_argument("config", type=Path)
    measure.add_argument("output", type=Path)
    for name in ("analyze", "export"):
        command = commands.add_parser(name)
        command.add_argument("output", type=Path)
        if name == "export":
            command.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "run":
            return run(args.config, args.output)
        report = load_report(args.output)
        destination = args.destination if args.command == "export" else args.output / "summary.json"
        write_json(destination, report)
        return 0
    except (OSError, UnicodeError, ValueError, KeyError, TypeError, AttributeError, OverflowError):
        print("Recipe replay failed: invalid inputs or inaccessible output; no raw diagnostics exported.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
