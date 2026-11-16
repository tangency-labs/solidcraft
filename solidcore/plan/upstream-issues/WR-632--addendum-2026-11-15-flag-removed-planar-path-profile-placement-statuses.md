Updates: WR-632

#### WR-632 (update, 2026-11-15). `planar_path_profile_placement_statuses` is removed

`position_profile_curves` returns the first failing status of its box, profile normal, angle, path evaluation, `SetCanonical`, `Transform` and `MakeUnitOrthoVectors` calls instead of positioning the profiles by a partial or undefined frame. That is now the only path, and the flag is gone. Removed: the field, the `checked` parameter of the placement helpers, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `primitive.primitive_creation_p1` `overflow_regression_{053,299,736,987}` (a path start tangent with a `1e300` component: `MakeUnitOrthoVectors` fails) and `position_single_line` (a one-line profile spans no plane: `ComputeCrvsNormal` fails) now return that error where they returned profiles placed by the partial frame.
- **Tests:** the WR-632 and #483 placement tests in `primitive::primitive_creation::p1`.
