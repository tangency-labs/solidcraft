# Electronics architecture

Part of the [electronics plan](README.md) and [RFC 0008](../rfcs/0008-electronics.md).

## 1. Where the code goes

Electronics is a domain model plus editors, not a geometry kernel. It uses `geom` for 2D shapes and
the kernel only to build the board body for the mechanical assembly.

| Layer | Crate | Holds |
|---|---|---|
| L1 | `eda` (new) | Parts, schematic, netlist, board, stackup, rules, ERC, DRC, connectivity, BOM, variants |
| L1 | `spice` (new) | Circuit solver: MNA, devices, DC/AC/transient/noise, results |
| L2 | `doc` | Electronics section of the design, embedded part snapshots, derived board body, invalidation |
| L3 | `io` | KiCad/Eagle/IPC-2581/Gerber/Excellon/SPICE netlist read and write, loss reports |
| L4 | `engine` | `library.*`, `schematic.*`, `board.*`, `fab.*`, `sim.*` commands, jobs, caches |
| L5 | `ui-egui` | Electronics workspace, editors, panels, cross-probe, progress |
| L5 | `render`, `gpu` | Board and component rendering, materials, scenes |
| shared | `viewport`, `viewport-egui` | 2D/3D drawing of board layers, selection, overlays |
| reuse | `fea`, `flow` | Board thermal, airflow; PI/SI field solves |

`eda` and `spice` are pure: no document, UI, filesystem, threads or GPU. They are tested on plain
data. `doc` owns persistence and derived geometry; `engine` owns orchestration and commands.

Dependency direction:

```
ui-egui ──► engine ──► doc ──► eda ──► geom
   │           │         │       ▲
   │           │         ├──► spice
   │           │         ├──► kernel (board body only)
   │           │         └──► fea / flow (thermal, PI)
   ├──► io ──► eda, doc, spice
   └──► render / gpu (board and components in the scene)
```

## 2. One document, two editors

Electronics lives in the same `.sc` container as the mechanical design. The container already stores
content-addressed blobs with SHA-256 and enforces per-part and total size caps, which is exactly what
embedded part models need.

```text
document
  timeline              mechanical features (unchanged)
  occurrences           assembly (board becomes a component)
  electronics
    library
      parts[]           exact revisions used by this design, with embedded payloads
      packs[]           source identity and provenance, not bytes
    schematic
      sheets[]          hierarchy, title blocks
      symbols[]         part instances (stable id, refdes, unit, position, fields)
      wires[], buses[], junctions[], labels[], ports[], no_connects[]
      net_classes[]
    board
      stackup           layers, dielectric, copper weight, impedance rules
      outline, cutouts, holes
      footprints[]      placement, side, rotation, pads
      tracks[], vias[], zones[], keepouts[]
      rules             clearance/width/annular/length/mask matrices
      dimensions, text, teardrops, test points
    variants[]          fitted / DNP / alternate
    simulations[]       analyses and their inputs
    fab_profiles[]      per-fab rules and column mappings
```

Blobs:

| Payload | Storage |
|---|---|
| Symbol, footprint, pad stack, rules | JSON inside the part snapshot |
| 3D component model | existing STEP blob (`blobs/<hash>.step`), converted to a display mesh in the render cache |
| Datasheet, vendor image | link by default; no bundled third-party bytes |
| Zone fill | derived; optional cache blob with an inputs hash, never trusted as source |
| Board render meshes | render cache only, never in the design |

## 3. Identity and ownership

- **Stable ids, not names.** Parts, nets, sheets, pins and pads get UUIDs at creation. Reference
  designators and net names are labels that can change without breaking connectivity.
- **The schematic owns connectivity.** The board owns physical geometry. A part instance is one
  object with a schematic side and a board side, so deleting or renaming it updates both.
- **The mechanical side owns placement of the board component in the assembly**, not the board's
  internal layout.
- **The board owns outline, holes and connector positions** that the enclosure references. Enclosure
  features can reference them by name, the same way they reference any other body.
- **Parameters are shared.** Board width, thickness, hole spacing and connector heights can be
  expressions in the same parameter graph as the enclosure.

## 4. Derived data

These are computed and never treated as source:

| Derived | From | Invalidation |
|---|---|---|
| Netlist | Schematic | Schematic hash |
| Connectivity | Board copper, pads, vias, zones | Board hash |
| Ratsnest | Netlist + placement | Both |
| BOM | Parts + variants | Parts, variants |
| CPL | Placements + variants | Board, variants |
| Zone fill | Zones + rules | Board, rules, zone params |
| Board body | Outline, cutouts, holes, thickness | Board geometry |
| Component envelopes | Footprint courtyard, package height | Part revision |
| ERC/DRC results | Rules + design | Inputs hash |
| Fab package | Board + profiles | Inputs hash |
| Simulation results | Netlist/board + models + analysis | Inputs hash |

