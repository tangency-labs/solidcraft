// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.local_solve_nd`.
// Wraps upstream SmLocalSolveNd / SmEvalNFunctionsObject (source/SMLib/inc/SmLocalSolveNd.h,
// source/SMLib/src/SmLocalSolveNd.cpp) driven by a test equation set that the Rust runner
// (crates/difftest/src/units/geom_local_solve_nd.rs) implements operation for operation.
#include "sc_shim.h"

#include <SmLocalSolveNd.h>

#include <cmath>
#include <vector>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_local_solve_nd.rs).
enum ScLsNdOp : int32_t
{
    OP_SOLVE = 0,    // SmLocalSolveNd(...).SolveIt(guess, acceptable)
    OP_JACOBIAN = 1, // Evaluate(guess) then ComputeJacobian(guess, F, J, intervals?)
};

// cfg[] layout
enum
{
    CFG_JAC_SCALE = 0,
    CFG_FAIL_RADIUS,
    CFG_FOUND_TOL,
    CFG_SIZE_SCALE,
    CFG_SCALED_ZERO,
    CFG_DESIRED,
    CFG_ACCEPTABLE,
    CFG_LEN
};

// icfg[] layout
enum
{
    ICFG_JAC_MODE = 0,     // 0 analytic, 1 ComputeJacobian, 2 ComputeJacobian with intervals
    ICFG_HAS_INTERVALS,    // HasIntervals() result
    ICFG_SET_INTERVALS_OK, // 1: SetIntervals overridden (SM_SUCCESS), 0: base (SM_ERR)
    ICFG_SET_DESIRED,      // 1: SetDesiredAccuracy(cfg[CFG_DESIRED])
    ICFG_MAX_ITER,         // -1: default, else SetMaximumIterations
    ICFG_STEP_CUTTING,     // -1: default, else SetStepCuttingEnabled
    ICFG_HANDLER,          // -1: default, else SetBoundaryHandler(handler, max_hits)
    ICFG_MAX_HITS,         // -1: SetBoundaryHandler default argument (3)
    ICFG_USE_INTERVALS,    // pass intervals to the solver
    ICFG_USE_PERIODIC,     // pass periodicities to the solver
    ICFG_LEN
};

// Trace flags per Evaluate call.
static const int64_t TRACE_JACOBIAN = 1; // a Jacobian was requested
static const int64_t TRACE_FAILED = 2;   // the call returned SM_ERR

// F_i = -b_i + sum_j a_ij x_j + c_i x_i^3 + d_i x_i x_k + e_i |x_i|, k = (i+1) % n.
class ScTestENFO : public SmEvalNFunctionsObject
{
public:
    int64_t n = 0;
    const double* a = nullptr;
    const double* b = nullptr;
    const double* c = nullptr;
    const double* d = nullptr;
    const double* e = nullptr;
    double jac_scale = 1.0;
    double fail_radius = 0.0;
    double found_tol = 0.0;
    double size_scale = 0.0;
    int64_t jac_mode = 0;
    bool has_intervals = false;
    bool set_intervals_ok = false;
    int64_t set_intervals_calls = 0;
    const int64_t* fail_calls = nullptr;
    int64_t n_fail = 0;
    SmTArray<SmExtent1d>* jac_intervals = nullptr;
    int64_t calls = 0;
    std::vector<double> trace;
    std::vector<int64_t> flags;

    SmBoolean HasIntervals() override { return has_intervals ? TRUE : FALSE; }

    SmStatus SetIntervals(const SmExtentNd* cpIntervals,
                          const SmTArray<SmBoolean>* cpPeriodicities) override
    {
        if (!set_intervals_ok)
            return SmEvalNFunctionsObject::SetIntervals(cpIntervals, cpPeriodicities);
        set_intervals_calls++;
        return SM_SUCCESS;
    }

