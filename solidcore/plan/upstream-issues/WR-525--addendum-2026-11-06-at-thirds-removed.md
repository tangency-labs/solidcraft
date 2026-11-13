Updates: WR-525

#### WR-525 (update, 2026-11-06). The at-thirds option is removed with the executive's blend curve

With `fillet_gn_section_generator` removed (WR-685's addendum of 2026-11-06), every
curvature-continuous round the executive builds (`SM_FSG_BLEND_CURVE`) is the G^n section. That
section has order-n contact with both faces by construction, and its thumbweight sets its shape
(the equal-area default, or any w below 1). The fork made the same change: when its G^n section
(11ea2d9) replaced the blend curve, it removed `SetBlendCurveAtThirds` (`fd3b6a7`). Here the
option is removed with it:
- `FilletSession::set_blend_curve_at_thirds`;
- `FilletExecutive::set_blend_curve_at_thirds` and its field;
- `FilletSurfaceGenerator::blend_curve_at_thirds`;
- `BlendCurveCrossSectionFsg::at_thirds` and its branch in the section;
- the ease's at-thirds G2 target, which is now always the order-2 G^n section at thumbweight 0.

The upstream generator, `BlendCurveCrossSectionFsg`, stays as the pinned C++ builds it (the
faired Hermite scaled by the blend scale). A caller can still build it directly, and the difftest
compares it.

- **Results:** a session that turned the option on now gets the G^n section, if it asks with a
  valid thumbweight. Thumbweight 1, which the option's callers passed, is refused as invalid
  input (`SetFilletParameters`, WR-685).
- **Tests:**
  - `issue_1438_blend_curve_at_thirds.rs` is removed with the option it tested.
  - The tests that turned it on (the eased setbacks of `issue_1376_setback_ease` and
    `wr_659_eased_corner_face_curve_halves`) build their curvature-continuous rounds with the
    G^n section at its default thumbweight.
  - The fork's at-thirds volume of the peg's front round (`fork_cd47efd_tangent_end_rib_sqrt_s`)
    and its mark in `fork_9bebf34_tangent_end_cases` (`front_curvature_round_r2`) are removed.
    The G^n round on the same edge is graded against its quadrature in
    `fork_cd47efd_tangent_end_rib_sqrt_s`.
