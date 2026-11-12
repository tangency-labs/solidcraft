# Electronics requirements

Part of the [electronics plan](README.md) and [RFC 0008](../rfcs/0008-electronics.md).

Requirements are numbered for traceability into tests and PRs. "Shall" means testable. Budgets are
targets set by measurement in E0/E2, not guesses made twice.

## 1. Users

| Persona | What they need | What would lose them |
|---|---|---|
| Hobbyist / maker | A board designed and ordered in an afternoon, with no licence, account or cloud | A rare crash, a lost file, a Gerber the fab rejects, a library that cannot be found |
| Student | Understand the circuit and the layout, see errors, learn from them | Errors that are hidden, or a tool that assumes expert knowledge |
| Small business / inventor | A complete product: board, enclosure, BOM, assembly files, renders | Anything that must be redone in another tool |
| KiCad user | Open existing projects and libraries, keep working | A one-way import, or lost net names |
| Mechanical engineer | The real board in the assembly, heights, holes and clearances | A board that is only a picture |

## 2. Functional requirements

### 2.1 Library and parts

- **E-R-001** The user shall create, edit, import, export and delete parts in local libraries.
- **E-R-002** A part shall carry: identity (manufacturer, MPN), package, pins, symbol, footprint,
  3D model, optional SPICE/IBIS models, datasheet link, supply data, provenance and licence.
- **E-R-003** A part revision shall be immutable; changing a part creates a new revision.
- **E-R-004** A design shall embed the exact revisions it uses, including symbol, footprint and model
   bytes, so it opens without the library, network or another machine.
- **E-R-005** Updating an embedded part in a design shall be an explicit command that reports what
   changed and what would break.
- **E-R-006** The library shall import KiCad symbol, footprint and 3D model libraries from the user's
   installation, without bundling them with SolidCraft.
- **E-R-007** Library validation shall check pin and pad numbering, duplicate pads, courtyard
   presence, pad-to-pin mapping, and 3D model alignment, and report failures per part.
- **E-R-008** A part with an invalid footprint or symbol shall refuse insertion and name the fault.
- **E-R-009** Parametric package generators shall produce footprints and 3D envelopes for common
   packages (0402…2512, SOT, SOIC, QFN, QFP, BGA, headers, connectors) without external data.
- **E-R-010** Every part shall record its provenance and licence, and a design shall carry those
   through to the BOM and the attribution report.

### 2.2 Schematic capture

- **E-R-020** The user shall create hierarchical sheets, symbols, wires, junctions, labels, buses,
  power symbols, no-connects and hierarchical ports.
- **E-R-021** Multi-unit symbols and alternate pin functions shall be supported.
- **E-R-022** The schematic shall be the source of truth for connectivity; the netlist is derived
  from it.
- **E-R-023** ERC shall check unconnected pins, conflicting pin types, missing power, unconnected
  hierarchical ports, duplicate reference designators and floating labels, and report each with a
  location and a severity.
- **E-R-024** Annotation shall assign reference designators deterministically and re-annotate
  without breaking existing links to the board unless requested.
- **E-R-025** Cross-probing shall select and highlight the same part, pin or net in schematic, board
  and 3D.
- **E-R-026** Variants shall mark parts as fitted, not fitted or alternate, and produce a variant BOM
  and CPL.
- **E-R-027** The BOM shall include quantity, reference designators, part identity, package, value,
  supply fields, variant state and provenance.
- **E-R-028** The user shall print/export a schematic to PDF and SVG with readable net names and
  title block.

### 2.3 Board layout

- **E-R-030** The board shall define outline, cutouts, mounting holes, stackup, layers, thickness
  and board edge clearance.
- **E-R-031** Footprints shall place pads, courtyards, silkscreen, assembly outlines, 3D models and
  keep-outs.
- **E-R-032** The user shall place, rotate, mirror, align and swap parts on either side.
- **E-R-033** Routing shall support tracks (including arcs), vias (through, blind, buried, micro),
  differential pairs, net classes and copper zones with thermal reliefs.
- **E-R-034** Zones shall fill deterministically from their rules and re-fill when the board changes.
- **E-R-035** DRC shall check clearance, track width, annular ring, hole size, hole-to-hole, board
  edge, courtyard, solder mask, silkscreen, zone connectivity, differential pair and length rules.
