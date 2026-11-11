#### HANG-38. `SmSurface::DropCurve` can subdivide the same span forever
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmSurfaceDropLib.cpp:5637` `SmSurface::DropCurve`, the span stack loop: a span that fails `TestSpanAccuracy` is split at `(sStart.m_dT + sEnd.m_dT) / 2.0` (`:5937`) and the split point pushed (`:6014`); a failing span is only accepted once it is shorter than `sInterval.GetLength() / 8000.0` (`:5824`).
- **What happens:** Nothing bounds the stack. When `1/8000` of the interval is at most one ulp of the span ends, a span of one ulp neither passes that test nor shrinks: the midpoint rounds to `sEnd.m_dT`, `GetNextPoint` succeeds there, and the same point is pushed again on every iteration, so the stack grows until memory runs out.
- **Trigger:** A non-degenerate curve interval of length `L` with `L / 8000 <= ulp(max |end|)` (some float spacing inside the interval is at least `L / 8000`) over a span that fails `TestSpanAccuracy`. A zero-length interval is not a trigger: `IsDegenerate` is `TRUE` on it, so no drop point gets `m_eDropDerivRtn == SM_SUCCESS` (`SmSurfaceDropLib.cpp:1894`-`1931`) and the keep-all rescue (`:5514`) needs a deviation below the tolerance.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** `intersect::surface_drop_lib::p3` bounds the stack at `MAX_SPAN_DEPTH` (64) drop points and returns `SolidError::NotConverged` past it (exact arithmetic needs about 15). The FFI rejects a non-degenerate interval (ends normalised, as `SmExtent1d` swaps them) of length `L` with `L / 8000 <= 2 ulp(max |end|)` (about 16000 ulps, twice the margin) and nothing else: zero-length, inverted and out-of-range intervals and any tolerance (zero and negative included) are accepted.
