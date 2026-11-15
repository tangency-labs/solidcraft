Updates: WR-662

#### WR-662 (update, 2026-11-15). `planar_path_sweep_statuses` is removed

`PrimitiveCreation::create_sweep_along_planar_path` returns the failures of its mandatory evaluations, geometry construction, cap removals, trims, mitre searches (`BrepIntersect`, `GlobalCurveSolve`, as the fork's c8f6a31 does) and cleanup, and releases every temporary B-rep it owns on any error; the path plane normal is computed only at a mitre. That is now the only path, and the flag is gone. Removed: the field, the ignore branches, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. With the statuses returned, seven `primitive.primitive_creation_p2` elbows (a circle of radius `0.1 s` along two perpendicular legs of length `s`, tolerance `1e-5 s`, `s` 1e4 to 1e6) failed at their mitre trims. Upstream ignored the failed trims and returned an invalid body of zero volume. The mitre's reference step now grows with the tolerance (WR-780 addendum of 2026-11-15), so the trims succeed and the elbows build with the closed-form volume. Their golden rows (status 0) are main's.
- **Tests:** `primitive::primitive_creation::p2::planar_path::audit_tests`.
