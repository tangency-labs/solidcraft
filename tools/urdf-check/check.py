#!/usr/bin/env python3
"""Export fixtures with solidcraft-cli and check them with urdfdom, yourdfpy, PyBullet and MuJoCo.

Expected values in expected/ are worked out by hand, never from SolidCraft's output. A missing tool
is a failure when CI is set, and a printed skip otherwise.
"""
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
TOLERANCE = 1e-6
SIM_SECONDS = 2.0
SIM_STEP = 1 / 2000
DROP_TOLERANCE = 1e-3


class Skip(Exception):
    pass


def need(module):
    try:
        return __import__(module)
    except ImportError as e:
        raise Skip(f"{module} is not installed") from e


def check_urdf(urdf, expected):
    tool = shutil.which("check_urdf")
    if not tool:
        raise Skip("check_urdf is not installed")
    r = subprocess.run([tool, str(urdf)], capture_output=True, text=True)
    if r.returncode:
        raise AssertionError((r.stdout + r.stderr).strip()[-400:])
    return "parsed"


def turn_of(rotation):
    return math.acos(max(-1.0, min(1.0, (rotation[0][0] + rotation[1][1] + rotation[2][2] - 1) / 2)))


def poses(urdf, expected):
    np = need("numpy")
    yourdfpy = need("yourdfpy")
    robot = yourdfpy.URDF.load(str(urdf), load_meshes=False, build_scene_graph=True)
    def transform(link, q):
        robot.update_cfg({name: q.get(name, 0.0) for name in robot.actuated_joint_names})
        return np.array(robot.get_transform(link, "base_link"))
    checked = 0
    for joint, want in expected.get("joints", {}).items():
        j = robot.joint_map[joint]
        got = {"effort": j.limit.effort, "velocity": j.limit.velocity, "damping": j.dynamics.damping if j.dynamics else None, "friction": j.dynamics.friction if j.dynamics else None}
        for key, value in want.items():
            # yourdfpy can hand back the attribute's text.
            if got[key] is None or abs(float(got[key]) - value) > TOLERANCE:
                raise AssertionError(f"{joint} {key}: {got[key]}, expected {value}")
        checked += 1
    for link, want in expected.get("mass", {}).items():
        got = robot.link_map[link].inertial.mass
        if abs(float(got) - want) > TOLERANCE:
            raise AssertionError(f"{link} mass: {got}, expected {want}")
        checked += 1
    for pose in expected.get("poses", []):
        for link, want in pose["links"].items():
            rest = transform(link, {})
            t = transform(link, pose["q"])
            xyz = t[:3, 3]
            if "xyz" in want and np.linalg.norm(xyz - np.array(want["xyz"])) > TOLERANCE:
                raise AssertionError(f"{link} at {pose['q']}: origin {xyz}, expected {want['xyz']}")
            if "from" in want:
                d = np.linalg.norm(xyz - np.array(want["from"]))
                if abs(d - want["distance"]) > TOLERANCE:
                    raise AssertionError(f"{link} at {pose['q']}: moved {d}, expected {want['distance']}")
            if "z" in want and abs(xyz[2] - want["z"]) > TOLERANCE:
                raise AssertionError(f"{link} at {pose['q']}: z {xyz[2]}, expected {want['z']}")
            relative = t[:3, :3] @ rest[:3, :3].T
            if "turn" in want and abs(turn_of(relative) - want["turn"]) > TOLERANCE:
                raise AssertionError(f"{link} at {pose['q']}: turned {turn_of(relative)}, expected {want['turn']}")
            if "about" in want:
                axis = np.array(want["about"], dtype=float)
                if np.linalg.norm(relative @ axis - axis) > TOLERANCE:
                    raise AssertionError(f"{link} at {pose['q']}: did not turn about {want['about']}")
            checked += 1
    return f"{checked} poses"


def bullet_world(urdf, gravity):
    pb = need("pybullet")
    client = pb.connect(pb.DIRECT)
    pb.setTimeStep(SIM_STEP, physicsClientId=client)
    pb.setGravity(*gravity, physicsClientId=client)
    # Without this flag PyBullet recomputes inertia from the collision shapes and ignores the file.
    robot = pb.loadURDF(str(urdf), useFixedBase=True, flags=pb.URDF_USE_INERTIA_FROM_FILE, physicsClientId=client)
    joints = {}
    for i in range(pb.getNumJoints(robot, physicsClientId=client)):
        info = pb.getJointInfo(robot, i, physicsClientId=client)
        joints[info[1].decode()] = i
        pb.setJointMotorControl2(robot, i, pb.VELOCITY_CONTROL, force=0, physicsClientId=client)
    return pb, client, robot, joints


