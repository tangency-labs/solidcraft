# Readiness checklists (advisory)

Part of the [Inventor's Toolbox RFC](../rfcs/0004-product-workflow.md).

These checklists help a person see what they may want before a step: choosing an architecture,
spending money on a prototype, or handing a product to someone else. They **report**. They do not
block, lock, require an approval, or become state that a command depends on.

## 1. The rule

- Readiness is **informational**. It lists what exists, what is missing and the tool that fills each
  gap.
- Save, export, prototype, fab output and release stay enabled at all times.
- "Not now" is a complete answer. Nothing is recorded unless the user asks to keep the report.
- No phase is required. No step is a prerequisite. A missing brief, concept, simulation or test
  creates no debt.
- The only refusals are technical: a mesh needs a body, a Gerber needs a board, a simulation needs
  a mesh. Those are facts about the input, not process gates.

A readiness report may be shown on a product home, opened from help, or produced by a command. It is
never a modal that must be answered.

## 2. What a report contains

For a chosen phase:

- **Present:** tools and artifacts that exist, with a link to each.
- **Missing or partial:** one line per gap, with the RFC or command that would fill it.
- **Suggested next:** up to three tools, phrased as suggestions ("you may want…"), not orders.
- **Not now:** a way to dismiss the report. It does not reappear unless asked.

Example:

```text
Concept — optional check
  present: concept assembly, board envelope, mass estimate
  missing: power budget (0006), interface list (0006), make/buy per part (0006)
  suggested next: power budget · interface list · print one bracket
```

## 3. Per-phase checklists

Each item is a question, not a requirement.

### Intent — optional

- [ ] Can you say the problem in one sentence?
- [ ] Do you know who it is for and what they will do with it?
- [ ] Is there at least one requirement you can test?
- [ ] Do you know the limits that matter: size, mass, power, cost, time, environment?
- [ ] Is there a risk you want to retire before spending money?
- [ ] Is there a safety or compliance question you should answer early?

Tool: the optional brief (RFC 0005). Skip it for a quick build.

### Concept — optional

- [ ] Do you know the main blocks: mechanical, electrical, firmware, power?
- [ ] Are the interfaces between them written down?
- [ ] Does the packaging fit the parts you expect to use?
- [ ] Do the rough mass, power, cost and size estimates fit the limits?
- [ ] Do you know which parts are made and which are bought?
- [ ] Is there an assembly layout, and can you reach the fasteners?

Tools: sketches, assemblies, block diagram, budget roll-up (RFC 0006).

### Detail — optional

- [ ] Do the parts exist, with the features the design needs?
- [ ] Do the assemblies fit, and are the fasteners reachable?
- [ ] Are the circuits and board complete, with ERC/DRC clean?
- [ ] Are the firmware pins and build in place?
- [ ] Are tolerances and fits known where they matter?
- [ ] Are drawings, BOM and assembly steps complete enough for someone else?

Tools: CAD, sheet metal, EDA, firmware, drawings, BOM (RFCs 0007–0009).

### Verify — optional

- [ ] Is there a reason to believe the weak point holds?
- [ ] Did the analysis converge, and what is its validity range?
- [ ] Do the electrical checks pass?
- [ ] Is there a hand calculation or reference to compare with?
- [ ] Is the remaining uncertainty acceptable for the next spend?

Tools: simulation, analysis, DRC/ERC (RFC 0010).

### Prototype — optional

- [ ] Do you know what this prototype is meant to prove?
- [ ] Is every part assigned to a process: print, laser, CNC, sheet, buy, PCB?
- [ ] Can the outputs be generated in one build?
- [ ] Does the firmware flash, and are the instructions enough to assemble it?

Tools: print/CAM/fab hand-off, build manifest (RFC 0011).

### Test — optional

- [ ] Does each must requirement have a test?
- [ ] Can you capture the measurement, not just look at it?
- [ ] If reality differs from the simulation, do you know why?
- [ ] Is the defect recorded with its evidence?

Tools: test plans, measurement import, sim-vs-test (RFC 0012).

### Release — optional

- [ ] Is the revision frozen and the change recorded?
- [ ] Are the drawings, instructions and test procedure complete?
- [ ] Are the compliance and safety notes present where they apply?
- [ ] Is packaging and labelling defined?
- [ ] Could someone else build it from the release?

Tools: release manifest, docs, compliance notes (RFC 0013). Export is never blocked.

## 4. Current tool coverage

This is a map of what exists, not a score. `partial` means some tools are there.

| Phase | Coverage | Largest gap |
|---|---|---|
| Intent | missing | No notebook or brief |
| Concept | partial | Assemblies and sketches exist; no architecture, interfaces or budget roll-up |
| Detail | partial | Mechanical is strong; EDA, firmware, drawings, tolerances and instructions are missing |
| Verify | partial | Motion and interference exist; simulation is planned; no tolerance analysis |
| Prototype | partial | Slicer/DXF/STEP hand-offs exist; no build manifest, firmware or instructions |
| Test | missing | No test plan, measurement capture or sim-vs-test |
| Release | missing | A mechanical BOM and exploded views exist; no release package |
| Produce | hand-off | Procurement, QC and serialization are external |

## 5. Proposed commands

```json
{"command":"workflow.status","params":{"product":"."}}
{"command":"workflow.readiness","params":{"phase":"P2"}}
{"command":"workflow.surfaces","params":{}}
```

- `workflow.status` returns the map and what exists, for the GUI, CLI, MCP and docs.
- `workflow.readiness` returns the advisory checklist for a phase, plus suggested next tools.
- `workflow.surfaces` reports which commands are reachable from the GUI, CLI, MCP and generated
  documentation.

None of them changes the design, blocks a command, or writes a "phase" into the file. They are
read-only. An optional product folder may store a report if the user asks.

## 6. What this is not

- Not a gate, a waiver system, a compliance record or a release approval.
- Not required for export, fab output, prototyping or release.
- Not a certification.
- Not a replacement for engineering judgement.
- Not a reason to nag. A dismissed report stays dismissed.
