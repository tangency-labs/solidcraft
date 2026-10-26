"""Fetch and validate the exact base revision's trusted CI report artifact."""
import re
from collections import Counter
from datetime import datetime, timezone


class BaselineError(ValueError):
    pass


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise BaselineError("duplicate JSON key: " + key)
        result[key] = value
    return result


def select_run(runs, base, repo, workflow_id):
    if not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
        raise BaselineError("explicit nonzero full base SHA required")
    trusted = [r for r in runs if (
        r.get("head_sha") == base
        and r.get("event") == "push"
        and r.get("head_branch") == "main"
        and r.get("path") in (".github/workflows/ci.yml", ".github/workflows/ci.yml@main", ".github/workflows/ci.yml@refs/heads/main")
        and r.get("workflow_id") == workflow_id
        and r.get("repository", {}).get("full_name") == repo
        and r.get("head_repository", {}).get("full_name") == repo
        and r.get("repository", {}).get("id") == REPOSITORY_ID
        and r.get("head_repository", {}).get("id") == REPOSITORY_ID
    )]
    if not trusted:
        raise BaselineError("no trusted exact-base push/main CI run; never use latest-main fallback")
    # Choose deterministically first; an unfinished/failed newest exact-base run
    # remains a visible wait/failure, rather than quietly choosing another result.
    run = max(trusted, key=lambda r: (r["id"], r.get("run_attempt", 1)))
    if run.get("status") != "completed":
        raise BaselineError("exact-base CI pending: bounded retry, then fail closed")
    if run.get("conclusion") != "success":
        raise BaselineError("exact-base CI not successful")
    return run


def select_artifact(artifacts, run, base, now):
    candidates = [a for a in artifacts if a.get("name") == "difftest-reports"]
    if len(candidates) != 1:
        raise BaselineError("missing or ambiguous exact-run report artifact")
    artifact = candidates[0]
    if uint(artifact.get("size_in_bytes"), "artifact size") > MAX_ZIP_BYTES:
        raise BaselineError("artifact download limit exceeded")
    wr = artifact.get("workflow_run", {})
    expected = {"id": run["id"], "head_sha": base, "head_branch": "main",
                "repository_id": run["repository"]["id"],
                "head_repository_id": run["head_repository"]["id"]}
    if any(wr.get(k) != v for k, v in expected.items()):
        raise BaselineError("artifact does not belong to selected trusted run/base/repository")
    if timestamp(artifact.get("created_at")) < timestamp(run.get("run_started_at")):
        raise BaselineError("artifact predates selected run attempt")
    expires = datetime.fromisoformat(artifact["expires_at"].replace("Z", "+00:00"))
    if artifact.get("expired") is not False or expires <= now:
        raise BaselineError("exact-base artifact expired")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", "")):
        raise BaselineError("artifact SHA256 digest required")
    return artifact


def uint(value, field):
    if type(value) is not int or not 0 <= value <= (1 << 64) - 1:
        raise BaselineError("invalid nonnegative integer: " + field)
    return value


def legacy_known_integer_presence(case):
    # compare.rs::exact_list increments exact_compared before emitting this
    # indexed integer mismatch. Length/key mismatches prove no compared value.
    for mismatch in case.get("mismatches", []):
        if not isinstance(mismatch, dict) or not re.fullmatch(r"ints\..+\[[0-9]+\]", mismatch.get("field", "")):
            continue
        values = [mismatch.get(key) for key in ["cpp", "rust"]]
        if all(isinstance(v, str) and re.fullmatch(r"-?[0-9]+", v) and -(1 << 63) <= int(v) < (1 << 63) for v in values):
            return True
    return False


