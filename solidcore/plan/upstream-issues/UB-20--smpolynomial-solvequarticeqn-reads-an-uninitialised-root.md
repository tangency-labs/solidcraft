#### UB-20. `SmPolynomial::SolveQuarticEqn` reads an uninitialised root
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: matrices and polynomials
- **Location:** `SmPolynomial.cpp:365`, `SmPolynomial::SolveQuarticEqn` (def. `:279`).
- **What happens:** `z = adCSol[0]` is taken from the resolvent cubic without checking the root count. With `dZeroTolerance > 1` the cubic degrades to a quadratic that may have no roots.
- **Trigger:** Any quartic with `r != 0` and zero tolerance `> 1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Uses `z = 0.0`.
