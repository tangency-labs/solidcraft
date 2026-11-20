#### WR-18540. Loop test 2 reports a correctly turned loop on a domain narrower than twice its side band
- **Severity:** Wrong result (a valid body is refused by the check)
- **Area:** Validity check (loop test 2, `SmLoop::Test #2 Bad Loop Orient`)
- **Location:** `SmEdgeuse::IsEdgeuseSideInSurface` (`source/SMLib/src/SmEdgeuse.cpp`), ported
  as `topology/edgeuse/p2.rs` `is_edgeuse_side_in_surface`. Each sample is classified against
  the sides of the surface's natural domain with `GetPoint2dBoundaries(uv, 2*sTol2d)`, twice the
  face's zone tolerance mapped to UV, and the bits of all samples are and-ed. A sample within
  that band of both U sides (or both V sides) keeps both bits. The direction test then takes
  the first side in the order U-min, U-max, V-min, V-max and asks every edge in that band to run
  the way that side's inside requires.
- **What happens:** on a face whose domain is narrower than twice the band in one direction (a
  plane fitted to a sliver triangle: a STEP importer that gives each face's plane the face's own
  UV box), every sample of every edge lies within the band of both opposite sides. The test
  holds each edge to U-min (or V-min), and the edge that runs along U-max (or V-max) the right
  way for U-max fails, although the loop is turned correctly for the face's normal:
  `Bad Loop Orient`, "loop may have wrong orientation".

  ![A sliver triangle in the UV domain of its plane, which is as narrow as the triangle; u runs down, v to the right. The band of the U-min side and the band of the U-max side each cover the whole domain. The loop runs along the long edge at U-max towards larger v, then up the right edge to the tip at U-min and down the left edge. On main every edge is held to U-min, the side tested first, so the long edge goes the wrong way and fails; now each point lies on the nearer side, the long edge runs along U-max the right way, and the short edges cross from one side to the other.](WR-18540--narrow-domain.svg)

  The two side bands cover the whole domain, so main holds the long edge to U-min.
- **Trigger:** a planar triangle 0.0088 mm high (sides 0.0823, 0.0528 and 0.1346 mm) under a
  0.005 mm face tolerance, its plane's domain its UV box (0.0088 by 0.1344). The 13,084-face
  faceted STEP of tangency-labs/solidcraft#18540 has 32 such faces: its body fails the check
  with 32 `Bad Loop Orient`, and every boolean on it is refused. A box 2 x 0.008 x 4 under a
  0.005 face tolerance fails the same way on its four thin faces.
- **How found:** tangency-labs/solidcraft#18540.
- **Confirmed:** the pinned C++ reads the same way (the classification and the direction test
  above, `SmEdgeuse.cpp`); no C++ run here. The port shows it: the box above and the STEP's
  body fail loop test 2 without the fix.
- **Fix (always on, no flag):** a sample within the band of both opposite sides lies on the
  nearer of the two, on neither when it is midway between them. The direction test then holds
  an edge along a side of a narrow face to that side, and an edge that crosses from one side to
  the other is on neither. A sample within the band of one side only is classified as upstream
  does, so only samples within the band of both sides of a pair are affected: on a plane, those
  of a face narrower than twice the band; on a surface whose mapped band changes along the face
  (a face that tapers to a tip), those where the domain is that narrow in UV. The downward face
  use of a narrow face still fails the test.
- **Not fixed here:** an edge within the band of one side only, that runs away from it at a
  slant without lying on it, is still held to that side and can fail, on any face (for example
  the UV triangle (0.019, 5), (0.011, 4.9), (0, 0) of a plane under a 0.01 band: its first edge
  is held to U-max and runs down in v).
- **Tests:** `a_face_narrower_than_the_side_band_has_its_loop_side_in_the_surface`
  (`crates/solidcore/tests/assert_valid_level0_helpers.rs`): a box 2 x 0.008 x 4 and a ring of
  radius 3 and height 0.008 under a 0.005 face tolerance; every upward loop use passes and the
  downward loop uses of the box's six faces and of the ring's wall fail.
