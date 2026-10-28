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

## File paths in a script

A relative file path inside a script file is beside the script, wherever the program is started:
`solidcraft-cli run scripts/r.json` reads `{"command": "file.insert_step", "params": {"path": "cap.step"}}`
from `scripts/cap.step`, and a `file.export` or `file.save_as` with a relative `path` writes there
too. So a script and the files it uses can be kept together, in a repository for example, and run
from any folder. This holds for `run`, `eval`, `snapshot`, `render` and `mcp --in`, and for every
command that reads or writes a file: `doc.open`, `file.insert_step`, `file.insert_mesh`,
`sketch.insert_dxf`, `sketch.insert_svg`, `sketch.drawing_bounds`, `canvas.insert`, `canvas.decal`,
`component.insert_design`, `component.relink`, `component.replace_with_link`, `component.extract`,
`parameters.import`, `parameters.export`, `library.inspect`, `library.apply`, `library.export`,
`file.save`, `file.save_as`, `file.export`, `file.save_mesh`, `file.export_view_dxf`,
`sketch.export_dxf`, `sheet.export_dxf`, `timeline.export_script`, `assembly.bom`,
`motion.export_animation`, `render.hdr.inspect`, `render.settings.capture_hdr`, `render.export`,
`doc.recovery_list` and `doc.recover` (`dir`), and a `sketch.text` or `sketch.edit_text` font given as
a file (an installed font's family name is not a path, and `~` and `%VAR%` expand as before).
File paths nested in a render setup (an environment's HDR, texture maps) still resolve against the
working folder.

Absolute paths are used as written. Paths given on the command line (`--in`, `--out`, `--save`,
`--json-out`, `snapshot --out`) are relative to the working folder, as usual for a command-line
program. Commands sent one at a time (`exec`, the MCP tools, the app's control channel) have no
script file, so their relative paths resolve against the working folder of the program that runs
them; give those absolute paths.