- **E-R-036** DRC shall run incrementally while editing and fully before fabrication output.
- **E-R-037** The board shall detect and report schematic-to-board differences (added, removed,
  changed parts, pins and nets) and apply an explicit update.
- **E-R-038** The board shall produce an exact 3D body (outline, cutouts, holes, thickness) for the
  assembly, plus component envelopes with heights.
- **E-R-039** Component keep-outs and heights shall be visible to mechanical clearance and
  interference checks.
- **E-R-040** The user shall be able to add copper text, dimensioning, teardrops, test points and
  assembly markings.
- **E-R-041** The board shall support panelisation with rails, fiducials, mouse-bites and V-grooves.

### 2.4 Simulation and verification

- **E-R-050** The schematic netlist shall run a SPICE-compatible simulation for DC operating point,
  DC sweep, AC, transient and noise, with probes and measurements.
- **E-R-051** Supported devices shall include R, L, C, independent and controlled sources, diodes,
  BJTs, MOSFETs, switches, ideal op-amps, transformers and lossless transmission lines.
- **E-R-052** The simulator shall support subcircuits, parameters, `.include`-style libraries from
  embedded part models, and temperature.
- **E-R-053** Non-convergence shall be reported with the failing node, iteration and suggested
  remedy; it shall never return a waveform as if valid.
- **E-R-054** Thermal analysis shall import component power, board stackup and copper distribution
  into the simulation substrate and report temperatures.
- **E-R-055** Power integrity shall report DC IR drop, current density and a PDN impedance sweep.
- **E-R-056** Signal integrity shall report impedance, propagation delay and crosstalk for defined
  transmission lines, and a basic eye diagram for a specified channel.
- **E-R-057** Simulation results shall be viewable as waveforms or fields, exportable, and tied to
  the exact board and netlist revision that produced them.
- **E-R-058** The tool shall state the validity range and non-claims of each analysis.

### 2.5 Fabrication output

- **E-R-060** Gerber X2 shall be written for every copper, mask, silkscreen, paste and mechanical
  layer, with correct polarity, apertures, attributes and drill alignment.
- **E-R-061** Excellon drill files shall be written for plated and non-plated holes, with slots and
  tool tables.
- **E-R-062** IPC-2581 and ODB++ export shall be available for assembly and exchange.
- **E-R-063** BOM and CPL shall be written in the column layouts required by common fabs, with
  rotations, sides, variants and DNP handling.
- **E-R-064** Fab/assembly profiles shall encode each fab's rules (minimum track, clearance, drill,
  annular ring, solder mask, silkscreen, board size) and be selectable per order.
- **E-R-065** DFM checks shall run against the selected profile and report violations with layer and
  location.
- **E-R-066** A single command shall produce a reviewable fabrication package (zip) with a manifest,
  readme, layer mapping and checksums.
- **E-R-067** The package shall be re-openable by the app's own Gerber/IPC importer for review.

### 2.6 Rendering and mechanical integration

- **E-R-070** The 3D view shall show the board with copper, solder mask, silkscreen, drills, solder
  and placed components.
- **E-R-071** Components shall render at one of three fidelity levels, stated in the UI: vendor
  3D model, parametric package model, or envelope.
- **E-R-072** The board and its components shall render with the enclosure in the same scene,
  materials and lighting.
- **E-R-073** The assembly shall use the exact board body and component envelopes for interference
  and clearance checks, not render meshes.
- **E-R-074** Board parameters (size, thickness, mounting holes, connector positions) shall be
  usable in the mechanical parameter graph and referenced by enclosure features.
- **E-R-075** The board outline, holes and keep-outs shall export to DXF/STEP for CAM and mechanical
  hand-off.
- **E-R-076** Renders shall record which component models were exact, simplified or envelope.

### 2.7 Compatibility and exchange

- **E-R-080** The app shall import and export KiCad schematic, board and library files for the
  supported version, preserving nets, references, values, footprints and 3D model links.
- **E-R-081** The app shall import Eagle XML schematics and boards.
- **E-R-082** The app shall import and export IPC-2581 and export ODB++.
- **E-R-083** A round trip (import, edit, export, re-import) shall preserve connectivity and
  reference designators, and shall report anything it cannot preserve rather than dropping it.
- **E-R-084** Unsupported constructs shall be preserved on save where the source format allows, and
  otherwise named in a loss report.
- **E-R-085** SPICE netlists shall import and export for external simulation.

### 2.8 Automation and agents

