#### WR-18540. Loop test 2 reports a correctly turned loop on a domain narrower than its side band
- **Severity:** Wrong result (a valid body is refused by the check)
- **Area:** Validity check (loop test 2, `SmLoop::Test #2 Bad Loop Orient`)
- **Location:** `SmEdgeuse::IsEdgeuseSideInSurface` (`source/SMLib/src/SmEdgeuse.cpp`), ported
  as `topology/edgeuse/p2.rs` `is_edgeuse_side_in_surface`. Each sample is classified against
  the sides of the surface's natural domain with `GetPoint2dBoundaries(uv, 2*sTol2d)`, twice the
  face's zone tolerance mapped to UV, and the bits of all samples are and-ed. A sample within
  that band of both U sides (or both V sides) keeps both bits. The direction test then takes
  the first side in the order U-min, U-max, V-min, V-max and asks every edge in that band to run
  the way that side's inside requires.
- **What happens:** a face whose domain is narrower than the band in one direction (a plane
  fitted to a sliver triangle: a STEP importer that gives each face's plane the face's own UV
  box) has every sample of every edge within the band of both opposite sides. The test reads
  each edge as running along U-min (or V-min) and fails the edge that runs the other way,
  although the loop is turned correctly for the face's normal: `Bad Loop Orient`, "loop may
  have wrong orientation".
- **Trigger:** a planar triangle 0.0088 mm high (sides 0.0823, 0.0528 and 0.1346 mm) under a
  0.005 mm face tolerance, its plane's domain its UV box (0.0088 by 0.1344). The 13,084-face
  faceted STEP of tangency-labs/solidcraft#18540 has 32 such faces: its body fails the check
  with 32 `Bad Loop Orient`, and every boolean on it is refused. A box 2 x 0.008 x 4 under a
  0.005 face tolerance fails the same way on its four thin faces.
- **How found:** tangency-labs/solidcraft#18540.
- **Confirmed:** the pinned C++ reads the same way (the classification and the direction test
  above, `SmEdgeuse.cpp`); no C++ run here. The port shows it: the box above and the STEP's
  body fail loop test 2 without the fix.
- **Fix (always on, no flag):** a sample within the band of both opposite sides does not count
  those two sides. Such a sample is no evidence of the side the edge's inside is on, as a
  sample whose drop fails (WR-663). A sample near only one side of the pair classifies as
  before, so a face wider than the band in both directions is checked as before.
- **Not fixed here:** a face narrower than the band is no longer checked for the side its loop
  faces in that direction. A wrongly turned loop on such a face passes loop test 2.
- **Tests:** `a_face_narrower_than_the_side_band_has_its_loop_side_in_the_surface`
  (`crates/solidcore/tests/assert_valid_level0_helpers.rs`).
