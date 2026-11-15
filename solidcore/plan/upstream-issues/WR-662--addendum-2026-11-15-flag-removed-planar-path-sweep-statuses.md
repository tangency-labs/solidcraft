Updates: WR-662

#### WR-662 (update, 2026-11-15). `planar_path_sweep_statuses` is removed

`PrimitiveCreation::create_sweep_along_planar_path` returns the failures of its mandatory evaluations, geometry construction, cap removals, trims, mitre searches (`BrepIntersect`, `GlobalCurveSolve`, as the fork's c8f6a31 does) and cleanup, and releases every temporary B-rep it owns on any error; the path plane normal is computed only at a mitre. That is now the only path, and the flag is gone. Removed: the field, the ignore branches, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** seven `primitive.primitive_creation_p2` planar-path cases (`gen0003`, `gen0028`, `gen0039`, `gen0178`, `gen0200`, `gen0208`, `gen0252`: a circle swept along two lines meeting at a right angle, with a tolerance of 0.1 to 10) now fail 1001: the mitre trim's reference points, 0.1 from the corner, lie within the tolerance of the cutting line, so the trim cannot tell their side. Upstream ignored the failed trims and united the two untrimmed pieces, each extended past the corner by twice the profile's diagonal, so its result kept material beyond the mitre.
- **Tests:** `primitive::primitive_creation::p2::planar_path::audit_tests`.
