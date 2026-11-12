# Electronics roadmap

Part of the [electronics plan](README.md) and [RFC 0008](../rfcs/0008-electronics.md).

No dates: releases are daily and features land when validated. Each milestone is a usable product,
not a partial KiCad. The first end-to-end board is E3; everything after deepens a working tool.

## Dependency graph

```
E0 foundation + vertical slice
   library · parts · board body · workspace · KiCad library import · render
 ├── E1 schematic ──► E2 board layout ──► E3 fabrication (first ordered board)
 │                        │                  └── E4 simulation
 │                        └── E5 depth (stackup, HDI, diff pairs, variants)
 └── E6 ecosystem (library authoring, templates, plugins, performance, full compatibility)
                          └── E7 polish (render, UX, benchmarks)

Tracks beside the milestones:
  K compatibility (KiCad, Eagle, IPC-2581, Gerber)
  S simulation (SPICE, thermal, PI, SI)
  R render (materials, solder, silk, LOD, product shots)
  P performance (large boards, undo cost, zone fill, DRC)
  M mechanical (board body, envelopes, clearances, parameters)
```

## E0 — Foundation and vertical slice

**Goal:** electronics is real in the document, the workspace and the assembly, before any editor is
finished.

Slices:

1. RFC 0008, ROADMAP/CONTRIBUTING update, RFC 0001 note.
2. `eda` crate: part, symbol, footprint, board and net types; validation; caps.
3. Library pack entry kind and `library.part.*` commands; design-embedded snapshots.
4. KiCad symbol/footprint/model import from the user's disk; loss report.
5. Electronics section in the document; one `format/pending` file; save/reopen/undo.
6. Electronics workspace shell with LIBRARY, BOARD and 3D tabs.
7. Board body derived from outline, cutouts, holes and thickness; component envelopes.
8. Board and component rendering with the existing materials and scene.
9. Cross-probe foundation: stable ids and a selection map across schematic, board, 3D.

**Exit:** import a user's KiCad library, insert parts into a SolidCraft assembly, get a real board
body with component envelopes, check clearances, and render the board in its enclosure. **Evidence:**
screenshots and a recording, plus a cold-reopen test with the library removed.

## E1 — Schematic capture

**Goal:** a schematic that produces a trustworthy netlist.

Slices:

1. Sheets, hierarchy, ports and title block.
2. Symbols, multi-unit parts, alternate pin functions, fields.
3. Wires, buses, junctions, labels, power symbols and no-connects.
4. Net resolution and deterministic netlist.
5. ERC rule engine with severities and locations.
6. Annotation and re-annotation with stable ids.
7. Cross-probe to board/3D; net highlighting.
8. BOM and variants (fitted, DNP, alternate).
9. KiCad schematic import/export with loss report; PDF/SVG export.

**Exit:** import a real KiCad schematic, edit it, pass ERC, produce the same netlist as KiCad, export
it and re-import it with no connectivity loss. **Evidence:** netlist comparison for a real project.

## E2 — Board layout

**Goal:** a manufacturable board from the schematic.

Slices:

1. Stackup editor and rules; board outline, cutouts, holes.
2. Footprint placement, rotation, mirroring, alignment, swap.
3. Tracks, arcs, vias, net classes, differential pairs.
4. Zones, thermal reliefs, deterministic fill.
5. Connectivity and ratsnest; incremental updates.
6. DRC: clearance, width, annular, hole, edge, courtyard, mask, silk, zone, diff pair, length.
7. Schematic-to-board differences and explicit update.
8. 2D layer view and 3D board view; cross-probe.
9. KiCad board import/export.
10. Interactive routing (walk-around first, shove later).

**Exit:** design a complete two-layer board from an imported schematic, pass a fab profile's DRC,
round-trip the board through KiCad, and render it with components. **Evidence:** the board file, the
DRC report and a render.

## E3 — Fabrication and the first ordered board

**Goal:** a package a fab accepts, for a board that is actually ordered and assembled.

Slices:

1. Gerber X2 writer with attributes and layer mapping.
2. Excellon writer for PTH/NPTH, slots and tool tables.
3. IPC-2581 export; ODB++ export.
4. BOM/CPL export with fab-specific column mappings and rotations.
5. Fab profiles and DFM checks.
6. Panelisation: rails, fiducials, mouse-bites, V-grooves.
7. `fab.package`: zip, manifest, readme, checksums.
8. Gerber/IPC reader for review and round-trip.
9. Order hand-off: package + open the fab page, no credentials or network calls.

