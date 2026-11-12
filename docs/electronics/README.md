# Electronics

Status: draft. The proposal is [RFC 0008](../rfcs/0008-electronics.md); this directory holds the
detailed plan behind it. Nothing here is implemented yet.

Electronics is the other half of the dream: a modern, integrated replacement for KiCad that shares a
design file, a parameter graph, an assembly, a renderer and a simulation stack with the mechanical
CAD. One program from schematic to ordered board to a rendered product in its enclosure.

## What "full EDA" means here

| Area | Included |
|---|---|
| Library | Parts, symbols, footprints, 3D models, SPICE/IBIS models, supply data, provenance |
| Schematic | Hierarchical sheets, buses, differential pairs, net classes, ERC, annotation, variants, BOM |
| Board | Stackup, footprints, pads, tracks, vias, zones, keep-outs, silkscreen, 3D body |
| Layout | Placement, interactive routing, length tuning, differential pairs, copper pours, panelisation |
| Verification | ERC, DRC, connectivity, schematic-to-board consistency, DFM |
| Simulation | SPICE, thermal, power integrity, signal integrity basics, coupled to the mechanical solvers |
| Fabrication | Gerber X2, Excellon, IPC-2581, ODB++, BOM, CPL, panel, fab profiles |
| Rendering | Board, copper, mask, silk, solder, components and enclosure in one render |
| Compatibility | KiCad project/library import and export, Eagle import, IPC-2581/ODB++ exchange |

Explicitly out for the first steps: full EM/EMC certification, high-frequency 3D electromagnetic
solving, rigid-flex and HDI depth, autorouting quality comparable to commercial tools, and a
provider/ordering network. Import/export and local libraries come first; the network is optional and
is not required for any of the above.

## Why in-tree

`docs/rfcs/0001-craft-bridge.md` proposed PCBCraft as a sibling app. The owner decision is the
opposite: electronics is a first-class part of SolidCraft. That is recorded in RFC 0008, which
supersedes only the PCBCraft rows of RFC 0001.

The reasons are concrete:

- One design file, so an enclosure, its board, its connectors and its parameters cannot drift apart.
- One assembly and one renderer, so the product shot includes the real board with real components.
- One undo history and one command surface, so an edit is an edit wherever it happens.
- One simulation substrate: board thermal and power integrity reuse the FEA and flow work.
- One parameter graph: board size, mounting holes, enclosure walls and component heights can be
  driven by the same expressions.
- The existing `.sc` container already embeds content-addressed blobs, which is exactly how part
  models and board assets should travel.
- The existing library pack proposal (#785–#787) already specifies immutable revisions and
  provenance for reusable definitions; parts are another entry kind, not a new system.

## Principles

1. **The schematic is the source of truth for connectivity.** The board follows it; a mismatch is
   an explicit, reviewable action, never a silent drift.
2. **Parts are immutable and embedded.** A design references exact part revisions and carries their
   symbols, footprints and models, so it opens offline years later.
3. **Import is the adoption path.** A tool that cannot open a KiCad project and its libraries will
   not replace KiCad. Compatibility is a first-class feature, not a plugin.
4. **Fabrication output is money.** A wrong Gerber costs a board and a week. Outputs are verified
   against independent parsers and real DFM rules before they are called done.
5. **The board is real geometry.** The mechanical assembly gets an exact board body, holes and
   component envelopes; renders get the detailed models.
6. **Everything is a command**, with schemas, scriptable and agent-drivable.
7. **Refuse, never crash.** Hostile files, huge boards, broken libraries and invalid rules produce
   named errors with a next step.
8. **One topic per PR**, and each milestone is a usable product, not a partial KiCad.

## Build order

```
E0 foundation and vertical slice
   library packs · part model · board body · electronics workspace · KiCad library import · render
E1 schematic capture
   sheets · symbols · wires/buses · ERC · netlist · annotation · BOM · KiCad schematic import/export
E2 board layout
   footprints · placement · routing · vias · zones · DRC · cross-probe · 3D board
E3 end to end
   Gerber/Excellon/IPC-2581 · BOM/CPL · DFM · panel · first ordered board
E4 simulation
   SPICE subset · thermal · IR drop/PDN · SI basics
E5 depth
   multilayer stackup · blind/buried vias · diff pairs · length tuning · variants · rigid-flex
E6 ecosystem
   library validation/authoring · templates · plugins · large-board performance · full KiCad round trip
E7 polish
   render fidelity · UX · docs · benchmarks
```

The first complete product — schematic, board, DRC, Gerber, render — is the E3 gate. Everything
after that deepens a working tool, not a demo.

## Documents

| Document | Contents |
|---|---|
| [requirements.md](requirements.md) | Numbered functional and non-functional requirements, definition of done, non-claims |
| [architecture.md](architecture.md) | Crates and layers, document model, schematic/board/library data, commands, revision control, undo |
| [library-and-formats.md](library-and-formats.md) | Part model, pack reuse, KiCad/Eagle/IPC compatibility, fab formats, licensing |
| [roadmap.md](roadmap.md) | E0–E7, slices, exit gates, integration and simulation tracks |
| [testing.md](testing.md) | ERC/DRC matrices, netlist and Gerber verification, SPICE benchmarks, compatibility corpus |
| [contributing.md](contributing.md) | RFC/PR process, repo rules, licence traps, risks, decision log |

## Open questions

1. **Board in the timeline?** The board body is derived from the electronics section. Should it
   appear as a timeline feature, an assembly component, or both (feature for rebuild, component for
   placement)? The proposal is both, with one owner.
2. **Variants.** Reuse mechanical configurations for board variants (DNP, alternate parts), or keep
   electronics variants separate with an explicit mapping?
3. **Browser level.** Full editing in wasm, or view/review only with desktop for heavy boards?
4. **Robot/router ambition.** Walk-around and shove in E2, or a simpler router first and quality
   later?
5. **KiCad version target.** Which KiCad file version is the compatibility contract for E1?

## Non-goals

- Certifying a board for safety, EMC or regulatory compliance.
- Replacing a fab's own DFM sign-off; the tool checks rules, the fab checks the order.
- Bundling manufacturer CAD or third-party libraries with incompatible redistribution terms.
- Matching every KiCad plugin on day one; the plugin API is the extension route.
