# URDF check

Exports each recipe in `fixtures/` with `solidcraft-cli` and loads the result in real URDF tools:

1. `check_urdf` (urdfdom) parses it.
2. `yourdfpy` loads it, and link poses match `expected/`.
3. PyBullet and MuJoCo load it, and a short simulation stays finite.

## These are external tools, not dependencies

They run as separate programs in a Python environment. Nothing is linked, vendored or shipped.
Values in `expected/` are worked out by hand, never taken from SolidCraft's output.

## Running

```sh
python3 -m venv /tmp/urdf-check && /tmp/urdf-check/bin/pip install -r tools/urdf-check/requirements.txt
sudo apt-get install liburdfdom-tools   # check_urdf
cargo build --release -p solidcraft-cli
PYTHON=/tmp/urdf-check/bin/python tools/urdf-check/run.sh [fixture...]
```

`SOLIDCRAFT_CLI` picks another binary. A missing tool is a skip locally and a failure when `CI` is set.

## Chain links are stable

A ball or planar joint exports as a chain with in-between links of 1e-6 kg and 1e-12 kg m². Both
simulators handle them correctly with a 10 mm cube as the child, at a 0.5 ms step:

| Check | Worked out by hand | PyBullet | MuJoCo |
|---|---|---|---|
| Ball joint swinging as a pendulum: period | 0.1831 s | 0.1832 s | 0.1832 s |
| Planar joint sliding under gravity for 0.1 s | 0.04905 m | 0.04920 m | 0.04930 m |

PyBullet must load with `URDF_USE_INERTIA_FROM_FILE`. Without it, PyBullet recomputes inertia from
the collision shapes and the same pendulum swings at 0.2335 s.
