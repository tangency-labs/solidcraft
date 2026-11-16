#### WR-532. The extended offset extends a cylinder by a tangent extension that leaves it, and a face reaching into it is fitted back off radius
- **Severity:** Wrong result (a re-derived cylinder 3.2e-10 off its radius, by an amount that depends on the platform's rounding; arcs built on the exact offset miss it)
- **Area:** Offsets and shells
- **Location:** `source/SMLib/src/SmOffsetExecutive.cpp`, `SmOffsetExecutive::GetCreateExtendedSurface` (def. `:2237`): every surface is extended with `CreateExtendedSurface(.., SM_CT_G1R, ..)`.
- **What happens:** A tangent extension (`SM_CT_G1R`) of a cylinder leaves the circle at once: 10 mm round an R4 offset, it is 6.8 mm off. A shell's rim faces reach a hair (5e-5 mm) past the offset rounds' ends into that extension. `SmMerge::MakeFaceuse` then turns each face's surface back into an analytic one (`CopyAndAddAnalytics`, `IsNurbSurfaceCone`). The fit comes out a cylinder 3.2e-10 off R4, and the rim's arcs, built on the rounds' exact offsets, miss it by 3.75e-10 on some platforms.
- **Trigger:** a box with rounded edges shelled inward (the fork's capi `shelled_rounded_box_rim`).
- **How found:** reported by SolidCraft. The fork's fix is tangency-labs/smlib@b283ea2 ("offset: a cone or cylinder is extended as itself"), refined by tangency-labs/smlib@3626d4b ("a cylinder extended round its axis keeps SMLib's circle knots and comes back a cone"). Neither is merged upstream.
- **Confirmed:** no C++ run was made here.
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.offset_extends_cones_as_themselves`, `OffsetExecutive::get_create_extended_surface` extends a cone or cylinder as itself (`extend_cone_as_itself`, the fork's `sm_ExtendConeAsItself`):
  - along its rulings with `SM_CT_CINFINITY`;
  - then around its axis with `SM_CT_CINFINITY`, by at most half the turn it leaves open on each side, measured as arc length on its smaller end circle (none for a closed cone). Each side is extended on its own, and each old end is then made a knot of multiplicity equal to the degree again (`full_knot`, `BSplineSurface::insert_one_knot`). That is SMLib's own form for a circle: extending both sides in one call leaves a single knot at the old end, and `IsNurbSurfaceCone` then no longer finds a cone.

  `tests/wr_532_shell_rounds_stay_on_radius.rs` shells a 60 x 40 x 20 box whose vertical edges are rounded r6, 2 mm inward and open at the top. It holds the cavity's four r4 cylinders (and any r6 ones) to 1e-12. A second test rounds a single edge r5 and checks that the cavity's round is a cylinder of radius 3.