**Exit:** a real board ordered from a real fab, accepted without manual Gerber edits, assembled and
tested; the package re-opens correctly in the app and in an independent Gerber viewer. **Evidence:**
the order confirmation, the fab's DFM result, photos of the assembled board, and the render.

## E4 — Simulation

**Goal:** answer "does the circuit work, does it get hot, does the power arrive" before ordering.

Slices:

1. `spice` crate: MNA, Newton, gmin/source stepping, DC/AC/transient/noise.
2. Devices: R, L, C, sources, diodes, BJT, MOSFET, switch, ideal op-amp, transformer, lossless line.
3. Subcircuits, parameters, temperature, embedded part models, netlist import/export.
4. Waveform viewer: probes, cursors, measurements, export.
5. Thermal: board stackup, copper fraction, component power, convection; reuse `fea`.
6. Power integrity: DC IR drop, current density, PDN impedance sweep.
7. Signal integrity: impedance, delay, crosstalk, basic eye diagram.
8. Validity ranges and non-claims in every result.

**Exit:** SPICE benchmarks match analytic results; a real board's thermal and IR-drop results match a
hand calculation and a reference tool within the stated tolerance; waveforms and fields export.

## E5 — Depth

Slices:

1. Multilayer stackups with impedance targets and controlled-dielectric layers.
2. Blind, buried and micro vias; via-in-pad; backdrill.
3. Differential pairs and length tuning with real-time constraints.
4. Variants across schematic, board, BOM, CPL and simulation.
5. Rigid-flex and HDI basics; panel and assembly drawings.
6. Router improvements: shove, hug, spread, multi-net.
7. Teardrops, test points, fiducials, mechanical holes and board-level documentation.
8. Copper balancing, thermal relief tuning, current-capacity checks.

**Exit:** a four-layer, impedance-controlled board with diff pairs and blind vias passes DRC and DFM,
simulates, and renders correctly.

## E6 — Ecosystem and performance

Slices:

1. Library authoring UI: symbol editor, footprint editor, 3D alignment, validation dashboard.
2. Templates and project creation for common boards.
3. Plugin commands for niche importers, fabricators and checks (RFC 0002 surface).
4. Full KiCad project round trip; Eagle importer; IPC-2581 importer.
5. Large-board performance: 10k components, 100k pads, incremental DRC, instanced drawing.
6. Undo cost measurement and, if needed, a command journal for board edits.
7. Documentation and tutorials that take a beginner from idea to ordered board.

**Exit:** the performance budgets in the requirements are met on the baseline laptop; a KiCad user
can move a real project in both directions; a beginner can follow the tutorial end to end.

## E7 — Polish

Slices:

1. Render fidelity: solder fillets, mask gloss, copper under mask, silk legibility, component LOD,
   labels, board edge, enclosure context.
2. UX: keyboard-first flows, selection filters, context menus, accessibility, reduced motion.
3. Cross-probe and comparison polish; board revisions and visual diffs.
4. Benchmarks against KiCad on specific tasks, with numbers, not claims.
5. Crash-free fuzzing and long-session tests.

**Exit:** measurable advantages on the chosen tasks (open, DRC, zone fill, 3D, render, mechanical
integration) with reproducible evidence, and no regressions in the core CAD.

## Tracks

| Track | Gate per milestone |
|---|---|
| Compatibility | Import/export corpus grows; netlist and round-trip equivalence tested |
| Simulation | Benchmarks and validity ranges; no result without convergence evidence |
| Render | Materials and fidelity levels; golden images; no third-party assets |
| Performance | Budgets measured on the baseline; regressions reported |
| Mechanical | Board body, envelopes, clearance, parameters, export to CAM |

## What ships when

- **E0:** the board and its components are real in the assembly and the render.
- **E1:** circuit capture and ERC.
- **E2:** layout and DRC.
- **E3:** a board you can order.
- **E4:** circuit, thermal, power and signal checks.
- **E5:** professional board features.
- **E6:** compatibility and large-board performance.
- **E7:** the product shot and the polish.

## Effort shape

E0 ~10 PRs, E1 ~12, E2 ~18, E3 ~10, E4 ~14, E5 ~12, E6 ~10, E7 ~8, plus the compatibility,
simulation, render and performance tracks. The binding constraint is validation and compatibility,
not code volume. Keep every PR independently testable and mergeable.
