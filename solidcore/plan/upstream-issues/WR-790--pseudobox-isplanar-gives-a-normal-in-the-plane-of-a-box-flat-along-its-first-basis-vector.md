#### WR-790. `SmPseudoBox::IsPlanar` gives a normal in the plane of a box flat along its first or second basis vector
- **Severity:** Wrong result (a thickened sheet refused by the check)
- **Area:** Geometry (pseudo boxes)
- **Location:** `source/SMLib/src/SmPseudoBox.cpp:1207` (`SmPseudoBox::IsPlanar`, def. `:1190`) sets
  `*pOptPlaneNormal = m_aBasis[0] * m_aBasis[1]` whichever of the three intervals is the flat one.
  solidcore `geom/pseudo_box.rs` `PseudoBox::is_planar`.
- **What happens:** a box flat along its third basis vector reports the right normal. A box flat
  along its first or second, and not its third, reports a normal that lies in the plane. A
  surface's box takes its basis from the corners of its control net (`SmBSplineSurface.cpp:509`;
  solidcore `nurbs/b_spline_surface/p1.rs`, `CalculateBoundingBox`): the sum of the corner
  differences along u, made orthonormal with the sum along v. Where the sum along u is zero the
  box keeps the world axes. A half ring (the flat wall between a half circle and its offset, a
  surface of revolution) has all four corners on one line, and its box keeps the world axes; in a
  plane x = const it is flat along the first, in y = const along the second. The SolidCraft
  fork's `sm_PutPlanarFaceOnItsPlane` puts a thickened sheet's planar side walls onto the plane
  `IsPlanar` names (solidcore `offset/offset_executive/p2.rs` `put_planar_face_on_its_plane`).
  A half tube's end rings then went onto planes through the tube's axis: the solid had the right
  volume and counts, and SMLib's check refused it (loop orientation and zero area on the two
  rings, eight edge and eight vertex tolerances).
- **Trigger:** a half circle of radius 5 about the X axis (the sketch plane YZ), extruded 10 along
  X as a sheet and thickened by 2: the end ring at x = 10 went onto the plane through
  (10, 0, 3.5) with axes X and Y (normal Z). The same sheet about Z thickens to a valid solid.
  `crates/kernel/src/surfaces.rs` `thicken_a_half_tube_on_every_origin_plane` (XY, YZ and XZ,
  thicknesses 1, 2 and 4 out, in and symmetric) and `geom/pseudo_box.rs`
  `planar_normal_is_the_flat_direction` (every set of flat intervals).
- **How found:** tangency-labs/solidcraft#13348 (Thicken of a half-circle surface extrusion on
  the YZ plane refused).
- **Fix (always on, no flag):** the normal runs along the last flat interval's direction, the
  cross product of the other two basis vectors in cyclic order. A box flat along its third basis
  vector, or flat nowhere, keeps `m_aBasis[0] * m_aBasis[1]`.
- **Not fixed here:** the world-axis box of a ring that is normal to no world axis (a half ring
  on a tilted plane) is not flat at all, so `IsPlanar` calls the planar face not planar. A half
  tube thickened about an axis tilted 35° is still a valid solid of the right volume; its rings
  stay surfaces of revolution.
- **Confirmed:** no C++ run; read from the pinned source, and measured in solidcore (both tests
  above fail without the fix).
- **Rust port:** was affected in the same way (the port was faithful here).
