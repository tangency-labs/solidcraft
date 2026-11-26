#### WR-536. `Boolean2D` rebuilds a face on a plane that could not be grown to cover it: edges far off their surface
- **Severity:** Wrong result (edge tolerances of thousands of millimetres; a later join fails, 1006)
- **Area:** Booleans
- **Location:** `source/SMLib/src/SmSurface.cpp`, `SmSurface::CoverCoincidentSurface` (def. `:7964`): the search domain is `GetMaxAnalyticDomain()`, which for a plane is its domain grown by 100 widths. `source/SMLib/src/SmPrimitiveCreation.cpp`, `SmPrimitiveCreation::Boolean2D` (def. `:7641`): `bSuccess` of each cover is ignored.
- **What happens:** `Boolean2D` rebuilds the second operand's faces on copies of the first operand's plane, after `CoverCoincidentSurface` has grown that plane to cover them. The cover only searches 100 widths of the plane. Two r 2 disks 1.5 m apart (a 3 m sheet's holes) are 1470 mm apart, beyond 100 widths of a 6 mm plane, so the cover fails. `Boolean2D` ignores that and builds the far disk on the near disk's 6 mm plane. Its pcurves then lie up to 1470 mm off their edges, and the edge tolerances rise to 5875 and 11756.
- **Trigger:** two r 2 disks 1.5 m apart joined with `SmApiBoolean2d` (the fork's capi test: four disks at the corners of a 3 m hole pattern, joined in pairs and then all four).
- **How found:** reported by SolidCraft. The fork's fix is tangency-labs/smlib@67b8a1e ("boolean2d: a plane grows to cover a face however far away it lies"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.boolean2d_plane_covers_far_faces`:
  - `Surface::cover_coincident_surface_*` on a plane adds the projections onto the plane (`ProjectPointToUVDomain`) of the corners of the other surface's tight box over its domain, grown by the tolerance, to the search domain. The tight box is the newly ported `Surface::calculate_tight_bounding_box`, `SmSurface::CalculateTightBoundingBox`.
  - `boolean_2d` refuses with `SM_ERR_INVALID_INPUT` when a cover still fails.

  `tests/wr_536_boolean_2d_far_disks.rs` joins two r 2 disks 1.5 m apart and checks for two faces with every edge tolerance under 1e-3.
