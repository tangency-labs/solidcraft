# RFC 0004: The Inventor's Toolbox

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Position | From a mental idea to a physical product, without a forced process |
| Replaces | the numbering of RFC 0004 (simulation, now 0010) and RFC 0005 (electronics, now 0008) |
| Related | [0005](0005-intent.md), [0006](0006-concept.md), [0007](0007-mechanical-detail.md), [0008](0008-electronics.md), [0009](0009-firmware.md), [0010](0010-simulation.md), [0011](0011-prototype.md), [0012](0012-test.md), [0013](0013-release.md) |
| Detailed plan | [docs/workflow](../workflow/README.md) |

## Summary

SolidCraft is becoming the **Inventor's Toolbox**: one free program that takes a person from a
mental idea to a physical product. Modelling, sheet metal, assemblies, renders and the planned
electronics and simulation work are the tools. What is missing is not a process engine; it is a
**map of the journey and the tools at each point** — and the tools themselves.

This RFC defines that map: Intent, Concept, Detail, Verify, Prototype, Test, Release, Produce. It is
**documentation and an optional in-app guide**, not a state machine. Every tool is available on its
own, every step is skippable, and nothing refuses to save, export, prototype or release because a
previous step was not done. The map exists so a new inventor can see what is possible and what is
missing, and so an experienced user can jump straight to the tool they need.

The first tool is [0005 Intent](0005-intent.md): an optional notebook for the problem, requirements,
budgets, risks and decisions. It is useful. It is not required.

## 1. The problem

- The tools are islands. A user cannot see the whole journey or what is missing from it.
- There is no place to jot down why a thing is being built, what it must do, or what it must cost
  when the user wants to.
- Cross-domain truth is missing: mass, power, heat, cost, connector maps, test evidence.
- The product often ends at a design file: no build package, no test record, no release note.
- New users do not know what to try next; experienced users cannot find the tool they know exists.
- The answer is not a mandatory wizard. The answer is discoverable tools and an honest map.

## 2. Goals and non-goals

**Goals**

- Position SolidCraft as the Inventor's Toolbox: idea to product, one program, free and offline.
- Document the whole journey and every missing tool, with one line per gap and a home RFC.
- Make every tool independently reachable and skippable.
- Offer optional notebooks and readiness views that help without blocking.
- Keep a product folder for people who want one, without making it mandatory.
- Ship every capability from the GUI, CLI, MCP server and documentation.
- Start with Intent because it is cheap and useful, not because it gates anything.

**Non-goals**

- A workflow engine, stage gates, mandatory phases, locks or forced wizards.
- A mandatory project container or a required brief.
- Rebuilding a slicer, CAM controller, G-code sender, IDE/compiler, ERP, PLM, cloud or ordering
  backend. Export and hand off.
- Certification, EMC testing or regulatory sign-off.
- A giant requirements list. The map carries one-line gaps; each feature RFC carries its detail.

## 3. Decisions

| # | Decision | Reason |
|---|---|---|
| D1 | The journey is a **map**, not an enforced workflow | Inventors jump around; the tool must not get in the way |
| D2 | Every tool is **independent and skippable** | A quick bracket should not require a brief, a concept review or a test plan |
| D3 | The product folder is **optional**; a single `.sc` stays first-class | One person with one part should never be forced into a project structure |
| D4 | The **spine is documentation**: the map, the help pages and an optional readiness view | Guidance without enforcement |
| D5 | **Assembly sits in Concept and Detail.** Concept owns layout and mechanism; Detail owns assembly design, fastening and fits. Release only consumes instructions, drawings and BOM rows | Assembly is design work, not a release artifact |
| D6 | The RFC order follows the journey, but features may be built in any useful order | Dependencies are technical, not procedural |
| D7 | Simulation and electronics are **renumbered**, not rewritten | Their detailed plans are sound; only the top-level story changes |
| D8 | No slicer, IDE, ERP or certification | Those tools exist and are not the differentiator |
| D9 | Every capability ships on **four surfaces**: GUI, CLI, MCP and documentation | One command registry, no second implementation |
| D10 | Budgets are optional cross-domain data; when present, they compare targets with measurements | Useful, never a prerequisite |
| D11 | Readiness checklists **report, never block** | Export, save, prototype and release stay available at all times |

## 4. The journey

```
Intent ─► Concept ─► Detail ─► Verify ─► Prototype ─► Test ─► Release ─► Produce
   ▲          ▲         ▲          ▲          ▲          ▲          ▲
   └──────────┴─────────┴──────────┴──────────┴──────────┴──────────┘
              jump anywhere; every step is optional
```

