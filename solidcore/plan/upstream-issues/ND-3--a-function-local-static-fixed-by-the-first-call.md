#### ND-3. A function-local static fixed by the first call
- **Severity:** Nondeterminism
- **Area:**
- **Location:** `SmFilletSolver.cpp:3604`, `SmFilletSolver::ComputePointValues` (def. `:3544`).
- **What happens:** `static double sdStaticStep = (rTsectPnt.m_ePointType == SM_IP_TANGENT_POINT) ? 1.0e-3 : 1.0e-5;` is initialised on the first call only. Every later call in the process uses the step chosen for the first point's type, so results depend on call history. (`SmFilletExecutive.cpp:1503`-`1504` also use function statics, but with constant initialisers.)
- **Trigger:** Two fillet runs in one process whose first intersection points differ in type, run in either order.
- **How found:** read during porting (static scan, `plan/closure.md`).
- **Confirmed:** no. The fillet units are not ported yet.
- **Severity:** nondeterminism (order-dependent results).
- **Rust port:** `Kernel::fillet_point_step` latch (per kernel). The `blend.fillet_solver_p4` difftest probes the C++ static (`sc_blend_fillet_solver_p4_static_step`) before every case that can reach the finite-difference block and seeds the latch with it; see `plan/findings.md` (blend.fillet_solver_p4).
