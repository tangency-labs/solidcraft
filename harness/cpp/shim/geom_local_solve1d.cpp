// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.local_solve1d`.
// Wraps upstream SmLocalSolve1d / SmIterQueue1d / SmEvalFunctionObject
// (source/SMLib/inc/SmLocalSolve1d.h, source/SMLib/src/SmLocalSolve1d.cpp).
//
// The solver is driven by a synthetic test function (ScTestFn below). The Rust
// runner (crates/difftest/src/units/geom_local_solve1d.rs) implements the same
// function with the same operation order; keep the two in sync.
#include "sc_shim.h"

#include <SmLocalSolve1d.h>

#include <cmath>
#include <vector>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_local_solve1d.rs).
enum ScLs1dOp : int32_t
{
    OP_SOLVE_IT = 0,  // one or more SolveIt calls on one solver
    OP_BISECTION = 1, // SolveByBisection
    OP_BRENT = 2,     // SolveByBrent
    OP_QUEUE = 3,     // SmIterQueue1d::Add + IsOscillating
};

// Function kinds (fn_params[0]).
enum ScLs1dKind : int32_t
{
    K_POLY = 0,  // sum c[i] t^i, i < 8 (Horner)
    K_SQRT = 1,  // a*sgn(t-r)*sqrt|t-r| + b
    K_TRIG = 2,  // a*sin(w t + ph) + b t + c
    K_ATAN = 3,  // a*atan(w (t-r)) + c
    K_CBRT = 4,  // a*cbrt(t-r) + c
    K_ABS = 5,   // a*|t-r| + b
    K_GAUSS = 6, // a*exp(-w (t-r)^2) + c
    K_BASE = 99, // SmEvalFunctionObject::Evaluate (base class)
};

namespace
{
const int64_t N_FN_PARAMS = 14; // kind, p[8], found_tol, fail_lo, fail_hi, fail_first, failure_write

// Test function.
//   fn_params: [kind, p0..p7, found_tol, fail_lo, fail_hi, fail_first, failure_write]
// Fails (SM_ERR_INVALID_INPUT) on the first `fail_first` calls and for
// fail_lo <= t <= fail_hi. Failure writes: 0 keeps outputs, 1 writes F,
// 2 zeroes F and F'. Always sets the found flag FALSE on failure.
// Records every call (t, ok, signal).
class ScTestFn : public SmEvalFunctionObject
{
public:
    explicit ScTestFn(const double* fp) : m_fp(fp) {}

