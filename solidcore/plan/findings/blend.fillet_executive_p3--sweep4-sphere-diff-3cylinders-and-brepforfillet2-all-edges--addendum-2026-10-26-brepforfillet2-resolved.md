# brepForFillet2 all-edges r=0.1: resolved (2026-10-26)

Updates: plan/findings/blend.fillet_executive_p3--sweep4-sphere-diff-3cylinders-and-brepforfillet2-all-edges.md

`sweep4-brepForFillet2-all-r0.1` now agrees with C++ within 1e-9 (status 0, 27/54/29,
mass properties) and its known-mismatch entry is removed.

Cause: the first refusal was `SolidError::InvalidInput` ("corner patch face without
surface") in the live corner insertion context
(`blend/fillet_executive/session/insertion/corner.rs`,
`register_blending_intersection`), reached from `SmFilletCorner::InsertIntersectionTopology`'s
blending loop (`SmFilletCorner.cpp:2353`). The corner is a convex NxN blending corner
whose one patch face (intermediate face 0, built by `MakeFaceBrep`) is deleted by the
fillet stitch's lamina-sliver removal (`SmStitch::DoStitching` -> `DeleteFace`, called
from `StitchFilletFaces` in `CreateFilletBrep`) before insertion; deleting the face
deletes the surface it owns. Upstream keeps `m_vSurfaces[jj]` as a raw pointer and
`RegisterExistingSSI` only appends it to the intersection table, so the entry can never
match a live face's surface. The port mapped the surface to its face's handle and
refused when that face no longer held one.

Fix: for a destroyed patch face (off the face list, no surface) the entry is registered
under that face's face-surface handle, which no face loop queries (face ids are never
reused), so it is inert as upstream's dangling entry. A live face without a surface
still refuses. Upstream's entry holds a freed address; a later surface allocated at the
same address could in principle match it (heap-dependent, not modelled).
