Updates: WR-510

#### WR-510 (update, 2026-11-13). The zone-tolerance tangency holds only when its tangent line lies in the face's sweep

With the flag removed (the addendum `flag-removed-cylinder-plane-tangent-within-tolerance`), `Cone::intersect_with_plane_owned` (`surface/cone/p3.rs`) took a plane within the zone tolerance of a cylinder face's wall as tangent and checked only that one tangent line against the face's sweep. A face whose sweep ends between the tangent line and a crossing then lost the crossing that lies in the face: a cylinder of radius 3 swept over 0.04°..0.06° only, cut by the plane `x = 3 - 1e-6` with a face zone tolerance of 1e-5, gave no curve instead of the crossing at +0.0468°. Bare surfaces and the flag-off path found it. Found in review of the change that retired the flag.

The zone-tolerance disjunct now also needs the tangent line in the sweep; otherwise each crossing is tested as for bare surfaces. Upstream's own tangency test (`|d| >= R`, or the two crossing lines within `tol` of each other) is unchanged, and so are bare surfaces.

- **Tests:** `issue1525_tangent_line_outside_sweep_keeps_crossing` asserts the crossing line at `(3 - 1e-6, sqrt(9 - (3 - 1e-6)^2))` for bare surfaces and with the zone tolerance on the plane's or the cone's face. `issue1525_plane_within_tolerance_of_wall_is_tangent` is unchanged. `docs/ssi.md` says when the crossings are tested instead.
- **Results:** no goldens row moves (all 283 units, measured at this change on the default kernel).
