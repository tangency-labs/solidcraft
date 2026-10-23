# Macros

A **macro** is a [Rhai](https://rhai.rs) script the engine runs. Record captures the commands you
use as Rhai; Save writes `.rhai`. You can add `if` and loops. Play is one undo step; a failure
rolls back.

Command **scripts** (`solidcraft-cli run file.json`) stay JSON. That is not the MACRO tab.

## Record / play

The toolbar's **MACRO** tab has Record, Pause, Resume and Stop. Pause keeps the recording and
skips the commands you run until Resume. A chip at the top of the window stays up for the whole
recording.

Save writes a `.rhai` file. Open loads one. Play runs it. A macro can ask for values first;
Play shows one row per parameter, prefilled with its default:

```text
// @param name Label [kind] [choice, options] | default
```

| kind | Play shows | Rhai sees | example |
|---|---|---|---|
| `text` (no kind) | a text field | text | `// @param label Part name | Plate` |
| `int` | a whole-number field | an integer | `// @param nx Count in X int | 1` |
| `number` | a number field | a number | `// @param gear_module Module number | 2` |
| `length` | a length field (design units) | a number or an expression | `// @param bore Bore length | 5 mm` |
| `angle` | an angle field (degrees) | a number or an expression | `// @param tilt Tilt angle | 90 deg` |
| `bool` | a toggle | a bool | `// @param both Chamfer both ends bool | yes` |
| `choice` | a dropdown | one of the options | `// @param mat Material choice [steel, brass] | steel` |

A `length` or `angle` default may be an expression (`5 mm`, `width + 2`); a plain number is the
design's own unit, so a macro can compute with it. The engine checks every value again when
`macro.play` runs, so the CLI, the control channel and MCP get the same rules without the window.

```rhai
// @param min_dia Minimum diameter length | 5 mm
let holes = run("inspect.find_holes", #{ min_diameter: min_dia });
run("solid.chamfer", #{ edges: holes.edges, distance: "1 mm" });
```

`run(command, params)` is `engine.execute`. `list_dir(path)` and `write_file(path, text)` are for
batch files (not in the browser). `file_stem(path)` is the name without extension.

`inspect.find_holes` finds cylindrical holes. `max_diameter` / `min_diameter` filter by size.
Empty `edges` on Chamfer is a no-op.

## Examples

Examples live in [examples/macros](../examples/macros). Open the `.rhai` file, then Play. Play
`setup_*` first when the next macro needs that design.

| Example | Play on | What it shows |
|---|---|---|
| `setup_fixtureplate.rhai` | empty | C-shaped sketch `clearance_hole` (100×60 mm, 50×30 mm notch from the right) |
| `example_fixtureplate.rhai` | after setup | 500×350×8 mm tray, six Ø8 holes chamfered 1 mm both ends, C cut patterned `nx`×`ny` at `spacing_x`/`spacing_y`, centered on the tray |
| `chamfer_holes.rhai` | a part with holes | Query holes, chamfer by size |
| `batch_export_stl.rhai` | empty | Folder of STEP → STL |
| `gear_generator.rhai` | empty | full-depth involute spur gear, typed params (Machinery's Handbook) |

The browser plays Rhai in the same engine. `list_dir` / `write_file` are desktop only.
