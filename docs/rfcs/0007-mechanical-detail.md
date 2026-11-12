# RFC 0007: Mechanical detail gaps — assembly, tolerances, drawings, BOM

| | |
|---|---|
| Status | Stub: scope fixed, detailed plan to write |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Maps to | phase P2, mechanical and documentation steps of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0006 Concept](0006-concept.md) |
| Scope detail | [rfc-roadmap](../workflow/rfc-roadmap.md#0007--mechanical-detail-gaps-assembly-tolerances-drawings-bom) |

## Summary

Mechanical modelling is strong; the outputs that make a design buildable are not. Assembly design
itself is already here and in Concept. This RFC adds tolerances, drawings, instructions, the
cross-domain BOM and the missing assembly capabilities around them.

## Scope

- Toleranced dimensions, geometric controls, worst-case and RSS stack-up, fit analysis.
- Part drawings and assembly drawings with BOM balloons and sections; PDF/DXF.
- Assembly instructions from exploded views: steps, callouts, tools, torque, adhesive, inspection.
- Cross-domain BOM: PCB, components, firmware, packaging, consumables; cost, lead time, MOQ,
  alternates, availability.
- Variant BOM and DNP across schematic, board, BOM and CPL.
- Flexible/deformable parts and cable routing.
- Harness and cabling: wire list, lengths, gauges, colours, labels, bend radius.
- Fastener torque, preload, engagement and tool access.
- Test/inspection points and serialization marks.
- DFM/DFA per production process.
- An optional detail review.

## Four surfaces

Every command ships in the GUI, CLI, MCP server and documentation, with the same schema and tests.

## Readiness (optional)

All parts documented; BOM and drawings complete; instructions and manufacturing intent exist.