    SmStatus Evaluate(double dT, double& rdF, double& rdFp, SmBoolean& rbFound,
                      SmBoolean bSignalErrors = TRUE) override
    {
        m_calls++;
        m_t.push_back(dT);
        int32_t kind = (int32_t)m_fp[0];
        const double* p = m_fp + 1;
        if (kind == K_BASE)
        {
            m_flags.push_back(0.0 + (bSignalErrors ? 2.0 : 0.0));
            // SolveIt reads rbFound after a failed first evaluation without an
            // interval (uninitialized upstream): make it defined.
            rbFound = FALSE;
            return SmEvalFunctionObject::Evaluate(dT, rdF, rdFp, rbFound, bSignalErrors);
        }
        double f = 0.0, fp = 0.0;
        switch (kind)
        {
        case K_POLY:
        {
            f = p[7];
            fp = 0.0;
            for (int i = 6; i >= 0; i--)
            {
                fp = fp * dT + f;
                f = f * dT + p[i];
            }
            break;
        }
        case K_SQRT:
        {
            double d = dT - p[1];
            double s = std::sqrt(std::fabs(d));
            f = (d >= 0.0 ? p[0] * s : -(p[0] * s)) + p[2];
            fp = s > 0.0 ? p[0] * 0.5 / s : 0.0;
            break;
        }
        case K_TRIG:
        {
            double arg = p[1] * dT + p[2];
#if defined(__APPLE__)
            // Apple clang combines the direct calls into __sincos_stret, whose
            // rounding can differ from the separate calls in the Rust evaluator.
            // Volatile function pointers keep this synthetic test function on
            // the same platform sin/cos paths without changing kernel flags.
            double (*volatile sine)(double) = std::sin;
            double (*volatile cosine)(double) = std::cos;
            f = p[0] * sine(arg) + p[3] * dT + p[4];
            fp = p[0] * p[1] * cosine(arg) + p[3];
#else
            f = p[0] * std::sin(arg) + p[3] * dT + p[4];
            fp = p[0] * p[1] * std::cos(arg) + p[3];
#endif
            break;
        }
        case K_ATAN:
        {
            double u = p[1] * (dT - p[2]);
            f = p[0] * std::atan(u) + p[3];
            fp = p[0] * p[1] / (1.0 + u * u);
            break;
        }
        case K_CBRT:
        {
            double q = std::cbrt(dT - p[1]);
            f = p[0] * q + p[2];
            fp = q != 0.0 ? p[0] / (3.0 * q * q) : 0.0;
            break;
        }
        case K_ABS:
        {
            double d = dT - p[1];
            f = p[0] * std::fabs(d) + p[2];
            fp = d >= 0.0 ? p[0] : -p[0];
            break;
        }
        case K_GAUSS:
        {
            double d = dT - p[2];
            double e = std::exp(-p[1] * d * d);
            f = p[0] * e + p[3];
            fp = -2.0 * p[0] * p[1] * d * e;
            break;
        }
        default:
            break;
        }
        double found_tol = m_fp[9];
        double fail_lo = m_fp[10];
        double fail_hi = m_fp[11];
        double fail_first = m_fp[12];
        bool partial = m_fp[13] != 0.0;
        bool fail = (double)m_calls <= fail_first || (fail_lo <= dT && dT <= fail_hi);
        if (fail)
        {
            if (m_fp[13] == 2.0)
            {
                rdF = 0.0;
                rdFp = 0.0;
            }
            else if (partial)
                rdF = f;
            rbFound = FALSE;
            m_flags.push_back(0.0 + (bSignalErrors ? 2.0 : 0.0));
            return SM_ERR_INVALID_INPUT;
        }
        rdF = f;
        rdFp = fp;
        rbFound = std::fabs(f) < found_tol ? TRUE : FALSE;
        m_flags.push_back(1.0 + (bSignalErrors ? 2.0 : 0.0));
        return SM_SUCCESS;
    }

    int64_t m_calls = 0;
    std::vector<double> m_t;
    std::vector<double> m_flags; // ok + 2 * signal
private:
    const double* m_fp;
};

void push_iv(std::vector<double>& o, const SmIterationValue& v)
{
    o.push_back(v.m_dT);
    o.push_back(v.m_dFOfT);
    o.push_back(v.m_dFPrimeOfT);
    o.push_back(v.m_dPrevStep);
}

void push_trace(std::vector<double>& o, const ScTestFn& fn)
{
    o.push_back((double)fn.m_t.size());
    o.insert(o.end(), fn.m_t.begin(), fn.m_t.end());
    o.insert(o.end(), fn.m_flags.begin(), fn.m_flags.end());
}
} // namespace

