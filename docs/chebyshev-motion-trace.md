# Chebyshev linkage motion-trace demo

The [command recipe](../examples/chebyshev-motion-trace.json) builds a coloured
Chebyshev lambda linkage with four revolute joints, a saved motion study and an
associative trace of the red tip. One crank revolution draws a closed curve with
an approximately straight section. The blue crank, fixed pivot spacing, teal
rocker and amber coupler use lengths 20, 40, 50 and 50 mm; the coupler extends
another 50 mm beyond the rocker pin. These are the classical lambda proportions
1:2:2.5:5 described in [Won et al. (2025)](https://doi.org/10.1002/rcs.70095).
The geometry is constructed directly from circle intersections.

## Build and play

From the repository root, build the CLI and generate the editable design:

```sh
cargo build --release -p solidcraft-cli
target/release/solidcraft-cli run examples/chebyshev-motion-trace.json --out target/chebyshev.solidcraft
```

On Windows the executable has the `.exe` extension. Open `target/chebyshev.solidcraft`
in SolidCraft. Open **Motion Study**, choose **Chebyshev cycle** and play it. The
red tip follows the retained **Chebyshev trajectory** sketch while the blue crank
turns through 360°. Playback reuses the trace rather than sampling it again.

## Change a dimension or the driving range

Open the design's parameters and change the favourite `coupler_length` from
`2.5 * unit_length` to `55 mm`. The amber link lengthens, its joint locations
follow the geometry, and the existing trajectory changes shape automatically.
Changing `unit_length` scales the link lengths together; changing `trace_sweep`
from `360 deg` to `180 deg` retains just the first half of the trajectory. The
saved study remains a full crank revolution so it can also show the untraced half.

For a repeatable CLI comparison, save this as `target/change-chebyshev.json`:

```json
[
  {
    "command": "parameters.change",
    "params": {"name": "coupler_length", "expression": "55 mm"}
  }
]
```

```sh
target/release/solidcraft-cli run target/change-chebyshev.json --in target/chebyshev.solidcraft --out target/chebyshev-length55.solidcraft --timings
```

Use `trace_sweep` and `180 deg` in that command to demonstrate the driving-range
edit instead. Both edits update the same sketch and wire. Undo/Redo restores the
corresponding geometry and trace together. The engine test executes the recipe,
compares all 145 trace points with an independent circle-intersection calculation,
and checks resizing, uniform scaling, the half-turn range and playback reuse.

## Export a demonstration

Save the following as `target/animate-chebyshev.json` and run it on the generated
design. This uses SolidCraft's renderer and motion driver to export a looping GIF:

```json
[
  {
    "command": "motion.export_animation",
    "params": {
      "study": "Chebyshev cycle",
      "path": "target/chebyshev-cycle.gif",
      "samples": 49,
      "fps": 16,
      "width": 900,
      "height": 650,
      "grid": false,
      "axes": false,
      "camera": {
        "target": [25, 40, 5],
        "yaw": 0,
        "pitch": 1.25,
        "distance": 215,
        "fov": 0
      }
    }
  }
]
```

```sh
target/release/solidcraft-cli run target/animate-chebyshev.json --in target/chebyshev.solidcraft
```

For comparable still images, use the same camera before and after the edit:

```sh
target/release/solidcraft-cli snapshot --in target/chebyshev.solidcraft --out target/chebyshev-before.png --width 900 --height 650 --camera '{"target":[25,40,5],"yaw":0,"pitch":1.25,"distance":215,"fov":0}' --no-grid --no-axes
target/release/solidcraft-cli snapshot --in target/chebyshev-length55.solidcraft --out target/chebyshev-after.png --width 900 --height 650 --camera '{"target":[25,40,5],"yaw":0,"pitch":1.25,"distance":215,"fov":0}' --no-grid --no-axes
```

`--timings` reports command execution separately from process startup and file
loading. A dimension edit includes rebuilding the affected solids and trace;
`motion.trace.edit` with a changed sample count measures trace regeneration on
existing geometry. Repeating the same trace definition measures the unchanged
case. Keep the build profile, sample count and hardware with any reported times.
