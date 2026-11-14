Updates: WR-581

#### WR-581 (update, 2026-11-07). `fillet_blend_curve_revolved_on_arc` is removed with the executive's blend curve

The revolve took only a solver whose generator is the blend curve
(`SmBlendCurveCrossSectionFSG`), never the G^n section. Since `fillet_gn_section_generator` is
removed (WR-685's addendum of 2026-11-06), the executive builds no blend-curve generator:
`SetFilletParameters` makes the G^n section for `SM_FSG_BLEND_CURVE`, `SurfaceSurfaceFillet`
takes linear or circular sections, and the corners copy their solvers' kind. So the flag and its
code are removed: `CalcTorusFilletGeom`'s blend section, `centre_line_on_axis`,
`blend_curve_section` and `SectionUv::Points`. The section's shared body,
`BlendCurveCrossSectionFsg::cross_section_through`, stays for `CreateSurfaceFromCurves`.

- **Results:** the R 10 cylinder's full-width G2 rim round is refused (1001) and leaves the
  cylinder as it was, as the pinned blend curve did. The G^n section is marched and has no
  section to revolve. G1 is invalid input (the G^n section's orders are 2 to 4).
- **Not fixed here:** the G2 round at r 9.99, which the blend curve built, runs for over 20
  minutes on the G^n section, as it already did on SolidCraft's set
  (tangency-labs/solidcore#2964).
- **Tests:** `issue_919_full_width_blend_curve` asserts the full-width refusal and the G1
  refusal. Its dome tests are removed with the revolve.
