Updates: WR-685. Under the same flag, `fillet_gn_section_generator`, the G^n section takes the fork's tangency-labs/smlib b9d6a72 and 7102bc5 (smlib#79's prerequisites, ported in #2274's first slice). They replace parts of the 2026-11-01 equal area addendum.

- **Any thumbweight below 1 (b9d6a72).** `GnSectionFsg::is_valid` accepts any finite w < 1. The section's weights `1 - w + w t` and `1 - w t` stay positive on [0, 1] for every such w.
  - `GN_THUMBWEIGHT_EQUAL_AREA` (the default) is now 2, outside that range. The old -2 is now an ordinary thumbweight.
  - `EqualAreaThumbweight` bisects on a bracket whose lower end steps down from -1 (w = 2w - 1) until it holds the root, giving up at -1000. Sharp wedges now have an equal area thumbweight (alpha 0.05 gives -40.16 at order 2), where before they were refused.
- **The equal area thumbweight is resolved at every station (7102bc5).** Each station takes its own wedge's w, not the first blend's middle station's, so w varies along the blend. `sm_GnStationThumbweight` finds w at each station and its s-derivative from the implicit function `I(w) = target(u)`. The target is `(sin u - u cos u) / sin^3 u`, with u half the angle between the ball's radii.
  - `I` is now the closed-form integrand summed by Simpson on 2048 intervals (`sm_GnAreaSum`). It replaces `sm_GnSectionAreaIntegral`, so the values differ in their last bits.
  - The surface interpolates the normalized form's homogeneous rows along s.
  - The certificate adds those rows' remainder (`sm_GnRowTermsOver`), with interval series for w through an interval Newton step.
  - `ContactOrderAt` takes the station's own thumbweight.
  - The generator's thumbweight is no longer written, so the session's sharing of a resolved value, described in the earlier addendum, is removed.
- **Checked against fork 7102bc5** (SMLib and SM_API only), built locally:
  - `EqualAreaThumbweight` for orders 2 to 4 at 8 wedge angles is bit-identical. Orders 1 and 5 are refused.
  - `IsValid` gives the same answers.
  - Blend nets within 1e-9 relative, and volumes within 1e-7. The cases are the box edge at the default and at w = 0, -1 and -3, the chamfer and box edges in both orders and alone (no longer order-dependent), and the pipe on a plate.
  - For a lone box edge the fork's rails run past the box (3 spans over [0, 80]), and here they run along it (2 spans). Fork d01d7b0 does the same, so this does not come from this change, and only that case's section values are compared.
- **Tests:**
  - `crates/solidcore/tests/wr685_gn_equal_area_per_station.rs`: the fork's values with the flag, and no G^n section without it.
  - The updated `wr685_gn_section.rs`.
  - The unit tests in `blend::fillet_section` (`section`, `tay`, `form`).
