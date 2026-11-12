# RFC roadmap: 0006–0014

Part of [RFC 0004](../rfcs/0004-product-workflow.md). Each RFC is the next missing piece in workflow order.
They are written one at a time; this page fixes their scope, dependencies and readiness so the order does
not drift.

## Universal contract

Every feature RFC below must specify, for every command:

- an input and output schema, typed errors, and no dialog dependency;
- the **four surfaces**: GUI, CLI, MCP server and documentation;
- tests in the area's existing file, including refusal cases;
- undo/save/reopen where it changes the product;
- evidence in the PR: one invocation per surface;
- no new default without a preference and the `Changes a default:` line;
- no GPL code or data, and no bundled third-party assets without a licence row.

A feature is not done while any of these is missing.

## 0006 — Concept: architecture, interfaces and budgets

**Why:** after Intent, the product needs an architecture and a feasible layout before detailed
design. Today there is no system diagram, no interface contract, no make/buy decision, and no budget
roll-up.

**Scope**

- System block diagram: mechanical, electrical, firmware, power, interfaces; blocks link to parts,
  boards, modules and parameters.
- Interface definitions: connectors, pins, mounts, keep-outs, ownership; machine-checkable.
- Spatial packaging: envelope volumes, space claims, board/battery/connector placement, clearance
  margins.
- Mechanism concept: DOF, range, force/torque estimate, motor sizing, concept motion study.
- Electrical concept: power tree, rails, loads, efficiency, battery life, MCU/peripheral choices.
- Make/buy/process per part: print, laser, CNC, sheet, mould, buy, PCB; rough cost and lead time.
- Assembly concept: layout, fastening intent, serviceability, access checks.
- Budget feasibility: mass, power, heat, cost, time against Intent targets.
- An optional concept review.

**Commands:** `concept.architecture.*`, `concept.interface.*`, `concept.packaging.*`,
`concept.power.*`, `concept.process.*`, `concept.assembly.*`, `concept.budget.*`, `concept.review`.

**Useful after:** 0005.

**Readiness (optional):** architecture and interfaces defined; budgets within target or waived; top risks have a
plan; a concept assembly exists.

## 0007 — Mechanical detail gaps: assembly, tolerances, drawings, BOM

**Why:** mechanical modelling is strong, but the outputs and rigour that make a design buildable are
missing. Assembly design itself stays here and in Concept; this RFC adds what surrounds it.

**Scope**

- Toleranced dimensions, geometric controls, worst-case and RSS stack-up, fit analysis.
- Part drawings: views, sections, dimensions, title block; assembly drawings with BOM balloons.
- Assembly instructions generated from exploded views: ordered steps, callouts, parts, tools, torque,
  adhesive, orientation, inspection notes; printable/exportable.
- Cross-domain BOM: PCB, components, firmware, packaging, consumables; cost at 1/10/1000, lead time,
  MOQ, alternates, availability.
- Variant BOM, DNP and assembly variants spanning schematic, board, BOM and CPL.
- Flexible/deformable parts: belts, hoses, cables, O-rings, springs; or explicit cable routing.
- Harness and cabling: wire list, lengths, gauges, colours, labels, bend radius.
- Fastener torque, preload, thread engagement and tool access metadata.
- Test/inspection points and serialization marks on the assembly.
- Process-specific DFM/DFA and a production process recorded per part.
- An optional detail review.

**Commands:** `draft.*`, `tolerance.*`, `stackup.*`, `drawing.*`, `instructions.*`, `bom.*`,
`harness.*`, `dfm.*`, `dfa.*`.

**Useful after:** 0006.

**Readiness (optional):** all parts documented; BOM and drawings complete; instructions and manufacturing intent
exist.

## 0008 — Electronics (EDA)

**Why:** the other half of the product. This is the former RFC 0005, renumbered; its detailed plan
stays at [docs/electronics](../../docs/electronics/README.md).

**Scope:** library and parts, schematic and ERC, board layout and DRC, stackup, routing, zones,
fabrication outputs (Gerber/Excellon/IPC-2581/ODB++/BOM/CPL), DFM, KiCad/Eagle compatibility,
board body and component envelopes for the mechanical assembly.

**Useful after:** 0006 (interfaces, packaging, power concept).

**Readiness (optional):** a complete two-layer board from schematic through fab package and render; E3 of the
electronics plan.

## 0009 — Firmware and testability

**Why:** a modern product has firmware, and the board must be designed to be programmed and tested.
Neither exists in the plan.

**Scope**

- Board definitions and pin maps generated from the schematic; net and pin names carried through.
- Firmware source in the product container; build via existing toolchains (PlatformIO, Arduino,
  Rust embedded); no IDE rebuild.
