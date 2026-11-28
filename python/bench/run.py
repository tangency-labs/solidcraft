"""The experiment of docs/python-api.md: the same tasks, model and checker on five setups.

    A  plain SolidCraft MCP (`solidcraft-cli mcp --headless`)
    B  scx v15.1 as shipped (math and itertools only)
    C  scx v15.1 with imports opened (numpy, scipy, pandas)
    D  the generated full API through the agent's own Python (Bash), with the change report
    E  D plus scx's vocabulary on top of the package

Each run is `claude -p` in an empty folder with only that setup's tools, none of the user's
settings, CLAUDE.md, skills or memory. The checker scores the final design: every expected body
found with volume within 0.1% and bounding box within 0.05 mm, and nothing extra (scx's rule).

    python bench/run.py --arms A,B,C,D,E --sets simple,big,relations,edit,numeric --reps 1 \\
        --model haiku --jobs 6 --out bench/results/pilot.jsonl
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PY = Path(sys.executable)
REPO = HERE.parents[1]
SCX = REPO / "docs" / "scx-hybrid"
SETS = {"simple": SCX / "tasks/simple.json", "big": SCX / "tasks/big.json", "relations": SCX / "tasks/relations.json", "edit": SCX / "tasks/edit.json", "numeric": HERE / "tasks/numeric.json"}
STACK = ("numpy", "scipy", "pandas")
sys.path.insert(0, str(HERE))
import assembly  # noqa: E402
import realworld  # noqa: E402
_print_lock = threading.Lock()


def log(msg: str) -> None:
    with _print_lock:
        print(msg, flush=True)


def load_tasks(sets: list[str], only: set[str] | None) -> list[tuple[str, str, dict]]:
    out = []
    for s in sets:
        if s == "realworld":
            items = {n: {"spec": t["spec"], "realworld": True} for n, t in realworld.TASKS.items()}
        elif s == "assembly":
            items = {n: {"spec": t["prompt"], "assembly": True} for n, t in assembly.TASKS.items()}
        else:
            items = json.loads(SETS[s].read_text())
        for name, t in items.items():
            if not only or name in only:
                out.append((s, name, t))
    return out


def prompt_for(t: dict) -> str:
    if t.get("assembly"):  # the request as a person makes it: no hints about what a good answer holds
        return f"{t['spec']}\n\nBuild it in SolidCraft, in the new, empty design that is open. When you are done, reply with a short summary of the design."
    if t.get("edit"):
        return f"{t['spec']}\n\nWhen you are done, reply with one word: DONE."
    return f"{t['spec']}\n\nBuild it in SolidCraft, in the new, empty design that is open. When you are done, reply with one word: DONE."


# ---------------------------------------------------------------- starting designs for edits


def start_daemon(env: dict, work: Path, design: Path | None) -> subprocess.Popen:
    args = [str(PY), "-m", "solidcraft", "serve", "--allow-path", str(work)] + (["--in", str(design)] if design else [])
    p = subprocess.Popen(args, env=env, cwd=work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    f = Path(env["SOLIDCRAFT_SESSION"])
    for _ in range(600):
        if f.exists():
            return p
        if p.poll() is not None:
            raise RuntimeError("the session daemon exited")
        time.sleep(0.05)
    raise RuntimeError("the session daemon did not start")


def stop_daemon(env: dict, p: subprocess.Popen) -> None:
    subprocess.run([str(PY), "-m", "solidcraft", "stop"], env=env, check=False, capture_output=True)
    try:
        p.wait(timeout=30)
    except subprocess.TimeoutExpired:
        p.kill()


def build_setup(name: str, script: str, cli: str, root: Path) -> Path:
    out = root / "setups" / f"{name}.sc"
    if out.exists():
        return out
    work = root / "setups" / f"_{name}"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    env = {**os.environ, "SOLIDCRAFT_CLI": cli, "SOLIDCRAFT_SESSION": str(work / "session.json"), "SOLIDCRAFT_REPORT": "0", "PYTHONPATH": str(HERE)}
    p = start_daemon(env, out.parent, None)  # its folder: where the setup is saved
    try:
        subprocess.run([str(PY), "-c", "from scx_vocab import *\n" + script], env=env, cwd=work, check=True, capture_output=True, text=True)
        save = f"import solidcraft as sc\nsc.session(report=False).save(path={str(out)!r}, overwrite=True)\n"
        subprocess.run([str(PY), "-c", save], env=env, cwd=work, check=True, capture_output=True, text=True)
    finally:
        stop_daemon(env, p)
    return out


# ---------------------------------------------------------------- one run


def mcp_config(arm: str, work: Path, final: Path, design: Path | None, cli: str, step: Path | None = None, native: Path | None = None) -> dict:
    proxy = [str(PY), "-I", str(HERE / "mcp_proxy.py"), "--out", str(final)] + (["--step", str(step)] if step else []) + (["--save", str(native)] if native else []) + ["--"]
    if arm == "A":
        server = [cli, "mcp", "--headless", "--allow-path", str(work)] + (["--in", str(design)] if design else [])
    elif arm == "B":
        server = [str(PY), "-I", str(SCX / "scx.py"), "--headless", "--text-only"] + (["--in", str(design)] if design else [])
    elif arm == "C":
        server = [str(PY), str(HERE / "scx_open.py"), "--headless", "--text-only"] + (["--in", str(design)] if design else [])
    else:
        return {"mcpServers": {}}
    return {"mcpServers": {"solidcraft": {"command": proxy[0], "args": proxy[1:] + server, "env": {"SCX_CLI": cli}}}}


def parse_stream(path: Path) -> dict:
    calls, names, stack, result = 0, {}, False, {}
    seen = set()
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        if ev.get("type") == "assistant":
            for c in ev.get("message", {}).get("content", []):
                if c.get("type") == "tool_use" and c.get("id") not in seen:
                    seen.add(c.get("id"))
                    calls += 1
                    names[c.get("name")] = names.get(c.get("name"), 0) + 1
                    text = json.dumps(c.get("input", {}))
                    stack = stack or any(f"import {m}" in text or f"from {m}" in text for m in STACK)
        elif ev.get("type") == "result":
            result = ev
    u = result.get("usage", {})
    return {
        "cost_usd": result.get("total_cost_usd"),
        "input_tokens": (u.get("input_tokens") or 0) + (u.get("cache_read_input_tokens") or 0) + (u.get("cache_creation_input_tokens") or 0),
        "output_tokens": u.get("output_tokens"),
        "turns": result.get("num_turns"),
        "duration_s": round((result.get("duration_ms") or 0) / 1000, 1),
        "tool_calls": calls,
        "tools": names,
        "uses_stack": stack,
        "claude_error": result.get("is_error"),
        "final_text": (result.get("result") or "")[:300],
    }


def check(final: dict, expected: list[dict]) -> tuple[bool, str]:
    if "bodies" not in final:
        return False, f"no measurement: {str(final.get('error', final))[:200]}"
    got = [{"name": b.get("name"), "volume": b.get("volume_mm3"), "bbox": [b["bbox"]["min"], b["bbox"]["max"]] if b.get("bbox") else None} for b in final["bodies"]]
    free = list(range(len(got)))
    missing = []
    for e in expected:
        hit = None
        for i in free:
            g = got[i]
            if g["volume"] is None or g["bbox"] is None:
                continue
            if abs(g["volume"] - e["volume"]) > 1e-3 * abs(e["volume"]):
                continue
            if all(abs(a - b) <= 0.05 for pa, pb in zip(g["bbox"], e["bbox"]) for a, b in zip(pa, pb)):
                hit = i
                break
        if hit is None:
            missing.append(e["name"])
        else:
            free.remove(hit)
    extra = [got[i]["name"] for i in free]
    if missing or extra:
        return False, f"missing {missing[:6]}{'...' if len(missing) > 6 else ''}; extra {extra[:6]}"
    return True, "ok"


def run_one(arm: str, set_name: str, task: str, t: dict, rep: int, args, root: Path) -> dict:
    work = root / "runs" / args.tag / arm / task / f"r{rep}"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    final_path = work / "final.json"
    design = build_setup(task, t["setup"], args.cli, root) if t.get("setup") else None
    env = {k: v for k, v in os.environ.items() if not k.startswith(("SOLIDCRAFT", "CLAUDE_CODE_SSE", "PYTHON"))}
    env.update({"SOLIDCRAFT_CLI": args.cli, "PATH": f"{PY.parent}:{env.get('PATH', '')}", "DISABLE_AUTOUPDATER": "1"})
    if args.no_tool_search:
        env["ENABLE_TOOL_SEARCH"] = "false"
    daemon = None
    if arm in ("D", "E"):
        env.update({"SOLIDCRAFT_SESSION": str(work / ".session.json"), "SOLIDCRAFT_REPORT": "1"})
        if arm == "E":
            env["PYTHONPATH"] = str(HERE)
        daemon = start_daemon(env, work, design)
    cfg = work / ".mcp.json"
    step_path = work / "final.step" if t.get("realworld") else None
    native_path = work / "final.sc" if t.get("assembly") else None
    cfg.write_text(json.dumps(mcp_config(arm, work, final_path, design, args.cli, step_path, native_path)))
    cmd = [
        args.claude, "-p", prompt_for(t), "--model", args.model, "--output-format", "stream-json", "--verbose",
        "--setting-sources", "project", "--strict-mcp-config", "--mcp-config", str(cfg), "--disable-slash-commands",
        "--no-session-persistence", "--permission-mode", "bypassPermissions", "--settings", json.dumps({"autoMemoryEnabled": False}),
        "--tools", ",".join((["Bash"] if arm in ("D", "E") else []) + (["WebSearch", "WebFetch"] if t.get("assembly") else [])),
    ]
    if args.budget:
        cmd += ["--max-budget-usd", str(args.budget)]
    if arm in ("D", "E"):
        cmd += ["--append-system-prompt", (HERE / "prompts" / f"{arm.lower()}.md").read_text()]
    stream = work / "stream.jsonl"
    t0 = time.time()
    timed_out = False
    with open(stream, "w", encoding="utf-8") as out:
        try:
            subprocess.run(cmd, cwd=work, env=env, stdout=out, stderr=subprocess.DEVNULL, timeout=args.timeout, check=False)
        except subprocess.TimeoutExpired:
            timed_out = True
    if daemon is not None:
        try:
            probe = "import json, solidcraft as sc\nd = sc.session(report=False)\n"
            if step_path:
                probe += f"try:\n    d.export(path={str(step_path)!r})\nexcept Exception:\n    pass\n"
            if native_path:
                probe += f"try:\n    d.save(path={str(native_path)!r}, overwrite=True)\nexcept Exception:\n    pass\n"
            probe += "print(json.dumps(d.measure(summary=True)))\n"
            r = subprocess.run([str(PY), "-c", probe], env={**env, "SOLIDCRAFT_REPORT": "0"}, capture_output=True, text=True, timeout=300)
            final_path.write_text(r.stdout.strip().splitlines()[-1] if r.returncode == 0 and r.stdout.strip() else json.dumps({"error": r.stderr[-500:]}))
        finally:
            stop_daemon(env, daemon)
    else:
        for _ in range(600):  # the proxy writes it once claude has closed the server
            if final_path.exists():
                break
            time.sleep(0.1)
        if step_path is not None:  # the proxy's last export may still be writing
            last = None
            for _ in range(60):
                now = step_path.stat().st_mtime_ns if step_path.exists() else None
                if now == last:
                    break
                last = now
                time.sleep(1)
    try:
        final = json.loads(final_path.read_text())
    except (OSError, ValueError):
        final = {"error": "no final measurement"}
    extra: dict = {}
    if t.get("assembly"):
        summary = ""
        for line in stream.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                ev = json.loads(line)
            except ValueError:
                continue
            if ev.get("type") == "result":
                summary = ev.get("result") or ""
        (work / "summary.md").write_text(summary)
        for _ in range(60):  # the proxy saves once claude has closed the server
            if native_path and native_path.exists():
                break
            time.sleep(1)
        if native_path and native_path.exists():
            try:
                f = assembly.facts(str(native_path), task)
            except Exception as e:  # noqa: BLE001
                f = {"error": str(e)[:300], "core": {}, "core_score": 0.0}
        else:
            f = {"error": "no final design", "core": {}, "core_score": 0.0}
        (work / "facts.json").write_text(json.dumps(f, indent=1))
        ok = f.get("core_score") == 1.0
        missing = [k for k, v in f.get("core", {}).items() if not v]
        detail = f.get("error") or ("ok" if ok else "missing: " + "; ".join(missing))
        extra = {"score": f.get("core_score", 0.0), "core": f.get("core", {}), "parts": f.get("parts"), "bodies": f.get("bodies"), "bom_items": f.get("bom_items")}
    elif t.get("realworld"):
        from checker import check_step

        if step_path and step_path.exists():
            r = check_step(task, str(step_path), allow=[str(work)])
        else:
            r = {"pass": False, "score": 0.0, "passed": 0, "total": len(realworld.TASKS[task]["checks"]()), "results": [], "error": "no final design"}
        ok = r["pass"]
        fails = [x for x in r["results"] if not x["ok"]]
        detail = "ok" if ok else (r.get("error") or "; ".join(f"{x['check'][:60]} -> {x['detail'][:80]}" for x in fails[:4]))
        extra = {"score": round(r["score"], 3), "checks_passed": r.get("passed"), "checks_total": r.get("total"), "unverified": sum(bool(x.get("unverified")) for x in r["results"]), "failed_checks": [x["check"] for x in fails]}
        (work / "checks.json").write_text(json.dumps(r, indent=1))
    if not t.get("assembly") and not t.get("realworld"):
        ok, detail = check(final, t["bodies"])
    row = {"arm": arm, "set": set_name, "task": task, "rep": rep, "model": args.model, "tag": args.tag, "pass": ok, "detail": detail, "timed_out": timed_out, "wall_s": round(time.time() - t0, 1), **extra}
    row.update(parse_stream(stream))
    score = f" {row['checks_passed']}/{row['checks_total']} checks" if "checks_passed" in row else (f" core {row['score']:.0%}" if "score" in row else "")
    log(f"{arm} {task} r{rep}: {'PASS' if ok else 'FAIL'}{score} {detail[:160] if not ok else ''} ${row['cost_usd'] or 0:.3f} {row['tool_calls']} calls {row['input_tokens'] // 1000}k in")
    return row


def recheck(results: Path, root: Path) -> int:
    from checker import check_step

    rows = [json.loads(line) for line in results.read_text().splitlines() if line.strip()]
    for r in rows:
        if r["set"] != "realworld":
            continue
        work = root / "runs" / r.get("tag", "") / r["arm"] / r["task"] / f"r{r['rep']}"
        step = work / "final.step"
        if step.exists():
            c = check_step(r["task"], str(step), allow=[str(work)])
        else:
            c = {"pass": False, "score": 0.0, "passed": 0, "total": len(realworld.TASKS[r["task"]]["checks"]()), "results": [], "error": "no final design"}
        fails = [x for x in c["results"] if not x["ok"]]
        r.update(
            {
                "pass": c["pass"],
                "detail": "ok" if c["pass"] else (c.get("error") or "; ".join(f"{x['check'][:60]} -> {x['detail'][:80]}" for x in fails[:4])),
                "score": round(c["score"], 3),
                "checks_passed": c.get("passed"),
                "checks_total": c.get("total"),
                "unverified": sum(bool(x.get("unverified")) for x in c["results"]),
                "failed_checks": [x["check"] for x in fails],
            }
        )
        (work / "checks.json").write_text(json.dumps(c, indent=1))
        log(f"{r['arm']} {r['task']} r{r['rep']}: {'PASS' if r['pass'] else 'FAIL'} {r['checks_passed']}/{r['checks_total']} {r['detail'][:200] if not r['pass'] else ''}")
    results.write_text("".join(json.dumps(r) + "\n" for r in rows))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--arms", default="A,B,C,D,E")
    ap.add_argument("--sets", default="simple,big,relations,edit,numeric")
    ap.add_argument("--tasks", default="")
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--model", default="haiku")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--timeout", type=int, default=1200)
    ap.add_argument("--cli", default=os.environ.get("SOLIDCRAFT_CLI", str(REPO / "target/ci/solidcraft-cli")))
    ap.add_argument("--claude", default=shutil.which("claude") or "claude")
    ap.add_argument("--root", default=str(HERE / "work"))
    ap.add_argument("--tag", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-tool-search", action="store_true", help="load MCP tools up front instead of behind ToolSearch")
    ap.add_argument("--recheck", action="store_true", help="score the real-world rows of --out again from their saved designs")
    ap.add_argument("--budget", type=float, default=0, help="the most one run may spend, in US dollars (claude --max-budget-usd)")
    args = ap.parse_args()
    os.environ["SOLIDCRAFT_CLI"] = args.cli  # the checker's own sessions use this build too
    if args.recheck:
        return recheck(Path(args.out), Path(args.root))
    root = Path(args.root)
    tasks = load_tasks(args.sets.split(","), set(filter(None, args.tasks.split(","))) or None)
    for _, name, t in tasks:  # build each edit setup once, before the runs share it
        if t.get("setup"):
            build_setup(name, t["setup"], args.cli, root)
    jobs = [(arm, s, n, t, r) for r in range(args.reps) for (s, n, t) in tasks for arm in args.arms.split(",")]
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    log(f"{len(jobs)} runs: {len(tasks)} tasks x {args.arms} x {args.reps} reps, model {args.model}")
    with open(args.out, "a", encoding="utf-8") as out, cf.ThreadPoolExecutor(args.jobs) as pool:
        futs = [pool.submit(run_one, *j, args, root) for j in jobs]
        for f in cf.as_completed(futs):
            try:
                row = f.result()
            except Exception as e:  # noqa: BLE001 - one broken run must not stop the rest
                log(f"run failed: {e!r}")
                continue
            with _print_lock:
                out.write(json.dumps(row) + "\n")
                out.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
