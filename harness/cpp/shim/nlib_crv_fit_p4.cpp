// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_fit_p4`.
// Wraps N_FitCrvApproxDerivs (with N_FitCalcKnotsEndDerivs) of upstream
// source/SMLib/src/NL_CrvFit.cpp, SmBSplineCurve::CreateApproximatingCurve
// (source/SMLib/src/SmBSplineCurveNLib.cpp) and SmApiCreateCurveApproxPoints
// (source/SM_API/src/SmApiCurves.cpp).
//
// One entry point, sc_nlib_crv_fit_p4_eval(op, in, n_in, out, cap, out_len). Layouts
// (validated by crates/solidcore-ffi/src/units/nlib_crv_fit_p4.rs):
//   op 0 N_FitCrvApproxDerivs:      n p par k l count (x y z)*count (x y z)*k (x y z)*l
//                                   (par: NL_UNIFORM 1, NL_CHORDLENGTH 2, NL_CENTRIPETAL 3)
//   op 1 CreateApproximatingCurve:  num_points param dim degree count (x y z)*count
//                                   nvec (x y z)*nvec has_more nmore (x y z)*nmore
//                                   (param: 0 uniform, 1 chord length, 2 centripetal)
//   op 2 SmApiCreateCurveApproxPoints: count (x y z)*count
// Output: status (op 0: 0 or NL_ERROR.eno, 1000 if unset; ops 1, 2: 0 or the SmStatus),
// then on success: ops 1, 2 the curve's dimension; all ops `n p m`, the control points
// (x y z w) and the knots of the NURB.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiCurves.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;
using sc::null_crv;

namespace
{
struct Reader
{
    const double* in;
    int64_t at;
    double next() { return in[at++]; }
    long idx() { return (long)in[at++]; }
};

std::vector<NL_POINT> read_epts(Reader& r, long c)
{
    std::vector<NL_POINT> P(c > 0 ? c : 1);
    for (long i = 0; i < c; i++)
    {
        P[i].x = r.next();
        P[i].y = r.next();
        P[i].z = r.next();
    }
    return P;
}

SmTArray<SmPoint3d> read_sm_pts(Reader& r, long c)
{
    SmTArray<SmPoint3d> P;
    for (long i = 0; i < c; i++)
    {
        double x = r.next(), y = r.next(), z = r.next();
        P.Add(SmPoint3d(x, y, z));
    }
    return P;
}

SmTArray<SmVector3d> read_sm_vecs(Reader& r, long c)
{
    SmTArray<SmVector3d> V;
    for (long i = 0; i < c; i++)
    {
        double x = r.next(), y = r.next(), z = r.next();
        V.Add(SmVector3d(x, y, z));
    }
    return V;
}

void bspline_result(std::vector<double>& o, SmStatus st, SmBSplineCurve* c)
{
    o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
    if (st == SM_SUCCESS && c != NULL)
    {
        o.push_back((double)c->GetDim());
        dump_crv(o, (NL_CURVE*)c->GetOrCreateGwNurbPointer());
    }
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_fit_p4_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    Reader r{in, 0};
    if (op == 0)
    {
        long n = r.idx();
        long p = r.idx();
        int par = (int)r.idx();
        long k = r.idx();
        long l = r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_epts(r, c);
        NL_VECTOR Ds[5], De[5];
        for (long i = 1; i <= k; i++)
        {
            Ds[i].x = r.next();
            Ds[i].y = r.next();
            Ds[i].z = r.next();
        }
        for (long i = 1; i <= l; i++)
        {
            De[i].x = r.next();
            De[i].y = r.next();
            De[i].z = r.next();
        }
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvApproxDerivs(P.data(), c - 1, Ds, k, De, l, n, (NL_DEGREE)p, par, cur, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_crv(o, cur);
    }
    else if (op == 1)
    {
        ULONG num = (ULONG)r.idx();
        ULONG param = (ULONG)r.idx();
        ULONG dim = (ULONG)r.idx();
        ULONG degree = (ULONG)r.idx();
        long c = r.idx();
        SmTArray<SmPoint3d> P = read_sm_pts(r, c);
        long nvec = r.idx();
        SmTArray<SmVector3d> V = read_sm_vecs(r, nvec);
        bool has_more = r.idx() != 0;
        long nmore = r.idx();
        SmTArray<SmVector3d> M = read_sm_vecs(r, nmore);
        SmContext ctx;
        SmBSplineCurve* c_out = NULL;
        SmStatus st = SmBSplineCurve::CreateApproximatingCurve(ctx, num, param, dim, degree, P, V,
                                                               has_more ? &M : NULL, c_out);
        bspline_result(o, st, c_out);
        delete c_out;
    }
    else
    {
        long c = r.idx();
        SmTArray<SmPoint3d> P = read_sm_pts(r, c);
        SmCurve* res = NULL;
        SmStatus st = (SmStatus)SmApiCreateCurveApproxPoints(P, res);
        bspline_result(o, st, (SmBSplineCurve*)res);
        delete res;
    }
    N_EndNurbs(&S);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
