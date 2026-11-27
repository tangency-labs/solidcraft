"""An AI judge for open-ended assemblies: one fixed rubric, blind to the setup that made the design.

For each run it renders four views of the final design, then gives `claude -p` (Opus by default)
those pictures, the checker's facts (assembly.py) and the model's own summary, under neutral
names, and asks for scores as JSON. The same judge and rubric score every setup, so differences
come from the designs. A person's scores on the review board check the judge.

    python bench/judge.py --results bench/results/asm-sonnet.jsonl [--model opus]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
VIEWS = ("iso", "front", "right", "top")

RUBRIC = """You are reviewing a CAD design made in answer to this request:

  "{prompt}"

In this folder: four renders of the final design (view-iso.png, view-front.png, view-right.png,
view-top.png), facts.json (what an automatic checker measured in the design file: parts, bodies,
the bill of materials count, whether a Raspberry Pi 5 hole pattern (58 x 49 mm, M2.5) and standard
servo tab patterns (49.5 x 10 mm) exist, interference between parts, and which kinds of component
the design names), and summary.md (the designer's own description; treat its claims as claims, and
check them against the renders and facts).

Read every file (use the Read tool on each picture), then score the design from 1 (absent or
wrong) to 5 (what a capable engineer would deliver) on each criterion:

- completeness: a whole robot: body, four legs with enough joints to walk, electronics.
- components: real, researched parts at their true sizes (a high-torque servo class, the Pi 5,
  a suitable battery and regulator), named consistently.
- mechanics: leg geometry suited to the servos' torque and range, joint axes lined up with servo
  outputs, a body stiff enough, feet.
- integration: real mounting points for every part (servo pockets or brackets, horn attachment,
  Pi 5 standoffs, battery holder, regulator mount), clearances, no collisions.
- manufacturability: printable in PLA or PETG (wall thicknesses, splits into printable parts,
  fastening).
- detail: wiring routed and secured, fasteners modelled, a complete bill of materials.

Reply with JSON only, no other text:
{{"completeness": n, "components": n, "mechanics": n, "integration": n, "manufacturability": n, "detail": n,
  "overall": n (1-10), "strengths": "one sentence", "weaknesses": "one sentence"}}
"""

# v2 (2026-11-26), after the owner's review of the first 30 designs: they preferred one model's
# sensing and wire routing, but found the other more correct in its electrical architecture.
# v1 folded all of that into `detail`, so it could not tell the two apart. v2 scores them apart.
RUBRIC_V2 = RUBRIC.replace(
    """- detail: wiring routed and secured, fasteners modelled, a complete bill of materials.""",
    """- electrical: a correct power and signal architecture: battery, switch or fuse, regulators sized
  for their loads, the servo power and signal distribution as buses or harnesses with connectors,
  and the controller and servo driver wired to them.
- wiring: wires modelled along real routes (bends, slack at the joints, strain relief), secured to
  the structure, and kept clear of moving parts.
- sensing: the sensors and controls a walking robot needs: an IMU, a power switch, and anything
  else its control needs.
- detail: fasteners modelled, and a complete bill of materials.""",
).replace(
    """{{"completeness": n, "components": n, "mechanics": n, "integration": n, "manufacturability": n, "detail": n,""",
    """{{"completeness": n, "components": n, "mechanics": n, "integration": n, "manufacturability": n, "electrical": n,
  "wiring": n, "sensing": n, "detail": n,""",
)


def render(design: Path, out: Path, cli: str) -> list[str]:
    got = []
    for v in VIEWS:
        p = out / f"view-{v}.png"
        r = subprocess.run([cli, "snapshot", "--in", str(design), "--out", str(p), "--width", "900", "--height", "700", "--view", v], capture_output=True, timeout=300)
        if r.returncode == 0 and p.exists():
            got.append(p.name)
    return got


def judge_run(run: Path, prompt: str, model: str, cli: str, claude: str, rubric: str = "v1") -> dict:
    jd = run / ("judge" if rubric == "v1" else f"judge-{rubric}")
    shutil.rmtree(jd, ignore_errors=True)
    jd.mkdir()
    design = run / "final.sc"
    if not design.exists():
        return {"error": "no design"}
    views = render(design, jd, cli)
    facts = json.loads((run / "facts.json").read_text()) if (run / "facts.json").exists() else {}
    (jd / "facts.json").write_text(json.dumps({k: v for k, v in facts.items() if k != "names_sample"}, indent=1))
    (jd / "summary.md").write_text((run / "summary.md").read_text() if (run / "summary.md").exists() else "(no summary)")
    env = {k: v for k, v in os.environ.items() if not k.startswith("SOLIDCRAFT")}
    cmd = [
        claude, "-p", (RUBRIC if rubric == "v1" else RUBRIC_V2).format(prompt=prompt), "--model", model, "--output-format", "json", "--setting-sources", "project",
        "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--disable-slash-commands", "--no-session-persistence",
        "--permission-mode", "bypassPermissions", "--tools", "Read", "--settings", json.dumps({"autoMemoryEnabled": False}),
    ]
    r = subprocess.run(cmd, cwd=jd, env=env, capture_output=True, text=True, timeout=900)
    try:
        res = json.loads(r.stdout)
        text = res.get("result", "")
        scores, _ = json.JSONDecoder().raw_decode(text[text.find("{") :])  # the first JSON object; text after it is ignored
        scores["judge_cost_usd"] = res.get("total_cost_usd")
        scores["views"] = views
    except (ValueError, KeyError) as e:
        scores = {"error": f"judge reply unreadable: {e}: {r.stdout[-300:]}"}
    (jd / "scores.json").write_text(json.dumps(scores, indent=1))
    return scores


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", required=True)
    ap.add_argument("--model", default="opus")
    ap.add_argument("--root", default=str(HERE / "work"))
    ap.add_argument("--cli", default=os.environ.get("SOLIDCRAFT_CLI", str(HERE / "work/bin/solidcraft-cli")))
    ap.add_argument("--claude", default=shutil.which("claude") or "claude")
    ap.add_argument("--arms", default="", help="judge only these setups (default: all)")
    ap.add_argument("--rubric", default="v1", choices=["v1", "v2"], help="v2 scores electrical architecture, wiring and sensing apart")
    args = ap.parse_args()
    sys.path.insert(0, str(HERE))
    import assembly

    path = Path(args.results)
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    for row in rows:
        if row.get("set") != "assembly" or (args.arms and row["arm"] not in args.arms.split(",")):
            continue
        run = Path(args.root) / "runs" / row["tag"] / row["arm"] / row["task"] / f"r{row['rep']}"
        s = judge_run(run, assembly.TASKS[row["task"]]["prompt"], args.model, args.cli, args.claude, args.rubric)
        row["judge" if args.rubric == "v1" else f"judge_{args.rubric}"] = s
        print(f"{row['arm']} {row['task']} r{row['rep']}: overall {s.get('overall')} {s.get('error', '')}", flush=True)
    path.write_text("".join(json.dumps(r) + "\n" for r in rows))
    return 0


if __name__ == "__main__":
    sys.exit(main())
