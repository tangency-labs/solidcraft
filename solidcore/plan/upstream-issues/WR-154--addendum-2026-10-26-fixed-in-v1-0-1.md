Updates: WR-154

#### WR-154 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): `SmCurveClassification::IntersectUVDiscontinuties`, `Intersect3DDiscontinuties` and `Intersect3DDiscontinuities` now select the last boundary slot with `U_count + V_count - 1` instead of `2 * V_count - 1`, and the volume slots use U+V offsets instead of V+V.
