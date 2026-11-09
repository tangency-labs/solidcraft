# surface.cone_p3 / surface.cone_p2: ignored trim-domain conversion status (fuzz after PR #226, 2026-10-20)

- **Disagreement.** fuzz02497 (a global cone/cone call whose first cone has a zero Y
  axis, so `MakeNurb` leaves no polar curve): C++ succeeds, Rust returned status 1006.
  `sm_GetEllipseTrimIntervals` ignores the status of `ConvertDomainFromNURBSToSTEP`
  (`SmCone.cpp:5325`). When a corner conversion fails (`NER(m_cpCurve)` in
  `ConvertToPolarParameter`), the domain stays default-constructed (`SmExtent2d::Init`,
  defined), but `ellipse_trim_intervals` propagated the error. It now ignores the status
  and keeps `Extent2d::new()`, so the domain matches upstream exactly.
  Fixture: `surface.cone_p3/fuzz_trim_domain.json`. UB-154 updated: only the line/plane
  part is undefined.
- **Results (Linux):** `PARTIAL surface.cone_p3 cases=1416 passed=1381 failed=0
  deferred=35 max_rel_err=0e0`, `--perturb` detects 1381/1381; `PASS surface.cone_p2
  cases=1550`; `--green` 222 units, 114 passed, 108 partial, 0 failed.
- **Still open (pre-existing on main, not fixed here).** The same 10,000-case fuzz finds
  two cases (fuzz00656, `/domains/2 = -1e+300`; fuzz09540, `/domains/5 = 1e+300`) where a
  huge trim-domain fraction gives a zero-length seam line. Rust's `line_plane_intersect`
  returns `InvalidInput` (status 1007); upstream ignores that status and reads the
  uninitialised `dParam` (UB-154), so C++ returns a value. The `surface.cone_p2` shim
  refuses these inputs (`trim_defined`); the `surface.cone_p3` path reaches the file-static
  `sm_GetEllipseTrimIntervals` through `IntersectWithCone`, which has no such guard yet.
  Reproduced with main's unpatched `cone/p2.rs` too. Tracked in issue #276; the
  `surface.cone_p2` refusals that are now stricter than needed are issue #277.
- **Issue #277 (2026-10-23).** The `surface.cone_p2` shim and runner no longer refuse a
  cone without a polar curve (`height_defined`) or a trim whose domain conversion fails:
  both are defined upstream (`GetHeight` keeps initialised points, UB-87; the domain stays
  default-constructed or becomes the NURB domain). The state now always carries the
  height. Only the UB-154 line/plane refusal remains. Fixture:
  `surface.cone_p2/no_polar_curve.json` (cones with Y axis `(0, -1e-300, 0)`: offset,
  trim, rebuild and recognize).