| Phase | What it is for | Tools that help | You can skip it when… |
|---|---|---|---|
| P0 Intent | Remember the problem, the limits and the decisions | Optional brief, notebooks, templates, budgets, risk list | You are exploring or building something small |
| P1 Concept | Choose an architecture and a layout | Sketches, assemblies, block diagrams, power/mass/cost estimates | The design is obvious or already decided |
| P2 Detail | Make the parts, circuits, firmware and documents | CAD, sheet metal, EDA, firmware integration, drawings, BOM | Never fully — this is the design itself |
| P3 Verify | Build confidence before spending money | Simulation, analysis, DRC/ERC, tolerance checks | A prototype is cheaper or the risk is low |
| P4 Prototype | Make one and learn | Print/laser/CNC hand-off, PCB fab, build manifest | You are only modelling or already have the part |
| P5 Test | Compare reality with the design | Test plans, measurement capture, sim-vs-test | The product is trivial or already proven |
| P6 Release | Make it buildable by someone else | Drawings, instructions, compliance notes, manifest | It is a one-off for yourself |
| P7 Produce | Hand off to manufacturing | Procurement and QC hand-off | You build every unit yourself |

The full phase → step → tool tree is in [map.md](../workflow/map.md). Status there means "does the
tool exist", not "has the user complied".

## 5. No forced workflow

These are the rules the implementation must keep:

1. **Every tool has a direct entry point.** The ribbon, command palette, help and MCP can reach it
   without visiting another phase.
2. **No tool is a prerequisite for another.** Meshing does not require a brief; a board does not
   require a concept review; export does not require a test plan.
3. **No modal phase.** The start page offers templates and a blank design equally.
4. **Skipping is normal.** A missing brief, budget, simulation or test produces no debt, no lock and
   no nagging dialog.
5. **Readiness reports, never blocks.** A readiness view may list gaps and suggest the next useful
   tool. Save, export, prototype, fab output and release stay enabled.
6. **The map is help, not state.** No required phase is stored in the design. The optional product
   folder may record what the user chose, but the design opens and works without it.
7. **Agents follow the same rule.** MCP tools are discoverable, but an agent cannot lock a step or
   refuse a command because a phase is incomplete.
8. **Technical refusals stay.** A mesh still needs a body; a Gerber still needs a board. Those are
   facts about the input, not process gates.

The only checklists are **optional design reviews**: a list of things worth checking, with links to
the tools that check them. They are useful before spending money, and they never stand in the way.

## 6. Optional readiness

A readiness view is a convenience, not a gate:

- It lists what exists and what is missing for a chosen phase.
- It links each gap to the tool that fills it.
- It can be exported as a design-review note.
- It never changes the design, never blocks a command and never requires an approval.
- "Not now" is a complete answer. The view remembers it only if the user asks it to.

The detailed, still-advisory checklist is in [readiness.md](../workflow/readiness.md).

## 7. The four surfaces

Every capability, including the optional notebooks and the readiness view, ships from:

| Surface | Requirement |
|---|---|
| **GUI** | Reachable in the relevant workspace, panel, menu or command palette; progress and cancel where long; no command depends on a dialog being open |
| **CLI** | `solidcraft-cli exec <id> '<json>'` with the same input and output schema; named failures and a non-zero exit |
| **MCP server** | Listed as a tool with parameter docs and output schema; callable headless or bridged |
| **Documentation** | Generated command reference from the schema (`cargo xtask book`), a `docs/features.md` row, and a human guide/help page |

The control channel uses the same command surface as the CLI. A command without all four is
`partial`, not done. Existing commands are audited too: `workflow.surfaces` can report missing
surfaces, and the gaps become issues. Updating all four is part of finishing a tool, not a follow-up.

## 8. The product folder (optional)

A single `.sc` file remains a complete way to work. For a multi-domain product, a folder can hold
everything together:

```text
my-product/
  product.json          optional manifest: name, revision, links, optional targets
  brief/                optional notebooks and decisions
  design.sc             mechanical + electronics (one design document)
  firmware/             source, board definitions, build config
  docs/                 drawings, instructions, test plan, compliance notes
  bom.csv               one BOM, mechanical + electrical + packaging
  tests/                measurements, logs, photos, results
  releases/v1/          manifest, outputs, checksums, release notes
```

`product.json` is optional. When it exists, it may carry targets and links; the design's parameters
carry model-driven values, and a budget view can compare the two. Nothing in the folder replaces the
`.sc` format. Opening `design.sc` directly must always work.

## 9. What it looks like

