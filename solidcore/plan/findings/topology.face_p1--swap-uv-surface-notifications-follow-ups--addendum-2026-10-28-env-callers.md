# topology.face_p1: SwapUV surface notifications, env-less callers addressed (2026-10-28)

Updates: plan/findings/topology.face_p1--swap-uv-surface-notifications-follow-ups.md

Item 1 (callers without a `NotifyEnv`) is addressed by PR #1380, with two limits that remain.

- `topology::brep::p1::swap_face_uv`, `topology::brep::p2::swap_uv`,
  `topology::brep::p9::orient_sheet_surfaces` and `heal::stitch::p1::Stitch::unify_normals`
  take a `NotifyEnv` and swap with `face::p1::swap_uv_with_notifications`.
  `heal::stitch::p1::api_unify_normals` (`SmApiUnifyNormals`) keeps its signature and
  notifies through the kernel context's callbacks (`brep::p3::with_kernel_callbacks`).
- The `topology.brep_p2` Swap op and the `topology.brep_p9` OrientSheetSurfaces op compare
  the topology change tracker's lists and events against the C++
  (`harness/cpp/shim/sc_tracked.h`). Disabling the surface notification fails exactly the
  cases that swap a face (38 in `topology.brep_p2`, 4 in `topology.brep_p9`).

Remaining:

- **Boolean Slice.** `boolean::merge::p4`'s `OrientSheetSurfaces` of the other operand
  notifies on records and attributes local to the call, without a context. The boolean holds
  no notification state for the other B-rep, so its attributes and the context's callbacks
  see none of those swaps (upstream's do). Fix: thread the boolean's notification state for
  the other operand to this call.
- **`heal.stitch_p1` comparison.** `UnifyNormals` now notifies, but the `heal.stitch_p1`
  runner does not compare its events yet (that runner is being reworked in #1297).
- **Multi-face `OrientSheetSurfaces`.** No `topology.brep_p9` fixture is a multi-face sheet
  with mixed face orientations, so the per-face swap branch (the marked walk over face-use
  neighbours) is not compared against the C++. All 4 swapping cases take the single-face
  `SwapUV` branch. Fix: add such a sheet fixture and compare its tracker events.

Item 2 (owner links of nested surfaces) is unchanged.