    SmStatus Evaluate(const SmTArray<double>& crX, SmTArray<double>& rF, SmMatrix* pOptJacobian,
                      SmBoolean& rbFoundAnswer) override
    {
        int64_t call = calls++;
        for (int64_t i = 0; i < n; i++)
            trace.push_back(crX[(ULONG)i]);
        int64_t flag = pOptJacobian ? TRACE_JACOBIAN : 0;
        bool fail = false;
        for (int64_t k = 0; k < n_fail; k++)
            if (fail_calls[k] == call)
                fail = true;
        if (fail_radius > 0.0)
            for (int64_t i = 0; i < n; i++)
                if (fabs(crX[(ULONG)i]) > fail_radius)
                    fail = true;
        if (fail)
        {
            flags.push_back(flag | TRACE_FAILED);
            return SM_ERR;
        }
        flags.push_back(flag);

        double dSum = 0.0;
        for (int64_t i = 0; i < n; i++)
        {
            int64_t k = (i + 1) % n;
            double xi = crX[(ULONG)i];
            double v = -b[i];
            for (int64_t j = 0; j < n; j++)
                v += a[i * n + j] * crX[(ULONG)j];
            v += c[i] * xi * xi * xi;
            v += d[i] * xi * crX[(ULONG)k];
            v += e[i] * fabs(xi);
            rF[(ULONG)i] = v;
            dSum += fabs(v);
        }
        m_dSolutionSize = size_scale * dSum;

        if (pOptJacobian)
        {
            if (jac_mode == 0)
            {
                for (int64_t i = 0; i < n; i++)
                {
                    int64_t k = (i + 1) % n;
                    double xi = crX[(ULONG)i];
                    for (int64_t j = 0; j < n; j++)
                    {
                        double g = a[i * n + j];
                        if (j == i)
                        {
                            g += 3.0 * c[i] * xi * xi;
                            g += d[i] * crX[(ULONG)k];
                            g += e[i] * (xi >= 0.0 ? 1.0 : -1.0);
                        }
                        if (j == k)
                            g += d[i] * xi;
                        g *= jac_scale;
                        (*pOptJacobian)[(ULONG)i][(ULONG)j] = g;
                    }
                }
            }
            else
            {
                SmTArray<double> sTmpX(crX);
                SmStatus st = ComputeJacobian(sTmpX, rF, *pOptJacobian,
                                              jac_mode == 2 ? jac_intervals : NULL);
                if (st != SM_SUCCESS)
                    return st;
            }
        }
        // set only on success: the Rust trait reports "found" through Ok(found)
        rbFoundAnswer = (found_tol > 0.0 && dSum < found_tol) ? TRUE : FALSE;
        return SM_SUCCESS;
    }
};

static int32_t put(const std::vector<double>& v, double* out, int64_t cap, int64_t& k)
{
    for (double x : v)
    {
        if (k < cap)
            out[k] = x;
        k++;
    }
    return 0;
}