A mismatch between the schematic hash and the board's recorded schematic hash produces the
**schematic-to-board differences** report. Applying it is explicit and undoable.

## 5. Part model and library reuse

The repo already proposes a portable, versioned pack envelope for materials, appearances and
component families (`docs/book/src/dev/libraries.md`, #785–#787): immutable revisions, stable
namespaced ids, provenance, licence, content digests and project scope. Extend it with an electronics
part entry kind instead of inventing a second library system.

```text
Part {
  id, revision, kind: "electronic-part",
  manufacturer, mpn, description, category, package,
  pins: [{ number, name, electrical_type, alternate_functions }],
  symbol: { graphics, pins, units, body },
  footprint: { pads, courtyard, silkscreen, assembly, keepouts, 3d_transform },
  model3d: { blob hash, unit, transform, fidelity },
  spice: { model text or subcircuit, pin mapping },
  ibis: optional,
  datasheet, supply: optional,
  provenance, licence
}
```

Rules:

- A revision is immutable. Editing creates a revision; the design keeps the old one until updated.
- The design embeds the exact revision and payloads it uses, so it opens offline and on another
  machine.
- Updating a part is an explicit command with a diff of pins, pads, models and nets affected.
- Bundled demo parts are original or openly licensed. User-installed KiCad libraries are imported
  from the user's disk, never redistributed by the app.

## 6. Schematic model

- Hierarchical sheets with ports, buses and bus members.
- Symbols may have multiple units and alternate pin functions; a part instance names its unit.
- Wires and buses are 2D polylines with stable ids; junctions and labels are separate objects.
- Net identity comes from labelled nets, power symbols, hierarchical ports and physical connections;
  the resolver is deterministic and reports ambiguity.
- ERC is a rule engine over pins, nets, sheets and labels, with severities and locations.
- Annotation is deterministic and idempotent; re-annotation preserves part ids.
- Variants select fitted/DNP/alternate per part and drive the BOM, CPL and simulation netlist.

## 7. Board model

- Stackup: ordered copper and dielectric layers with thickness, material, copper weight, impedance
  targets and mask/silk layers.
- Geometry: outline and cutouts as polygons with arcs; holes with plating and tolerance; pads with
  shape, mask and paste expansion; tracks with width, layer and net; vias with drill, annular ring
  and span; zones with net, layer, clearance, thermal relief and priority.
- Rules: a matrix of clearances by net class and layer, widths by class, annular ring by drill,
  hole spacing, edge clearance, mask/silk rules, differential pair and length constraints.
- Connectivity: union-find over pads, tracks, vias and filled zone polygons, maintained
  incrementally; zone fills are connected as polygons, not as meshes.
- DRC: spatial index over each layer, rule evaluation, incremental dirty regions, and a stable
  ordering so reports are deterministic.
- 2D field solver for impedance and crosstalk: microstrip, stripline and differential pairs, using
  the stackup; closed-form where valid, a 2D boundary solve where not.
- Zone fill: polygon offset and boolean operations in 2D, deterministic fill order and thermal
  reliefs; the same result for the same inputs.
- Board body: outline extruded to thickness, cutouts and holes subtracted through the kernel; stable
  body and face names for assembly references.
- Component envelopes: parametric boxes/cylinders from the package and courtyard, used for
  mechanical clearance; detailed STEP models are render-only.

## 8. Commands

Namespaces follow the existing registry conventions (dotted ids, one-line params, schemas).

| Area | Commands |
|---|---|
| Library | `library.part.create/edit/duplicate/delete/list/validate`, `library.part.import/export`, `library.pack.install/remove/list`, `library.import_kicad` |
| Schematic | `schematic.sheet.create/edit/delete`, `schematic.symbol.place/move/rotate/mirror/edit`, `schematic.wire.add/edit/delete`, `schematic.label.*`, `schematic.bus.*`, `schematic.no_connect`, `schematic.port.*`, `schematic.annotate`, `schematic.erc`, `schematic.netlist`, `schematic.bom`, `schematic.variant.*`, `schematic.export` |
| Board | `board.create`, `board.stackup.set`, `board.footprint.place/move/rotate/mirror/swap`, `board.track.add/edit/delete`, `board.via.add/edit/delete`, `board.zone.add/edit/delete/fill`, `board.keepout.*`, `board.hole.*`, `board.outline.*`, `board.text.*`, `board.teardrop.*`, `board.update_from_schematic`, `board.diff`, `board.drc`, `board.panelise`, `board.export` |
| Fabrication | `fab.profile.add/edit/delete`, `fab.dfm`, `fab.package`, `fab.export_gerber/excellon/ipc2581/odbpp/bom/cpl` |
| Simulation | `sim.spice.*`, `sim.thermal.*`, `sim.power.*`, `sim.signal.*` |
| Exchange | `eda.import`, `eda.export`, `eda.loss_report` |

Every command has input and output schemas, never opens a dialog, and is callable from the UI, CLI,
control channel and MCP. DRC/ERC/DFM return lists of typed violations, not prose only.

## 9. Editors and UI

Electronics workspace tabs, registered in `crates/ui-egui/src/workspace.rs`:

| Tab | Panels |
|---|---|
| LIBRARY | Packs, parts, symbol/footprint preview, validation, import |
| SCHEMATIC | Sheets, tools, properties, ERC, nets, BOM |
| BOARD | Layers, nets, rules, placement/routing tools, DRC, 3D |
| SIMULATE | SPICE analyses, probes, waveforms; thermal/PI/SI fields |
| FABRICATE | Fab profile, DFM, layer preview, package, manifest |
| 3D | Board + enclosure scene, component fidelity, render controls |

- One outer container, theme tokens, shared UI kit.
- Cross-probe: selecting a part/pin/net highlights it everywhere.
- The board view is a 2D layer view with GPU-instanced tracks/pads and a 3D view from the same data.
- Waveforms are a new shared component (axes, probes, cursors, measurements), testable on plain data.
- Progress and cancel follow the existing job patterns; DRC is debounced and incremental.

## 10. Undo, snapshots and performance

The session snapshots the document before each command. A large board must not make that expensive:

- Board geometry is held in `Arc`-shared, immutable chunks (per layer, per net or per tile), so an
  edit replaces one chunk and reuses the rest.
- Drags commit once on release, not per pointer move.
- Zone fills, DRC results, ratsnest, netlist and render meshes are derived caches outside the
  snapshot.
- A command journal may replace full snapshots for board edits if measurements show it is needed;
  the contract is a measured per-edit cost, not a specific implementation.

Large-board performance comes from: per-layer spatial indexes, incremental connectivity, incremental
DRC on dirty regions, instanced drawing, LOD for tracks and silkscreen, and background zone fill.

## 11. Simulation integration

- SPICE runs on the schematic netlist with embedded part models; results are waveforms tied to the
  netlist hash.
- Board thermal imports stackup, copper fraction, component power and boundary conditions into the
  simulation `fea` substrate; results are fields on the board mesh.
- PI/SI use the stackup and board geometry: DC resistive solve for IR drop, AC solve for PDN
  impedance, 2D transmission-line solve for impedance/crosstalk.
- Every result records the board/netlist hash, solver version, precision, convergence and validity
  range. Results are derived and never saved stale.
- The simulation plan's substrate (studies, fields, jobs, cancellation, GPU precision policy)
  applies unchanged. SPICE adds a waveform result type, not a second job system.

## 12. Mechanical and render integration

- The board body is a derived body in the assembly: exact outline, holes and thickness.
- Component envelopes are derived bodies or occurrences for clearance; their height comes from the
  part revision and footprint courtyard.
- The render scene uses the board data plus component models at the fidelity level recorded on the
  part: vendor STEP, parametric package, or envelope.
- Board materials (solder mask, copper, ENIG/HASL, FR-4 edge, silk, solder) are original material
  definitions with attribution rows, extending the existing render material catalog.
- The enclosure, board and components share one render, one lighting setup and one camera; this is
  the product shot.

## 13. Format

- One `crates/doc/src/format/pending/electronics.rs` with a `//!` note and a step only if an older
  design needs rewriting.
- Unknown electronics keys and study kinds are preserved on save where the container allows.
- Caps: per-part blob size, total embedded part bytes, board object counts, layer count and text
  lengths. Refusal names the cap and the setting.
- A design with electronics remains openable by older builds with the standard "newer format"
  warning; they keep what they can read.

## 14. Import, export and loss reports

- Every importer returns a typed loss report: what was preserved, what was approximated, what was
  dropped, with source locations.
- Unsupported constructs that the source format can carry are preserved on save; otherwise the loss
  report is shown before the user commits.
- Round-trip tests compare netlists, refdes, footprints and layer stacks, not just "opened without
  error".
- The loss report is a command output, so agents and CI can assert on it.
