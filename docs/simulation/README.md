# Simulation

Status: draft. The proposal is [RFC 0010](../rfcs/0010-simulation.md); this directory holds the
detailed plan behind it. Nothing here is implemented yet.

The goal is to give someone on a normal laptop the engineering answers that usually need a
workstation or a specialist: *will this hold, how much will it bend, will it shake, how hot will it
get, where should the material go, and roughly how does air move around it* — computed locally,
offline, on the hardware they already own, with an honest statement of what the number does and does
not prove.

## Scope

| In | Out for now |
|---|---|
| Linear static stress, with bonded assembly interfaces | General nonlinear contact, plasticity, creep |
| Modal and linear buckling | Random vibration, response spectrum, acoustics |
| Steady thermal and thermal-stress coupling | Transient thermal, conjugate heat transfer |
| Fatigue (stress-life) | Fracture mechanics, crack growth |
| Topology optimisation (SIMP) | Shape morphing of B-rep features |
| Indicative external and internal flow (LBM) | Certified aerodynamics, engineering-Reynolds turbulence |
| Shells for sheet metal | Composites with progressive damage |
| Rigid-body dynamics UI over `crates/dynamics` | Flexible multibody, multi-body stress coupling |
| GPU acceleration with CPU fallback | Distributed or cloud solving |

Explicitly not claimed: certification, regulatory approval, flight/medical/safety-critical
sign-off, or wind-tunnel accuracy. The UI says *indicative* where that is the truth.

## Why in-tree

- One command surface, one undo history, one persistent face and edge naming system, one design file.
- One results viewer for stress, thermal, modal and flow.
- The parameter graph and configurations already exist, so parameter and topology optimisation is a
  driver over existing machinery rather than a second application.
- CAM and simulation need the same derived-result, invalidation, preflight, job and cancel
  substrate. It is built once.
- Topology optimisation and CAM share draw direction, minimum member size and stock constraints.

## Principles

1. **Correctness is the product.** A fast wrong number is worse than a slow refusal. Every solver
   ships with a benchmark and a convergence report before it ships with a button.
2. **The CPU f64 solver is the oracle.** Every GPU path is judged against it, never the reverse.
3. **Exact geometry decides, the mesh discretises.** Picks, supports, loads and couplings resolve
   through exact B-rep and persistent names. The analysis mesh is generated from exact geometry and
   is never the display mesh.
4. **Derive, never store stale.** A result is valid only for `hash(geometry + materials + boundary
   conditions + mesh settings + solver version)`.
5. **Refuse, never crash.** Bad input, ugly imports, exhausted memory and a lost GPU all produce
   named errors with a next step.
6. **Honest hardware use.** Use the GPU when it is faster *and* no less trustworthy; otherwise say
   so and use the CPU.
7. **Local and offline.** No cloud, no telemetry, no account. The browser build works offline.
8. **Explain the number.** Every result carries its assumptions, material sources, mesh quality,
   convergence, precision mode and warnings.
9. **Everything is a command**, with schemas, callable from the UI, CLI, control channel and MCP.
10. **One topic per PR.** Solver slices land validated, not as a monolith.

## Build order

```
M0 foundation (studies, commands, results viewer, dynamics UI, device survey)
 → M1 mesher → M2 static → M3 real parts → M4 shells (sheet metal)
                                          ├ M5 modal and buckling
                                          ├ M6 thermal and thermal-stress
                                          └ M7 fatigue
 → M8 topology → M9 flow → M10 parameter optimisation
```

Static, modal, buckling, thermal, thermal-stress and topology share the same mesh, assembly and
field pipeline. Only flow is a separate numerical core, so it can run in parallel. The full
milestones and exit gates are in [roadmap.md](roadmap.md).

## Documents

| Document | Contents |
|---|---|
| [requirements.md](requirements.md) | Users, numbered functional and non-functional requirements, definition of done, non-claims |
| [architecture.md](architecture.md) | Crates and layers, study and result data model, invalidation, commands, meshing, solvers, UI, format |
| [gpu-and-platforms.md](gpu-and-platforms.md) | Precision, kernels, adapter selection, unified memory, multi-GPU, watchdogs, wasm, platform matrix |
| [roadmap.md](roadmap.md) | M0–M10, dependencies, slices, exit gates, GPU and platform tracks |
| [testing.md](testing.md) | Test pyramid, benchmark catalogue with tolerances, cross-backend testing, CI matrix |
| [contributing.md](contributing.md) | RFC and PR process, format/locale/attribution rules, licence traps, risks, decision log |

## Open questions

1. **Sheet metal first?** Shell elements reuse the flat-pattern machinery and are a differentiator
   for this app. Put them in the first static release or after solid statics?
2. **Flow ambition.** Is indicative external flow with a stated validity range enough, or should
   flow stay an export/hand-off until a real turbulence capability exists?
3. **Results in the design file.** The plan says no: results are derived, can be large, and go
   stale. Confirm.
4. **Browser level.** Small solves in wasm, or results viewing only in the browser?
5. **Multi-GPU priority.** Concurrent independent jobs first, single-solve domain decomposition
   later. Confirm this ordering.
