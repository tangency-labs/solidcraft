# The Inventor's Toolbox map

Phase → step → feature. A feature is one line: the smallest missing capability that makes the step
usable. Details live in the feature RFC named in brackets.

Status: `done` (implemented and tested), `partial` (exists with gaps), `missing`, `out` (external by
design). Evidence for `done`/`partial` is the command or guide named in **Have**.

Every feature below must ship on all four surfaces: **GUI, CLI, MCP server and documentation**. A
feature that exists only in one is `partial`, never `done`.

No step below is required. The map shows what exists and what could be useful; nothing blocks a user
who skips a step or jumps ahead.

## P0 — Intent

**I1 Problem and users** — `missing`
Have: nothing.
- [ ] One-page brief: problem, users, jobs, non-goals, one-sentence pitch (0005)

**I2 Requirements** — `missing`
Have: nothing.
- [ ] Must/should/could requirements, each with an acceptance test and an owner (0005)
- [ ] Requirement links to the feature, test and evidence that satisfy it (0005)

**I3 Success criteria** — `missing`
Have: nothing.
- [ ] Measurable success criteria per must requirement (0005)

**I4 Constraints and budgets** — `missing`
Have: parameters and physical material cards, but no targets.
- [ ] Mass, power, cost, size, time and environment targets with units and tolerances (0005)
- [ ] A budget view that compares targets with design measurements (0005)

**I5 Risks and unknowns** — `missing`
Have: nothing.
- [ ] Risk register: likelihood, impact, mitigation, owner, trigger (0005)
- [ ] A cheap-check suggestion per risk: calculation, simulation or prototype (0005)

**I6 Compliance and safety screen** — `missing`
Have: nothing.
- [ ] Screening questions: mains, battery, RF, laser, heat, medical, toy, vehicle, outdoor (0005)
- [ ] A decision per trigger: in scope, out of scope, needs a test house (0005)

**I7 Decisions** — `missing`
Have: nothing.
- [ ] Decision log with date, options, choice, reason, revisit condition (0005)

**I8 Feasibility and prototype plan** — `missing`
Have: nothing.
- [ ] What to prove first, by what method, at what fidelity (0005)

**I9 Help and templates** — `missing`
Have: `ui.help`, command reference, guides.
- [ ] A guided brief with explanations and examples, not a blank form (0005)
- [ ] Product templates: enclosure + board + firmware, mechanical-only, electronics-only (0005)

**Suggested check (optional):** brief has problem, ≥1 must requirement with a test, mass/power/cost/size targets, one risk,
compliance screen answered, decision log seeded. **Current: missing.**

## P1 — Concept

**C1 System architecture** — `missing`
Have: nothing.
- [ ] Block diagram of mechanical, electrical, firmware, power and interfaces (0006)
- [ ] Blocks link to parts, boards, modules and parameters (0006)

**C2 Interfaces** — `missing`
Have: sketches, parameters, assembly references.
- [ ] Connector, pin, mount and keep-out definitions that are machine-checkable (0006)
- [ ] Interface ownership: which block supplies, which consumes (0006)

**C3 Spatial packaging** — `partial`
Have: assemblies, occurrences, sketches, interference.
- [ ] Envelope volumes and space claims between board, battery, connectors and enclosure (0006)
- [ ] A packaging view with claimed vs free space and clearance margins (0006)

**C4 Mechanism and motion concept** — `partial`
Have: joints, constraints, motion studies, contact sets.
- [ ] A quick concept mode: DOF, range, force/torque estimate, motor sizing (0006)
- [ ] A concept motion study that does not need detailed parts (0006)

**C5 Electrical and power concept** — `missing`
Have: nothing implemented; EDA planned.
- [ ] Power tree with rails, loads, efficiency and battery life (0006)
- [ ] MCU/peripheral block choices and pin budget (0006)

**C6 Make/buy and process** — `missing`
Have: `parts.library` fasteners, pins and bearings.
- [ ] A process decision per part: print, laser, CNC, sheet, mould, buy, PCB (0006)
- [ ] A rough cost and lead-time estimate per decision (0006)

**C7 Assembly concept** — `partial`
Have: components, occurrences, joints, grounding, exploded views.
- [ ] Concept-level assembly layout with fastening and serviceability intent (0006)
- [ ] Assembly sequence sketch and access checks (0006)

**C8 Budget feasibility** — `missing`
Have: mass properties and `assembly.bom` counts.
- [ ] Mass, power, heat, cost and time roll-up against the Intent targets (0006)

**C9 Concept prototype plan** — `partial`
Have: `file.send_to_slicer`, STEP/3MF export.
- [ ] What to build, at what fidelity, to retire the top unknowns (0006)

**C10 Concept review** — `missing`
Have: nothing.
- [ ] Suggested checklist (optional): architecture, interfaces, budgets, risks, prototype plan (0006)

