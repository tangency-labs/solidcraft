# curve.curve_p5: curve caches are memoised in the curve value (2026-10-25)

Unit: `curve.curve_p5` (`make_curve_cache`, upstream `SmCurve::CacheMakeOrValidate`), with
`curve.curve_p2` (the `Curve` field), `curve.curve_cache` and `geom.tree` (the handle
copies). Design: option C of issue #982, as refined in its comments.

**Upstream.** `SmCacheMgr::GetOrCreateObjectCache` keeps one `SmCurveCache` per curve
object in the context's global cache (at most `SM_DEFAULT_MAXCOUNT_CURVECACHE` = 1000).
`SmCurve::Notify` deletes it on `SM_NO_PRE_EDIT` and `SM_NO_DESTRUCTION`.

**Before.** The port built a new cache for every request unless one call chain kept it
(#539, #753, #951). On the #948 loft at 20 sections (main `4fe6f4d51`, gdb breakpoints
on `Tessellate`), the Rust made 16,918 builds for 22,661 requests, and the C++ 1,393.

**Now.** `make_curve_cache` keeps a memo of its result in the `Curve` value
(`CurveMemo`):

- `Clone` and the evaluation copies share the memo. `kind_mut` and every editing method
  that writes the geometry start an empty one, so an edit cannot leave a stale entry.
- The entry is keyed by everything the build reads besides the geometry
  (`CurveMemoKey`): `Kernel::tolerances` and the context scalars (zone tolerance, model
  size estimate, size ratio, `doing_boolean`, `trim_uv_curves_at_corner_drops`). The
  owner is not an input, because the build works on an ownerless copy.
- Only pure values are memoised (`memo_pure`): B-spline, line, ellipse, hyperbola,
  parabola, Hermite and conic curves, and offset, projected and composite curves made
  only of these. Iso curves, curves on surfaces and curves in volumes hold a surface or
  volume with cache state of its own, so they always build.
- The entry is handle-free; a hit returns a copy in the caller's handle space
  (`CurveCache::retag`, `Tree::retag_curve_tree`).

**Why no result can depend on it.** The build takes `&Kernel` (no RNG, no UB record) and
writes nothing outside the new cache, so its result is a function of the curve value and
the key. A hit returns that result. Fill order, which clone filled the memo and the
thread do not matter. Memory: one entry per live curve value, freed with its last clone;
no count limit. `Curve` stays `Send` and not `Sync` (as on main). No `unsafe`.

**How to check it.** `Kernel::memoize_curve_caches` turns the memo off; the
`curve-memo-off` feature of `solidcore` makes that the default, so difftest runs in both
positions:

```sh
cargo run --profile difftest -p difftest --features cpp -- --unit <unit>
cargo run --profile difftest -p difftest --features cpp,solidcore/curve-memo-off -- --unit <unit>
```

`tests/curve_cache_memo.rs` runs booleans of primitives and lofted skins with the memo
off and on and compares every output bit for bit.

**After.** Same loft and method: 1,409 builds against the C++ 1,393, the same count per
caller within a few builds (see the PR for the table).

**Not covered.** The other `CurveCache::new` builders (their own options) build every
time. A hit copies the spans and tree, because the solves take `&mut CurveCache` (the
lazy analytic curve).