// Runs one operation.
//   fn_params[14]: test function (see ScTestFn)
//   s[n_s]: operands
//     OP_SOLVE_IT : [use_interval, lo, hi, periodic, algorithm, has_desired, desired,
//                    acceptable, guess...] -> per guess [status, found, t, found_accuracy,
//                    termination, lower_hits, upper_hits, iter, have_lo, lo_f, lo_t,
//                    have_hi, hi_f, hi_t, curr(4), prev1(4), prev2(4)] (found/t are 0
//                    when status != 0), then the trace
//     OP_BISECTION: [level, lo_t, hi_t, lo_f, hi_f] -> [status, t, f] + trace
//     OP_BRENT    : [ax, bx, cx, tol, maximize] -> [status, t, f] + trace
//     OP_QUEUE    : [size, step, prev_step...] -> IsOscillating(step) after each Add
//   trace: [n, t[n], flags[n]]
SC_EXPORT int32_t sc_geom_local_solve1d_run(int32_t op, const double* fn_params, const double* s,
                                            int64_t n_s, double* out, int64_t cap,
                                            int64_t* out_len)
{
    SC_TRY
    std::vector<double> o;
    ScTestFn fn(fn_params);
    switch (op)
    {
    case OP_SOLVE_IT:
    {
        if (n_s < 8)
            return SC_ERR_ARG;
        SmExtent1d ext(s[1], s[2]);
        SmLocalSolve1d* solver = (s[0] != 0.0)
                                     ? new SmLocalSolve1d(fn, ext, s[3] != 0.0 ? TRUE : FALSE)
                                     : new SmLocalSolve1d(fn);
        solver->SetSolverAlgorithm((SmLocalSolverAlgorithmType)(int32_t)s[4]);
        if (s[5] != 0.0)
            solver->SetDesiredAccuracy(s[6]);
        for (int64_t g = 8; g < n_s; g++)
        {
            SmBoolean bFound = FALSE;
            double dT = 0.0;
            SmStatus st = solver->SolveIt(s[g], s[7], bFound, dT);
            o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
            o.push_back(st == SM_SUCCESS ? (double)bFound : 0.0);
            o.push_back(st == SM_SUCCESS ? dT : 0.0);
            o.push_back(solver->GetFoundAccuracy());
            o.push_back((double)solver->GetTerminationReason());
            o.push_back((double)solver->GetLowerBoundHits());
            o.push_back((double)solver->GetUpperBoundHits());
            o.push_back((double)solver->m_lIter);
            o.push_back((double)solver->m_bHaveLowerBracket);
            o.push_back(solver->m_dLowerBracketF);
            o.push_back(solver->m_dLowerBracketT);
            o.push_back((double)solver->m_bHaveUpperBracket);
            o.push_back(solver->m_dUpperBracketF);
            o.push_back(solver->m_dUpperBracketT);
            push_iv(o, solver->m_vCurr);
            push_iv(o, solver->m_v1Prev);
            push_iv(o, solver->m_v2Prev);
        }
        delete solver;
        push_trace(o, fn);
        break;
    }
    case OP_BISECTION:
    {
        if (n_s < 5)
            return SC_ERR_ARG;
        SmLocalSolve1d solver(fn);
        double dT = 0.0, dF = 0.0;
        SmStatus st = solver.SolveByBisection((ULONG)s[0], s[1], s[2], s[3], s[4], dT, dF);
        o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
        o.push_back(st == SM_SUCCESS ? dT : 0.0);
        o.push_back(st == SM_SUCCESS ? dF : 0.0);
        push_trace(o, fn);
        break;
    }
    case OP_BRENT:
    {
        if (n_s < 5)
            return SC_ERR_ARG;
        SmLocalSolve1d solver(fn);
        double dT = 0.0, dF = 0.0;
        SmStatus st = solver.SolveByBrent(s[0], s[1], s[2], s[3], s[4] != 0.0 ? TRUE : FALSE, dT, dF);
        o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
        o.push_back(st == SM_SUCCESS ? dT : 0.0);
        o.push_back(st == SM_SUCCESS ? dF : 0.0);
        push_trace(o, fn);
        break;
    }
    case OP_QUEUE:
    {
        if (n_s < 2 || s[0] < 1.0)
            return SC_ERR_ARG;
        SmIterQueue1d q((ULONG)s[0]);
        for (int64_t i = 2; i < n_s; i++)
        {
            SmIterationValue v;
            v.m_dPrevStep = s[i];
            q.Add(v);
            o.push_back((double)q.IsOscillating((ULONG)s[1]));
        }
        break;
    }
    default:
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
