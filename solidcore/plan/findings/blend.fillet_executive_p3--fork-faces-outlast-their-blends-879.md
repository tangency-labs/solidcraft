# blend.fillet_executive_p3: the SolidCraft fork's `FacesOutlastTheirBlends` (#879) is an opt-in fork feature, not an upstream defect (2026-10-26)

Unit: `blend.fillet_executive_p3` (`SmFilletExecutive::CreateFilletBrep`). PR:
tangency-labs/solidcore#1233.

Upstream SMLib has no `FacesOutlastTheirBlends`: the check is a function of the SolidCraft
fork only, so a blend that takes the whole of a face it rolls on is upstream's designed
behaviour, not an NVIDIA defect, so it has no `plan/upstream-issues/` record (the id WR-320 that
the `UpstreamFixes` field's doc and #1233's comments cite was never filed). The port keeps the
check as an opt-in fork feature; default behaviour is SMLib's.

- **Kind:** fork feature (behaviour SolidCraft's fork adds; SMLib builds the blend)
- **Area:** Blends (fillets, chamfers)
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp` `SmFilletExecutive::CreateFilletBrep`: nothing between the rails' trimming and classification and the fillet faces' construction asks whether a face the blends roll on keeps any of itself.
- **What happens:** a blend as wide as the face it rolls on is built. A radius-10, height-20 cylinder's top rim chamfered 10 across the top comes back as a 3-face body: the top face is gone and the chamfer's cone ends in a point at the top's centre. The rail on the top has shrunk to that point (every point of its B-spline at (0, 0, 20)), and a point rail by itself is fine (the corners of tangency-labs/solidcore#420 have them), so nothing notices that no part of the top is left.
- **Trigger:** every edge of a loop of a face blended, each of those blends' rails on that face a point, and some blend there with a cross section that meets the face at an angle (the linear section of a chamfer). A round's arc meets the face tangentially at its rail, so a round as wide as the top closes the end as a hemisphere, tangent to the side and smooth at the pole; that is a valid result.
- **How found:** tangency-labs/solidcore#879 (the SolidCraft fork's `smlib_chamfer_sides` and its kernel tests), with gdb on the fork.
- **Confirmed:** in the fork's C++ build (the 3-face body passes SMLib's level-2 check); not run here. Upstream 1.0.1 does not change this path: its only `SmFilletExecutive.cpp` change rejects input edges that are not live edges of the target B-rep, and its only `SmFilletSolver.cpp` change is `sm_TestAnalyticSurface`'s cone test.
- **Rust port:** faithful by default: `FilletExecutive::create_fillet_brep` builds the blend as upstream does. With the opt-in `Kernel::fixes.fillet_refuse_face_consuming_blends`, `FilletExecutive::refuse_blends_larger_than_faces` (`crates/solidcore/src/blend/fillet_executive/p3.rs`) runs the fork's `SmFilletExecutive::FacesOutlastTheirBlends` (tangency-labs/smlib `ebad7bb`, `8bffb23`) once every solver's geometry exists, through the live session's `geometry_faces_outlast` (`crates/solidcore/src/blend/fillet_executive/session.rs`). If the blends take the whole of a face with some section meeting it at an angle (`FilletSurfaceGenerator::meets_its_faces_tangentially`, `crates/solidcore/src/blend/fillet_solver/p1.rs`), it records `SM_FILERR_NO_ROOM` and fails with `SM_ERR`; accessors without the rails' geometry return `Unported`. Test `chamfer_as_wide_as_its_face_is_refused` in `crates/solidcore/tests/fillet_wr300_320.rs`.

Refs tangency-labs/solidcore#879
