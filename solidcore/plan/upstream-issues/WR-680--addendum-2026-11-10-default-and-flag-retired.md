Updates: WR-680

#### WR-680 (update, 2026-11-10). The exact UV trim is the default; flag retired

`UpstreamFixes::cylinder_plane_section_trims_exact` is retired. `Surface::drop_curve_base`
(`crates/solidcore/src/intersect/surface_drop_lib/p3.rs`) always tries `exact_uv_curve`
(`surface_drop_lib/cylinder_section.rs`) once the domain checks pass; a curve it does not
apply to, or whose UV curve does not lift onto it within `ON_SURFACE_REL`, is traced as
upstream. The construction and its eligibility are unchanged. SolidCraft's preset and the
kernel rig's `fixes.json` had the flag on, so their results do not move.

- **Tests.** `crates/solidcore/tests/issue_2109_cylinder_plane_section_trims.rs` runs the
  default kernel: the tilted peg's rim trims, the rounded peg's rail trims and the tilted
  hole's trims are exact (under `1e-12` off their edges), and the side areas and volumes are
  their closed forms. At the test commit they failed on the default kernel: the tilted peg's
  and the hole's rim trims were cubics 8.7e-6 off their edges at 15 degrees, and the rounded
  peg had no rational rail trim. The two tests of the traced path are removed with it.
- **#2358's boss.** `the_union_is_its_closed_form` in
  `crates/solidcore/tests/issue_2358_mixed_end_overflow.rs`: the plate united with the R6
  cylinder tilted 20 degrees measures its closed form, 38601.117929444641
  (`36000 + 900 pi` less the cylinder below the plate). The traced cubics gave
  38601.117965258774 on the default kernel (3.6e-5 off), the fork's value. The file's
  removed volumes (every fix on) were pinned to the fork's with this flag off and are
  re-pinned as this kernel's, with their tolerances unchanged:
  - tilt 20, r 0.25: -0.576293104474 to -0.576120035475;
  - tilt 20, r 0.2625: -0.635789178705 to -0.635614755905;
  - tilt 40, seam 125.13, r 1.5: -33.432934267425 to -33.432512368854;
  - tilt 20, seam 170, r 0.262514174: -0.636117736700 (the fork's) to -0.635683901346.
  Their face counts, the rails' ends and the refusal sizes do not move.
- **Goldens.** `--all --goldens` moves 44 rows of two bodies, values only (no case added,
  removed, broken or refused differently); rows that moved only within `1e-9` keep their
  text. Every moved row with its old and new values is in
  `plan/findings/intersect.surface_drop_lib_p3--exact-cylinder-trims-move-goldens-2026-11-10.md`.
  - `blend.fillet_executive_p3` `piecewise-box-edges-*` (8 rows): the 10 x 20 x 30 box with
    edges rounded r0.5 and mitred where two meet. The mitre of two equal perpendicular
    cylinders is an ellipse in the bisecting plane, so the volume has a closed form:
    `6000 - r^2 (1 - pi/4) L + n r^3 (5/3 - pi/2)` for rounded length `L` and `n` mitred
    corners (`mitred_box_rounds_are_their_closed_form` in the #2109 test file checks it to
    `1e-12` relative). Integrated to `1e-10`, edges 0 and 1 (`L = 30`, `n = 1`) measure
    5998.402470018067 against 5998.402470017965 (1.0e-10 off), where the traced trims gave
    5998.402474490056 (4.5e-6 off); edges 0, 1 and 2 are 4.3e-10 off (traced 4.5e-6), and
    edges 0, 4 and 8 (`n = 2`) 8.2e-10 off (traced 8.1e-6). The rounds' trims are within
    2.1e-14 of their edges, where the traced cubics were 4.3e-6 off. The unit integrates its
    mass to `1e-4`, so its rows move by less than that.
  - `sphere_diff_3cylinders.smb` (`pt_TestFiles/Fillet`; 36 rows: 10 in
    `blend.fillet_executive_p3`, 15 in `topology.face_p3`, 10 in `topology.edgeuse_p3`, 1 in
    `corpus.parity`). Read with its UV trims rebuilt, 17 trims on its cylinders are dropped;
    they become rational quartics within 1.6e-13 of their edges, where the traced cubics were
    1.3e-5 off (two of them 9.6e-3). The `topology` rows digest these trims. In the fillet
    and parity rows the volumes move by at most 2.6e-3 of 169497 (1.5e-8 relative) and the
    areas by at most 2.2e-4 of 38237. Read without rebuilding, the file's own trims do not move;
    measuring the body drops curves on its cylinders again (after the measure, 10 of its
    cylinder trims are rational), which moves the parity row.
- **Rig.** `kernel_cases/fixes.json` listed the flag, so the rig's results do not move.
