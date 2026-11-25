# general: first STEP Tools corpus run, Rust vs C++ on 302 real B-rep bodies (2026-10-25)

PR #985 (`feat: clean-room STEP AP203/AP214 import for corpus testing`). `difftest
--step-corpus` imports the 75 STEP Tools AP203/AP214 sample files
(`scripts/fetch-steptools-corpus.sh`, not committed) with the clean-room
`crates/step-import` and builds every body on both kernels from the same encoded numbers:
`CreateCanonical` surfaces, bounded edge curves, `CreateFacesWithCurves` per face,
`SmApiStitchIntoSolid`, counts, `SmApiBrepIsManifoldSolid`, area and volume (accuracy
`1e-6`), `SmApiHealBrep`, then counts, manifold test and volume again. Rule: statuses and
counts equal, values within `1e-9` relative.

Result on `main` + this PR: 302 bodies in 71 files (4 files have no B-rep body; one body
uses an unsupported `HYPERBOLA`): **MATCH 177**, **HEAL-DIFF 101** (every build stage
agrees; the first difference is after `SmApiHealBrep`), **DIFF 8**, **DEFERRED 13**
(heal path; build stages agree before it), **CRASH 3** (C++ only).

## Build-stage differences (DIFF)

| file / body | stage | C++ | Rust |
|---|---|---|---|
| ap214/io1-ug-214 0 | area | 137183.7522791481 | 132210.955837231 |
| ap214/io1-md-214 0 | area | 265297.23952824157 | 261737.50813812987 |
| ap203/vs_training 0 | area | 324310.8671477748 | 325452.1442453766 |
| ap203/clevis22 0 | volume | 905990.105927068 | 905990.1109915222 (5.6e-9 rel) |
| ap203/moon_buggy 0 | stitch status | 0 (solid) | 1001 `SM_ERR` |
| ap203/moon_buggy_asm 9 | stitch status | 0 (solid) | 1001 `SM_ERR` |
| ap203/jack_in_the_box 25 | stitch solid flag | 1 | 0 |
| ap203/nasty_cheese 0 | faces made | 465 | 249 |

The stitch and face-count rows are places where Rust does worse than C++ (the owner's
"at least as good as C++" bar); not yet investigated. The area rows agree in every
earlier stage (same faces, edges, vertices), so they point at the area integration
(`SmApiBrepComputeArea` / `SmApiFaceComputeArea`) on these faces.

## Heal differences (HEAL-DIFF)

In 101 bodies everything up to and including the volume agrees and `SmApiHealBrep`
returns success on both sides, but the healed B-rep differs, mostly in its edge count,
Rust keeping fewer edges (e.g. `ap203/4pinplug` body 1: 9 edges in C++, 3 in Rust;
`ap214/boxy_with_*`: 241 vs 124). `heal.heal_data_p*` are green on their own fixtures,
so the real data reaches heal paths those fixtures do not.

To reach the healer at all, this PR wires `SmBrep::IsLiveTopologyMember` in
`heal::heal_data::p3::classification_has_dead_topology` (`HasDeadTopologyPointObject` got
a classification geometry that resolved no topology handle, so every call on real data
returned `Unported(topology.brep_p1)`; it now resolves `LoopObject::as_topology` to the
healed B-rep). `heal.heal_data_p3` difftest after the change: `PASS cases=480 passed=480
deferred=0`.

## Deferred (heal path)

10 bodies on `SmFace::GetTolerance` (`topology.face_p2`) and 3 on `SmBrep mutable owning
context (SmVertex::SetTolerance)` (`topology.brep_p1`), all inside `SmApiHealBrep`.

## C++ crashes

`ap214/as1-tc-214` bodies 2 and 4 and `ap203/tork` body 0 segfault in C++ in
`SmBezierAux2d::BuildPolarBox` (via `SmStitch::StitchIntoSolid` ->
`SmBrep::FindInfiniteRegion` -> `RayFire` -> `SmSurface::GlobalLineIntersect`), the
defect already logged in `plan/upstream-issues.md` (`SmSurfaceCache.cpp:1184`). The Rust
side completes these bodies.
