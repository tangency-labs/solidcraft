#### UB-187. `SmOffsetSurface` extension of a plane with a reversed domain reads a half-built NLib surface
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmPlane.cpp:3013-3016` (`SmPlane::MakeNurb`: the knots are the domain's corners as stored), `source/SMLib/src/SmOffsetSurface.cpp:527-533` (`sm_ExtendSurface`: a failed `N_SrfEvalPtDerivsUnbounded` is retried with the same `esur`), `source/SMLib/src/NL_SrfAdv.cpp:5470` (`N_SrfGetParameterBounds` on the partly extended `esur`).
- **What happens:** `SmPlane(o, x, y, scale, SmExtent2d(u0, v0, u1, v1))` keeps a reversed domain, so `MakeNurb` gives NLib a decreasing knot vector. Evaluating an offset of it outside that domain extends it: the first `N_SrfEvalPtDerivsUnbounded` call fails part-way through the extension, and the retry reads the `esur` it left behind.
- **Trigger:** `SmConstantRadiusFS::LoadJacobian` (via `SmSurface::Evaluate1stDerivatives` on its `SmOffsetSurface`) on a plane whose domain is `[0, 1] x [0, -1e10]`. Reversed by `1e9` or less, the calls return.
- **How found:** `difftest --fuzz 3000` of `blend.fillet_standard_solver_p1` (PR #168 review fixes), case built on a two-plane `cr_jac` fixture.
- **Confirmed:** yes (SIGSEGV in `N_SrfGetParameterBounds` under gdb).
- **Severity:** crash/UB (read of a freed or uninitialised surface).
- **Rust port:** no crash. The `blend.fillet_standard_solver_p1` FFI validator rejects a reversed plane domain for the solvers' base surfaces, so the case is not compared.
