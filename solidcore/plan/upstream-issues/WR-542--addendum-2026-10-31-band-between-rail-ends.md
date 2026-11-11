Updates: WR-542

#### WR-649 (WR-542 addendum, 2026-10-31). The band check sampled each rail past its end vertices, so valid rounds were refused as no room
- **Severity:** Wrong result (a valid fillet refused, `NoRoom`), only with the opt-in
  `fillet_refuse_overlapping_bands`
- **Area:** Fillets
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp`, `SmFilletExecutive::BandsClearOtherRails`
  (the fork's check from tangency-labs/smlib@c0d74ee; not in the pinned SMLib).
- **What happens:** the check runs before the rails are trimmed. A rail runs on past the
  corners that end it, as far as a mitre needs and beyond. Sampling each rail over its whole
  curve tests points beyond its ends. Rounds meeting at a face's concave corner are mitred
  (`fillet_mitre_concave_face_corners`, WR-642). So on SolidCraft's #871 part, the
  plate-top round's rail on the end face (z 9.5) ran on past the shoulder mitre's rail-rail
  point (y 13.244). There it crossed the band of the window's y 14 round, whose rail is at
  y 13.5, at the sample (23.5, 13.8999, 9.5), and r 0.5 was refused as too large.
- **Trigger:** SolidCraft's #871 part (a stepped 14-wide bar with a window through it),
  with all 24 edges of both end faces rounded at r 0.5.
- **How found:** SolidCraft; the fork's fix is tangency-labs/smlib@d548305 ("a blend's band
  on a face is its rail between the rail's ends").
- **Confirmed:** no C++ run was made here (the check is not in the pinned C++).
- **Rust port:** this refines `fillet_refuse_overlapping_bands` (no new field).
  `geometry_bands_clear_other_rails` samples the first rail only between its end vertices.
  It takes the primary edgeuse's vertex and its mate's vertex. If both are the same vertex,
  the rail is closed and sampled whole. Otherwise, at each end whose fillet vertex a corner
  has processed, the rail parameter comes from `DropPoint` of that vertex's point, used
  when the drop succeeds. Any other end keeps the rail's own interval end.
  `tests/wr649_band_between_rail_ends.rs` rebuilds the #871 part. At r 0.5 it checks
  38 faces, a clean `check_brep`, and a volume within 1e-7 relative of
  `V0 - 2 L r^2 (1 - pi/4)` (L = 107.6). At r 1, where the rounds overlap across the
  1.26 wall, it checks that the fillet is still refused.
