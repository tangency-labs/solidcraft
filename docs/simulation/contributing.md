# Simulation: contributing, process and risks

Part of the [simulation plan](README.md) and [RFC 0010](../rfcs/0010-simulation.md).

## 1. Before code: RFC and roadmap

1. Check open PRs and issues first (`CONTRIBUTING.md`, "Working as an agent"). Simulation may already
   have a claim.
2. Use **`docs/rfcs/0010-simulation.md`**, written in the RFC 0001/0002 shape:
   status, apps, discussion link, goals/non-goals, data model, commands, format, benchmark gates,
   non-claims, phased plan, open questions.
3. The RFC moves simulation out of "deferred" in `ROADMAP.md` and adds a Simulation milestone (the
   same way Manufacture/CAM has one). Update the "Where help is wanted most" section when the first
   slice is ready for contributors.
4. Land the RFC as a design PR with no behavior change. Then claim slices with draft PRs.

## 2. Claiming and slicing

- One topic per PR. A solver PR that also refactors the ribbon will not land.
- Claim with a draft PR early; the lowest-numbered PR wins a contested issue.
- Slice boundaries are the ones in [roadmap.md](roadmap.md); each slice must build, test and be useful alone.
- Independent slices (flow vs FEA, modal vs thermal) can run in parallel. Dependent slices stay
  sequential.
- Every PR states: what changed, why, what was measured, what was not verified, and the
  `Changes a default:` line.

## 3. New crates and layers

For each new crate (`meshing`, `fea`, `flow`, `sim-gpu`):

- A row in `TABLE` in `xtask/src/layers.rs` with its class and a comment.
- A row in `docs/architecture.md`.
- `[lints] workspace = true`, the workspace licence, and (for shared crates) a `README.md`;
  `cargo xtask shared` checks the shared ones.
- Run `cargo xtask layers`, `cargo xtask shared`, `cargo xtask wasm`.
- No `unwrap`/`expect`/`panic`/`unsafe` in non-test code; clippy `-D warnings`.
- No new solidcore seam: use `solidcore-ops`, `solidcore-tess`, `solidcore-record` only.

## 4. Format, strings, assets, commands

| Change | Rule |
|---|---|
| Study data in `.sc` | One `crates/doc/src/format/pending/studies.rs` with a `//!` note; never edit `FORMAT` or another change |
| UI strings | `assets/locales/pending/<slug>.json` |
| New assets/icons | Original or openly licensed; row in `attribution/pending/<slug>.md`; read `assets/AGENTS.md`; `cargo xtask assets`, `icons` |
| Commands | Input/output schema, or an entry in `schema/unconverted.txt` that only shrinks |
| Command list | `cargo xtask features` regenerates `docs/features.md`; never hand-edit |
| Preferences | Row in `preference_settings!`; old behaviour reachable; reset works; label/help through catalogs |
| Sorted lists | Message catalogs, `mod` lines, languages stay sorted (`cargo xtask lists`) |
| Tests | The area's existing file, not a per-bug file (#3226) |
| Generated files | Never edit `docs/features.md` or `THIRD-PARTY-LICENSES.txt` by hand |

## 5. Evidence

- UI changes: real screenshots or a recording captured by the contributor.
- Solver changes: benchmark table, convergence study, CPU/GPU comparison or an explicit skip.
- Performance claims: medians, range, machine, load, and the proof that geometry did not change
  (`docs/perf.md` pattern).
- Platform claims: the checklist in [testing.md](testing.md) §7, with the machines named and gaps stated.
- Run `cargo xtask ci` before every commit/push.

## 6. Licence ground rules

The repo forbids reading or linking GPL/LGPL/AGPL code. For simulation this rules out most of the
familiar ecosystem. Treat this list as a starting point and verify every dependency with
`cargo xtask licences --check` plus a manual read of the crate's licence:

| Project / crate | Usual licence | Status here |
|---|---|---|
| TetGen | AGPL | No — no mesh library, no reading |
| Gmsh | GPL | No |
| OpenFOAM | GPL | No |
| Code_Aster | GPL | No |
| CalculiX | GPL | No |
| Elmer | LGPL | No |
| SU2 | LGPL | No |
| FreeCAD FEM | GPL | No |
| FEniCS / dolfin | LGPL | No |
| deal.II | LGPL | No |
| MFEM | BSD-3 | Verify, likely usable |
| PETSc | BSD-2 | Verify, likely usable (native only; wasm concerns) |
| Eigen | MPL-2.0 | Verify, likely usable |
| `faer` | MIT | Verify |
| `sprs` | MIT | Verify |
| `nalgebra` | Apache-2.0 | Existing workspace dependency |
| METIS / graph partitioning | Check version licence | Verify before use |
| Triangle (2D) | Non-OSI, non-commercial terms | Avoid |

Implementation sources must be papers, textbooks and public specifications (e.g. the NIST RS-274/NGC
G-code spec is irrelevant here; the finite-element and LBM literature is not). Do not port code from
a GPL project, and do not read it "for reference".

## 7. Risks

| Risk | Why it matters | Mitigation |
|---|---|---|
| Meshing long tail | Real parts fail in new ways; this is the feature's ceiling | Stage the claim; refuse loudly; fuzz with real files; ship shells/simple solids first if needed |
| Numerical trust | A wrong stress number is worse than no number | f64 CPU oracle, benchmark gates, refinement reports, "verified" flag |
| GPU precision | Apple GPUs have no f64; f32 stress can mislead | Mixed precision, f64 residual/recovery, CPU fallback for ill-conditioned cases |
| Watchdogs and device loss | Long kernels kill the device or the tab | Batch < 50 ms, poll, cancel, retry on CPU, never crash |
| Memory on iGPUs | Preflight must not be a lie | Conservative budgets, chunked buffers, refusal before allocation |
| Multi-GPU complexity | Domain decomposition is a project of its own | Job-level parallelism first; single-solve decomposition behind a preference |
| Driver quirks | DX12 shader compile, Vulkan multi-GPU, software adapters | Capability checks, calibration, platform checklist, skip-with-reason in CI |
| Browser limits | wasm has no threads and a memory ceiling | Cooperative solver, small caps, WebGPU-when-available |
| Material data gaps | Fatigue needs S-N; some cards are incomplete | Refuse and name the missing property; extend the card schema with provenance |
| Contact/nonlinear scope | The most common "real" request and the most expensive | Explicitly out of v1; research spikes with go/no-go gates |
| Review bandwidth | Big solver PRs stall | Small validated slices, one topic each, benchmark evidence in the PR |
| Over-trust | Users treat indicative results as certified | UI non-claims, validity ranges, "what this does not prove" on every result |
| Licensing | One GPL dependency taints the project | Watchlist, `licences --check`, clean-room rule, paper-based implementation |

## 8. Decision log

| # | Decision | Rationale |
|---|---|---|
| D1 | Simulation is in-tree, not a sibling app | RFC 0002 precedent for CAM; one command/format/naming surface |
| D2 | Study inputs saved, results derived | Small files, no stale results, matches features/toolpaths |
| D3 | Pure L1 solver crates, L5 GPU backend behind an L1 trait | Layer rules; existing render job pattern |
| D4 | CPU f64 authoritative, GPU f32 accelerated | Portability (no shader f64 on Apple) and honesty |
| D5 | One device per problem for v1 | Multi-GPU job parallelism is the useful, simple win |
| D6 | Mesh from exact geometry, never the display mesh | Repo's mesh-decision rule; analysis accuracy |
| D7 | Solver ships with benchmark + convergence + non-claims | The project's evidence culture; prevents a fast wrong answer |
| D8 | macOS first, full CPU path everywhere | The owner's platform, but no platform loses features |

## 9. First three PRs

1. **RFC 0010** — scope, model, commands, gates. No code.
2. **Study envelope** — `sim.study.*`, format pending, undo, save/reopen, broken-name behaviour.
   Tests in the engine's area file. No solver.
3. **Dynamics UI** — the Simulation workspace renders existing dynamics results. Proves the viewer,
   the job path and the workspace before any new numerics.

Only then does the mesher PR start.
