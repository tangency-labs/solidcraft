#!/usr/bin/env python3
"""Export each fixture as MJCF with solidcraft-cli and check it in MuJoCo against expected/.

Values in expected/ are worked out by hand, never taken from SolidCraft's output. MuJoCo runs as a
separate Python package; nothing is linked. Without it the check is a failure when CI is set and a
printed skip otherwise.
"""
import json
import math
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
TOLERANCE = 1e-6
DYNAMICS_TOLERANCE = 1e-3
STEP = 1 / 2000


def cli():
    return os.environ.get("SOLIDCRAFT_CLI") or str(ROOT / "target" / "release" / "solidcraft-cli")


def export(fixture, out):
    r = subprocess.run([cli(), "run", str(fixture), "--out", str(out)], capture_output=True, text=True, check=False)
    if r.returncode:
        raise AssertionError((r.stdout + r.stderr).strip()[-600:])


def near(got, want, tol, what):
    if any(abs(g - w) > tol for g, w in zip(got, want)):
        raise AssertionError(f"{what}: {list(got)}, expected {want}")


def check(mujoco, xml, expected):
    m = mujoco.MjModel.from_xml_path(str(xml))
    d = mujoco.MjData(m)
    mujoco.mj_forward(m, d)
    kinds = {int(mujoco.mjtJoint.mjJNT_HINGE): "hinge", int(mujoco.mjtJoint.mjJNT_SLIDE): "slide", int(mujoco.mjtJoint.mjJNT_BALL): "ball"}
    joints = {m.joint(i).name: i for i in range(m.njnt)}
    if set(joints) != set(expected["joints"]):
        raise AssertionError(f"joints {sorted(joints)}, expected {sorted(expected['joints'])}")
    for name, want in expected["joints"].items():
        j = joints[name]
        kind = kinds.get(int(m.jnt_type[j]), "other")
        if kind != want["type"]:
            raise AssertionError(f"joint {name} is {kind}, expected {want['type']}")
        if "ref" in want:
            near([m.qpos0[m.jnt_qposadr[j]]], [want["ref"]], TOLERANCE, f"joint {name} ref")
        if "range" in want and want["range"] is None and m.jnt_limited[j]:
            raise AssertionError(f"joint {name} is limited")
    if m.neq != expected["equalities"]:
        raise AssertionError(f"{m.neq} equality constraints, expected {expected['equalities']}")
    for body, com in expected.get("com", {}).items():
        near(d.xipos[m.body(body).id], com, TOLERANCE, f"{body} centre of mass")
    for body, want in expected.get("com_from", {}).items():
        c = d.xipos[m.body(body).id]
        p = want["point"]
        near([math.dist(c, p), c[2] - p[2]], [want["distance"], want["dz"]], TOLERANCE, f"{body} centre from {p}")
    for case in expected.get("set", []):
        before = {b: d.xmat[m.body(b).id].reshape(3, 3).copy() for b in case.get("turn_z", {})}
        for name, value in case["qpos"].items():
            d.qpos[m.jnt_qposadr[joints[name]]] = value
        mujoco.mj_kinematics(m, d)
        for body, com in case.get("com", {}).items():
            near(d.xipos[m.body(body).id], com, TOLERANCE, f"{body} centre of mass at {case['qpos']}")
        for body, angle in case.get("turn_z", {}).items():
            r = d.xmat[m.body(body).id].reshape(3, 3) @ before[body].T
            near([math.atan2(r[1][0], r[0][0]), r[2][2]], [angle, 1.0], TOLERANCE, f"{body} turn about z")
        mujoco.mj_resetData(m, d)
        mujoco.mj_forward(m, d)
    m.opt.timestep = STEP
    if "fall" in expected:
        f = expected["fall"]
        body = m.body(f["body"]).id
        z0 = d.xipos[body][2]
        for _ in range(round(f["seconds"] / STEP)):
            mujoco.mj_step(m, d)
        near([z0 - d.xipos[body][2]], [f["drop"]], DYNAMICS_TOLERANCE, f"{f['body']} drop")
        mujoco.mj_resetData(m, d)
    if "spin" in expected:
        s = expected["spin"]
        j = joints[s["joint"]]
        q0 = d.qpos[m.jnt_qposadr[j]]
        d.qvel[m.jnt_dofadr[j]] = 1.0
        for _ in range(round(s["seconds"] / STEP)):
            mujoco.mj_step(m, d)
        near([abs(d.qpos[m.jnt_qposadr[j]] - q0)], [s["turned"]], DYNAMICS_TOLERANCE * 10, f"{s['joint']} turn in {s['seconds']} s")
        mujoco.mj_resetData(m, d)
    for _ in range(round(1.0 / STEP)):
        mujoco.mj_step(m, d)
    if not all(math.isfinite(x) for x in d.qpos):
        raise AssertionError("a second of simulation is not finite")


def main():
    try:
        import mujoco
    except ImportError:
        if os.environ.get("CI"):
            print("FAIL mujoco is not installed")
            return 1
        print("skip: mujoco is not installed (pip install -r tools/mjcf-check/requirements.txt)")
        return 0
    names = sys.argv[1:] or sorted(p.stem for p in (HERE / "fixtures").glob("*.json"))
    failed = 0
    with tempfile.TemporaryDirectory() as tmp:
        for name in names:
            xml = Path(tmp) / name / f"{name}.mjcf"
            xml.parent.mkdir()
            try:
                export(HERE / "fixtures" / f"{name}.json", xml)
                check(mujoco, xml, json.loads((HERE / "expected" / f"{name}.json").read_text()))
                print(f"ok   {name}")
            except AssertionError as e:
                failed += 1
                print(f"FAIL {name}: {e}")
    print(f"MuJoCo {mujoco.__version__}: {len(names) - failed} of {len(names)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
