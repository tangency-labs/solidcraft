# RFC 0008: Electronics (EDA)

| | |
|---|---|
| Status | Stub: renumbered full RFC, detailed plan written |
| Started | 2026-11-12 (renumbered from RFC 0005) |
| Apps | SolidCraft |
| Maps to | phase P2 electronics steps of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0006 Concept](0006-concept.md) |
| Detailed plan | [docs/electronics](../../docs/electronics/README.md) |

## Summary

Full EDA in-tree: library and parts, schematic and ERC, board layout and DRC, fabrication outputs
and simulation, sharing the design file, assembly, renderer and simulation substrate with the
mechanical CAD. The existing detailed plan is unchanged; only the RFC number moves.

## Scope

As [docs/electronics/README.md](../../docs/electronics/README.md): E0 library and vertical slice,
E1 schematic, E2 board, E3 fabrication, E4 simulation, E5 depth, E6 ecosystem, E7 polish.

## Four surfaces

Every `library.*`, `schematic.*`, `board.*`, `fab.*` and `sim.*` command ships in the GUI, CLI, MCP
server and documentation, with the same schema and tests.

## Readiness (optional)

E3 of the electronics plan: a complete two-layer board from schematic through fab package and
render.

## Note

This RFC supersedes the PCBCraft rows of RFC 0001. The CamCraft rows stand.