def case_has_values(report, case):
    if report.get("schema_version") == 1 and "exact_compared" not in case:
        raise BaselineError("modern schema requires exact comparison count")
    floats = uint(case.get("values_compared"), "values_compared")
    exact = uint(case.get("exact_compared", 0), "exact_compared")
    if floats or exact:
        if case.get("vacuous", False) or case.get("both_errored", False):
            raise BaselineError("case claims values and vacuity")
        return True
    if case["outcome"] == "known_mismatch" and "exact_compared" not in case:
        if not case.get("both_errored", False) and not case.get("vacuous", False) and legacy_known_integer_presence(case):
            return True  # Proven Boolean presence, never an invented exact count.
        raise BaselineError("legacy known-mismatch integer coverage is ambiguous")
    if case.get("both_errored", False) or case.get("vacuous", False):
        return False
    if "exact_compared" in case:
        if case["outcome"] == "pass" and report.get("schema_version") == 1:
            raise BaselineError("modern zero-value success must be marked vacuous")
        return False
    if case["outcome"] == "pass" and "cases_vacuous" in report:
        # #550 writer omitted exact_compared, but is_vacuous() explicitly tested
        # BOTH float and exact counts. Absence of vacuous here proves exact>0.
        return True
    if case["outcome"] == "pass":
        raise BaselineError("pre-550 zero-float pass has unknown integer coverage")
    return False


def validate_report(report, filename_unit):
    if not isinstance(report, dict):
        raise BaselineError("report must be an object")
    if not re.fullmatch(r"[A-Za-z0-9_.]+", filename_unit) or report.get("unit") != filename_unit:
        raise BaselineError("report unit does not match filename")
    if "schema_version" in report and (type(report["schema_version"]) is not int or report["schema_version"] != 1):
        raise BaselineError("unsupported report schema")
    cases = report.get("cases")
    if not isinstance(cases, list):
        raise BaselineError("cases array required")
    if any(not isinstance(c, dict) for c in cases):
        raise BaselineError("case must be an object")
    names = [c.get("name") for c in cases]
    if any(not isinstance(n, str) or not n for n in names) or len(set(names)) != len(names):
        raise BaselineError("case names must be nonempty and unique")
    allowed = {"pass", "fail", "deferred", "rejected", "known_mismatch", "cpp_defect"}
    counts = Counter(c.get("outcome") for c in cases)
    if set(counts) - allowed:
        raise BaselineError("unknown/missing case outcome")
    if uint(report.get("cases_total"), "cases_total") != len(cases):
        raise BaselineError("partial report: case total disagrees")
    for outcome, field in [("pass", "cases_passed"), ("fail", "cases_failed"),
                           ("deferred", "cases_deferred"), ("rejected", "cases_rejected"),
                           ("known_mismatch", "cases_known_mismatch"), ("cpp_defect", "cases_cpp_defect")]:
        if uint(report.get(field) if outcome in {"pass", "fail", "deferred"} else report.get(field, 0), field) != counts[outcome]:
            raise BaselineError("case outcome count disagrees: " + field)
    for c in cases:
        if type(c.get("pass")) is not bool or c["pass"] != (c["outcome"] == "pass"):
            raise BaselineError("case pass/outcome disagree")
        for flag in ["both_errored", "vacuous", "only_zeros"]:
            if flag in c and type(c[flag]) is not bool:
                raise BaselineError("case flag must be boolean: " + flag)
        case_has_values(report, c)
    for field, flag in [("cases_vacuous", "vacuous"), ("cases_both_errored", "both_errored")]:
        if field in report and uint(report[field], field) != sum(c.get(flag, False) for c in cases):
            raise BaselineError("aggregate flag count disagrees: " + field)
    uint(report.get("seeds"), "seeds")
    if report.get("status") not in {"green", "partial", "fail"}:
        raise BaselineError("unknown report status")
    expected_status = ("fail" if report.get("harness_error") is not None or not cases
                       or counts["fail"] or counts["rejected"] == len(cases)
                       else "partial" if counts["deferred"] or counts["known_mismatch"] else "green")
    if report["status"] != expected_status or type(report.get("pass")) is not bool or report["pass"] != (expected_status != "fail"):
        raise BaselineError("report status/pass disagree with cases or harness error")
    passing = [c for c in cases if c["outcome"] == "pass"]
    values = sum(case_has_values(report, c) for c in passing)
    return {"passed": len(passing), "with_values": values,
            "floor_ok": values > 0 and values * 4 >= len(passing) * 3}


