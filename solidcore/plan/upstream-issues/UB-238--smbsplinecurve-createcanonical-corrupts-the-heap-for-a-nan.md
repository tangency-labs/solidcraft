#### UB-238. `SmBSplineCurve::CreateCanonical` corrupts the heap for a NaN knot
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `SmApiCreateCanonicalCurve` -> `SmBSplineCurve::CreateCanonical` and its NLib curve construction; the write itself was not isolated (no valgrind on the reference machine). glibc aborts later: `free()` in `N_EndNurbs` (`NL_Util.cpp:401`, "double free or corruption (out)") or the next `realloc`.
- **Trigger:** `--fuzz 30000` of `primitive.api_curves`: `fuzz21945` (degree 4, knots `[0, 0.18, 0.54, 0.91, NaN]`, multiplicities `[5, 3, 2, 1, 5]`) and `fuzz21932` (degree 5, knots `[0, NaN, 1.14]`, multiplicities `[6, 2, 6]`); a third NaN-knot case (`fuzz08941`) returned a record instead.
- **How found:** `--fuzz 30000` on main `16f04231`; gdb backtraces of both aborts.
- **Confirmed:** yes (SIGABRT from glibc's heap checks in the reference build, both cases).
- **Severity:** crash (heap corruption).
- **Rust port:** the `primitive.api_curves` FFI refuses a non-finite knot for the canonical curve. The Rust side returns an error status on `fuzz08941`. Number follows the open PRs' UB-237; renumber on merge if taken.
