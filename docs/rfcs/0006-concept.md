# RFC 0006: Concept — architecture, interfaces and budgets

| | |
|---|---|
| Status | Stub: scope fixed, detailed plan to write |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Maps to | phase P1 of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0005 Intent](0005-intent.md) |
| Scope detail | [rfc-roadmap](../workflow/rfc-roadmap.md#0006--concept-architecture-interfaces-and-budgets) |

## Summary

Turn a brief into a feasible architecture and layout before detailed design. The concept is where
mechanical, electrical, firmware, power and interface choices become one system, and where the
budgets are first tested.

## Scope

- System block diagram with blocks linked to parts, boards, modules and parameters.
- Interface definitions: connectors, pins, mounts, keep-outs, ownership; machine-checkable.
- Spatial packaging: envelope volumes, space claims, board/battery/connector placement, clearance.
- Mechanism concept: DOF, range, force/torque estimate, motor sizing, concept motion study.
- Electrical concept: power tree, rails, loads, efficiency, battery life, MCU/peripheral choices.
- Make/buy/process per part: print, laser, CNC, sheet, mould, buy, PCB; rough cost and lead time.
- Assembly concept: layout, fastening intent, serviceability, access checks.
- Budget feasibility: mass, power, heat, cost and time against the Intent targets.
- An optional concept review.

## Four surfaces

Every command ships in the GUI, CLI, MCP server and documentation, with the same schema and tests.

## Readiness (optional)

Architecture and interfaces defined; budgets within target or accepted as is; top risks have a plan; a
concept assembly exists.

## Open questions

1. Is the block diagram a new document or a view over parts and boards?
2. Do interface definitions live in the product, the design, or both?
3. How rough may a concept assembly be before it becomes detail?
