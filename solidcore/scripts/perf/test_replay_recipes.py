"""Contract and process-failure tests for recipe replay; no real kernel builds."""

import copy
import io
import signal
import shutil
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location("replay_recipes", Path(__file__).with_name("replay_recipes.py"))
replay = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(replay)


def measurement():
    """A complete evaluation with typed geometry and deliberately private text."""
    return {"body_count": 1, "total": {"faces": 6, "edges": 12, "vertices": 8, "volume_mm3": 1.0, "area_mm2": 6.0},
            "timeline": [{"ms": 5, "name": "PRIVATE_SENTINEL"}],
            "bodies": [{"closed": True, "bbox": {"min": [0, 0, 0], "max": [1, 1, 1]}, "name": "PRIVATE_SENTINEL"}]}


class ReplayTests(unittest.TestCase):
    def setUp(self):
        """Create isolated inputs and a complete replay configuration."""
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "binary").write_bytes(b"prebuilt-binary")
        (self.root / "recipe.json").write_text("{}")
        self.config = {"schema_version": 1, "rounds": 3, "threads": 2, "timeout_seconds": 1, "warmup_case": "case",
                       "variants": [{"id": "baseline", "binary": "binary"}], "cases": [{"id": "case", "recipe": "recipe.json"}]}
        self.config_path = self.root / "config.json"
        replay.write_json(self.config_path, self.config)
        self.manifest, _ = replay.prepare(self.config_path)
        self.manifest["state"] = "complete"

    def figures(self, data=None):
        path = self.root / "eval.json"
        path.write_text(json.dumps(measurement() if data is None else data))
        return replay.figures(path)

    def sample(self, **changes):
        result = {"case": "case", "variant": "baseline", "phase": "measured", "round": 1,
                  "status": "ok", "exit": 0, "seconds": 2.0, "cpu_user_seconds": 0.1, "cpu_system_seconds": 0.0, "max_rss_kib": 100, "figures": self.figures()}
        result.update(changes)
        return replay.clean_run(result, self.manifest)

    def test_manifest_is_path_free_and_hashes_exact_inputs(self):
        """Provenance retains content hashes without exposing input locations."""
        text = json.dumps(self.manifest)
        self.assertNotIn(str(self.root), text)
        self.assertNotIn('"binary"', text)
        self.assertEqual(self.manifest["variants"][0]["binary_sha256"], replay.digest(self.root / "binary"))
        for key, value in (("schema_version", True), ("rounds", True), ("threads", 0), ("timeout_seconds", float("nan"))):
            bad = copy.deepcopy(self.manifest)
            bad[key] = value
            with self.assertRaises(replay.Invalid):
                replay.public_manifest(bad)

    def test_paths_are_not_accepted_as_public_ids(self):
        """Reject path-shaped labels at the public publication boundary."""
        for value in ("../case", "user@example.test", "machine/name", "has space", ""):
            with self.assertRaises(replay.Invalid):
                replay.identifier(value)

    def test_malformed_missing_and_nonfinite_eval_are_rejected(self):
        """Malformed evaluation data cannot become a successful sample."""
        self.assertIsNone(replay.figures(self.root / "missing"))
        for mutate in (
            lambda d: d["total"].update(volume_mm3=float("nan")),
            lambda d: d["total"].update(faces=True),
            lambda d: d.update(body_count=2),
            lambda d: d["timeline"][0].update(ms=-1),
            lambda d: d["bodies"][0].update(closed="true"),
            lambda d: d["bodies"][0]["bbox"].update(max=[1, float("inf"), 1]),
            lambda d: d["bodies"][0]["bbox"].update(min=[2, 0, 0]),
            lambda d: d["bodies"][0].pop("bbox"),
        ):
            data = measurement()
            mutate(data)
            self.assertIsNone(self.figures(data))

    def test_incomplete_geometry_does_not_become_a_fast_success(self):
        """Partial geometry remains ineligible for successful timing summaries."""
        for key in ("errors", "suppressed", "rolled_back", "open_bodies_skipped"):
            run = self.sample()
            run["figures"][key] = 1
            self.assertFalse(replay.successful(run))
        run = self.sample()
        run["figures"]["closed"] = [False]
        self.assertFalse(replay.successful(run))

    def test_medians_exclude_failures_but_keep_failed_durations(self):
        """Keep failure evidence while excluding it from eligible medians."""
        good = self.sample(seconds=4.0)
        failed = self.sample(round=2, status="exit_error", exit=1, seconds=0.01)
        warmup = self.sample(round=0, phase="warmup", seconds=0.001)
        row = replay.summarize(self.manifest, [good, failed, warmup])["summary"][0]
        self.assertEqual(row["median_seconds"], 4.0)
        self.assertEqual(row["successful_samples"], 1)
        self.assertEqual(row["not_attempted"], 1)
        self.assertEqual(row["failures"][0]["seconds"], 0.01)
        self.manifest["state"] = "input_changed"
        row = replay.summarize(self.manifest, [good])["summary"][0]
        self.assertIsNone(row["median_seconds"])
        self.assertFalse(row["timing_eligible"])

    def test_export_allowlist_discards_private_fields_at_every_level(self):
        """Nested unknown fields never cross the public export boundary."""
        self.manifest["private_path"] = "PRIVATE_SENTINEL"
        self.manifest["variants"][0]["binary"] = "PRIVATE_SENTINEL"
        self.manifest["cases"][0]["recipe"] = "PRIVATE_SENTINEL"
        run = self.sample()
        run["stderr"] = "PRIVATE_SENTINEL"
        run["figures"]["face_types"] = {"PRIVATE_SENTINEL": 3}
        replay.write_json(self.root / "manifest.json", self.manifest)
        (self.root / "runs.jsonl").write_text(json.dumps(run) + "\n")
        exported = replay.load_report(self.root)
        self.assertNotIn("PRIVATE_SENTINEL", json.dumps(exported))
        self.assertEqual(exported["samples"][0]["figures"]["faces"], 6)

    def test_duplicate_sample_is_rejected(self):
        """Repeated sample identities invalidate a report instead of biasing it."""
        replay.write_json(self.root / "manifest.json", self.manifest)
        (self.root / "runs.jsonl").write_text((json.dumps(self.sample()) + "\n") * 2)
        with self.assertRaises(replay.Invalid):
            replay.load_report(self.root)

    def test_bad_resource_metrics_are_rejected(self):
        """Require complete finite resource measurements with integer RSS."""
        path = self.root / "resources"
        for text in ("", "nan 2 3", "1 -2 3", "1 2 3.5", "1 2", "not metrics"):
            path.write_text(text)
            self.assertIsNone(replay.resources(path))
        path.write_text("diagnostic line\n1.5 2.5 300\n")
        self.assertEqual(replay.resources(path)["max_rss_kib"], 300)

    def test_spawn_failure_and_timeout_are_recorded(self):
        """Distinguish launch failure from a deadline-triggered cleanup."""
        with patch.object(replay.subprocess, "Popen", side_effect=OSError("PRIVATE_SENTINEL")):
            result = replay.execute(["missing"], {}, self.root / "failed", 1)
        self.assertEqual(result["status"], "spawn_error")
        self.assertNotIn("PRIVATE_SENTINEL", json.dumps(result))
        process = Mock(returncode=-9)
        released = replay.threading.Event()
        process.wait.side_effect = lambda: (released.wait(2), -9)[1]
        with patch.object(replay.subprocess, "Popen", return_value=process), patch.object(replay, "stop_process", side_effect=lambda p: released.set()) as stop:
            result = replay.execute(["fake"], {}, self.root / "timeout", 0.01)
        stop.assert_called_once_with(process)
        self.assertEqual(result["status"], "timeout")

    def test_empty_geometry_and_missing_resources_are_not_successes(self):
        """Empty or unmeasured evaluations cannot qualify as successful."""
        run = self.sample()
        run["figures"].update(bodies=0, closed=[], bounds=[])
        self.assertFalse(replay.successful(run))
        for key in ("cpu_user_seconds", "cpu_system_seconds", "max_rss_kib"):
            run = self.sample()
            run[key] = None
            self.assertFalse(replay.successful(run))

    def test_success_uses_blocking_wait_without_timeout_polling(self):
        """Successful evaluations use a blocking wait with no polling timeout."""
        process = Mock(returncode=0)
        process.wait.return_value = 0
        with patch.object(replay.subprocess, "Popen", return_value=process), patch.object(replay, "process_group_exists", return_value=False):
            result = replay.execute(["fake"], {}, self.root / "blocking", 1)
        process.wait.assert_called_once_with()
        self.assertEqual(result["status"], "ok")

    def test_watchdog_startup_interrupt_cleans_owned_evaluator(self):
        """Cancellation before the watchdog starts still reaps the evaluator."""
        process = Mock(returncode=-15)
        with patch.object(replay.subprocess, "Popen", return_value=process), patch.object(replay.threading.Thread, "start", side_effect=KeyboardInterrupt), patch.object(replay, "stop_process") as stop:
            result = replay.execute(["fake"], {}, self.root / "startup-interrupt", 1)
        stop.assert_called_once_with(process)
        self.assertEqual(result["status"], "interrupted")
        self.assertEqual(result["exit"], -15)

    def test_watchdog_startup_failure_cleans_owned_evaluator(self):
        """A thread-start failure cannot leak a successfully spawned evaluator."""
        process = Mock(returncode=-15)
        with patch.object(replay.subprocess, "Popen", return_value=process), patch.object(replay.threading.Thread, "start", side_effect=RuntimeError("thread startup failed")), patch.object(replay, "stop_process") as stop:
            with self.assertRaises(RuntimeError):
                replay.execute(["fake"], {}, self.root / "startup-failure", 1)
        stop.assert_called_once_with(process)

    @unittest.skipUnless(sys.platform.startswith("linux"), "owned process-group cleanup requires Linux")
    def test_startup_interrupt_reaps_real_evaluator(self):
        """Inject interruption at watchdog startup and verify real child exit."""
        processes = []
        popen = subprocess.Popen

        def spawn(*args, **kwargs):
            process = popen(*args, **kwargs)
            processes.append(process)
            return process

        try:
            with patch.object(replay.subprocess, "Popen", side_effect=spawn), patch.object(replay.threading.Thread, "start", side_effect=KeyboardInterrupt):
                result = replay.execute([sys.executable, "-c", "import time; time.sleep(60)"], os.environ.copy(), self.root / "real-startup", 5)
            self.assertEqual(result["status"], "interrupted")
            self.assertIsNotNone(processes[0].poll())
            self.assertFalse(replay.process_group_exists(processes[0]))
        finally:
            for process in processes:
                if process.poll() is None:
                    replay.stop_process(process)

    def run_mocked(self, execute):
        """Exercise orchestration without invoking platform-specific utilities."""
        # Exercise orchestration on all hosts without invoking Linux utilities.
        with patch.object(replay.sys, "platform", "linux"), patch.object(replay.shutil, "which", return_value="nice"), patch.object(replay.Path, "is_file", return_value=True), patch.object(replay, "execute", side_effect=execute):
            return replay.run(self.config_path, self.root / "output")

    def fake_success(self, command, environment, prefix, timeout):
        """Emit a complete fake evaluation and verify its thread budget."""
        replay.write_json(Path(str(prefix) + ".json"), measurement())
        Path(str(prefix) + ".resources").write_text("0.01 0.02 100\n")
        self.assertEqual(environment["RAYON_NUM_THREADS"], "2")
        return {"status": "ok", "exit": 0, "seconds": 1.0}

    def test_warmup_failure_stops_before_measured_samples(self):
        """A failed warmup censors all measured runs for the experiment."""
        self.assertEqual(self.run_mocked(lambda *args: {"status": "exit_error", "exit": 1, "seconds": 0.1}), 2)
        report = replay.load_report(self.root / "output")
        self.assertEqual(report["manifest"]["state"], "warmup_failed")
        self.assertEqual(len(report["samples"]), 1)
        self.assertEqual(report["summary"][0]["samples"], 0)

    def test_interrupted_warmup_preserves_the_reason(self):
        """An interrupted warmup is not blamed on the evaluated binary."""
        self.assertEqual(self.run_mocked(lambda *args: {"status": "interrupted", "exit": -15, "seconds": 0.1}), 2)
        report = replay.load_report(self.root / "output")
        self.assertEqual(report["manifest"]["state"], "interrupted")
        self.assertEqual(len(report["samples"]), 1)
        self.assertIsNone(report["summary"][0]["median_seconds"])

    def test_abort_reasons_are_fixed_codes_not_exception_text(self):
        """I/O, invalid sample and load failures retain only public reason codes."""
        for error, reason in ((OSError("PRIVATE_SENTINEL"), "io_error"), (replay.Invalid("PRIVATE_SENTINEL"), "invalid_sample")):
            with self.subTest(reason=reason):
                self.root = Path(self.temp.name) / reason
                self.root.mkdir()
                stream = io.StringIO()
                with patch("sys.stderr", stream):
                    self.assertEqual(self.run_mocked(Mock(side_effect=error)), 2)
                report = replay.load_report(self.root / "output")
                self.assertEqual(report["manifest"]["abort_reason"], reason)
                self.assertIn(reason, stream.getvalue())
                self.assertNotIn("PRIVATE_SENTINEL", stream.getvalue() + json.dumps(report))
        self.root = Path(self.temp.name) / "load"
        self.root.mkdir()
        self.config["max_load"] = 0
        replay.write_json(self.config_path, self.config)
        with patch.object(replay.os, "getloadavg", return_value=(1, 1, 1), create=True), patch("sys.stderr", io.StringIO()) as stream:
            self.assertEqual(self.run_mocked(self.fake_success), 2)
        report = replay.load_report(self.root / "output")
        self.assertEqual(report["manifest"]["abort_reason"], "load_exceeded")
        self.assertEqual(report["samples"], [])
        self.assertIn("load_exceeded", stream.getvalue())
        report["manifest"]["abort_reason"] = "PRIVATE_SENTINEL"
        with self.assertRaises(replay.Invalid):
            replay.public_manifest(report["manifest"])

    def test_interruption_preserves_completed_samples(self):
        """Cancellation keeps earlier samples and the interrupted attempt."""
        count = 0

        def execute(*args):
            nonlocal count
            count += 1
            return self.fake_success(*args) if count <= 2 else {"status": "interrupted", "exit": -15, "seconds": 0.2}

        self.assertEqual(self.run_mocked(execute), 2)
        report = replay.load_report(self.root / "output")
        self.assertEqual(report["manifest"]["state"], "interrupted")
        self.assertEqual(len(report["samples"]), 3)
        self.assertEqual(report["summary"][0]["successful_samples"], 1)

    def test_timeout_censors_remaining_repeats_without_faking_samples(self):
        """Skipped repeats are reported as unattempted, never synthetic timings."""
        count = 0

        def execute(*args):
            nonlocal count
            count += 1
            return self.fake_success(*args) if count == 1 else {"status": "timeout", "exit": -9, "seconds": 1.0}

        self.assertEqual(self.run_mocked(execute), 0)
        report = replay.load_report(self.root / "output")
        self.assertEqual(len(report["samples"]), 2)
        self.assertEqual(report["summary"][0]["not_attempted"], 2)
        self.assertIsNone(report["summary"][0]["median_seconds"])

    def test_mutated_binary_invalidates_the_experiment(self):
        """Changing a measured executable invalidates its timing summary."""
        def execute(*args):
            (self.root / "binary").write_bytes(b"changed")
            return self.fake_success(*args)

        self.assertEqual(self.run_mocked(execute), 2)
        report = replay.load_report(self.root / "output")
        self.assertEqual(report["manifest"]["state"], "input_changed")
        self.assertIsNone(report["summary"][0]["median_seconds"])

    @unittest.skipUnless(sys.platform.startswith("linux"), "owned process-group cleanup requires Linux")
    def test_normal_leader_exit_cleans_surviving_descendant(self):
        """A detached workload cannot outlive a nominally successful evaluation."""
        child_file = self.root / "child.pid"
        program = "\n".join([
            "import os, signal, time, pathlib",
            "child = os.fork()",
            "if child == 0:",
            " signal.signal(signal.SIGTERM, signal.SIG_IGN)",
            f" pathlib.Path({str(child_file)!r}).write_text(str(os.getpid()))",
            " time.sleep(60)",
            "else:",
            f" while not pathlib.Path({str(child_file)!r}).exists(): time.sleep(.01)",
        ])
        result = replay.execute([sys.executable, "-c", program], os.environ.copy(), self.root / "orphan", 5)
        self.assertEqual(result["status"], "descendants_remaining")
        self.assert_process_stopped(int(child_file.read_text()))

    @unittest.skipUnless(sys.platform.startswith("linux") and shutil.which("nice") and Path("/usr/bin/time").is_file(),
                         "SIGTERM lifecycle requires Linux, nice and GNU time")
    def test_sigterm_during_warmup_cleans_child_and_finalizes_manifest(self):
        """External termination preserves interrupted state and stops owned work."""
        child_file = self.root / "evaluator.pid"
        binary = self.root / "binary"
        binary.write_text("#!/usr/bin/env python3\nimport os, pathlib, time\n"
                          + f"pathlib.Path({str(child_file)!r}).write_text(str(os.getpid()))\n"
                          + "time.sleep(60)\n")
        binary.chmod(0o700)
        self.config["timeout_seconds"] = 30
        replay.write_json(self.config_path, self.config)
        output = self.root / "output"
        runner = subprocess.Popen([sys.executable, str(Path(replay.__file__)), "run", str(self.config_path), str(output)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        child = None
        try:
            deadline = time.monotonic() + 5
            while not child_file.exists() and runner.poll() is None and time.monotonic() < deadline:
                time.sleep(.02)
            self.assertTrue(child_file.exists(), "evaluator did not start")
            child = int(child_file.read_text())
            runner.send_signal(signal.SIGTERM)
            self.assertEqual(runner.wait(timeout=5), 2)
            report = replay.load_report(output)
            self.assertEqual(report["manifest"]["state"], "interrupted")
            self.assertEqual(report["samples"][0]["status"], "interrupted")
            self.assert_process_stopped(child)
        finally:
            if runner.poll() is None:
                runner.kill()
                runner.wait()
            if child is not None:
                try:
                    os.kill(child, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def assert_process_stopped(self, pid):
        """Permit an exited child awaiting reaping, but never a running orphan."""
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            stat = Path(f"/proc/{pid}/stat")
            if not stat.exists() or stat.read_text().split()[2] == "Z":
                return
            time.sleep(0.02)
        self.fail("runner left its descendant running")

    @unittest.skipUnless(sys.platform.startswith("linux"), "owned process-group cleanup requires Linux")
    def test_timeout_kills_descendant_even_when_leader_exits_first(self):
        """Deadline cleanup terminates surviving descendants after leader exit."""
        child_file = self.root / "child.pid"
        program = ("import os,signal,time,pathlib; p=os.fork(); "
                   "signal.signal(signal.SIGTERM, signal.SIG_IGN) if p==0 else None; "
                   f"pathlib.Path({str(child_file)!r}).write_text(str(os.getpid())) if p==0 else None; "
                   "time.sleep(60)")
        result = replay.execute([sys.executable, "-c", program], os.environ.copy(), self.root / "process", 1)
        self.assertEqual(result["status"], "timeout")
        pid = int(child_file.read_text())
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            stat = Path(f"/proc/{pid}/stat")
            if not stat.exists() or stat.read_text().split()[2] == "Z":
                break
            time.sleep(0.02)
        else:
            self.fail("runner left its timed-out descendant running")


if __name__ == "__main__":
    unittest.main()