**Suggested check (optional):** architecture and interfaces defined, budgets within target or waived, top risks have a
plan, concept assembly exists. **Current: missing.**

## P2 — Detail

### Mechanical

**D1 Part modelling** — `done`
Have: sketch, solid, surface, sheet metal, plastic features, patterns, holes, threads.

**D2 Assembly design** — `partial`
Have: components, occurrences, joints, constraints, interference, exploded views, mass properties,
`assembly.bom`, STEP assembly structure, URDF.
- [ ] Fits and clearance intent per joint/interface, not only interference (0007)
- [ ] Flexible/deformable parts: belts, hoses, cables, O-rings, springs (0007)
- [ ] Variant-aware assembly (connector option, DNP, enclosure option) (0007)
- [ ] Fastener torque, adhesive and installation metadata (0007)
- [ ] Test/inspection points marked on the assembly (0007)

**D3 Fasteners and standard parts** — `partial`
Have: `parts.library` seven families, `parts.insert`, generated part numbers.
- [ ] Torque, preload, thread engagement and tool access data (0007)
- [ ] Supplier part numbers and lifecycle where known (0007)

**D4 Tolerances and GD&T** — `missing`
Have: dimensions in sketches and features.
- [ ] Toleranced dimensions and geometric controls (0007)
- [ ] Worst-case and RSS stack-up across parts and joints (0007)
- [ ] Fit analysis: clearance/interference at tolerance limits (0007)

**D5 Drawings** — `missing` (deferred on the roadmap)
Have: none for 2D manufacturing drawings.
- [ ] Part drawings: views, sections, dimensions, tolerances, title block (0007)
- [ ] Assembly drawings with BOM balloons and exploded views (0007)
- [ ] PDF/DXF export and revision block (0007)

**D6 Manufacturing and DFM/DFA** — `partial`
Have: sheet-metal flat pattern and DXF, draft/curvature analysis.
- [ ] Process-specific DFM checks: print, sheet, CNC, moulding, PCB (0007)
- [ ] DFA checks: access, insertion direction, fastener reach (0007)
- [ ] Production process recorded per part (0007)

**D7 Assembly instructions** — `missing`
Have: exploded views and animations.
- [ ] Instructions generated from an exploded view: ordered steps, callouts, parts, tools (0007)
- [ ] Torque, adhesive, orientation and inspection notes (0007)
- [ ] Images/diagrams per step and a printable/exportable document (0007)

**D8 Cross-domain BOM and cost** — `partial`
Have: `assembly.bom` with levels, grouping, BOM types, part metadata, CSV/JSON.
- [ ] PCB, components, firmware, packaging and consumables in one BOM (0007)
- [ ] Cost at 1/10/1000, lead time, MOQ, alternates and availability (0007)
- [ ] Variant BOM and DNP handling (0007)

**D9 Variants and configurations** — `partial`
Have: configurations for parameters, suppressions, materials and appearances.
- [ ] One variant spanning schematic, board, BOM, CPL and assembly (0007)

**D10 Harness and cabling** — `missing`
Have: nothing.
- [ ] Connector-to-connector wire list, lengths, gauges, colors, labels (0007)
- [ ] Routing path and bend radius checks (0007)

### Electronics

**D11 Library and parts** — `missing`
Have: `parts.library` mechanical families only.
- [ ] Part model: symbol, footprint, 3D, SPICE/IBIS, supply, provenance (0008)

**D12 Schematic and ERC** — `missing`
- [ ] Hierarchical capture, buses, net classes, ERC, annotation, variants, BOM (0008)

**D13 Board layout and DRC** — `missing`
- [ ] Stackup, pads, tracks, vias, zones, DRC, schematic-to-board sync (0008)

**D14 Fabrication outputs** — `missing`
- [ ] Gerber X2, Excellon, IPC-2581/ODB++, BOM/CPL, DFM, panel (0008)

### Firmware

**D15 Board definitions and pin map** — `missing`
- [ ] Generate pin maps and board definitions from the schematic (0009)

**D16 Firmware project and build** — `missing`
- [ ] Source in the product, build via existing toolchains, flash and monitor (0009)

**D17 Debug and test hooks** — `missing`
- [ ] Serial/logic capture, test points, programming access, HIL later (0009)

### Systems

**D18 Enclosure and board integration** — `partial`
Have: CAD assemblies; board body planned in 0008.
- [ ] Exact board body, component envelopes, connector cut-outs, mounting references (0008)

**D19 Design notes and revision history** — `missing`
- [ ] Notes, assumptions and change history attached to the product (0013)

**D20 Detail review** — `missing`
- [ ] Suggested checklist (optional): buildable, documented, BOM complete, drawings and instructions exist (0007)

**Suggested check (optional):** all parts, circuits, firmware and documents exist; BOM and drawings complete; instructions
exist. **Current: partial.**

## P3 — Verify

