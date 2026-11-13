# General: object-tolerance audit, parked (2026-10-31)

**Question:** where upstream reads an object's own tolerance, does the port use the system zone tolerance instead? Upstream reads it through `SmTol::Get{Zone,Approx,XSect}Tol3d(<object>)` or `->GetTolerance()`, which is the edge's for an edge-held curve and the face's for a face's surface (`SM_USE_OLDTOL`). This follows gen1440.

**Method:**
- I inventoried every C++ object-argument tolerance read in `source/SMLib/src`: 260 calls, 63 of them on `this`.
- I compared each against its Rust port side by side, unit by unit.
- A real gap gets a fix only with evidence: a difftest case where a shim reaches the site, otherwise a Rust regression marked "not compared against C++".

**Units audited:**

| Unit / class | Result |
| --- | --- |
| Geometry methods on `this` (`SmBSplineCurve`, `SmSurface`, `SmCurveCache`, `SmEllipse`, `SmCrvOnSurf`) | **Gap:** `SmBSplineCurve::Trim` reads `GetZoneTol3d(this)`, which is the holding edge's tolerance, and the port used the system tolerance. Fixed at gen1440 (#1880) and in the edge-held trim sweep (6 sites). `SmEllipse::STEPInversion`'s edge default: `MakeNurb` fixed (`plan/findings/curve.ellipse_p1--step-inversion-reads-the-edge-tolerance.md`); the bare-curve callers are open in that finding. Every other method is owner-aware (face cache, owner tolerance or `Unported`) or NEWTOL-only. |
| intersect.curve_class (p1-p9) | Clean: every read goes through `object_zone_tol_3d` / `obj_zone_tol_3d` / `surface_zone_tol_3d`. The system sites are the system branch in C++ too (no classification, `SmVolume`). |
| intersect.point_class | Clean: `object_zone_tol_3d` mirrors `GetZoneTol3d(const SmObject*)` branch for branch. |
| heal face_props | Clean (face, edge and vertex tolerances; `XSect` from zone pairs). |
| heal heal_data, edge props, vertex props | Clean. The context and system sites match `GetZoneTol3d(m_cpContext)` and `GetApproxTol3d()`. |
| topology.face_p4 | Clean (`GetZoneTol3d(GetSurface())`). |
| topology trimming_tools | Clean (edge tolerance; the `SM_ZONE_TOL_3D / 10` literals are upstream's). |
| blend fillet_geom p1-p6 | Clean apart from the trims: all 14 `SmFilletGeom.cpp` reads map to `face2::tolerance`, `edge::tolerance` and `managed_edge_tolerance`. |
| topology proto_topology (p1, p2, p3) | Clean: `XSect(obj, obj)` is the sum of the two zone tolerances, and the `PC` object tolerances use `object_zone_tol_3d`. |
| topology api_projection | Clean. `curve_zone_tol_3d` is owner-aware (it refuses an owned curve without its record). The lift uses `SM_ZONE_TOL_3D`, as `SmApiCurves.cpp:1153` does. |

**Not audited:**
- query/poly (11 system sites), skipped by priority.
- surface/surface p1 (5) and surface trim_srf_cache (4).
- The remaining roughly 120 object-argument reads in units with few sites.
- The topology `GetTolerance()` reads in general. Rust reads the arena record there, so I expect them to be right. I spot-checked them but did not audit them.

**Conclusion:** the gap class was geometry that cannot reach its owner, inside a method upstream gives `GetZoneTol3d(this)`. Trim was the one case that fell back to the system tolerance without an error. With returns falling, the sweep is parked here.
