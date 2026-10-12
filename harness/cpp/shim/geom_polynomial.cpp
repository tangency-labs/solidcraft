// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.polynomial`.
// Wraps upstream SmPolynomial::SolveQuadraticEqn / SolveCubicEqn / SolveQuarticEqn
// (source/SMLib/src/SmPolynomial.cpp).
#include "sc_shim.h"

#include <SmCoreTypes.h>
#include <SmMessages.h>
#include <SmPolynomial.h>

// Solves the polynomial of `degree` (2, 3 or 4) with coefficients c[0..=degree]
// (ascending powers). Writes the root count to *out_n and the roots to out[0..n).
// out must hold 4 doubles. Returns the SmStatus (SM_SUCCESS = 1000) in *out_status.
SC_EXPORT int32_t sc_geom_polynomial_solve(int32_t degree, const double* c, double tol,
                                           double* out, int64_t* out_n, int32_t* out_status)
{
    if (degree < 2 || degree > 4)
        return SC_ERR_ARG;
    SC_TRY
    double coeff[5] = {0.0, 0.0, 0.0, 0.0, 0.0};
    for (int32_t i = 0; i <= degree; ++i)
        coeff[i] = c[i];
    double sol[4] = {0.0, 0.0, 0.0, 0.0};
    ULONG n = 0;
    SmStatus st = SM_SUCCESS;
    if (degree == 2)
        st = SmPolynomial::SolveQuadraticEqn(coeff, tol, n, sol);
    else if (degree == 3)
        st = SmPolynomial::SolveCubicEqn(coeff, tol, n, sol);
    else
        st = SmPolynomial::SolveQuarticEqn(coeff, tol, n, sol);
    if (n > 4)
        return SC_ERR_BUFFER;
    *out_n = (int64_t)n;
    *out_status = (int32_t)st;
    for (ULONG i = 0; i < n; ++i)
        out[i] = sol[i];
    return SC_OK;
    SC_CATCH
}
