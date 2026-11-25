# KiCad PCB import

SolidCraft reads KiCad printed-circuit boards (`.kicad_pcb`, or a `.kicad_pro` project, which resolves to its same-stem board). The reader in `crates/io/src/kicad_in.rs` is written from KiCad's documented s-expression format; it brings the substrate in as an exact solid, with its copper, silkscreen and footprint models as meshes in a component named after the file, building the 3D models of its footprints as nested components laid out on the board.

- **Open**: File → Open (`doc.open`) lists KiCad boards (`.kicad_pcb` and `.kicad_pro`) and asks for the import settings before opening one as a new design; `solidcraft board.kicad_pcb`; the MCP `open` tool, which opens a board with the default settings.
- **Drag and drop** a board on the window: it opens as a new design through the same import options window.

The board's geometry is built once, at import: the design stores the resulting solids and meshes, so it reopens without the board file or its 3D models. Changing the options means opening the board again.

## What comes in

The board substrate always comes in:

- **Outline**: the closed contours on the `Edge.Cuts` layer at board level and inside footprints — lines, rectangles, arcs, circles, filled polygons, curves and Bézier curves (arcs, circles and curves are drawn as polylines within 0.02 mm). Ends of outline items up to 0.01 mm apart are joined as one corner, as KiCad's STEP export joins them by default, so an outline drawn with gaps of a few nanometres is closed. An open contour is dropped with a warning.
- **Cutouts**: a closed contour inside the outline is a hole through the board (even-odd nesting, so an island inside a cutout is board again).
- **Drilled holes**: pad drills and via drills, round or slotted, are removed from the substrate. If a drill that touches a cutout defeats the profile, the board is rebuilt without the drills rather than failing.
- **Placement**: the board's lower-left corner (the smallest X and Y of its outline) comes in at X = Y = 0. A board drawn anywhere on KiCad's project sheet lands at the design's origin with its corner there, not at the coordinates the file stores; the import's component carries that one shift, so its contents keep the board's own coordinates inside it.
- **Thickness**: the board's own `general.thickness`, unless the `thickness` option overrides it. The board's substrate spans Z = 0 (the back face) to Z = thickness (the front face), so front-side components stand on the front face at Z = thickness and back-side parts hang below Z = 0, as in KiCad's own STEP export.
- **Solid substrate**: the board is a B-rep solid (`representation: "brep"`, the default): an Import feature of its own, so its faces and edges can be picked and used in later features. A profile the kernel cannot build falls back to the mesh, with a warning; `representation: "mesh"` keeps the triangle mesh deliberately.

The other layers are options:

