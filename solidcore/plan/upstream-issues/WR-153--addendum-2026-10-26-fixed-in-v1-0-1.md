Updates: WR-153

#### WR-153 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): SmCone::IsNurbSurfaceCone, a failed CreateIsoParametricCurve (top, bottom and middle iso curves) now returns FALSE instead of passing its SmStatus through SER as the SmBoolean. Verification: surface.cone_p2 recognize cases, pending the pin101 difftest run (no current case makes an iso curve fail).
