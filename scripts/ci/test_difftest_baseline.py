"""Fail-closed artifact tests: no network, native compiler or extra dependency."""
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
import zipfile
from datetime import datetime, timezone
import difftest_baseline as b

BASE = "a" * 40
NOW = datetime(2026, 10, 23, tzinfo=timezone.utc)


def run_metadata():
    repo = {"id": b.REPOSITORY_ID, "full_name": b.REPOSITORY}
    return dict(id=101, run_attempt=1, head_sha=BASE, event="push", head_branch="main",
                path=b.WORKFLOW_PATH, workflow_id=b.WORKFLOW_ID, repository=repo,
                head_repository=repo.copy(), status="completed", conclusion="success",
                run_started_at="2026-10-22T00:00:00Z")


def artifact():
    return dict(id=201, name="difftest-reports", expired=False, size_in_bytes=100,
                expires_at="2027-01-01T00:00:00Z", created_at="2026-10-22T01:00:00Z",
                digest="sha256:" + "b" * 64, workflow_run=dict(id=101, head_sha=BASE,
                head_branch="main", repository_id=b.REPOSITORY_ID, head_repository_id=b.REPOSITORY_ID))


def report():
    return dict(unit="a.b", seeds=16, status="green", cases_total=1, cases_passed=1,
                cases_failed=0, cases_deferred=0, cases_vacuous=0,
                cases=[dict(name="integer", outcome="pass", pass_=True, values_compared=0)])


def valid_report():
    r = report()
    r["pass"] = True
    r["cases"][0]["pass"] = r["cases"][0].pop("pass_")
    return r


def archive(files):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for name, value in files:
            z.writestr(name, value)
    return buf.getvalue()


