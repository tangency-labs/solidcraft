#### WR-15. `SmPointSet3d::SetUpCentroid` drops 50 points for sizes that are multiples of 50
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmPointSet.cpp:410`, `SmPointSet3d::SetUpCentroid`.
- **What happens:** The first block holds `size % 50` points and the loop runs `size / 50 + (extras ? 1 : 0)` times. For multiples of 50, the last 50 points are never added (50 points give `(0,0,0)`). `m_bCentroidSet` is never set, and after `GetNormal` the centroid comes from the best-fit plane instead, so `GetCentroid` depends on call order.
- **Trigger:** `GetCentroid` on 50 points.
- **How found:** adversarial / edge-case fixtures `centroid_blocks_*`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
