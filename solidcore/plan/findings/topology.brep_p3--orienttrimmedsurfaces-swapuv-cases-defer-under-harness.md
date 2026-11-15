# topology.brep_p3 — `OrientTrimmedSurfaces` SwapUV cases defer under `harness:surface-notify-identity` (PR #204, 2026-10-21)

- **Root cause.** `SmFace::SwapUV` calls `pSurface->SwapUV()`. `SmBSplineSurface::SwapUV`
  rebuilds the surface with `SetCanonical`, which notifies `SM_NO_PRE_EDIT` /
  `SM_NO_POST_EDIT` on the surface itself (`SmBSplineSurface.cpp:4585` / `:4592`,
  `(this, SM_NO_GET_OWNER(this) = NULL, NULL)`). `SmOffsetSurface::SwapUV` forwards to its
  base surface, and `SmSrfInVolume::SwapUV` notifies itself. So every upstream swap
  sends a geometry `POST_EDIT`. The tracker records it and, through the surface's
  owner, puts the face in its `Chg` list. A Rust `Surface` has no notification
  identity (a `Curve` has its `IdentitySlot`), so the port cannot send that event.
  Found when #364 (#239 `topology.face_p4`) let the orient fixtures reach `SwapUV`: 31
  `orient_*_swap0` cases, 29 with one swap and 2 with two, differed only in the
  tracker's event list (`orient_Mouse_swap0`: C++ 577 values, Rust 557, i.e. one
  `Chg` entry and one geometry event per swap).
- **Fixed in the same round.** The face's own `PRE_EDIT` / `POST_EDIT` now go through the
  caller's environment (`face::p1::swap_uv_with_notifications`) and carry
  `(this, NULL, NULL)`: `SM_NO_GET_BREP(this)` is a null pointer (`SmTypes.h:659`).
- **Deferral.** The `topology.brep_p3` runner defers an `Orient` case exactly when
  `SmFace::SwapUV` ran (the face's `POST_EDIT` is in the tracker's events). Orient cases
  that swap nothing are still compared in full. The whole case defers: its face and
  topology values share one output vector with the tracker's lists, and no case
  boundary separates them.
- **Follow-up.** Co-port a notification `IdentitySlot` onto `Surface` (owned by
  `surface.surface_p1`, as `Curve`'s). Send the surface's `PRE/POST_EDIT` around the
  surface swap in `swap_uv_with_notifications`. Have the runner issue each face
  surface's identity before the op, with the face as owner for the tracker's owner
  walk (as it does for edge curves). Then remove the deferral.
- **Update (re-check of `766472a1`).** The whole-case deferral was decided from the
  Rust side alone, so a Rust-only wrong swap would have been hidden, and the 31 cases
  compared nothing. `harness:surface-notify-identity` is gone. Both the shim and the
  runner now end an `Orient` output with a trailer: each pre-op face's `SmFace::SwapUV`
  `POST_EDIT` count from that side's own tracker events, the face count, and the
  tracker's offset. The runner splits it into `out` (status, flag, face domains and
  upward face uses, counts), `swapped` and `tracker`. `out` and `swapped` are always
  compared. Only a side whose own events show a swap leaves out `tracker`, so a swap on
  one side only fails on `swapped`, and on `tracker`'s presence too.
- **MakeWireEdge (above): Shared is wireable.** `MergeContext::Shared(&mut Kernel)` does
  carry a mutable kernel, so that path could be wired now. Only `Split` needs the
  signature change. Both are left for the follow-up.
**Resolved (wire/curve-class-p4, PR #365):** both back-edges are now wired.

- The 4 `ConnectIntervals` disagreements are upstream undefined behaviour, not a port
  bug. Each case squeezes the only interval out of a classification (`ConnectIntervals`
  removes it, leaving size 0) and then runs `GetLoopCrossings` over loops, whose
  `GetPointClassifiesConnectedTo` reads `m_pData[0]` of the empty classification.
  Upstream reads a stale or missing interval there, while Rust reports `InvalidInput`.
  The p4 shim now rejects `GetLoopCrossings` over loops on an empty classification.
- `merge_plane10_line` defers as `topology.brep_p2`
  (`MergeCurvesOnSurface: topology handles of the merged B-rep`), because the brep_p2
  runner's `MergeObjects` resolves only the case curves.
  `merge_curves_on_surface` holds the B-rep as `&mut`, so no `ClassGeometry` adapter can
  lend it to `topology_entity` without a change to brep_p2's objects trait.