One path through the toolbox for a battery-powered plant sensor. It is an example, not a
prescription; any step can be skipped or revisited.

| Phase | What the person does | What the toolbox offers |
|---|---|---|
| Intent | Jots the problem, six-month battery, IP65, under $40, 80×60×30 mm; maybe marks battery life and antenna range as worries | An optional notebook, templates, budget fields and a risk list; a blank design if they prefer |
| Concept | Blocks the ESP32-C3, sensor, 18650, boost, gasket and shell; picks module vs custom PCB | Sketches, a concept assembly, optional block diagram, power and mass estimates |
| Detail | Models the shell and probe, designs the board, writes firmware, sets fits and tolerances | CAD, EDA, firmware integration, drawings, BOM |
| Verify | Runs a battery-life calculation, a regulator thermal check, SPICE on the front end, a gasket stack-up | Simulation, analysis, DRC/ERC, tolerance checks; all optional |
| Prototype | Prints the shell, orders the PCB and parts, flashes firmware, assembles | Print/fab hand-off, one build manifest, instructions |
| Test | Measures sleep current, range, moisture accuracy, drop and leak | Test plans, measurement import, sim-vs-test comparison |
| Release | Freezes revision B, checks notes and packaging, exports | Drawings, instructions, compliance notes, manifest; export is never blocked |
| Produce | Orders ten, tests each, records serials, receives a field issue | Hand-off package; a field issue becomes a revision |

The product home, when a product folder exists, is one screen: the map, the tools, what exists, and
the next useful action. Without a folder, the same tools are still in the ribbon, palette, CLI, MCP
and help.

## 10. RFC roadmap

Each RFC is one missing tool or group of tools, in the order the journey suggests. The order is
advice, not a dependency: implement what is useful first.

| RFC | Title | Useful before | Technical dependency |
|---|---|---|---|
| [0005](0005-intent.md) | Intent: optional brief, requirements, budgets, risks, decisions | Anything you want to remember | — |
| [0006](0006-concept.md) | Concept: architecture, interfaces, make/buy, budgets | Detailed design | 0005 (if used) |
| [0007](0007-mechanical-detail.md) | Mechanical detail gaps: tolerances, drawings, assembly outputs, cross-domain BOM | Prototyping or release | 0006 (if used) |
| [0008](0008-electronics.md) | Electronics (EDA), renumbered from old 0005 | A board | 0006 (if used) |
| [0009](0009-firmware.md) | Firmware and testability | A working product | 0008 |
| [0010](0010-simulation.md) | Simulation and verification, renumbered from old 0004 | Spending money | Design and materials |
| [0011](0011-prototype.md) | Prototype, CAM and build | A physical prototype | Design and fab data |
| [0012](0012-test.md) | Test and measurement | A release | A prototype, if tested |
| [0013](0013-release.md) | Release and compliance | Production | Documentation |
| [0014](0014-produce.md) | Produce and handoff (optional) | Manufacturing | Release data |

## 11. What this adds that no existing plan covers

1. **A map** of the journey and every missing tool, in one place.
2. **Intent** as an optional notebook, with help and templates.
3. **Cross-domain budgets** (mass, power, heat, cost, time) when the user wants them.
4. **Firmware and testability** as part of the product.
5. **The test loop**: measurement, sim-vs-test, defects.
6. **Release**: drawings, instructions, compliance notes, manifest.
7. **Discoverability**: what exists, what is missing, and the next useful tool.

## 12. Open questions

1. **Product folder.** Optional folder/repo with `product.json`, plus a zip export? A single `.sc`
   must stay first-class.
2. **Readiness.** A command and an optional panel, always advisory? No hard stops anywhere?
3. **Intent location.** In the design file, the optional product folder, or both?
4. **Guidance.** How much should the start page suggest without nagging?
5. **Browser.** Can open a design and a read-only product view; heavy editing stays desktop?
6. **Firmware depth.** Board definitions, build and flash integration only, or an editor too?

## 13. Detailed plan

| Document | Contents |
|---|---|
| [workflow map](../workflow/map.md) | Phase → step → tool, status and RFC home |
| [readiness](../workflow/readiness.md) | Advisory checklists, no gates |
| [0005 Intent](0005-intent.md) | Optional brief, requirements, budgets, risks, decisions, help |
| [RFC roadmap](../workflow/rfc-roadmap.md) | 0006–0014 scope and technical dependencies |
| [docs/simulation](../../docs/simulation/README.md) | Detailed simulation plan (becomes RFC 0010) |
| [docs/electronics](../../docs/electronics/README.md) | Detailed electronics plan (becomes RFC 0008) |
