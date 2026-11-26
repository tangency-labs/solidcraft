Updates: WR-464

#### WR-464 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Unchanged in v1.0.1: `SmOffsetGeometryCreation::FaceuseOffset` is `SmOffsetGeometryCreation.cpp:118`-`:265` with `if ( rNewSurface == NULL )` still at `:167` returning `SM_SUCCESS`. The callers moved: `DoSolidOffset` calls it at `SmOffsetExecutive.cpp:553` (was `:552`) and `DoExtendedOffset` at `:1332` (was `:1289`), its `if (sOffsetSurface == NULL)` test at `:1339` (was `:1296`). `collapse_thresholds_of_the_analytic_surfaces` still passes on `main` with the v1.0.1 port; difftest agrees with the C++ 1.0.1 by default (`offset.offset_executive_p1`, `_p2`, `offset.offset_geometry_creation`: all pass).