**V1 Structural** — `missing` (0010)
**V2 Modal and buckling** — `missing` (0010)
**V3 Thermal and thermal-stress** — `missing` (0010)
**V4 Flow** — `missing` (0010)
**V5 Mechanisms and dynamics** — `partial`
Have: motion studies, interference, contact sets, `dynamics` crate (RFC #9719).
- [ ] Force/torque reactions, motor sizing, duty cycle and efficiency (0010)
**V6 Circuits (SPICE)** — `missing` (0008/0010)
**V7 Signal and power integrity** — `missing` (0008/0010)
**V8 Tolerance and worst-case** — `missing` (0007)
**V9 Reliability and safety** — `missing`
- [ ] Derating, FMEA, drop/vibration, IP, battery safety (0010)
**V10 Cost and supply verification** — `missing` (0007)
**V11 Compliance pre-check** — `missing` (0013)
**V12 Benchmarks and hand calculations** — `missing`
- [ ] Compare each analysis with a hand calculation or reference before trusting it (0010)
**V13 Evidence traceability** — `partial`
Have: render results and reports; no unified evidence.
- [ ] Every must requirement links to a result, report or test (0010/0012)
**V14 Verification review** — `missing`
- [ ] Suggested checklist (optional): evidence exists or the gap is known (0010)

**Suggested check (optional):** every must requirement has evidence or a recorded decision to accept the risk; analyses have convergence/validity.
**Current: missing.**

## P4 — Prototype

**P1 Prototype plan** — `missing`
- [ ] What to prove, fidelity, process per part, cost and lead time (0011)
**P2 Mechanical outputs** — `partial`
Have: STL/3MF export, `file.send_to_slicer`, sheet-metal DXF, STEP.
- [ ] Laser/CNC profiles, bought-part list, print orientation and material intent (0011)
**P3 Electronics outputs** — `missing`
- [ ] PCB fab/assembly package, module and breadboard options (0008/0011)
**P4 Firmware build and flash** — `missing` (0009)
**P5 Assembly instructions** — `missing` (0007)
**P6 Build manifest** — `missing`
- [ ] One command collects outputs, firmware, docs and checksums into a revisioned package (0011)
**P7 Prototype review** — `missing`
- [ ] What was learned, what changes, next prototype (0011)

**Suggested check (optional):** a complete prototype builds from the project with one command. **Current: partial.**

## P5 — Test

**T1 Test plan** — `missing` (0012)
**T2 Fixtures and instrumentation** — `missing` (0012)
**T3 Measurement capture** — `missing`
- [ ] Import serial/logic/scope data, photos, caliper and scale readings (0012)
**T4 Simulation versus measurement** — `missing` (0012)
**T5 Defects and root cause** — `missing` (0012)
**T6 Regression and retest** — `missing` (0012)
**T7 Test review** — `missing` (0012)

**Suggested check (optional):** every must requirement tested; differences from simulation explained. **Current: missing.**

## P6 — Release

**R1 Freeze and revision** — `partial`
Have: undo/timeline, design metadata; no revisions or branches.
- [ ] Freeze a revision and record what changed (0013)
**R2 Production documentation** — `partial`
Have: mechanical BOM.
- [ ] Drawings, instructions, test procedure, labels (0013)
**R3 Compliance evidence** — `missing` (0013)
**R4 Packaging and labels** — `missing` (0013)
**R5 Volume cost and sourcing** — `missing` (0007/0013)
**R6 Release manifest** — `missing`
- [ ] Manifest, checksums, notes, build instructions, release notes (0013)
**R7 Release review** — `missing` (0013)

**Suggested check (optional):** a third party could build the product from the release. **Current: missing.**

## P7 — Produce (handoff)

**Pd1 Procurement** — `out`
**Pd2 Assembly, QC and serialization** — `out`
**Pd3 Field feedback and revisions** — `out`

The tool exports what these need; it does not run them.

## Cross-cutting

**X1 Product container and phases** — `missing` (0004/0005)
**X2 Help and guidance** — `partial`
Have: help, command reference, guides.
- [ ] "What do I do next?" based on the workflow map (0004)
**X3 Version control and collaboration** — `missing`
- [ ] Git-friendly project, diffs, comments, review before release (0013)
**X4 Four surfaces: GUI, CLI, MCP, docs** — `partial`
Have: command registry, `solidcraft-cli exec`, control channel, MCP, generated features and book.
- [ ] Every workflow command is reachable in the GUI, CLI and MCP, with generated command docs and a human guide (0004–0013)
- [ ] Workflow status and readiness checks are commands, not UI state only (0004)
- [ ] Each feature PR records one invocation from each surface as evidence (0004)
**X5 Import and export** — `partial`
Have: STEP, 3MF, STL, OBJ, DXF, SVG, glTF, URDF.
- [ ] KiCad, Eagle, IPC-2581, Gerber, SPICE (0008)
