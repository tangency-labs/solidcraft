# surface.surface_p8: CoincidenceCheck keeps each surface's boundary curves (2026-10-25)

Unit: `surface.surface_p8` (`SmSurface::CoincidenceCheck`, base body), with
`surface.surface_p2` (the `Surface` field). Not upstream: this goes beyond the C++.

**Upstream.** `CoincidenceCheck` (`SmSurface.cpp:7553-7580`) calls
`CreateNaturalUVTrimCurves` on both surfaces for every check and deletes the curves at
the end (`SmObjsDelete`), so their `SmCacheMgr` entries go with them. A boolean runs the
check for every face pair, so the C++ builds the same boundary curves and their curve
caches again for every pair. On the #948 loft at 20 sections both kernels made 600
curve-cache builds there (gdb on `Tessellate`), 30 % of the C++ samples at 40 sections.

**Now.** Each `Surface` value keeps the result of its natural boundary curves in a memo
(`BoundaryMemo`, a `OnceLock`), keyed by the domain's bits and the kernel inputs of the
build (`CurveMemoKey::of_kernel`: tolerances and the context scalars). The check
borrows the curves (it only reads them), and the curves carry their own curve cache
memos (`plan/findings/curve.curve_p5--curve-cache-memo.md`), so each boundary curve's
cache is built once per surface value. Only B-spline, plane, cone, sphere and torus
surfaces are memoised: their iso-curve overrides read nothing but their own value. A
copy of a surface starts with an empty memo; an edit through `&mut Surface` replaces it.
One entry per live surface, freed with it. No `unsafe`; `Surface` stays `Send`.

**Why no result can depend on it.** `CreateNaturalUVTrimCurves` takes `&Kernel` and the
surface; its result is a function of the surface value, the domain and the key, so a
hit gives what a fresh build gives. `Kernel::memoize_curve_caches` (and the
`curve-memo-off` feature) turns it off; the unit test
`boundary_memo_hit_is_the_fresh_build` and `tests/curve_cache_memo.rs` compare both
positions bit for bit.

**After.** Same loft and method: 128 builds in `CoincidenceCheck` (C++ 600), 937 in the
whole run (C++ 1,393).
