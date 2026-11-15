# topology.face_p1 MoveSeam split cases over handles that name topology (2026-10-24)

Fixes the cause in "topology.face_p1 fork trap merged with main: 2 MoveSeam-with-split
mismatches" above. The runner's classification handle `H` named loops only, so the
seam problem `MoveSeam` recomputes after the move (`has_seam_problem_in_brep`, the face's
own `BrepSeamObjects`) failed `InvalidInput` and `move_seam` read that as upstream's
`FALSE`. `H` now names the B-rep's entities, the face surface, edge and UV trim curves,
the classify curves and the seam temporaries (the handle of the `heal.face_props`
runner), as upstream's `SmObject` pointers do. The classifications rebuilt from the
probe still hold no objects, so nothing before the post-move check changes.

- **Result.** `cone.smb_1_f5_op9` and `torus.smb_1_f5_op9` leave
  `known_mismatches.toml`. The check now finds the seam problem, the port splits, and
  both cases defer in the split (Rust-side `Unported`, no C++ output read): 2315 cases,
  2276 pass, 0 failed, 2 deferred, 37 rejected, 0 known mismatches (was 0 deferred,
  2 known mismatches).
- **cone** defers on `smsurf_GetSurfaceCache (trimmed surface cache history: tolerance
  growth)` (`surface.surface_p2`, the face-query tolerance check in
  `intersect::curve_class::p7`).
- **torus** defers on `SmEdge::GetTolerance` (`topology.edge`, green). This is not a
  back-edge into that unit: `split_at_seam_depth`'s `merge_intervals` gets the caller's
  `ClassGeometry`, and the runner's (like `trimming_tools`' `SeamGeometry` and the
  healer's `NoGeometry`) resolves nothing, because it cannot borrow the B-rep the call
  holds `&mut`. The handles the post-move `HasSeamProblem` made are `BrepSeamObjects`
  handles, so `point_obj_zone_tol_3d` falls through to `topology_of`'s `Unported`
  (`intersect/point_class.rs`, the `ClassObjectKind::Edge` arm). Checked by
  instrumenting the port (not committed): with `BrepSeamObjects::new(k, &*b, f)` as
  the geometry of both merges, the case reaches `SmCurveClassification::MergeInterval
  (imprinting)` (`intersect.curve_class_p6`) instead, the real blocker. Both are
  deferrals; only the attribution differs.
- **Library follow-up (not done here).** The split's merges should resolve the
  `BrepSeamObjects`-made handles against the B-rep's own objects, and the imprinting
  `MergeInterval` still needs a mutable B-rep context. Both hinge on `BrepSeamObjects`
  taking a mutable B-rep (planned separately), so this change stays in the runner.
