# RFC 0011: Prototype, CAM and build

| | |
|---|---|
| Status | Stub: scope fixed, detailed plan to write |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Maps to | phase P4 of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0007](0007-mechanical-detail.md)–[0010](0010-simulation.md) |
| Scope detail | [rfc-roadmap](../workflow/rfc-roadmap.md#0011--prototype-cam-and-build) |

## Summary

Turn the design into a physical prototype and a reproducible build package. Hand off to the tools
that already do their job: slicers, CAM senders, fabs. Do not rebuild them.

## Scope

- Prototype plan: what to prove, fidelity, process per part, cost and lead time.
- Mechanical outputs: print (slicer hand-off), laser, CNC, sheet, bought parts; orientation and
  material intent.
- Electronics outputs: PCB fab/assembly package, modules, breadboard options.
- Firmware build and flash in the package.
- Assembly instructions included.
- One-command build manifest: outputs, firmware, docs, checksums, revision, notes.
- CAM toolpaths for CNC/sheet where the existing CAM plan applies; export only.
- Handoffs: slicer, CAM sender, fab, assembly.
- Prototype review: what was learned, what changes.

## Four surfaces

Every command ships in the GUI, CLI, MCP server and documentation. The build manifest is a command,
not a dialog.

## Readiness (optional)

A complete prototype builds from the project with one command.

## Non-goals

- Slicer, printer firmware, CAM controller, G-code sender, ordering backend.
