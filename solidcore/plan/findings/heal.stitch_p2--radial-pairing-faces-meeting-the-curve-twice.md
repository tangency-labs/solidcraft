# heal.stitch_p2: the radial pairing applies only where each face meets the curve once (2026-11-05)

- **Unit:** heal.stitch_p2 (`Stitch::glue_edges_at_vertex`, `RadialGlue` in
  `crates/solidcore/src/heal/stitch/p2.rs`), #2506, PR #2536.
- **What was measured.** A first flag-free version of the radial pairing checked every
  lamina edge on the pair's curve. `difftest --green` at the base `b4de2149a` and at that
  head `c48e69d62` (same machine, all 280 units) moved exactly 8 cases from pass to fail:

  | Unit | Case | First value (base, head) |
  |---|---|---|
  | heal.stitch_p1 | `cov_tjunc11_into_shell` | `out[248]` 10, 9 |
  | heal.stitch_p1 | `attributed_cov_tjunc11_into_shell` | `out[248]` 10, 9 |
  | heal.stitch_p2 | `cov_tjunc11_do_nonmanifold_gaps` | `out[37]` 0.01014, 0.02027 |
  | offset.offset_executive_p2 | `pe_Fillet_cyl_on_box_w_fillet2_f1_d0_out_ext` | max X 0.75, 5.75 |
  | topology.brep_local_operation | `nurb_fillet_face3_same` | edges 31, 49 |
  | topology.brep_local_operation | `attributed_nurb_fillet_face3_same` | edges 31, 49 |
  | topology.brep_local_operation | `fc0004` | `geometry[597]` 0.266, 3.80 |
  | topology.brep_local_operation | `fc0030` | `geometry[616]` 5.3e-15, 0 |

- **Cause.** A probe of each skipped pair showed a face with two of the edges on the
  curve in every case: the `cov_tjunc11` cases glue the two long sides of a strip 0.01
  wide at stitch tolerance 0.1 or 0.5 (closing it collapses the strip); the offset and
  `nurb_fillet_face3_same` cases close a face's seam with its own two edges; in `fc0004`
  and `fc0030` the faces said to lie between the pair are the pair's own faces. Such a face
  leaves the curve in two directions and has no single place in the radial order, so the
  order says nothing about the pairing (Rossignac and Cardoze's Matchmaker, ACM Solid
  Modeling 1999, pairs the 2k faces of a non-manifold edge that each meet it once). In
  `nurb_fillet_face3_same` the skip turned the base's closed solid into an open body.
- **Resolution in #2536.** `RadialGlue::crosses` returns at once when two of the edges on
  the curve belong to one face; the gluing there is upstream's. Test:
  `heal::stitch::p2::tests::radial_glue_leaves_a_face_meeting_the_curve_twice_to_the_gluing`
  (the `cov_tjunc11` strip reduced). With it, `difftest --green` at the base `eff55aeed`
  and at the fix gives the same outcome in every case of all 280 units (591 949 cases), the
  8 above included: the three stitch cases pass, the other five are known mismatches at
  both since the exact surface extension
  (`plan/findings/surface.exact-extension--difftest-cases-moved-2026-11-05.md`).
- **The extended offset.** At `b4de2149a` the base (and the C++) lost the projecting
  cylinder of `pe_Fillet_cyl_on_box_w_fillet2_f1_d0_out_ext` (maximum X 0.75 instead of
  5.75), and the wider check kept it. Since the exact surface extension the base's Rust
  result reaches 5.75 without it, so nothing here is left open.