def check_unit_set(actual, expected):
    if set(actual) != set(expected):
        raise BaselineError("artifact report set differs from exact-base numeric selection: missing=" + repr(sorted(set(expected) - set(actual))) + " extra=" + repr(sorted(set(actual) - set(expected))))


def validate_set(reports, expected_base_units):
    check_unit_set(reports, expected_base_units)
    return {u: validate_report(d, u) for u, d in reports.items()}


# Cases listed in known_mismatches/<unit>.toml that still mismatch: either kind may turn
# into the other or into a pass; a pass may not turn into either (no silent waiver).
LISTED = {"known_mismatch", "cpp_defect"}


def ratchet_reports(baseline, current):
    """Coverage checks only; value-share floor stays a separate audit/check."""
    violations = []
    for unit, before in baseline.items():
        if unit not in current:
            violations.append((unit, None, "baseline unit dropped")); continue
        after = current[unit]
        validate_report(before, unit); validate_report(after, unit)
        if before.get("seeds") != after.get("seeds"):
            raise BaselineError("CI ratchet requires matching seeds; no generated-case bypass")
        now = {c["name"]: c for c in after["cases"]}
        for old in before["cases"]:
            if old["outcome"] == "rejected":
                continue
            new = now.get(old["name"])
            if new is None:
                violations.append((unit, old["name"], "baseline case dropped"))
            elif old["outcome"] == "pass" and new["outcome"] != "pass":
                violations.append((unit, old["name"], "passing case no longer passes"))
            elif old["outcome"] in LISTED and new["outcome"] not in LISTED | {"pass"}:
                violations.append((unit, old["name"], "known mismatch no longer exercised"))
            elif case_has_values(before, old) and not case_has_values(after, new):
                violations.append((unit, old["name"], "value coverage lost"))
    return violations


# Deliberately fixed trust anchors, never derived from a fork URL or CLI input.
REPOSITORY = "tangency-labs/solidcore"
REPOSITORY_ID = 1415992886
WORKFLOW_ID = 382450238
WORKFLOW_PATH = ".github/workflows/ci.yml"
MAX_ZIP_BYTES = 128 * 1024 * 1024
MAX_REPORT_BYTES = 64 * 1024 * 1024
MAX_EXPANDED_BYTES = 1024 * 1024 * 1024
MAX_WAIT_SECONDS = 25 * 60  # main CI takes about 13 minutes, before runner queueing.


def timestamp(value):
    if not isinstance(value, str):
        raise BaselineError("timestamp required")
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if result.tzinfo is None:
            raise ValueError("timezone required")
        return result
    except ValueError as error:
        raise BaselineError("invalid timestamp") from error


def resolve_base(event, base, payload):
    if payload.get("repository", {}).get("id") != REPOSITORY_ID or payload.get("repository", {}).get("full_name") != REPOSITORY:
        raise BaselineError("untrusted event repository")
    if event == "pull_request":
        expected = payload.get("pull_request", {}).get("base", {}).get("sha")
    elif event == "merge_group":
        if payload.get("merge_group", {}).get("base_ref") != "refs/heads/main":
            raise BaselineError("baseline merge group must target main")
        expected = payload.get("merge_group", {}).get("base_sha")
    elif event == "push":
        if payload.get("ref") != "refs/heads/main":
            raise BaselineError("baseline push must target main")
        expected = payload.get("before")
        if expected == "0" * 40 and base == expected:
            return None  # Genuine first push only, explicitly logged by main().
    elif event == "workflow_dispatch":
        inputs = payload.get("inputs", {})
        expected = inputs.get("baseline_sha", "")
        if not expected and inputs.get("diagnostic_only") in (True, "true") and not base:
            return None  # Explicit diagnostic run, never implicit fallback.
    else:
        raise BaselineError("unsupported baseline event: " + event)
    if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{40}", expected) or expected == "0" * 40 or base != expected:
        raise BaselineError("event requires its exact nonzero full base SHA")
    return expected


