# MJCF check

Exports each recipe in `fixtures/` with `solidcraft-cli` as MJCF (docs/mjcf.md) and loads the result
in MuJoCo, which checks:

1. The joints: their names, kinds, limits and `ref`, the design's current value.
2. The equality constraints a loop gets.
3. Where the bodies' centres of mass are, as the design is and with a joint moved.
4. Motion: a free slide falls as gravity says, a loop closed by a hinge on the joint's own axis
   lets it turn, one closed by a rigid joint locks it, and a second of simulation stays finite.

## MuJoCo is an external tool, not a dependency

It runs as a separate Python package. Nothing is linked, vendored or shipped. Values in
`expected/` are worked out by hand, never taken from SolidCraft's output; each file says how in
`why`.

## Running

```sh
python3 -m venv /tmp/mjcf-check && /tmp/mjcf-check/bin/pip install -r tools/mjcf-check/requirements.txt
cargo build --release -p solidcraft-cli
/tmp/mjcf-check/bin/python tools/mjcf-check/check.py [fixture...]
```

`SOLIDCRAFT_CLI` picks another binary. Without MuJoCo the check is a skip locally and a failure
when `CI` is set.
