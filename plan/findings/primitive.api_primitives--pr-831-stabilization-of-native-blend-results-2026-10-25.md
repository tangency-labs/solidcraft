# primitive.api_primitives — PR #831 stabilization of native blend results (2026-10-25)

The earlier draft's two kernel-fidelity blockers have source-backed corrections:

1. `SmApiPrimitives.cpp:1180` clears `rpBlendFace`; the face/edge
   `SmPrimitiveCreation::CreateBlendPrimitive` overload ignores the inner overload's
   status (`SmPrimitiveCreation.cpp:7558-7569`). Success/NULL is therefore a real
   outcome, not `NullPointer`. The Rust API now returns `Result<Option<FaceId>>`,
   preserving success/Some, success/None and failure separately, including the
   mutated B-rep record. Repository callers were inspected and adjusted.
2. `SmSurface.cpp:15443-15453` deliberately ignores both boundary-sampling
   statuses. `GetSampleValues` sizes arrays before filling them; `SmTArray::SetSize`
   (`SmTArray.h:525-545`) uses calloc/memset, so unwritten slots are zero, not the
   `SmVector3d` default constructor's UNDEF. The safe status-aware sampler now
   returns the valid prefix, zero-filled remainder and status separately; the blend
   builder ignores only the ordinary status, as upstream. The public fallible
   sampler still reports failure; Unported and count-safety failures propagate.
   Sample counts are checked and bounded at 65,536 intervals before padding.

A standalone probe linked to the existing C++ reference confirmed the actual
behavior on a unit box (edges 0/1, face 1, same direction): with four sampling
intervals, `GetSampleValues` returns 1001 after four valid samples and leaves the
last position/derivative zero while retaining parameter 1. The high-level API
still succeeds with a non-null face and seven total faces. Probe source and raw
output are retained under `bin/evidence/pr831`. No upstream library was rebuilt.

The two E0277 harness compile errors are corrected by explicit, local error
translation for box edge/face enumeration; no global conversion was added.
The original nine fixed inputs and the broad generator are unchanged. Two
explicit success/Some and success/None fixtures were added, not substituted.

Focused evidence on the redesign over authoritative main `c18bb3d6c`:

- Nine kernel tests passed: the sampler's eight tests, including failures at the
  first/interior/last sample and extreme count guards, plus the API contract test
  covering success/Some (including adjacent edges), success/None and invalid
  association errors.
- Native fixed blend cases: 11/11 substantive passes, zero failures/deferrals,
  maximum relative error 0. Perturbation: 11 changes detected out of 11.
- `cargo clippy -p solidcore --all-targets -- -D warnings` passed.
- The full local primitive unit reached its 60-second budget and the harness
  all-target Clippy check reached its 90-second budget. Both are incomplete,
  not passes, and neither was repeated. Canonical exact-head CI owns the complete
  optimized unit, C++-feature lint, wasm and full comparison gate.

This supersedes the earlier harness-only WIP scope and blocker attribution, not
its saved failure evidence. Numeric formulas and operation order in the sampling
loop are unchanged. No failing fixture was removed, no generator narrowed, no
tolerance relaxed and no skip/rejection added. The previously documented separate
macOS partial-cylinder observation is not treated as repaired by this change.
