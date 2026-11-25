Updates: WR-707

#### WR-707 (addendum 2026-11-02): the break-through test refused blends that run off their faces

- **Severity:** Wrong result (in the port's opt-in fix: valid blends refused)
- **Area:** Blends
- **Status in the port:** the same flag, `UpstreamFixes::fillet_refuse_rounds_breaking_through`, off by default. Nothing changes with it off: the pinned behaviour, which difftest gates, does not reach the test.
- **Location:** `crates/solidcore/src/blend/fillet_executive/session/band_cuts.rs`, `section_breaks_through` (called by `band_breaks_through_the_part`).
- **What happened:** the first version refused a convex blend wherever its section crossed any face of the part inside that face. A blend whose section runs off its face also crosses faces: past a cliff (the body's bottom face), into a notch, a hole or an opening. The construction trims the blend to those faces, and fork 5e8c9c1 builds such blends. With every flag on, five SolidCraft rig cases were refused (`SolidError::FilletRefused`, `NoRoomReason::BreaksThrough`). With only this flag off, each one built to its expected volume:
  - `1080-chamfer-runs-into-openings-cut-by-them` (fit 4.99): 3350;
  - `1080-fillet-notched-block-top-outline-r8` (fit 7.0018): 10198.718039;
  - `324-fillet-over-hole-cut-by-overlong-cylinder` (fit 2.563): 7580.287315;
  - `every-hole-top-rim-r12-past-plate-bottom` (fit 10.004): 13726.857967;
  - `190-fillet-disc-rim-past-its-2mm-wall`: 957.692239.

  A round over a through hole that lies between its rails was refused the same way, and so was SolidCraft's `4782-fillet-bar-edge-wider-than-both-faces`: an R10 round of a 5 x 5 bar, which the fork does not build yet. The construction builds both to their closed forms (the bar: 80 times the area of the bar's square inside the circle of centre (10, -5) and radius 10, 630.2934873).
- **What changes:** a section breaks through only where it leaves the part's material and comes back into it through an inner face. The section is split at every point where it meets a face, and each piece between two such points is classified by its middle. A piece breaks through when all of these hold:
  - it lies outside the material;
  - one of its ends crosses a face inside that face;
  - neither end lies on a face at the blended edge's ends, as before;
  - one of its ends lies on a face that shares no edge with the blended faces. Such a face bounds material behind a wall, and the construction does not reach it over the blended faces' boundaries.

  So a piece outside the part that reaches a rail is never a break-through: there the blend runs off its face. A piece through a hole in a blended face is not one either, because the hole's wall bounds that face. A hollow behind a wall still is. #2299's shelled box breaks through its inner faces. So does a blind pocket or a cavity under a blended face: without the flag the construction builds those without trimming the pocket, 0.83 short of the closed form in the regression case. A piece within the zone tolerance of the boundary does not count as material. The bound, and so the fit of #2299's r7 refusal, is unchanged: 6.818444195803968, against the closed form 2 sqrt 2 / (sqrt 2 - 1) - 0.01 = 6.818427.
- **Regression evidence:** `crates/solidcore/tests/wr707_rounds_running_off_their_faces.rs`. The five rig cases build with the flag on: one lump, `check_brep` clean, and each volume matches its closed form, or the rig's expected value for the notched block. A through hole between the rails and the 4782 bar build to their closed forms. A blind pocket is refused, and builds 0.83 short without the flag. #2299 stays refused with the same fit. The seven build tests fail without this change. `crates/solidcore/tests/wr707_round_breaking_through.rs` is unchanged and passes.
- **Tracking:** tangency-labs/solidcore#2299.
