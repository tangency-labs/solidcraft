Updates: WR-700

#### WR-700 (update, 2026-11-06). `face_trim_curves_kept_where_exact` is removed

The fix is the default (tangency-labs/solidcore#2273, step 4a): `topology::face::p1::shrink_geometry`
always asks `Surface::trim_keeps_parameterization` before the trim and keeps the face's trim curves
where it answers true. The flag field, its read site and its entry in the rig's `fixes.json` are
gone; no path removes every trim curve any more.

- **Tests:** `tests/wr700_shrink_keeps_exact_trims.rs` runs its cases on `Kernel::default()` and
  keeps the flag-on halves alone; the flag-off assertions went with the behaviour they pinned.
- **Results:** on the default kernel the kept trims change these tests' results:
  - `tests/issue_2020_offset_mitred_below_depth.rs`, the mitred rounds' top moved to -9 with
    SolidCraft's flags: the volume is 1006.3206273052182, the closed form's 1006.3206273131506
    to 7.9e-12 relative, where the fork's 2fc375f body is 1006.320632128887 (4.8e-9 over it).
    The test now gates on the closed form to 1e-10 and on the fork's value to 1e-8.
  - `tests/issue_2479_pcurve_ends_in_domain.rs`: one trim of the box bottom's rounds
    (cross-section 1, the fillet executive's old path) ends an ulp past `u = 1`. The shrink used
    to remove it and project the edge again; it now keeps it. The case joins the old chamfer in
    that test's survey of the fillet code the generic blend replaces.
  - The rig ran with the flag on; the kernel rig at the head is the PR's full gate.
  - Goldens: `blend.fillet_executive_p3` (29 cases against main's golden at 387a55ad6) and `primitive.api_trim_ops` (2) move with
    the kept trims. Their bodies' pcurves lie on their edges as before or closer (the largest
    distance, sampled, is unchanged to 1e-8 or drops, 4.1e-5 to 3.5e-5 on the tabbed plate's
    side), no edge use passes its tolerance that did not before, and the bodies' integrated
    volumes move by at most 6.8e-9 relative.
