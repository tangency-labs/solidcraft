# RFC 0014: Produce and handoff (optional)

| | |
|---|---|
| Status | Stub: scope fixed, detailed plan to write |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Maps to | phase P7 of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0013 Release](0013-release.md) |
| Scope detail | [rfc-roadmap](../workflow/rfc-roadmap.md#0014--produce-and-handoff-optional) |

## Summary

The boundary of the tool. Prepare what production needs, accept what production learns, and stop
there. Procurement, QC and serialization are external systems; SolidCraft exports and imports their
data instead of becoming an ERP or MES.

## Scope

- Procurement handoff: BOM, supplier references, quantities, alternates.
- Assembly and QC plan, serialization ranges, test records.
- Field feedback and revision intake.
- Round-trip a field issue into a new revision with evidence.

## Four surfaces

Every command ships in the GUI, CLI, MCP server and documentation.

## Readiness (optional)

Handoff package complete; a field issue can become a revision without losing its evidence.

## Non-goals

- ERP, MES, warehouse or ordering backends.
- Automated purchasing.