- Flash and serial monitor; probe/debug integration; test-point and programming-header awareness.
- Test hooks: serial/logic capture, HIL later, factory test access.
- Firmware revision linked to the board revision and release manifest.
- Documentation: build, flash, recover, test.

**Commands:** `firmware.board.*`, `firmware.build`, `firmware.flash`, `firmware.monitor`,
`firmware.test.*`.

**Useful after:** 0008.

**Readiness (optional):** blink an LED and read a sensor using only pins defined by the schematic; firmware builds
and flashes from the product.

## 0010 — Simulation and verification

**Why:** evidence before spending money. This is the former RFC 0004, renumbered; its detailed plan
stays at [docs/simulation](../../docs/simulation/README.md).

**Scope:** structural static, modal, buckling, thermal and thermal-stress, fatigue, topology, flow,
mechanism forces, SPICE, SI/PI, tolerance/worst-case, reliability/safety checks, benchmarks and
evidence traceability.

**Useful after:** 0006, 0007, 0008 (geometry, materials, circuits).

**Readiness (optional):** every must requirement has evidence or a recorded decision to accept the risk; analyses have convergence and validity
ranges.

## 0011 — Prototype, CAM and build

**Why:** the prototype is where the design meets reality, and it needs a plan and a package.

**Scope**

- Prototype plan: what to prove, fidelity, process per part, cost and lead time.
- Mechanical outputs: print (hand-off to the slicer), laser, CNC, sheet, bought parts; print
  orientation and material intent.
- Electronics outputs: PCB fab/assembly package, modules, breadboard options.
- Firmware build and flash in the package.
- Assembly instructions included.
- One-command build manifest: outputs, firmware, docs, checksums, revision, notes.
- CAM: toolpaths for CNC/sheet where the existing CAM plan applies; export, do not control machines.
- Handoffs: slicer, CAM sender, fab, assembly. No slicer, controller or ERP rebuild.
- Prototype review: what was learned, what changes.

**Commands:** `prototype.plan.*`, `build.manifest`, `build.export`, `cam.*`, `fab.send`.

**Useful after:** 0007–0010.

**Readiness (optional):** a complete prototype builds from the project with one command.

## 0012 — Test and measurement

**Why:** simulation is not truth. The test loop is missing entirely.

**Scope**

- Test plan per requirement: method, fixture, pass/fail, sample size.
- Fixtures and instrumentation: jigs designed in CAD, probe points, test software hooks.
- Measurement capture: serial/logic/scope data, photos, caliper and scale readings; import and link.
- Simulation vs measurement: compare, explain differences, update models.
- Defects, root cause, corrective action.
- Regression and retest after a change.
- An optional test review.

**Commands:** `test.plan.*`, `test.run`, `test.measure.import`, `test.compare`, `test.defect.*`,
`test.review`.

**Useful after:** 0011.

**Readiness (optional):** every must requirement tested; differences from simulation explained.

## 0013 — Release and compliance

**Why:** the product must be buildable and supportable by someone else.

**Scope**

- Freeze a revision; record what changed and why.
- Production documentation: drawings, instructions, test procedure, labels.
- Compliance evidence: RoHS/REACH, FCC/CE/UKCA, battery, packaging, safety; screening, not
  certification.
- Packaging and labels.
- Volume cost, sourcing, alternates.
- Release manifest: outputs, checksums, notes, build instructions, release notes.
- Git-friendly project, design diffs, comments, review before release.
- An optional release review. It never blocks export; open must requirements are listed, not enforced.

**Commands:** `release.freeze`, `release.manifest`, `release.verify`, `release.export`,
`compliance.*`, `review.*`.

**Useful after:** 0012.

**Readiness (optional):** a third party could build the product from the release.

## 0014 — Produce and handoff (optional)

**Why:** the boundary of the tool. It should prepare what production needs and accept feedback; it
should not become an ERP.

**Scope:** procurement handoff, assembly/QC/serialization plans, field feedback and revision intake.

**Readiness (optional):** handoff package is complete and round-trips a field issue into a new revision.

## Ordering

| Order | RFC | Unlocks | Detailed plan |
|---|---|---|---|
| 1 | 0005 Intent | Starting a product | this set |
| 2 | 0006 Concept | A feasible architecture | to write |
| 3 | 0007 Mechanical gaps | Buildable mechanical design | to write |
| 4 | 0008 Electronics | Schematic and board | `docs/electronics/` |
| 5 | 0009 Firmware | A working product | to write |
| 6 | 0010 Simulation | Evidence | `docs/simulation/` |
| 7 | 0011 Prototype/build | A physical prototype | to write |
| 8 | 0012 Test | Proof | to write |
| 9 | 0013 Release | A shippable product | to write |
| 10 | 0014 Produce | Handoff | to write |

Parallel work is allowed only where dependencies allow and the earlier phase's gaps are waived.