def resolve_queue_base(api, queue_base):
    """Find the queue's main ancestor, pinning main once before any artifact wait."""
    # Later queue entries are based on speculative merges, which have never had
    # push/main CI. Compare their cumulative changes with the shared main ancestor.
    # Do not parse GitHub's temporary branch names or use a latest-successful run.
    ref = api.json(f"repos/{REPOSITORY}/git/ref/heads/main")
    main = ref.get("object", {}).get("sha")
    if (ref.get("ref") != "refs/heads/main" or ref.get("object", {}).get("type") != "commit"
            or not isinstance(main, str) or not re.fullmatch(r"[0-9a-f]{40}", main) or main == "0" * 40):
        raise BaselineError("invalid trusted main reference")
    comparison = api.json(f"repos/{REPOSITORY}/compare/{main}...{queue_base}")
    base = comparison.get("merge_base_commit", {}).get("sha")
    if (comparison.get("base_commit", {}).get("sha") != main
            or not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40):
        raise BaselineError("invalid queue/main merge base")
    # main may have advanced since the event, including merging queue_base itself.
    # The immutable comparison must still prove the selected ancestry relation.
    relations = {
        "identical": main == queue_base == base,
        "ahead": base == main and main != queue_base,
        "behind": base == queue_base and main != queue_base,
        "diverged": base not in (main, queue_base) and main != queue_base,
    }
    if not relations.get(comparison.get("status"), False):
        raise BaselineError("inconsistent queue/main ancestry")
    return base, main


class GitHub:
    def raw(self, path):
        import subprocess
        try:
            return subprocess.check_output(["gh", "api", "--hostname", "github.com", "--method", "GET", path], timeout=120)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            raise BaselineError("GitHub request failed: " + path) from error

    def json(self, path):
        import json
        try:
            data = json.loads(self.raw(path), object_pairs_hook=unique_object)
            if not isinstance(data, dict):
                raise BaselineError("API object required: " + path)
            return data
        except (ValueError, TypeError) as error:
            raise BaselineError("invalid API JSON: " + path) from error

    def pages(self, path, key):
        values = []
        for page in range(1, 1001):
            data = self.json(path + ("&" if "?" in path else "?") + f"per_page=100&page={page}")
            batch = data.get(key)
            if not isinstance(batch, list):
                raise BaselineError("missing API collection: " + key)
            values.extend(batch)
            if len(batch) < 100:
                return values
        raise BaselineError("API pagination limit exceeded")


def fetch_exact(api, base, wait_seconds=MAX_WAIT_SECONDS):
    import time
    workflow = api.json(f"repos/{REPOSITORY}/actions/workflows/ci.yml")
    if workflow.get("id") != WORKFLOW_ID or workflow.get("path") != WORKFLOW_PATH:
        raise BaselineError("trusted workflow identity changed")
    deadline = time.monotonic() + wait_seconds
    while True:
        runs = api.pages(f"repos/{REPOSITORY}/actions/workflows/ci.yml/runs?branch=main&event=push&head_sha={base}", "workflow_runs")
        try:
            selected = select_run(runs, base, REPOSITORY, WORKFLOW_ID)
            # Re-fetch detail: list metadata alone is not sufficient provenance.
            run = api.json(f"repos/{REPOSITORY}/actions/runs/{selected['id']}")
            run = select_run([run], base, REPOSITORY, WORKFLOW_ID)
            if (run["id"], run.get("run_attempt", 1)) != (selected["id"], selected.get("run_attempt", 1)):
                raise BaselineError("run attempt changed during selection; retry")
            artifacts = api.pages(f"repos/{REPOSITORY}/actions/runs/{run['id']}/artifacts", "artifacts")
            artifact = select_artifact(artifacts, run, base, datetime.now(timezone.utc))
            return run, artifact
        except BaselineError as error:
            # Only absent/pending runs may be retried. Failed or corrupt provenance
            # is a visible failure; never choose an older/different-base fallback.
            retryable = str(error).startswith(("no trusted exact-base", "exact-base CI pending"))
            if not retryable or time.monotonic() >= deadline:
                raise
            time.sleep(min(10, max(0, deadline - time.monotonic())))