class BaselineTests(unittest.TestCase):
    def test_event_matrix(self):
        repo = dict(id=b.REPOSITORY_ID, full_name=b.REPOSITORY)
        for event, extra in [("pull_request", {"pull_request": {"base": {"sha": BASE}}}),
                             ("merge_group", {"merge_group": {"base_sha": BASE}}),
                             ("push", {"ref": "refs/heads/main", "before": BASE}),
                             ("workflow_dispatch", {"inputs": {"baseline_sha": BASE}})]:
            p = dict(repository=repo, **extra)
            self.assertEqual(b.resolve_base(event, BASE, p), BASE)
            for wrong in ["", "0" * 40, "b" * 40]:
                with self.subTest(event=event, wrong=wrong), self.assertRaises(b.BaselineError):
                    b.resolve_base(event, wrong, p)
        with self.assertRaises(b.BaselineError):
            b.resolve_base("workflow_dispatch", "", dict(repository=repo, inputs={}))
        self.assertIsNone(b.resolve_base("workflow_dispatch", "", dict(repository=repo, inputs={"diagnostic_only": "true"})))
        self.assertIsNone(b.resolve_base("push", "0" * 40, dict(repository=repo, ref="refs/heads/main", before="0" * 40)))
        with self.assertRaises(b.BaselineError):
            b.resolve_base("push", "", dict(repository=repo, ref="refs/heads/main", before=""))

    def test_run_trust_and_latest_failure(self):
        r = run_metadata()
        self.assertEqual(b.select_run([r], BASE, b.REPOSITORY, b.WORKFLOW_ID), r)
        for key, value in [("head_sha", "b" * 40), ("event", "pull_request"), ("head_branch", "fork"),
                           ("workflow_id", 1), ("path", "nightly.yml")]:
            with self.subTest(key=key), self.assertRaises(b.BaselineError):
                b.select_run([dict(r, **{key: value})], BASE, b.REPOSITORY, b.WORKFLOW_ID)
        for key in ["repository", "head_repository"]:
            wrong = copy.deepcopy(r); wrong[key]["id"] = 1
            with self.assertRaises(b.BaselineError): b.select_run([wrong], BASE, b.REPOSITORY, b.WORKFLOW_ID)
        for status, conclusion in [("in_progress", None), ("completed", "failure")]:
            with self.assertRaises(b.BaselineError):
                b.select_run([r, dict(r, id=102, status=status, conclusion=conclusion)], BASE, b.REPOSITORY, b.WORKFLOW_ID)

    def test_artifact_boundaries(self):
        r, a = run_metadata(), artifact()
        self.assertEqual(b.select_artifact([a], r, BASE, NOW), a)
        for items in [[], [a, a], [dict(a, expired=True)], [dict(a, digest="")],
                      [dict(a, expires_at="2020-01-01T00:00:00Z")], [dict(a, created_at="2020-01-01T00:00:00Z")]]:
            with self.assertRaises(b.BaselineError): b.select_artifact(items, r, BASE, NOW)
        for key in a["workflow_run"]:
            wrong = copy.deepcopy(a); wrong["workflow_run"][key] = "wrong"
            with self.assertRaises(b.BaselineError): b.select_artifact([wrong], r, BASE, NOW)

    def test_coverage_bridge_and_corrupt_reports(self):
        r = valid_report()
        self.assertEqual(b.validate_report(r, "a.b")["with_values"], 1)
        for field in ["cases_vacuous", "cases"]:
            wrong = copy.deepcopy(r); wrong.pop(field)
            with self.assertRaises(b.BaselineError): b.validate_report(wrong, "a.b")
        for mutation in [lambda x: x["cases"].append(x["cases"][0]), lambda x: x.update(cases_total=3),
                         lambda x: x.update(unit="wrong"), lambda x: x.update(status="fail"),
                         lambda x: x.update(**{"pass": False}), lambda x: x.update(harness_error="broken"),
                         lambda x: x.update(seeds=2**64), lambda x: x["cases"][0].update(values_compared=-1),
                         lambda x: x["cases"][0].update(outcome="known_mismatch")]:
            wrong = copy.deepcopy(r); mutation(wrong)
            with self.assertRaises(b.BaselineError): b.validate_report(wrong, "a.b")
        with self.assertRaises(b.BaselineError):
            json.loads('{"a":1,"a":2}', object_pairs_hook=b.unique_object)
        with self.assertRaises(b.BaselineError): b.validate_set({"a.b": r}, ["a.b", "missing"])
        for invalid in ["", "a/b", "a-b"]:
            with self.subTest(unit=invalid), self.assertRaises(b.BaselineError):
                b.validate_report(dict(r, unit=invalid), invalid)

    def test_known_outcomes_and_value_presence_are_retained(self):
        def known(outcome="known_mismatch", exact=1):
            r = valid_report()
            r["schema_version"] = 1
            c = r["cases"][0]
            c.update(outcome=outcome, exact_compared=exact)
            c["pass"] = outcome == "pass"
            for kind, field in [("pass", "cases_passed"), ("fail", "cases_failed"), ("deferred", "cases_deferred"), ("rejected", "cases_rejected"), ("known_mismatch", "cases_known_mismatch")]:
                r[field] = int(outcome == kind)
            r["status"] = "fail" if outcome in {"fail", "rejected"} else "partial" if outcome in {"known_mismatch", "deferred"} else "green"
            r["pass"] = r["status"] != "fail"
            if outcome == "pass" and exact == 0:
                c["vacuous"] = True; r["cases_vacuous"] = 1
            return r
        before = known()
        for outcome in ["known_mismatch", "pass"]:
            self.assertEqual(b.ratchet_reports({"a.b": before}, {"a.b": known(outcome)}), [])
            self.assertEqual(len(b.ratchet_reports({"a.b": before}, {"a.b": known(outcome, 0)})), 1)
        for outcome in ["deferred", "rejected", "fail"]:
            self.assertEqual(len(b.ratchet_reports({"a.b": before}, {"a.b": known(outcome, 0)})), 1)
        legacy = known(); legacy.pop("schema_version"); legacy["cases"][0].pop("exact_compared")
        legacy["cases"][0]["mismatches"] = [dict(field="ints.status[0]", cpp="1006", rust="1001")]
        b.validate_report(legacy, "a.b")
        self.assertTrue(b.case_has_values(legacy, legacy["cases"][0]))
        for bad in [[], [dict(field="ints.status.len", cpp="1", rust="2")], [dict(field="ints.status[0]", cpp="missing", rust="1")]]:
            wrong = copy.deepcopy(legacy); wrong["cases"][0]["mismatches"] = bad
            with self.assertRaises(b.BaselineError): b.validate_report(wrong, "a.b")

    def test_atomic_zip_and_digest(self):
        blob = archive([("a.b.json", json.dumps(valid_report()))])
        provenance = dict(digest="sha256:" + hashlib.sha256(blob).hexdigest())
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "baseline"
            b.install_zip(blob, out, ["a.b"], provenance)
            self.assertTrue((out / "reports/a.b.json").is_file())
            self.assertEqual(json.loads((out / "provenance.json").read_text())["units"], ["a.b"])
            with self.assertRaises(b.BaselineError): b.install_zip(blob, out, ["a.b"], provenance)
        for bad, expected, prov in [(b"truncated", ["a.b"], provenance),
                                   (blob, ["missing"], provenance),
                                   (blob, ["a.b"], dict(digest="sha256:" + "0" * 64))]:
            with tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp) / "baseline"
                with self.assertRaises((b.BaselineError, zipfile.BadZipFile)):
                    b.install_zip(bad, out, expected, prov)
                self.assertFalse(out.exists()); self.assertEqual(list(Path(tmp).iterdir()), [])
        for files in [[("../escape.json", "{}")], [("a.b.json", "{}"), ("a.b.json", "{}")], [("other.txt", "x")]]:
            bad = archive(files)
            with tempfile.TemporaryDirectory() as tmp, self.assertRaises(b.BaselineError):
                b.install_zip(bad, Path(tmp) / "out", ["a.b"], dict(digest="sha256:" + hashlib.sha256(bad).hexdigest()))

    def test_modern_counts_and_explicit_nightly_selection(self):
        r = valid_report(); r["schema_version"] = 1
        with self.assertRaises(b.BaselineError): b.validate_report(r, "a.b")
        r["cases"][0]["exact_compared"] = 7
        self.assertEqual(b.validate_report(r, "a.b")["with_values"], 1)
        self.assertEqual(r["cases"][0]["exact_compared"], 7)
        import importlib.util
        spec = importlib.util.spec_from_file_location("fuzz_units", Path(__file__).with_name("fuzz-units.py"))
        module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
        plans = [dict(id="a.b", status="green"), dict(id="audit.recursion", status="green"), dict(id="other", status="todo")]
        self.assertEqual(module.select(plans, ["a.b"]), ["a.b"])
        with self.assertRaises(ValueError): module.select(plans, [])
        with self.assertRaises(ValueError): module.select([], ["a.b"])

    def test_main_installs_only_after_complete_trust_chain(self):
        import contextlib
        import os
        import sys
        from unittest.mock import patch
        blob = archive([("a.b.json", json.dumps(valid_report()))])
        art = artifact(); art.update(digest="sha256:" + hashlib.sha256(blob).hexdigest(),
                                    size_in_bytes=len(blob), expires_at="9999-01-01T00:00:00Z")
        class API(b.GitHub):
            def json(self, path):
                if path.endswith("/ci.yml"): return dict(id=b.WORKFLOW_ID, path=b.WORKFLOW_PATH)
                if "/runs?" in path: return {"workflow_runs": [run_metadata()]}
                if path.endswith("/artifacts?per_page=100&page=1"): return {"artifacts": [art]}
                if path.endswith("/artifacts/201"): return art
                if path.endswith("/runs/101"): return run_metadata()
                raise AssertionError(path)
            def raw(self, path):
                assert path.endswith("/artifacts/201/zip"), path
                return blob
        with tempfile.TemporaryDirectory() as tmp:
            event = Path(tmp) / "event.json"
            event.write_text(json.dumps(dict(repository=dict(id=b.REPOSITORY_ID, full_name=b.REPOSITORY),
                                              pull_request=dict(base=dict(sha=BASE)))))
            out = Path(tmp) / "baseline"
            stdout, stderr = io.StringIO(), io.StringIO()
            args = ["helper", "pull_request", BASE, str(out), "--wait-seconds", "0"]
            with patch.object(b, "GitHub", API), patch.object(b, "base_units", return_value=["a.b"]), \
                 patch.dict(os.environ, GITHUB_EVENT_PATH=str(event)), patch.object(sys, "argv", args), \
                 contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                self.assertEqual(b.main(), 0)
            self.assertEqual(stdout.getvalue().strip(), "--baseline " + str(out / "reports"))
            provenance = json.loads((out / "provenance.json").read_text())
            self.assertEqual((provenance["run_event"], provenance["run_branch"], provenance["base_sha"]), ("push", "main", BASE))
            self.assertEqual(provenance["source_event"], "pull_request")
            # A corrupted download must emit no usable argument and install nothing.
            art["digest"] = "sha256:" + "0" * 64
            args[3] = str(Path(tmp) / "corrupt")
            stdout = io.StringIO()
            with patch.object(b, "GitHub", API), patch.object(b, "base_units", return_value=["a.b"]), \
                 patch.dict(os.environ, GITHUB_EVENT_PATH=str(event)), patch.object(sys, "argv", args), \
                 contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(b.main(), 1)
            self.assertEqual(stdout.getvalue(), "")
            self.assertFalse(Path(args[3]).exists())

    def test_api_pagination_and_errors(self):
        class API(b.GitHub):
            def __init__(self): self.calls = []
            def json(self, path):
                self.calls.append(path)
                return {"items": [1] * 100 if path.endswith("&page=1") else [2]}
        api = API(); self.assertEqual(len(api.pages("path", "items")), 101)
        self.assertEqual(len(api.calls), 2)
        import subprocess
        from unittest.mock import patch
        for code in [403, 404, 410]:
            with self.subTest(status=code), patch("subprocess.check_output", side_effect=subprocess.CalledProcessError(1, ["gh"], stderr=f"HTTP {code}")), self.assertRaises(b.BaselineError):
                b.fetch_exact(b.GitHub(), BASE, 0)
        with patch("subprocess.check_output", side_effect=subprocess.TimeoutExpired(["gh"], 120)), self.assertRaises(b.BaselineError):
            b.GitHub().raw("repos/fixed/path")
        class Pending(b.GitHub):
            def json(self, path):
                if path.endswith("ci.yml"): return dict(id=b.WORKFLOW_ID, path=b.WORKFLOW_PATH)
                return {"workflow_runs": [dict(run_metadata(), status="in_progress", conclusion=None)]}
        with self.assertRaises(b.BaselineError): b.fetch_exact(Pending(), BASE, 0)
        with patch("time.monotonic", side_effect=[0, 0, 0, 2]), patch("time.sleep") as sleep, self.assertRaises(b.BaselineError):
            b.fetch_exact(Pending(), BASE, 1)
        sleep.assert_called_once_with(1)

    def test_workflow_checks_empty_selection_before_native_setup(self):
        workflow = (Path(__file__).resolve().parents[2] / ".github/workflows/ci.yml").read_text()
        start = workflow.index("      - name: Validate exact-base report artifact\n")
        block = workflow[start:workflow.index("      - name:", start + 1)]
        self.assertNotIn("        if:", block)
        self.assertIn('--current-plan "$GITHUB_WORKSPACE"', block)
        self.assertLess(start, workflow.index("      - name: Upstream sources, sccache and the C++ reference"))

    def test_script_retains_base_units_when_current_selection_is_empty(self):
        import os
        import subprocess
        import sys
        helper = Path(__file__).resolve().with_name("difftest-baseline.sh")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plan = root / "plan/units/a.b.toml"
            plan.parent.mkdir(parents=True)
            plan.write_text('id = "a.b"\nstatus = "green"\n')
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
            git("init", "--quiet")
            git("add", "plan")
            git("-c", "user.name=Gate test", "-c", "user.email=gate@example.invalid",
                "commit", "--quiet", "-m", "base with one selected unit")
            base = git("rev-parse", "HEAD")
            blob = archive([("a.b.json", json.dumps(valid_report()))])
            run = run_metadata(); run["head_sha"] = base
            art = artifact()
            art["workflow_run"]["head_sha"] = base
            art.update(digest="sha256:" + hashlib.sha256(blob).hexdigest(),
                       size_in_bytes=len(blob), expires_at="9999-01-01T00:00:00Z")
            prefix = "repos/" + b.REPOSITORY + "/actions/"
            responses = {
                prefix + "workflows/ci.yml": dict(id=b.WORKFLOW_ID, path=b.WORKFLOW_PATH),
                prefix + f"workflows/ci.yml/runs?branch=main&event=push&head_sha={base}&per_page=100&page=1": {"workflow_runs":[run]},
                prefix + "runs/101": run,
                prefix + "runs/101/artifacts?per_page=100&page=1": {"artifacts":[art]},
                prefix + "artifacts/201": art,
            }
            paths = {}
            for i, (api, response) in enumerate(responses.items()):
                file = root / f"response-{i}.json"
                file.write_text(json.dumps(response)); paths[api] = str(file)
            zipped = root / "reports.zip"; zipped.write_bytes(blob)
            paths[prefix + "artifacts/201/zip"] = str(zipped)
            mapping = root / "responses.json"; mapping.write_text(json.dumps(paths))
            calls = root / "calls"
            bin_dir = root / "fake-bin"; bin_dir.mkdir()
            gh = bin_dir / "gh"
            gh.write_text("#!" + sys.executable + "\n" +
                "import json,os,pathlib,sys\n"
                "with open(os.environ['GATE_TEST_CALLS'],'a') as f: f.write(sys.argv[-1]+'\\n')\n"
                "paths=json.loads(pathlib.Path(os.environ['GATE_TEST_RESPONSES']).read_text())\n"
                "sys.stdout.buffer.write(pathlib.Path(paths[sys.argv[-1]]).read_bytes())\n")
            gh.chmod(0o755)
            event = root / "event.json"
            payload = dict(repository=dict(id=b.REPOSITORY_ID, full_name=b.REPOSITORY),
                           pull_request=dict(base=dict(sha=base)), inputs=dict(diagnostic_only=True))
            event.write_text(json.dumps(payload))
            env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"],
                       GITHUB_EVENT_PATH=str(event), GATE_TEST_CALLS=str(calls),
                       GATE_TEST_RESPONSES=str(mapping))
            def invoke(name, event_name="pull_request", sha=base):
                return subprocess.run(["bash", str(helper), event_name, sha, str(root / name),
                    "--wait-seconds", "0", "--current-plan", str(root)], cwd=root, env=env,
                    text=True, capture_output=True)
            for state in ["todo", "blocked", "removed", "different_green"]:
                if state == "different_green":
                    plan.with_name("new.unit.toml").write_text('id = "new.unit"\nstatus = "green"\n')
                if state == "removed": plan.unlink()
                else:
                    status = "todo" if state == "different_green" else state
                    plan.write_text(f'id = "a.b"\nstatus = "{status}"\n')
                result = invoke(state)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertIn("current --green selection drops exact-base numeric units", result.stderr)
                self.assertTrue((root / state / "provenance.json").is_file())
                self.assertIn(prefix + "artifacts/201/zip", calls.read_text())
            # Empty selection must not bypass artifact integrity checks either.
            zipped.write_bytes(b"corrupt")
            result = invoke("corrupt")
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("current --green selection drops", result.stderr)
            self.assertEqual(result.stdout, "")
            zipped.write_bytes(blob)
            plan.write_text('id = "a.b"\nstatus = "partial"\n')
            result = invoke("retained")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "--baseline " + str(root / "retained/reports"))
            # The producer prerequisite adds counts without a schema-version bump.
            # A length-only known mismatch is ambiguous without that actual count,
            # even when an unrelated new unit is selected by the current plan.
            mixed = valid_report()
            mixed.update(status="partial", cases_total=2, cases_known_mismatch=1)
            mixed["cases"][0]["exact_compared"] = 1
            mixed["cases"].append(dict(name="length-only", outcome="known_mismatch",
                **{"pass": False}, values_compared=0,
                mismatches=[dict(field="ints.ids.len", cpp="2", rust="1")]))
            for label, explicit_count in [("legacy-length", False), ("producer-length", True)]:
                if explicit_count:
                    mixed["cases"][1]["exact_compared"] = 0
                migrated = archive([("a.b.json", json.dumps(mixed))])
                zipped.write_bytes(migrated)
                art.update(digest="sha256:" + hashlib.sha256(migrated).hexdigest(),
                           size_in_bytes=len(migrated))
                for api in [prefix + "runs/101/artifacts?per_page=100&page=1",
                            prefix + "artifacts/201"]:
                    Path(paths[api]).write_text(json.dumps(
                        {"artifacts": [art]} if "/artifacts?" in api else art))
                result = invoke(label)
                if explicit_count:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    saved = json.loads((root / label / "reports/a.b.json").read_text())
                    self.assertEqual(saved["cases"][1]["exact_compared"], 0)
                    self.assertNotIn("schema_version", saved)
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("legacy known-mismatch integer coverage is ambiguous", result.stderr)
                    self.assertEqual(result.stdout, "")
                    self.assertFalse((root / label).exists())
            # Explicit diagnostic/first-push semantics remain explicit; PR inputs above did not bypass.
            before = calls.read_text()
            payload.pop("pull_request")
            event.write_text(json.dumps(payload))
            self.assertEqual(invoke("diagnostic", "workflow_dispatch", "").returncode, 0)
            payload.update(ref="refs/heads/main", before="0" * 40)
            event.write_text(json.dumps(payload))
            self.assertEqual(invoke("bootstrap", "push", "0" * 40).returncode, 0)
            self.assertEqual(calls.read_text(), before)



if __name__ == "__main__": unittest.main()
