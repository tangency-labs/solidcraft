# KiCad PCB import

SolidCraft reads KiCad printed-circuit boards (`.kicad_pcb`, or a `.kicad_pro` project, which resolves to its same-stem board). The reader in `crates/io/src/kicad_in.rs` is written from KiCad's documented s-expression format; it brings the substrate in as an exact solid, with its copper, silkscreen and footprint models as meshes in a component named after the file, building the 3D models of its footprints as nested components laid out on the board.

- **Open**: File → Open (`doc.open`) lists KiCad boards (`.kicad_pcb` and `.kicad_pro`) and asks for the import settings before opening one as a new design; `solidcraft board.kicad_pcb`; the MCP `open` tool.
- **Drag and drop** a board on the window: it opens as a new design through the same settings window.

The board's geometry is built once, at import: the design stores the resulting solids and meshes, so it reopens without the board file or its 3D models. Changing the options means opening the board again.

## What comes in

The board substrate always comes in:

- **Outline**: the closed contours on the `Edge.Cuts` layer at board level and inside footprints — lines, rectangles, arcs, circles, filled polygons, curves and Bézier curves (arcs, circles and curves are drawn as polylines within 0.02 mm). An open contour is dropped with a warning.
- **Cutouts**: a closed contour inside the outline is a hole through the board (even-odd nesting, so an island inside a cutout is board again).
- **Drilled holes**: pad drills and via drills, round or slotted, are removed from the substrate. If a drill that touches a cutout defeats the profile, the board is rebuilt without the drills rather than failing.
- **Thickness**: the board's own `general.thickness`, unless the `thickness` option overrides it. The substrate top is Z = 0, so components sit above it and back-side parts below.
- **Solid substrate**: the board is a B-rep solid (`representation: "brep"`, the default): an Import feature of its own, so its faces and edges can be picked and used in later features. A profile the kernel cannot build falls back to the mesh, with a warning; `representation: "mesh"` keeps the triangle mesh deliberately.

The other layers are options:

- **3D models** (`components`, default on): the STEP and VRML models that footprints reference, placed by the footprint's position and rotation on the board's top face (a back-side footprint's model is turned over and stands on the bottom face). A model keeps the colours of its own materials (a package's body and its pins come in with the colours the model gives them), so it does not arrive as one grey lump. A model is authored for the front; the model's own `offset`, `scale` and `rotate` (X, then Y, then Z, with KiCad's sign and its legacy `at` token's inches) are applied, and a mirroring scale is refused. STEP models are tessellated at 0.1 mm; VRML 2.0 models are read directly (see below). A model referenced by several footprints is read once and placed by each, and every footprint that places a model becomes a component (named after its reference, e.g. `R1`) under the import's component. A model's path is found the way KiCad finds it: `KIPRJMOD` is the board's folder, and a variable is looked up in the environment first, then in KiCad's own path table (`environment.vars` in `kicad_common.json`, the newest configured version — Preferences → Configure Paths, which on Linux is the only place KiCad keeps them), and any `KICAD<n>_3DMODEL_DIR` or `KISYS3DMOD` finally lands in the 3D library installed here, whatever version the board was drawn with. The format follows the file that is read: KiCad 10's 3D packages ship STEP only, so a footprint that names `….wrl` gets the `….step` of the same name beside it (as KiCad's own 3D viewer does), and a model that is nowhere on this machine is a warning.
- **Copper** (`copper`, default `"none"`): `"overlay"` draws the tracks and track arcs, vias, filled zones, board-level copper graphics, copper text and pads as a 0.001 mm copper film on the board (an overlay: it looks like copper without adding meaningful volume); `"mesh"` models them with their 0.035 mm thickness as triangle meshes. Pads are always included whenever copper is enabled. The solder mask is not modelled as a solid — it is thin and covers the whole board — but its look is: copper under the mask is drawn in the board's colour, a shade warmed by copper, and only the mask's openings stay bare metal — gold plating on a finished pad, not copper: a pad whose layers include `F.Mask`/`B.Mask` (or `*.Mask`) comes in as its own body, `Copper F.Cu pads`/`Copper B.Cu pads`, in the gold of its plating, while traces, zones, vias, copper graphics and text are `Copper F.Cu`/`Copper B.Cu` in the copper under the mask. A copper zone that spans both sides (KiCad 9's `(layers "F.Cu" "B.Cu")`) brings each of its filled polygons in on the layer it was filled on. A routed trace is one item: KiCad stores it as one `segment` or `arc` per leg, and legs that meet end to end on the same layer and width are joined into one continuous trace (one outline, one solid, mitred corners) — a junction with more than one continuation, a different width or another layer starts a new trace. Drilled holes stay open in the copper: a via's copper is a ring around its drill, a through-hole pad's copper has its drill (round or oval) cut through it, and a trace stops at every drilled hole it runs into or through (a trace crossing a hole comes back as the copper on either side), cut half the trace's width clear of the drill so not even a corner reaches into the hole. Any hole a pad or a copper pour covers is open in it too, whatever the pad's own data names — a mounting hole under a ground pour stays a hole. A trace also stops where a pad's copper begins — the pad carries on from its edge — so the masked trace and the open pad never share a face in different colours. (The bare pad also stands 0.005 mm proud of the masked copper, since two coplanar faces in different colours would otherwise fight for the pixel at every joint.)
- **Silkscreen** (`silkscreen`, default on): the lines, filled shapes and text of the front and back silk layers, 0.01 mm above (or below) the substrate, in a white colour. A closed shape (a rectangle, a circle, a polygon, a filled outline) is drawn all the way round; a stroke is one continuous item with mitred corners, like copper.
- **Text**: board and footprint text on the silk and copper layers becomes filled glyph outlines of SolidCraft's built-in font. KiCad's `size` gives the glyph height and width, bold and italic are set, left/right/top/bottom justification places the anchor, hidden text is skipped, and back-side text marked `mirror` reads from the back of the board. Text boxes, dimensions and tables are not modelled yet, which is reported as a warning.
- **Board colour** (`color`): the substrate colour, `"#rrggbb"` (or `"#rgb"`) or `[r, g, b]`. The default is solder-mask green.

```json
{"command": "doc.open", "params": {
  "path": "keyboard.kicad_pro",
  "components": true,
  "copper": "mesh",
  "silkscreen": true,
  "color": "#103c2a"
}}
```

## Options

| option | default | meaning |
| --- | --- | --- |
| `path` | — | `.kicad_pcb` board or `.kicad_pro` project (any case) |
| `thickness` | the board's | substrate thickness in millimetres (finite, more than 0 and at most 1000) |
| `components` | `true` | load the footprint 3D models |
| `copper` | `"none"` | `"none"`, `"overlay"` (a 0.001 mm copper film on the board) or `"mesh"` (modelled with thickness; `true` also means mesh; `false` also means none) |
| `silkscreen` | `true` | build the front and back silkscreen |
| `color` | solder-mask green | board substrate colour, `"#rrggbb"`/`"#rgb"` or `[r, g, b]` |
| `representation` | `brep` | `brep` (the board as a solid) or `mesh` (triangle meshes only) |

`doc.open` validates every option before reading the board, so a bad colour or board representation fails with a message and leaves the current design alone. The command's reply reports the feature, the component (`component`, `component_name`, `occurrence`), the body names (`board_solid` is `true` when the substrate came in as a solid) and the warnings the reader collected.

## In the window

File → Open… and choose a board (the picker lists KiCad PCB files), or drop one on the window: SolidCraft opens **Open KiCad PCB** first — **Include components**, the copper as **None**, **Overlay on the board** or **Modelled**, **Include silkscreen**, the board colour and thickness (starting at the board's own) — and **Open** brings the board in as a new design with those options; **Cancel** leaves the current design alone.

`doc.open` takes the same options, so a script, the CLI or the MCP tool opens a board without the window.

## VRML models

`.wrl` footprint models are read by `crates/io/src/vrml_in.rs`: VRML 2.0 with `Transform` (translation, rotation, scale), `Group`, `Switch` and the other grouping nodes, `Shape`/`Appearance` (one `diffuseColor` colours the model; several are reported and the first is used), `IndexedFaceSet`, `IndexedTriangleSet`, and the `Box`, `Sphere`, `Cylinder` and `Cone` primitives, with `DEF`/`USE` sharing. `PROTO`, `EXTERNPROTO` and `ROUTE` are skipped, VRML 1.0 is refused, and a file without geometry is refused. Textures, normal interpolation and lights are not used.

## Limits

The board file is untrusted input: its size (128 MB), token count, nesting, footprints, models, pads, tracks, vias, zones, graphics, vertices and triangles are capped, numbers must be finite and bounded, and the kernel calls that build the bodies are guarded. Passing a limit becomes a warning and skips the remaining items; a board that cannot be built fails the command.

Not modelled yet:

- **Text boxes, dimensions and tables**: their text is not drawn; a board with them gets one warning.
- **Inner copper layers**: tracks and zones on layers other than `F.Cu` are drawn with the back copper, and pads that exist only on inner layers are skipped. There is no stackup, layer thickness or dielectric model.
- **Copper holes**: drills are cut from the substrate only.
- **Other footprint layers**: only `Edge.Cuts` graphics inside a footprint join the outline; courtyard, fab and other layers are ignored.
- **Nets, ratsnest, DRC, drawings and panelisation** (the board's own panel array is not built).
