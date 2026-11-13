# Kernel-aware Surface integration: PR #133 review follow-up (2026-10-16)

The review reproduced five missing-Kernel compile errors in a temporary merge
with main after #131 landed (bbox/degeneracy/curve point calls). This branch now
merges main and threads the existing caller Kernel through those sites without
changing intersection formulas, branches, result lifecycle or tolerances. The
review also found the inherited Apple portability prerequisites conflicted with
[the maintainer's policy](https://github.com/bherbruck/solidcore/pull/131#pullrequestreview-5543522828):
`build.rs` and both unrelated shared shim files are restored to main. Workflow
changes remain confined to the fork's disposable validation branch. Historical
Apple results above required those prerequisites and do not certify a clean
Apple build after their removal. The final GNU rerun is recorded below when
complete. No #71/#100 numerical implementation is imported. Common intersection
adapter errors now identify the remaining Surface integration rather than the
already-partial cone numerical unit.

The review also found `Surface::evaluate_simple` recursively delegates through
curve-bounded bases without the preflight used by the other common evaluators.
It now checks the same 64-level owned-geometry limit before dispatch; a 70-level
curve-bounded regression verifies normal `InvalidInput` rather than recursion.

The new native common-API corpus uses the shim's default `SmContext`. A Rust
regression uses zone tolerance 0.002 and checks that lazy/solver callbacks borrow
the caller Kernel and generic iso/STEP evaluation retains it; custom native
context settings are not differentially covered by this PR.

Concrete cone part 3 tests use its typed intersection targets. Wrapped-other
cone dispatch through `IntersectTarget::Surface` is not certified here; that
helper currently reaches the supplied general callback rather than recognizing
the new Cone variant as an analytic target. This remains part of the common
intersection adapter handoff, along with the explicit Surface method stubs.

### Moving-base integration during review validation

While the f804bc5f run was executing, main advanced through 8f6a9378 with
#128 (including heal.face_props) and #142's edge-use review fixes. The branch
merges that base and adapts six healing call sites to the borrowed Kernel. The
edge-use runner conflict is resolved by retaining both Kernel forwarding and
the incoming numerical gap arrays, exact-plane import and inexact-geometry
deferrals. Their numerical implementations are not duplicated or rewritten.

The completed [f804bc5f GNU run](https://github.com/Karl-VonRichter/solidcore/actions/runs/38884360383)
passes 183 units: 99 green / 84 partial, 466597 total cases, 453941 compared,
12656 deferred, zero failed. Sensitivity detects 453859/453859 perturbations,
with 82 empty outputs unperturbed. Workspace tests pass 1218 (1196 core), plus
eight focused FFI tests. These results certify f804bc5f only. The new 185-unit
run on 8cb9ff44 was cancelled at the user's pause request and does not certify the final merged revision.


### User pause (2026-10-16)

Work is paused with implementation 8cb9ff44 saved in draft PR #133. The
185-unit GNU validation run 38889088718 was cancelled at the user's request.
All required Rust checks, C++-feature Clippy and 1230 workspace tests (1208
core) pass on this revision. Its full native gate, sensitivity and focused FFI
tests remain incomplete; f804bc5f's completed 183-unit run is historical
evidence only. See STATUS for the exact handoff, artifact paths and resume steps.
