`SmSurfaceCache::GetSurfaceContinuity` checks the last internal V knot (`iv + 1 < rVConts.GetSize()`) rather than skipping it (`iv + 1 < rVConts.GetSize() - 1`) (WR-42).
