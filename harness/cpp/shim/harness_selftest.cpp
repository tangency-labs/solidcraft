// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `harness.selftest`.
// Wraps upstream N_BasisEval (source/SMLib/src/NL_BasisBasic.cpp).
#include "sc_shim.h"

#include <nurbs.h>

#include <algorithm>
#include <stdexcept>
#include <thread>
#include <vector>

// Pure-C sanity probe: lets the harness verify the shim lib links at all.
SC_EXPORT int32_t sc_harness_selftest_ping(double x, double* out)
{
    SC_TRY
    *out = x * 2.0 + 1.0;
    return SC_OK;
    SC_CATCH
}

// Deterministic lock regression: another thread must fail try_lock while this
// entrypoint holds SC_TRY's guard. Mode 1 tests unwinding; mode 2 nests a shim.
SC_EXPORT int32_t sc_harness_selftest_native_guard(int32_t mode, int64_t* available)
{
    if (!available || mode < 0 || mode > 2) return SC_ERR_ARG;
    SC_TRY
    bool acquired = false;
    std::thread observer([&] {
        if (sc_native_mutex().try_lock()) {
            acquired = true;
            sc_native_mutex().unlock();
        }
    });
    observer.join();
    *available = acquired ? 1 : 0;
    if (mode == 1) throw std::runtime_error("guard unwind probe");
    if (mode == 2) {
        double value = 0.0;
        int32_t status = sc_harness_selftest_ping(3.0, &value);
        if (status != SC_OK || value != 7.0) return SC_ERR_ARG;
    }
    return SC_OK;
    SC_CATCH
}

// N_BasisEval(knt, p, u, flg, N, spn).
//   knots[n_knots]  : knot vector U (m = n_knots - 1)
//   degree          : p (caller guarantees 0 <= p <= NL_MAXDEG and n_knots >= 2p+2)
//   flag            : NL_LEFT (1) or NL_RIGHT (2)
//   out_basis[cap]  : receives N[0..=p] (cap must be >= p+1)
//   out_span        : receives the span index
// Returns the upstream error flag (0 ok, 1 error) or an SC_ERR_* code.
SC_EXPORT int32_t sc_harness_selftest_basis_eval(const double* knots, int64_t n_knots, int32_t degree,
                                                 double u, int32_t flag, double* out_basis,
                                                 int64_t cap, int64_t* out_span)
{
    if (n_knots < 2 || degree < 0 || degree > NL_MAXDEG || cap < degree + 1 || n_knots < 2 * degree + 2)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt;
    knt.m = (NL_INDEX)(n_knots - 1);
    knt.U = const_cast<NL_REAL*>(knots);
    NL_INDEX spn = -1;
    NL_FLAG err = N_BasisEval(&knt, (NL_DEGREE)degree, u, (NL_FLAG)flag, out_basis, &spn);
    *out_span = (int64_t)spn;
    return (int32_t)err;
    SC_CATCH
}

// Variable-length output probe: writes `in` reversed via sc_copy_out
// (exercises the SC_ERR_BUFFER retry path of solidcore_ffi::call_with_buffer).
SC_EXPORT int32_t sc_harness_selftest_reverse(const double* in, int64_t n, double* out, int64_t cap,
                                              int64_t* out_len)
{
    if (n < 0)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> r(in, in + n);
    std::reverse(r.begin(), r.end());
    return sc_copy_out(r.data(), (int64_t)r.size(), out, cap, out_len);
    SC_CATCH
}