def period_of(samples):
    """Mean period from the upward zero crossings of (time, value) samples."""
    crossings = []
    for (t0, a), (t1, b) in zip(samples, samples[1:]):
        if a < 0 <= b:
            crossings.append(t0 + (t1 - t0) * (-a) / (b - a))
    if len(crossings) < 2:
        return None
    return (crossings[-1] - crossings[0]) / (len(crossings) - 1)


def pybullet(urdf, expected):
    pb, client, robot, joints = bullet_world(urdf, (0, 0, -9.81))
    try:
        for _ in range(int(SIM_SECONDS / SIM_STEP)):
            pb.stepSimulation(physicsClientId=client)
        for i in joints.values():
            for x in pb.getJointState(robot, i, physicsClientId=client)[:2]:
                if not math.isfinite(x):
                    raise AssertionError("simulation is not finite")
    finally:
        pb.disconnect(client)
    notes = ["finite"]
    if "pendulum" in expected:
        want = expected["pendulum"]
        pb, client, robot, joints = bullet_world(urdf, (0, 0, 9.81 * want["gravity_sign_z"]))
        try:
            joint = joints[want["joint"]]
            pb.resetJointState(robot, joint, want["start"], physicsClientId=client)
            samples = []
            for k in range(int(SIM_SECONDS / SIM_STEP)):
                pb.stepSimulation(physicsClientId=client)
                samples.append((k * SIM_STEP, pb.getJointState(robot, joint, physicsClientId=client)[0]))
        finally:
            pb.disconnect(client)
        notes.append(pendulum_note(samples, want))
    if "free_slide" in expected:
        notes.append(slide_note(urdf, expected["free_slide"], bullet_slide))
    if "drops" in expected:
        notes.append(drop_note(urdf, expected["drops"]))
    return ", ".join(notes)


def pendulum_note(samples, want):
    period = period_of(samples)
    if period is None:
        raise AssertionError("the pendulum did not swing")
    if abs(period - want["period"]) > want["tolerance"] * want["period"]:
        raise AssertionError(f"period {period:.4f} s, expected {want['period']:.4f} s")
    return f"period {period:.4f} s (expected {want['period']:.4f})"


def slide_direction(urdf, joint):
    np = need("numpy")
    yourdfpy = need("yourdfpy")
    robot = yourdfpy.URDF.load(str(urdf), load_meshes=False, build_scene_graph=True)
    child = robot.joint_map[joint].child
    robot.update_cfg({name: 0.0 for name in robot.actuated_joint_names})
    a = np.array(robot.get_transform(child, "base_link"))[:3, 3]
    robot.update_cfg({name: (0.01 if name == joint else 0.0) for name in robot.actuated_joint_names})
    b = np.array(robot.get_transform(child, "base_link"))[:3, 3]
    return (b - a) / 0.01


def slide_note(urdf, want, simulate):
    distance = simulate(urdf, want["joint"], slide_direction(urdf, want["joint"]) * 9.81, want["time"])
    if abs(distance - want["distance"]) > want["tolerance"] * want["distance"]:
        raise AssertionError(f"slid {distance:.5f} m, expected {want['distance']:.5f} m")
    return f"slid {distance:.5f} m (expected {want['distance']:.5f})"


def drop_note(urdf, drops):
    """Drop a 5 mm ball on each shape: it rests at the worked-out height, or falls through."""
    pb, client, _, _ = bullet_world(urdf, (0, 0, -9.81))
    try:
        ball = pb.createCollisionShape(pb.GEOM_SPHERE, radius=0.005, physicsClientId=client)
        bodies = []
        for drop in drops:
            start = [drop["xy"][0], drop["xy"][1], 0.03]
            bodies.append(pb.createMultiBody(0.01, ball, basePosition=start, physicsClientId=client))
        for _ in range(int(SIM_SECONDS / SIM_STEP)):
            pb.stepSimulation(physicsClientId=client)
        for drop, body in zip(drops, bodies):
            z = pb.getBasePositionAndOrientation(body, physicsClientId=client)[0][2]
            want = drop["rest_z"]
            if want is None:
                if z > 0:
                    raise AssertionError(f"{drop['shape']}: the ball stopped at {z:.4f} m, expected it to fall through")
            elif abs(z - want) > DROP_TOLERANCE:
                raise AssertionError(f"{drop['shape']}: the ball rests at {z:.4f} m, expected {want:.4f} m")
    finally:
        pb.disconnect(client)
    return f"{len(drops)} drops"


