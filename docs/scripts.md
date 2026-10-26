# Scripts


A script is a JSON list of commands, the same ones the UI runs:

```json
{"commands": [
  {"command": "sketch.create", "params": {"plane": "XY"}},
  {"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [40, 30]}},
  {"command": "solid.extrude", "params": {"distance": 20}},
  {"command": "solid.fillet", "params": {"edges": [[0, 0, 10]], "radius": 3}}
]}
```

Run one with `solidcraft-cli run script.json --out part.step`; see [building](building.md) and the [command list](features.md).

## Moving bodies

`solid.move` rotates about `origin` (default `[0, 0, 0]`) and then applies `translate`.
The inputs are in world coordinates, including for bodies in placed components. `axis`
defaults to Z; `angle` accepts an angle expression (numeric values are degrees). Origin coordinates accept numbers in millimetres or length expressions. Translation keeps
its existing design-unit convention; explicit units such as `"10 mm"` are unambiguous. For example,
this turns a box about its centre without moving it away from that centre:

```json
{"commands": [
  {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "body_name": "B"}},
  {"command": "solid.move", "params": {"bodies": ["B"], "origin": [105, 5, 0], "axis": [0, 0, 1], "angle": "90 deg"}}
]}
```

Alternatively, pass `matrix`: a row-major 3×4 or 4×4 rigid transform, either flat or
as rows. Its last column is translation in millimetres. For example,
`[0,-1,0,30, 1,0,0,0, 0,0,1,0]` turns 90° about world Z and then moves 30 mm along X.
Do not combine `matrix` with `translate`, `origin`, `axis` or `angle`. Scale, shear,
mirror and projective matrices are refused. Rounding is accepted within `tolerance`
(default `1e-9`, positive and at most `1e-3`) and corrected to a rigid rotation.

Either form accepts `copy: true` to retain the originals and transform new bodies.
A matrix is saved as a Move feature's translation and axis/angle; an explicit origin
is saved separately so its length expressions remain editable and parametric.