def base_units(base):
    import subprocess
    try:
        subprocess.run(["git", "cat-file", "-e", base + "^{commit}"], check=True, capture_output=True)
    except subprocess.CalledProcessError:
        subprocess.run(["git", "fetch", "--quiet", "--no-tags", "https://github.com/" + REPOSITORY + ".git", base], check=True)
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", base, "plan/units"], text=True).splitlines()
    units = []
    for path in paths:
        if not path.endswith(".toml"):
            continue
        text = subprocess.check_output(["git", "show", base + ":" + path], text=True)
        import sys
        from pathlib import Path
        sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "harness/plan"))
        from units import parse_unit
        plan = parse_unit(text, path)
        unit = path.rsplit("/", 1)[-1][:-5]
        if plan.get("id") != unit or not isinstance(plan.get("status"), str):
            raise BaselineError("unsupported exact-base unit plan: " + path)
        if plan["status"] in {"green", "partial"}:
            units.append(unit)
    if "audit.recursion" in units:
        # The exact base's selector, not a missing-runner heuristic, declares the
        # crate-wide review meta-unit outside numeric difftest execution.
        selection = subprocess.check_output(["git", "show", base + ":crates/difftest/src/plan.rs"], text=True)
        meta = subprocess.check_output(["git", "show", base + ":plan/units/audit.recursion.toml"], text=True)
        if '.filter(|u| u.id != "audit.recursion")' not in selection or 'rust_module = "(crate-wide)"' not in meta or 'upstream = []' not in meta:
            raise BaselineError("unrecognized audit meta-unit selection policy")
        units.remove("audit.recursion")
        import sys
        print("difftest-baseline: exact-base selector excludes audit.recursion (crate-wide review meta-unit, no numeric runner)", file=sys.stderr)
    if not units or len(units) != len(set(units)):
        raise BaselineError("missing/duplicate exact-base unit set")
    return sorted(units)


def validate_current_selection(expected, root):
    """Prior numeric units must remain selected, even when today's selection is empty."""
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "harness/plan"))
    from units import load_units
    plans = load_units(root)
    if any(not isinstance(unit.get("status"), str) for unit in plans):
        raise BaselineError("unsupported current unit plan")
    selected = {unit["id"] for unit in plans
                if unit["id"] != "audit.recursion" and unit["status"] in {"green", "partial"}}
    missing = set(expected) - selected
    if missing:
        raise BaselineError("current --green selection drops exact-base numeric units: " + repr(sorted(missing)))


