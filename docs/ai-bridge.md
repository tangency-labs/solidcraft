# AI Bridge (FreeCAD AIBridge tools)

SolidCraft can serve the tool set of FreeCAD's **AI Bridge** add-on: the same 107 tool names,
descriptions and JSON Schemas. An AI client written for FreeCAD's bridge drives SolidCraft
without any changes.

The names are a second vocabulary, not a second server. They are served by SolidCraft's own MCP
server (docs/mcp.md), so they inherit its transport, its token, its path allowances and its
connection to the running app; there is no separate port and no second thing to secure.

## Turning it on

It is **off** by default: with it off, `tools/list` names only SolidCraft's own tools and a
FreeCAD name is an unknown tool. Either of these turns it on:

```
solidcraft-cli mcp --freecad
SOLIDCRAFT_MCP_FREECAD=1 solidcraft-cli mcp
```

Both work with `--connect` (driving the running app, where the view, render and preference tools
need a window) and headless. A FreeCAD name never shadows one of SolidCraft's own, which a test
holds to.

```json
{"mcpServers": {"solidcraft": {"command": "solidcraft-cli", "args": ["mcp", "--freecad"]}}}
```

Results come back as MCP text content holding the tool's JSON, and `isError` is set on failure,
with FreeCAD's error code in front of the message. Arguments are checked against each tool's
schema the way Python's `tool(**arguments)` would be, so a misspelled keyword is refused rather
than ignored.

## Where it differs from FreeCAD's bridge

- **The whole browser tree expands or collapses.** FreeCAD expands one object; SolidCraft has no
  command for a single row, so `tree_expand` acts on the tree. The object named still has to
  exist.
- **`console_print` does not write to the status bar.** The line is kept and `console_tail`
  reads it back, along with the app's own messages.
- **A design cannot be renamed in place.** `doc_new` names the design as it creates it
  (`file.new`); nothing renames one afterwards.
- **State lives for the life of the server process** -- placements, spreadsheets and console
  lines. One agent talks to one server, so one set of them.
## How FreeCAD concepts map

| FreeCAD | SolidCraft |
|---|---|
| Document | Design tab (`doc` argument = its name) |
| `Part::Feature`, primitives, booleans | Bodies (and the timeline features that make them) |
| `FaceN` / `EdgeN` / `VertexN` | `model.faces` / `model.edges` index N-1; a vertex is an edge end (vertices are numbered as edge ends in edge order) |
| PartDesign Body, `App::Part`, Assembly, Compound | Component |
| Sketch geometry index | Position in the sketch's curve list; point positions 1/2/3 = start/end/centre |
| Placement | Tracked per body for the session; moves are Move features (angles in degrees) |
| Spreadsheet cell | Design parameter `<sheet>_<cell>` (or its alias); `=A1*2` refers to other cells |
| Python exec | SolidCraft command script: one `command {json}` per line, or JSON `[{command, params}]` |
| Python eval | Expression over the design's parameters, with units |
| Workbenches | DesignWorkspace, RenderWorkspace |
| GUI commands | SolidCraft command ids |
| Preferences | Group `User parameter:BaseApp/Preferences/SolidCraft` is SolidCraft's settings. Other groups are kept for the session. |

### Booleans

Booleans work on copies. The result is a new body, and the inputs are hidden, as FreeCAD shows
them.

### Mesh tools

| Tool | What it does |
|---|---|
| `mesh_to_shape`, `mesh_rebuild_cad` | Run Convert Mesh. |
| `mesh_loft_solid`, `mesh_to_solid_multi_axis` | Slice the mesh into convex sections, sketch each one, and loft them. |

## Limits

- `part_create` makes these types:
  - `Part::Box`, `Part::Cylinder`, `Part::Sphere`, `Part::Cone`
  - `Part::Torus`, `Part::Plane`, `Part::Prism`, `Part::Ellipsoid`
- Sketch constraints cover these types:
  - Coincident, PointOnObject, Horizontal, Vertical
  - Parallel, Perpendicular, Tangent, Equal
  - Distance, DistanceX, DistanceY, Radius, Diameter, Angle
  - Fixed/Block, Symmetric
- `pd_revolution` takes `V_Axis` or `H_Axis`.
- `view_screenshot` writes PNG only.
- `display_refine`: the anti-aliasing setting applies after a restart.

## Source

| File | Contents |
|---|---|
| `crates/mcp/src/freecad/` | The tool definitions (`tools.json`) and their implementations |
| `crates/mcp/src/tools.rs` | Lists and routes them when the option is on |
| `crates/mcp/src/options.rs` | `--freecad` and `SOLIDCRAFT_MCP_FREECAD` |

Every tool goes out through the MCP server's `Backend`, so it runs SolidCraft commands, and the
ones that need a window run control-channel methods, which a server with no app refuses saying
so.
