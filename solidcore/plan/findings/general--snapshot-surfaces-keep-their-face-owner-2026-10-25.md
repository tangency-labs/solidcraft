# snapshot surfaces keep their face owner (2026-10-25)

Follow-up to "trimmed surface cache history: tolerance-growth deferral removed".
- **Cause of heal.face_props gen0646 / gen0790 / gen1312** (torus faces, face tolerance 1000, line-edge loops off
  the surface): the private B-rep copy the face classifications of `query::face::p2` run on
  (`Brep::clone_sharing_surface_caches`) was made with the plain copy constructor, which resets every
  surface's owner. Upstream runs these queries on the B-rep itself, where `GetFace()` finds the face and
  `SmTol::GetZoneTol3d(pSurface)` is the face tolerance. On the copy, `SmVertexuse::ComputeUVSector`'s
  `IsOnSeam(uv, NULL)` used the system zone tolerance. gen1312: at uv (0.12109721038429916, 1), C++ returns TRUE
  (SM_SP_BOTH) and `IsSectorMissingSeam` returns TRUE (U); the port returned false, so `FindVertexSector`
  skipped every V-seam sector, and intervals 2-5 classified OUTSIDE instead of FACE. Confirmed with gdb on the C++
  reference (`ComputeUVSector` outputs identical apart from the seam flags) and Rust prints.
- **Fix:** the copy keeps each surface's owner and cached face tolerance. The two `SmTemporaryChangeValue(m_pOwner,
  NULL)` scopes on the copy (IntersectVertices' vertex/face gaps, IntersectEdges' edge views) now clear the owner,
  as upstream does; they previously only recorded the clear, which made no difference while the copy had no owner.
  Without that second step, intersect.curve_class_p4 gen0925 regresses (out[1342] C++ 1, port 3).
- **Still parked:** intersect.curve_class_p4 gen1570 / gen1622 / gen1841 / gen1876 / gen1968 / gen2061 fail the same
  way with and without this fix (gen1570 out[965] C++ 0 / port 1). gen1570 is a sphere face (tolerance 1e-4, a
  vertex at the pole), not a torus. Cause not found.