def install_zip(blob, out, expected, provenance):
    import hashlib, io, json, pathlib, shutil, stat, tempfile, zipfile
    if len(blob) > MAX_ZIP_BYTES or "sha256:" + hashlib.sha256(blob).hexdigest() != provenance["digest"]:
        raise BaselineError("artifact ZIP size/digest mismatch")
    out = pathlib.Path(out)
    if out.exists():
        raise BaselineError("output already exists; refusing to mix baseline revisions")
    out.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(tempfile.mkdtemp(prefix=".baseline-", dir=out.parent))
    try:
        reports = {}
        target = staging / "reports"
        target.mkdir()
        with zipfile.ZipFile(io.BytesIO(blob)) as archive:
            members = archive.infolist()
            if len(members) > 10000 or sum(m.file_size for m in members) > MAX_EXPANDED_BYTES:
                raise BaselineError("artifact expansion limit exceeded")
            seen = set()
            for item in members:
                name = item.filename
                if name in seen or "/" in name or "\\" in name or item.is_dir() or stat.S_ISLNK(item.external_attr >> 16):
                    raise BaselineError("unsafe/duplicate ZIP member")
                seen.add(name)
                if name == "times.tsv":
                    continue  # Queue timing is never provenance or gate input.
                if not name.endswith(".json") or item.file_size > MAX_REPORT_BYTES:
                    raise BaselineError("unexpected artifact member: " + name)
                data = archive.read(item)
                report = json.loads(data, object_pairs_hook=unique_object)
                unit = name[:-5]
                validate_report(report, unit)
                reports[unit] = {"seeds": report["seeds"]}  # Retain no full case corpus in memory.
                (target / name).write_bytes(data)
        check_unit_set(reports, expected)
        provenance = dict(provenance, schema="validated-550-or-exact-v1", units=expected,
                          seeds={unit: report["seeds"] for unit, report in reports.items()})
        (staging / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        staging.rename(out)  # Same-filesystem atomic publication only after validation.
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main():
    import argparse, json, os, sys
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("event")
    parser.add_argument("base")
    parser.add_argument("out")
    parser.add_argument("--wait-seconds", type=int, default=MAX_WAIT_SECONDS)
    parser.add_argument("--current-plan", help="working-tree root whose --green selection must retain the base units")
    args = parser.parse_args()
    try:
        if not 0 <= args.wait_seconds <= MAX_WAIT_SECONDS:
            raise BaselineError(f"wait must be between zero and {MAX_WAIT_SECONDS} seconds")
        with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as stream:
            payload = json.load(stream, object_pairs_hook=unique_object)
        base = resolve_base(args.event, args.base, payload)
        if base is None:
            print("difftest-baseline: explicitly running without a prior base (bootstrap/diagnostic mode)", file=sys.stderr)
            return 0
        api = GitHub()
        source_base = base
        queue_main = None
        if args.event == "merge_group":
            base, queue_main = resolve_queue_base(api, source_base)
            print(f"difftest-baseline: queue base {source_base}, main snapshot {queue_main}, baseline {base}", file=sys.stderr)
        run, artifact = fetch_exact(api, base, args.wait_seconds)
        expected = base_units(base)
        provenance = {"repository": REPOSITORY, "repository_id": REPOSITORY_ID,
                      "workflow_id": WORKFLOW_ID, "workflow_path": WORKFLOW_PATH,
                      "source_event": args.event, "run_event": run["event"],
                      "source_base_sha": source_base,
                      "run_branch": run["head_branch"], "run_head_sha": run["head_sha"],
                      "base_sha": base, "run_id": run["id"], "run_attempt": run.get("run_attempt", 1),
                      "artifact_id": artifact["id"], "digest": artifact["digest"],
                      "selection_policy": "exact-base green/partial numeric units; audit.recursion excluded only when explicitly declared by exact-base selector"}
        if queue_main is not None:
            provenance["queue_main_snapshot_sha"] = queue_main
        detail = api.json(f"repos/{REPOSITORY}/actions/artifacts/{artifact['id']}")
        select_artifact([detail], run, base, datetime.now(timezone.utc))
        if detail.get("id") != artifact["id"] or detail.get("digest") != artifact["digest"]:
            raise BaselineError("artifact identity changed before download")
        blob = api.raw(f"repos/{REPOSITORY}/actions/artifacts/{artifact['id']}/zip")
        current = select_run([api.json(f"repos/{REPOSITORY}/actions/runs/{run['id']}")], base, REPOSITORY, WORKFLOW_ID)
        if current.get("run_attempt", 1) != run.get("run_attempt", 1):
            raise BaselineError("run attempt changed while downloading")
        install_zip(blob, args.out, expected, provenance)
        if args.current_plan is not None:
            validate_current_selection(expected, args.current_plan)
        print("difftest-baseline: validated " + json.dumps(provenance, sort_keys=True), file=sys.stderr)
        print("--baseline " + os.path.join(args.out, "reports"))
        return 0
    except (BaselineError, ValueError, KeyError, OSError, TypeError) as error:
        print("difftest-baseline: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