// Runs one operation on the test equation set of size n.
//   coef[n*n + 4n] : a (row-major), b, c, d, e
//   cfg[7], icfg[10]: see the enums above
//   guess[n]       : start point (OP_SOLVE) / evaluation point (OP_JACOBIAN)
//   ivl[2n]        : interval lo, hi per parameter; ivl_flags[n]: bit0 unbounded, bit1 periodic
//   fail_calls[n_fail]: Evaluate call indices (0-based) that return SM_ERR
// Outputs:
//   OP_SOLVE   : out = x[n], trace; ints = [found, reason, iteration, calls,
//                set_intervals_calls, flags...]
//   OP_JACOBIAN: out = J[n*n] (row-major), x[n], trace; ints = [calls, flags...]
//   *status    : SmStatus of SolveIt / ComputeJacobian, SM_SUCCESS reported as 0.
// Returns 0, SC_ERR_ARG or SC_ERR_BUFFER (with *out_len / *ints_len set to the
// needed sizes).
SC_EXPORT int32_t sc_geom_local_solve_nd_run(int32_t op, int64_t n, const double* coef,
                                             const double* cfg, const int64_t* icfg,
                                             const double* guess, const double* ivl,
                                             const int64_t* ivl_flags, const int64_t* fail_calls,
                                             int64_t n_fail, double* out, int64_t cap,
                                             int64_t* out_len, int64_t* ints, int64_t ints_cap,
                                             int64_t* ints_len, int32_t* status)
{
    if (n < 1 || n > 8 || n_fail < 0 || n_fail > 64)
        return SC_ERR_ARG;
    if (icfg[ICFG_JAC_MODE] < 0 || icfg[ICFG_JAC_MODE] > 2)
        return SC_ERR_ARG;
    if (icfg[ICFG_MAX_ITER] > 1000 || icfg[ICFG_HANDLER] > 2)
        return SC_ERR_ARG;
    SC_TRY
    ScTestENFO sF;
    sF.n = n;
    sF.a = coef;
    sF.b = coef + n * n;
    sF.c = coef + n * n + n;
    sF.d = coef + n * n + 2 * n;
    sF.e = coef + n * n + 3 * n;
    sF.jac_scale = cfg[CFG_JAC_SCALE];
    sF.fail_radius = cfg[CFG_FAIL_RADIUS];
    sF.found_tol = cfg[CFG_FOUND_TOL];
    sF.size_scale = cfg[CFG_SIZE_SCALE];
    sF.m_dScaledZero = cfg[CFG_SCALED_ZERO];
    sF.jac_mode = icfg[ICFG_JAC_MODE];
    sF.has_intervals = icfg[ICFG_HAS_INTERVALS] != 0;
    sF.set_intervals_ok = icfg[ICFG_SET_INTERVALS_OK] != 0;
    sF.fail_calls = fail_calls;
    sF.n_fail = n_fail;

    SmExtentNd sIntervals((ULONG)n);
    SmTArray<SmBoolean> sPeriodic;
    SmTArray<SmExtent1d> sJacIntervals;
    for (int64_t i = 0; i < n; i++)
    {
        SmExtent1d sE(ivl[2 * i], ivl[2 * i + 1]);
        if (ivl_flags[i] & 1)
            sE.SetUnbounded();
        sIntervals[(ULONG)i] = sE;
        sJacIntervals.Add(sE);
        sPeriodic.Add((ivl_flags[i] & 2) ? TRUE : FALSE);
    }
    sF.jac_intervals = &sJacIntervals;

    SmTArray<double> sGuess;
    for (int64_t i = 0; i < n; i++)
        sGuess.Add(guess[i]);

    std::vector<double> vOut;
    std::vector<int64_t> vInts;
    SmStatus st = SM_SUCCESS;
    if (op == OP_SOLVE)
    {
        SmLocalSolveNd sLS(sF, icfg[ICFG_USE_INTERVALS] ? &sIntervals : NULL,
                           icfg[ICFG_USE_PERIODIC] ? &sPeriodic : NULL);
        sLS.m_lCurrentIteration = 0; // not initialised by the constructor
        if (icfg[ICFG_SET_DESIRED])
            sLS.SetDesiredAccuracy(cfg[CFG_DESIRED]);
        if (icfg[ICFG_MAX_ITER] >= 0)
            sLS.SetMaximumIterations((ULONG)icfg[ICFG_MAX_ITER]);
        if (icfg[ICFG_STEP_CUTTING] >= 0)
            sLS.SetStepCuttingEnabled(icfg[ICFG_STEP_CUTTING] ? TRUE : FALSE);
        if (icfg[ICFG_HANDLER] >= 0)
        {
            SmBoundaryHandlerType eH = (SmBoundaryHandlerType)icfg[ICFG_HANDLER];
            if (icfg[ICFG_MAX_HITS] >= 0)
                sLS.SetBoundaryHandler(eH, (ULONG)icfg[ICFG_MAX_HITS]);
            else
                sLS.SetBoundaryHandler(eH);
        }
        SmBoolean bFound = FALSE;
        SmTArray<double> sSol;
        st = sLS.SolveIt(sGuess, cfg[CFG_ACCEPTABLE], bFound, sSol);
        if (st == SM_SUCCESS)
        {
            for (ULONG i = 0; i < sSol.GetSize(); i++)
                vOut.push_back(sSol[i]);
            vInts.push_back(bFound ? 1 : 0);
            vInts.push_back((int64_t)sLS.GetTerminationReason());
            vInts.push_back((int64_t)sLS.m_lCurrentIteration);
        }
        else
        {
            vInts.push_back(0);
            vInts.push_back(0);
            vInts.push_back(0);
        }
        vInts.push_back(sF.calls);
        vInts.push_back(sF.set_intervals_calls);
    }
    else if (op == OP_JACOBIAN)
    {
        SmTArray<double> sFv;
        sFv.SetSize((ULONG)n);
        SmBoolean bFound = FALSE;
        st = sF.Evaluate(sGuess, sFv, NULL, bFound);
        SmMatrix sJ((ULONG)n, (ULONG)n);
        sJ.Clear();
        if (st == SM_SUCCESS)
            st = sF.ComputeJacobian(sGuess, sFv, sJ,
                                    icfg[ICFG_USE_INTERVALS] ? &sJacIntervals : NULL);
        for (ULONG r = 0; r < (ULONG)n; r++)
            for (ULONG cc = 0; cc < (ULONG)n; cc++)
                vOut.push_back(sJ.GetAt(r, cc));
        for (ULONG i = 0; i < (ULONG)n; i++)
            vOut.push_back(sGuess[i]);
        vInts.push_back(sF.calls);
    }
    else
    {
        return SC_ERR_ARG;
    }
    vOut.insert(vOut.end(), sF.trace.begin(), sF.trace.end());
    vInts.insert(vInts.end(), sF.flags.begin(), sF.flags.end());
    *status = (st == SM_SUCCESS) ? 0 : (int32_t)st;

    *out_len = (int64_t)vOut.size();
    *ints_len = (int64_t)vInts.size();
    if (*out_len > cap || *ints_len > ints_cap)
        return SC_ERR_BUFFER;
    int64_t k = 0;
    put(vOut, out, cap, k);
    for (size_t i = 0; i < vInts.size(); i++)
        ints[i] = vInts[i];
    return SC_OK;
    SC_CATCH
}