- **E-R-090** Every action shall be a command with an input and output schema.
- **E-R-091** Commands shall never open a dialog and shall be callable from the UI, CLI, control
  channel and MCP.
- **E-R-092** Commands shall return machine-readable diagnostics: ERC/DRC lists, loss reports,
  connectivity checks, simulation convergence.
- **E-R-093** A headless run shall be able to open a project, run ERC/DRC/DFM, export fabrication
  files and report pass/fail.
- **E-R-094** Long operations (zone fill, DRC, simulation, render) shall report progress and be
  cancellable.

## 3. Non-functional requirements

### 3.1 Reliability

- **E-N-001** No `unwrap`, `expect`, `panic` or `unsafe` in non-test code; all failures are `Result`.
- **E-N-002** Every array, string, file, board, layer count, pad count, net count and simulation step
  has a cap.
- **E-N-003** A failed import, DRC, simulation or export leaves the design unchanged.
- **E-N-004** A crash or power loss never loses more than the last committed edit; the `.sc` backup
  and recovery path applies to electronics too.
- **E-N-005** Undo/redo shall cover every electronics edit, including library assignment, placement,
  routing, rules and variants.
- **E-N-006** Opening a file with a newer format warns and opens what it can; it never rewrites it.

### 3.2 Performance

Measured on an 8 GB Apple Silicon laptop, release build, default settings.

| Workload | Target |
|---|---|
| Open a 10,000-component, 100,000-pad board | ≤ 3 s |
| Pan/zoom a dense board | 60 fps at 4K |
| Incremental DRC after a move | ≤ 200 ms |
| Full DRC, 10,000 pads | ≤ 2 s |
| Zone fill, 10 layers | ≤ 1 s typical |
| Route feedback per pointer move | ≤ 16 ms |
| Save a 200 MB design | ≤ 3 s |
| SPICE transient, 100k points, 1k devices | ≤ 10 s |
| Board 4K product render | ≤ 4 min on the path tracer |

- **E-N-010** Undo snapshots shall not clone all board geometry per edit; board data shall use
  structural sharing or a journal, with a measured per-edit cost.
- **E-N-011** Large-board operations shall show progress and remain cancellable.
- **E-N-012** The browser build shall cap board size and refuse clearly above it.

### 3.3 Portability

- **E-N-020** All library crates shall build for `wasm32-unknown-unknown`.
- **E-N-021** The electronics core shall not depend on OS, filesystem, threads or GPU APIs.
- **E-N-022** Files are deterministic: the same design saves to the same bytes where the container
  allows, and at minimum to the same semantic content.
- **E-N-023** Locale and decimal separators shall not affect files or results.
- **E-N-024** No network is required for library use, design, simulation or fabrication output.

### 3.4 Compatibility and contribution

- **E-N-030** New electronic data shall use one `format/pending` file; unknown electronics sections
  shall be preserved by older builds where the format allows.
- **E-N-031** New crates shall have a layer row, an architecture row, a licence, a README (shared
  crates) and pass `cargo xtask layers`, `shared`, `wasm`.
- **E-N-032** UI strings go in `assets/locales/pending/<slug>.json`; assets get attribution rows.
- **E-N-033** Every command has a schema or an entry in `schema/unconverted.txt` that only shrinks.
- **E-N-034** Tests live in the area's existing file, never a per-bug file.
- **E-N-035** A changed default keeps the old behaviour behind a preference and fills
  `Changes a default:`.

## 4. Definition of done

A feature area is done only when all of these exist:

1. Requirements implemented and covered by command tests.
2. A compatibility or correctness corpus with independent references (netlist, Gerber, SPICE
   analytic, DRC rule matrix).
3. Refusal tests for hostile input and invalid rules.
4. Deterministic output where it matters (zone fill, netlist, Gerber, BOM ordering).
5. Undo, save, cold reopen and recovery.
6. UI with progress, cancel and a "what this does not prove" statement where relevant.
7. Screenshots or a recording for observable changes.
8. `cargo xtask ci` green.

## 5. Explicit non-claims

The product shall not state or imply that:

- a DRC/DFM pass is a guarantee that a fab will accept or that the board will work;
- a simulation is a substitute for measurement, EMC testing or regulatory approval;
- an imported KiCad/Eagle/IPC file preserved every construct unless the loss report says so;
- a component model is the manufacturer's exact geometry unless its provenance says so;
- a routing result is optimal or manufacturable without the selected fab rules.
