#### UB-207. RETIRED 2026-10-20 (was: the elliptical `SmBSplineFilletLaw` keeps a null law curve after a failed interpolation)
- **Severity:** Crash / undefined behaviour
- **Area:**
- **Reason:** no triggering input exists. The pre-review of PR #267 probed zero span with equal radii, zero radii, span pi and span 1000, among others, and both sides built the full degree-2 curve every time; the interpolation parameters are the distinct `i / 100`. The port still returns an interpolation error where upstream would keep a null curve.