def bullet_slide(urdf, joint_name, gravity, seconds):
    pb, client, robot, joints = bullet_world(urdf, tuple(float(x) for x in gravity))
    try:
        for _ in range(int(seconds / SIM_STEP)):
            pb.stepSimulation(physicsClientId=client)
        return pb.getJointState(robot, joints[joint_name], physicsClientId=client)[0]
    finally:
        pb.disconnect(client)


def mujoco_model(urdf):
    mj = need("mujoco")
    cwd = os.getcwd()
    os.chdir(urdf.parent)
    try:
        model = mj.MjModel.from_xml_path(urdf.name)
    finally:
        os.chdir(cwd)
    model.opt.timestep = SIM_STEP
    # Parts in these fixtures touch at the joint; contact is not what is being measured.
    model.opt.disableflags |= mj.mjtDisableBit.mjDSBL_CONTACT
    return mj, model


def mujoco(urdf, expected):
    mj, model = mujoco_model(urdf)
    data = mj.MjData(model)
    for _ in range(int(SIM_SECONDS / SIM_STEP)):
        mj.mj_step(model, data)
    if not all(math.isfinite(x) for x in data.qpos):
        raise AssertionError("simulation is not finite")
    notes = ["finite"]
    if "pendulum" in expected:
        want = expected["pendulum"]
        mj, model = mujoco_model(urdf)
        model.opt.gravity[:] = (0, 0, 9.81 * want["gravity_sign_z"])
        data = mj.MjData(model)
        address = model.jnt_qposadr[mj.mj_name2id(model, mj.mjtObj.mjOBJ_JOINT, want["joint"])]
        data.qpos[address] = want["start"]
        samples = []
        for k in range(int(SIM_SECONDS / SIM_STEP)):
            mj.mj_step(model, data)
            samples.append((k * SIM_STEP, float(data.qpos[address])))
        notes.append(pendulum_note(samples, want))
    if "free_slide" in expected:
        notes.append(slide_note(urdf, expected["free_slide"], mujoco_slide))
    return ", ".join(notes)


def mujoco_slide(urdf, joint_name, gravity, seconds):
    mj, model = mujoco_model(urdf)
    model.opt.gravity[:] = gravity
    data = mj.MjData(model)
    for _ in range(int(seconds / SIM_STEP)):
        mj.mj_step(model, data)
    return float(data.qpos[model.jnt_qposadr[mj.mj_name2id(model, mj.mjtObj.mjOBJ_JOINT, joint_name)]])


TOOLS = [("check_urdf", check_urdf), ("yourdfpy", poses), ("pybullet", pybullet), ("mujoco", mujoco)]


def main():
    cli = os.environ.get("SOLIDCRAFT_CLI", str(ROOT / "target/release/solidcraft-cli"))
    names = sys.argv[1:] or sorted(p.stem for p in (HERE / "fixtures").glob("*.json"))
    failed = 0
    for name in names:
        expected = json.loads((HERE / "expected" / f"{name}.json").read_text())
        with tempfile.TemporaryDirectory() as tmp:
            urdf = Path(tmp) / f"{name}.urdf"
            r = subprocess.run([cli, "run", str(HERE / "fixtures" / f"{name}.json"), "--out", str(urdf)], capture_output=True, text=True)
            if r.returncode or not urdf.exists():
                print(f"{name}: export FAILED: {(r.stdout + r.stderr).strip()[-400:]}")
                failed += 1
                continue
            for tool, check in TOOLS:
                try:
                    print(f"{name}: {tool}: ok ({check(urdf, expected)})")
                except Skip as e:
                    print(f"{name}: {tool}: SKIPPED ({e})")
                    failed += 1 if os.environ.get("CI") else 0
                except Exception as e:
                    print(f"{name}: {tool}: FAILED: {e}")
                    failed += 1
    print("all passed" if not failed else f"{failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
