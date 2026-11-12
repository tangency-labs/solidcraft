# RFC 0010: Simulation and verification

| | |
|---|---|
| Status | Stub: renumbered full RFC, detailed plan written |
| Started | 2026-11-12 (renumbered from RFC 0004) |
| Apps | SolidCraft |
| Maps to | phase P3 of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0006](0006-concept.md), [0007](0007-mechanical-detail.md), [0008](0008-electronics.md) |
| Detailed plan | [docs/simulation](../../docs/simulation/README.md) |

## Summary

Evidence before spending money: structural static, modal, buckling, thermal and thermal-stress,
fatigue, topology, flow, mechanism forces, SPICE, SI/PI, tolerance and worst-case, reliability and
safety, with benchmarks and evidence traceability. The existing detailed plan is unchanged; only the
RFC number moves.

## Scope

As [docs/simulation/README.md](../../docs/simulation/README.md): M0 foundation through M10
optimisation, plus the four-surface requirement for every `sim.*` command.

## Four surfaces

Every `sim.*` command ships in the GUI, CLI, MCP server and documentation, with the same schema and
tests. A result that only the GUI can read is not done.

## Readiness (optional)

Every must requirement has evidence or a recorded decision to accept the risk; analyses record convergence and validity ranges.

## Note

The simulation plan's substrate (studies, fields, jobs, cancellation, GPU precision) is reused by
board thermal, PI/SI and the test loop.
