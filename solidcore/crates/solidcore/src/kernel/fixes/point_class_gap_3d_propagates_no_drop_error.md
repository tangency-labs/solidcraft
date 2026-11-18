`SmPointClassification::GetGap3d(pOptSurface)` returns `Err(SM_ERR)` (and propagates
object-point, surface-drop, and surface-evaluation failures) when the object point
has no single drop point on `pOptSurface` even at `100 * sObjZoneTol3d` (WR-58).
Upstream writes `return SM_ERR;` inside a `double`-returning function (`SmPointClass.cpp:464`)
and ignores failing `SmStatus` returns along the surface-drop path, so `1001.0` (or
undefined evaluation geometry) masquerades as a large 3D in-surface gap to callers
such as `SmCurveClassification::HasNearMisses`. Off by default for pinned C++ parity.
