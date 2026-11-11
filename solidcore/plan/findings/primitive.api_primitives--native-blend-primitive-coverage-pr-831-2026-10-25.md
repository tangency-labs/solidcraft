# primitive.api_primitives — native blend-primitive coverage (PR #831, 2026-10-25)

Native `SmApiCreateBlendPrimitive` (op 26 of the unit's shim: a `SmApiCreateBox`, then the
API on edge/face list indices) is now difftested. Two divergences block full association
coverage; neither is fixed here (the kernel is untouched by this PR):

- **Success with a NULL out face is not representable in the Rust API.** For some valid
  associations the C++ returns `SM_SUCCESS` with `rpBlendFace == NULL` and leaves the
  B-rep unchanged; for example box `1x1x1`, `pEdge1` list index 0 on `pFace1` index 1 and
  `pEdge2` index 3 on `pFace2` index 2: C++ record `[0, 0, 6, 12, 8, ...]` (361 values),
  Rust `create_blend_primitive` returns `SolidError::NullPointer` (1006) because the
  wrapper maps `blend_face == None` to an error
  (`primitive::api_primitives::create_blend_primitive`) and its signature is
  `Result<FaceId>`. Upstream distinguishes success/NULL from failure, so the public API
  needs an `Option<FaceId>` out face (a signature change, not made here). Inputs in this
  class are not difftested.
- **Adjacent/selected edge pairs fail in `DerivSurfDefinition::sample_values` where the
  C++ builds a face.** Examples on box `1x1x1`: edges list indices 0+1 (both on face 1,
  meeting at a vertex) and 0 on face 3 + 1 on face 5; also 0 on face 1 + 4 on face 0.
  Rust: `Upstream { code: 1001 }` from `surface::surface::p6::DerivSurfDefinition::sample_values`
  (via `create_edge_edge_blend`); C++: `SM_SUCCESS`, 7 faces / 16 edges and a blend face
  (669 or 648 values). Reproduced before merging `upstream/main` (the pre-merge brute
  force had no success for 0+1), so not a regression from that merge. Other associations
  (0+2, 0+3, 1+3, 0+4, 6+8, 0+6 with specific faces) compare bit-identically
  (`max_rel_err = 0`) when both sides build a face, but the fixture and generator keep
  the original broader associations so the failing classes stay visible. The PR is a
  draft WIP until the kernel-side callers are fixed.
- **Merged-main local failure, unrelated to this PR:** generated case `gen1183`
  (`partial_cylinder`, not a blend case; its generator arm and Rust/C++ paths are
  untouched by this PR) compares vertex tolerances `1.0000000010658142e-5` (C++) vs `1e-5`
  (Rust), and `1.0000000030126803e-5` vs `1.0000000003552714e-5` at record indices 24/28
  (relative 1.07e-9 and 2.66e-9, just over the 1e-9 gate). It appears after merging
  `upstream/main` d19b28a09 (#803 primitive-model wiring) onto 6416ae183; the canonical
  Linux exact-head CI verdict is authoritative.