- **3D models** (`components`, default on): the STEP and VRML models that footprints reference, placed by the footprint's position and rotation on the board's top face (a back-side footprint's model is turned over and stands on the bottom face). A model keeps the colours of its own materials (a package's body and its pins come in with the colours the model gives them), so it does not arrive as one grey lump. A model is authored for the front; the model's own `offset`, `scale` and `rotate` (X, then Y, then Z, with KiCad's sign and its legacy `at` token's inches) are applied, a back-side model's offset is turned over with the model (so its Z offset still points away from the board), and a mirroring scale is refused. STEP models are tessellated at 0.1 mm; VRML 2.0 models are read directly (see below). A model referenced by several footprints is read once and placed by each: each model becomes a component under the import's component, named after its file (e.g. `R_0603_1608Metric`), and each footprint that places it an occurrence of that component named after the footprint's reference (e.g. `R33`; a reference that two footprints share gets ` (2)` on the second, and an empty or all-digit one keeps the usual `R_0603_1608Metric:3`). A footprint that names both forms of one model (`….wrl` and `….step`) gets one model, not two, and one footprint's missing model costs only that model: the others of the same footprint still come in. A model's path is found the way KiCad finds it: `KIPRJMOD` is the board's folder, and a variable is looked up in the environment first, then in KiCad's own path table (`environment.vars` in `kicad_common.json`, the newest configured version — Preferences → Configure Paths, which on Linux is the only place KiCad keeps them), and any `KICAD<n>_3DMODEL_DIR` or `KISYS3DMOD` finally lands in the 3D library installed here, whatever version the board was drawn with. The format follows the file that is read: KiCad 10's 3D packages ship STEP only, so a footprint that names `….wrl` gets the `….step` of the same name beside it (as KiCad's own 3D viewer does), and a model that is nowhere on this machine is a warning that names the folder searched (`JST_SH.step: not found in /libs/jst`; a folder under the board's own is named from the board, as `the board's folder, models`). Identical warnings are said once with a count (`(x7)`), and an unset variable is one line for all the models that name it. A model that only partly comes in says so in one short line — how many of its solids stayed out and why, such as `DIN41612_C_2x16_Female_Vertical_THT.step: 1 of 33 solids not imported (this wire is not simple)` — and a model that gives nothing at all says what kept it out, instead of a bare "failed to import".
- **Copper** (`copper`, default `"overlay"`): `"overlay"` draws the tracks and track arcs, vias, filled zones, board-level copper graphics, copper text and pads as flat surfaces 0.001 mm above or below the board; `"mesh"` models them with their 0.035 mm thickness as closed solids — the layer's items are united, so a via's ring inside a pour or a trace running into its pad is one surface rather than shells meeting along an edge. A layer whose copper touches itself only at a point comes back as more than one body, named `Copper F.Cu`, `Copper F.Cu (2)` and so on, because a body welds its points together and would not be manifold across such a point; where one body's own surface passes the same point twice, the two sides are moved apart by 1 nm — one unit of a KiCad board file — so every body is a manifold. Pads are always included whenever copper is enabled. The solder mask is not modelled as a solid — it is thin and covers the whole board — but its look is: copper under the mask is drawn in the board's colour, a shade warmed by copper, and only the mask's openings stay bare metal — gold plating on a finished pad, not copper: a pad whose layers include `F.Mask`/`B.Mask` (or `*.Mask`) comes in as its own body, `Copper F.Cu pads`/`Copper B.Cu pads`, in the gold of its plating, while traces, zones, vias, copper graphics and text are `Copper F.Cu`/`Copper B.Cu` in the copper under the mask. A copper zone that spans both sides (KiCad 9's `(layers "F.Cu" "B.Cu")`) brings each of its filled polygons in on the layer it was filled on. A routed trace is one item: KiCad stores it as one `segment` or `arc` per leg, and legs that meet end to end on the same layer and width are joined into one continuous trace (one outline, one solid, mitred corners) — a junction with more than one continuation, a different width or another layer starts a new trace. Drilled holes stay open in the copper: a via's copper is a ring around its drill, a through-hole pad's copper has its drill (round or oval) cut through it, and a trace stops at every drilled hole it runs into or through (a trace crossing a hole comes back as the copper on either side), cut half the trace's width clear of the drill so not even a corner reaches into the hole. Any hole a pad or a copper pour covers is open in it too, whatever the pad's own data names — a mounting hole under a ground pour stays a hole. A trace also stops where a pad's copper begins — the pad carries on from its edge — so the masked trace and the open pad never share a face in different colours. (The bare pad also stands 0.005 mm proud of the masked copper, since two coplanar faces in different colours would otherwise fight for the pixel at every joint.)
- **Silkscreen** (`silkscreen`, default on): the lines, filled shapes and text of the front and back silk layers, 0.01 mm above (or below) the substrate, in a white colour. A closed shape (a rectangle, a circle, a polygon, a filled outline) is drawn all the way round; a stroke is one continuous item with mitred corners, like copper.
- **Text**: board and footprint text on the silk and copper layers becomes filled glyph outlines of SolidCraft's built-in font. KiCad's `size` gives the glyph height and width, bold and italic are set, left/right/top/bottom justification places the anchor, a footprint text's rotation is the one the file stores (on the board, not added to the footprint's) and, unless the text is unlocked, is drawn upright as KiCad draws it (a rotation past a quarter turn is drawn half a turn round, with the same anchor and justification), hidden text is skipped, and back-side text marked `mirror` reads from the back of the board. Text boxes, dimensions and tables are not modelled yet, which is reported as a warning.
- **Text variables**: a text's `${NAME}` is put in as KiCad draws it. The title block's `${TITLE}`, `${ISSUE_DATE}`, `${REVISION}`, `${COMPANY}` and `${COMMENT1}`…`${COMMENT9}` (empty when the board does not set them) and `${LAYER}` (the layer's name as the board's layer table gives it, `F.Silkscreen` by default) work in any text; a footprint's text also reads `${REFERENCE}`, `${VALUE}`, any of its fields by exact name (hidden ones too), `${FOOTPRINT_NAME}` and `${FOOTPRINT_LIBRARY}`; `${R7:VALUE}` reads footprint R7's field (`<Unknown reference: R7>` when there is none), and so does the form KiCad saves, with the footprint's id in place of `R7`. A field's own variables are put in where the field is read. Names are case-sensitive, a name is expanded first (`${${REFERENCE}}`), and an unknown name stays as written. A text that comes out empty is not drawn. Not read yet: the project's own variables (`text_variables` in the `.kicad_pro`), dates, file and sheet names.
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
| `copper` | `"overlay"` | `"overlay"` (flat surfaces 0.001 mm above or below the board), `"none"` or `"mesh"` (modelled with thickness; `true` also means mesh; `false` also means none) |
| `silkscreen` | `true` | build the front and back silkscreen |
| `color` | solder-mask green | board substrate colour, `"#rrggbb"`/`"#rgb"` or `[r, g, b]` |
| `representation` | `brep` | `brep` (the board as a solid) or `mesh` (triangle meshes only) |

`doc.open` validates every option before reading the board, so a bad colour or board representation fails with a message and leaves the current design alone. The command's reply reports the feature, the component (`component`, `component_name`, `occurrence`), the body names (`board_solid` is `true` when the substrate came in as a solid) and the warnings the reader collected.

## In the window

File → Open… and choose a board (the picker lists KiCad PCB files), or drop one on the window: SolidCraft opens **Open KiCad PCB** first, the import options window every format with options shares — **Include components**, the copper as **Overlay on the board** (the default), **None** or **Modelled**, **Include silkscreen**, the **Board colour** and **Board thickness** (starting at the board's own), and the **Board** as a **Solid** (the default) or a **Mesh** — and **Open** (or Enter) brings the board in as a new design with those options; **Cancel** (or Esc) leaves the current design alone. Only the options changed in the window are sent, so a board opened without changes comes in exactly as `doc.open` with its path alone brings it. A board always asks, even with Preferences › General › Dialogs "Ask for a file's import options" off, which only skips the other formats' questions; a board chosen with Insert Design opens as a design of its own.

`doc.open` takes the same options, so a script or the CLI opens a board without the window. The MCP `open` tool takes only the path, so it opens a board with the default settings and does not forward the KiCad import options.

## VRML models

`.wrl` footprint models are read by `crates/io/src/vrml_in.rs`: VRML 2.0 with `Transform` (translation, rotation, scale), `Group`, `Switch` and the other grouping nodes, `Shape`/`Appearance` (one `diffuseColor` colours the model; several are reported and the first is used), `IndexedFaceSet`, `IndexedTriangleSet`, and the `Box`, `Sphere`, `Cylinder` and `Cone` primitives, with `DEF`/`USE` sharing. `PROTO`, `EXTERNPROTO` and `ROUTE` are skipped, VRML 1.0 is refused, and a file without geometry is refused. Textures, normal interpolation and lights are not used.

## Limits

The board file is untrusted input: its size (128 MB), token count, nesting, footprints, models, pads, tracks, vias, zones, graphics, vertices and triangles are capped, numbers must be finite and bounded, and the kernel calls that build the bodies are guarded. Passing a limit becomes a warning and skips the remaining items; a board that cannot be built fails the command.

Not modelled yet:

- **Text boxes, dimensions and tables**: their text is not drawn; a board with them gets one warning.
- **Inner copper layers**: tracks and zones on layers other than `F.Cu` are drawn with the back copper, and pads that exist only on inner layers are skipped. There is no stackup, layer thickness or dielectric model.
- **Other footprint layers**: only `Edge.Cuts` graphics inside a footprint join the outline; courtyard, fab and other layers are ignored.
- **Nets, ratsnest, DRC, drawings and panelisation** (the board's own panel array is not built).
