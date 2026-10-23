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

A [macro](macros.md) is a Rhai script the MACRO tab records and plays. Command scripts stay JSON.
