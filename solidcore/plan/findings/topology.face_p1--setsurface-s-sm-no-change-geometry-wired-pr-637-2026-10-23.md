# topology.face_p1: `SetSurface`'s `SM_NO_CHANGE_GEOMETRY` wired (PR #637, 2026-10-23)

- **What.** The `UNPORTED(topology.face_p3): SmFace::Notify` stub in
  `topology::face::p1::set_surface` is gone. Its justification was the
  `topology.face_p3` "Stubs left for this unit" entry above (line ~7535): a face-owned
  surface has no object identity, so the data arguments cannot be formed. That no
  longer holds:
  - `set_surface` notifies `SM_NO_CHANGE_GEOMETRY` in the empty B-rep face environment,
    with null data. Upstream the two surfaces reach only `SmTopology::Notify`, which
    copies attributes from the old surface to the new one and calls the context
    callbacks; that environment has neither.
  - `SmBrep::ReplaceSurface` calls the new `set_surface_notified` with the caller's
    environment. `Surface` now carries a notification identity slot (as `Curve` does).
    The caller's context issues the keys for the new and the old surface.
  - A change tracker then records the same `ChangeGeometry` event as upstream: the
    face, the new surface, null (`SM_NO_GET_BREP`), and the old surface.
  - Without these identities, the brep_p3 tracker disagreed on all 47 `TurnToNURBS`
    cases: event 26 of the C++ side's 26 was missing, and so was the face's `Chg` entry.
- **Unmasked and fixed: extrusion split.** `Surface::split_at` returned `SM_ERR` for a
  surface of extrusion without a NURB, which is how `CopyAndAddAnalytics`' extrusion
  recogniser leaves it. Upstream `SplitAt` makes the NURB first
  (`GetOrCreateGwNurbPointer`). The port now splits a temporary copy with the NURB made
  (`SurfOfExtrusion::base_with_nurb`).
  - Found by face_p1 `Mouse.smb_0_f1_op9` (`MoveSeam`). The V pass split the U pass's
    joined surface, which had been recognised as an extrusion.
  - Before the fix, C++ returned status 0 with `MoveOutOfFace` in both directions;
    Rust returned `SM_ERR`.
- **Still deferred: geometry edit notifications.** Six `topology.brep_p3`
  `TurnToNURBS` cases are deferred to `unmodelled:geometry-edit-notify`:
  `nurbs_off_Mouse_0.1`, `nurbs_off_Mouse_-0.05`, `nurbs_off_toruswvy_0.1`,
  `gen0138`, `gen0164` and `gen0241`.
  - In these cases upstream geometry objects notify their own edits with
    `Notify(SM_NO_PRE_EDIT / SM_NO_POST_EDIT, this, owner, NULL)`. This happens on
    `SmBSplineCurve`, `SmLine`, `SmBSplineSurface` and others, for example on the curves
    `CreateUVTrimCurves` builds after `ReplaceSurface`.
  - The tracker records these events. Through the geometry's owner, they also become
    `Chg` entries of edges.
  - The port's geometry types send no edit notifications. Values seen:
    - `gen0164`: C++ has 32 events, including six `PostEdit`s of unowned geometry
      `-30 2..5`; Rust has 26.
    - `nurbs_off_Mouse_0.1`: the C++ `Chg` list has 6 entries and 34 events; Rust has 4
      entries and 20 events.
    - Everything before the tracker agrees.
  - The runner's `geometry_edit_deferral` reads the C++ tracker at the offset where the
    Rust tracker starts. It defers the case when that tracker holds a pre- or post-edit
    whose caller is geometry.
  - The fix is to give geometry edits notifications in the curve and surface units: an
    identity slot plus a context at each edit. This is a cross-unit job and has not
    been started.
- **Update (PR #661, 2026-10-23): known mismatches, not a C++-output deferral.** The
  runner no longer reads the C++ tracker to choose a deferral: `geometry_edit_deferral`
  and the `TurnToNURBS` tracker-offset trailer are removed, since difftest must not pick
  a deferral from the reference output. The six cases above now compare and fail on the
  tracker. They are listed in `crates/difftest/known_mismatches.toml`, so they still run
  and report as `known_mismatch`, and `topology.brep_p3` stays `partial`.
  - The real fix is unchanged: port the curve and surface `SM_NO_PRE_EDIT` /
    `SM_NO_POST_EDIT` notifications (an identity slot plus a context at each geometry
    edit). This is queued as follow-up work. When it lands, remove the six
    `known_mismatches.toml` entries (a listed case that passes fails the unit).


### blend.fillet_executive_p1 — standalone validation supersedes dependency assumption

The loader's current standalone fixtures pass without PR #596's edgeuse change:
89/89 executive p1, 267 passes / 8 deferrals p2, and 245 / 46 p3. Their complete
records match the prior actual-#596 integration. At `ed5331d30` on main
`e055c6fbc`, the standalone 260-unit sweep completed 554,897 cases with zero
failures. After main advanced to `1ba79426f` (including the fillet-law changes),
the nine affected loader/solver/intersector suites also pass, with all three
loader records unchanged. This supersedes the earlier statement that the loader
requires #596 or must remain blocked by that integration's offset failure.

The separate #596 integration's eight `gen0730` offset discrepancies remain
real. On the standalone sweep this case defers at `SmFace::GetTolerance`
(`topology.face_p2`, zero values compared); it is not presented as a passing
comparison or relabeled. No #596-owned source is included in this contribution.
