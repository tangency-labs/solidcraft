// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.bezier`.
// Wraps the Bezier routines of upstream source/SMLib/src/NL_Bezier.cpp owned by the unit.
//
// One entry point, sc_nlib_bezier_eval(op, in, n_in, out, cap, out_len): `op` selects
// an upstream routine, `in` holds its inputs, `out` receives a flat record of doubles
// (layouts in crates/difftest/src/units/nlib_bezier.rs). The first value of every
// record is the upstream status: 0 = ok, else NL_ERROR.eno (cleared before the call;
// 1000 if the flag was set without a number). Outputs follow only on success.
// Control points are x y z w. The Rust wrapper validates every degree, index and
// array length, so upstream only touches the memory it is given.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::nl_status;

namespace
{

struct Reader
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    NL_INDEX idx() { return (NL_INDEX)in[at++]; }
    NL_CPOINT cpt()
    {
        NL_CPOINT c;
        c.x = in[at];
        c.y = in[at + 1];
        c.z = in[at + 2];
        c.w = in[at + 3];
        at += 4;
        return c;
    }
    std::vector<NL_CPOINT> cpts(int64_t k)
    {
        std::vector<NL_CPOINT> v((size_t)k);
        for (int64_t i = 0; i < k; i++)
            v[(size_t)i] = cpt();
        return v;
    }
    std::vector<double> nums(int64_t k)
    {
        std::vector<double> v((size_t)k);
        for (int64_t i = 0; i < k; i++)
            v[(size_t)i] = num();
        return v;
    }
};

void put(std::vector<double>& o, const NL_CPOINT& c)
{
    o.push_back(c.x);
    o.push_back(c.y);
    o.push_back(c.z);
    o.push_back(c.w);
}

NL_CPOINT czero()
{
    NL_CPOINT c;
    c.x = c.y = c.z = c.w = 0.0;
    return c;
}

// Reads one surface (same encoding as the nlib.srf_basic shim), allocated on S.
NL_SURFACE* read_srf(Reader& rd, NL_STACKS* S)
{
    NL_INDEX N = rd.idx(), M = rd.idx(), R = rd.idx(), SS = rd.idx();
    NL_INDEX n = rd.idx(), m = rd.idx();
    NL_DEGREE p = (NL_DEGREE)rd.idx(), q = (NL_DEGREE)rd.idx();
    NL_INDEX r = rd.idx(), s = rd.idx();
    NL_SURFACE* sur = N_AllocSrfAndArrays(N, M, p, q, R, SS, S);
    if (sur == NULL)
        return NULL;
    for (NL_INDEX i = 0; i <= N; i++)
        for (NL_INDEX j = 0; j <= M; j++)
            sur->net->Pw[i][j] = rd.cpt();
    for (NL_INDEX i = 0; i <= R; i++)
        sur->knu->U[i] = rd.num();
    for (NL_INDEX i = 0; i <= SS; i++)
        sur->knv->U[i] = rd.num();
    N_SrfSetSizeIndices(sur, n, m, p, q, r, s);
    return sur;
}

void dump_srf(std::vector<double>& o, NL_SURFACE* sur)
{
    NL_INDEX n, m, r, s;
    NL_DEGREE p, q;
    NL_CPOINT** Pw;
    NL_REAL *U, *V;
    N_SrfGetCPtsDegreesAndKnots(sur, &n, &m, &Pw, &p, &q, &r, &s, &U, &V);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)p);
    o.push_back((double)q);
    o.push_back((double)r);
    o.push_back((double)s);
    for (NL_INDEX i = 0; i <= n; i++)
        for (NL_INDEX j = 0; j <= m; j++)
            put(o, Pw[i][j]);
    for (NL_INDEX i = 0; i <= r; i++)
        o.push_back(U[i]);
    for (NL_INDEX j = 0; j <= s; j++)
        o.push_back(V[j]);
}

// Band of an elevation (lo = i - t) or product (lo = i - q) matrix.
void dump_band(std::vector<double>& o, NL_RMATRIX* mat, NL_INDEX rows, NL_INDEX lo_shift, NL_INDEX hi)
{
    NL_REAL** A;
    N_GetRealMatrixPtr(mat, &A);
    for (NL_INDEX i = 0; i <= rows; i++)
    {
        NL_INDEX jl = i - lo_shift > 0 ? i - lo_shift : 0;
        NL_INDEX jh = i < hi ? i : hi;
        for (NL_INDEX j = jl; j <= jh; j++)
            o.push_back(A[i][j]);
    }
}
} // namespace

SC_EXPORT int32_t sc_nlib_bezier_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                      int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    std::vector<double> o;
    Reader rd{in, 0};
    NL_STACKS S;
    N_InitNurbs(&S);
    int32_t rc = SC_OK;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (op)
    {
        case 0: // N_BezGetDegreeElevationMatrix: p t
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_INDEX t = rd.idx();
            NL_RMATRIX dm;
            N_InitRealMatrix(&dm);
            err = N_BezGetDegreeElevationMatrix(p, t, &dm, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_band(o, &dm, p + t, t, p);
            break;
        }
        case 1: // N_BezFuncMultiplyBezCrvMatrix: p q
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx(), q = (NL_DEGREE)rd.idx();
            NL_RMATRIX pm;
            N_InitRealMatrix(&pm);
            err = N_BezFuncMultiplyBezCrvMatrix(p, q, &pm, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_band(o, &pm, p + q, q, p);
            break;
        }
        case 2: // N_BezElevateDegree: p t f l pre, Pw[p+1]
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_INDEX t = rd.idx(), f = rd.idx(), l = rd.idx();
            int pre = (int)rd.num();
            std::vector<NL_CPOINT> Pw = rd.cpts(p + 1);
            std::vector<NL_CPOINT> Qw((size_t)l + 1, czero());
            NL_RMATRIX dm;
            N_InitRealMatrix(&dm);
            if (pre)
                err = N_BezGetDegreeElevationMatrix(p, t, &dm, &S);
            if (err == NL_NO)
                err = N_BezElevateDegree(Pw.data(), p, t, &dm, f, l, Qw.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (NL_INDEX i = f; i <= l; i++)
                    put(o, Qw[(size_t)i]);
            break;
        }
        case 3: // N_BezFuncMultiplyBezFunc: p q s e pre, f[p+1], g[q+1]
        case 4: // N_BezFuncMultiplyBezCrv: p q s e pre, f[p+1], Pw[q+1]
        case 5: // N_BezFuncMultiplyBezCrv4D: same as 4
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx(), q = (NL_DEGREE)rd.idx();
            NL_INDEX s = rd.idx(), e = rd.idx();
            int pre = (int)rd.num();
            std::vector<double> f = rd.nums(p + 1);
            NL_RMATRIX pm;
            N_InitRealMatrix(&pm);
            if (pre)
                err = N_BezFuncMultiplyBezCrvMatrix(p, q, &pm, &S);
            // Output buffers hold up to index max(e, p+q) as upstream requires.
            NL_INDEX len = (e > p + q ? e : p + q) + 1;
            if (op == 3)
            {
                std::vector<double> g = rd.nums(q + 1);
                std::vector<double> fg((size_t)len, 0.0);
                if (err == NL_NO)
                    err = N_BezFuncMultiplyBezFunc(f.data(), p, g.data(), q, &pm, s, e, fg.data());
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    for (NL_INDEX i = s; i <= e; i++)
                        o.push_back(fg[(size_t)i]);
            }
            else
            {
                std::vector<NL_CPOINT> Pw = rd.cpts(q + 1);
                std::vector<NL_CPOINT> Qw((size_t)len, czero());
                if (err == NL_NO)
                {
                    if (op == 4)
                        err = N_BezFuncMultiplyBezCrv(f.data(), p, Pw.data(), q, &pm, s, e, Qw.data());
                    else
                        err = N_BezFuncMultiplyBezCrv4D(f.data(), p, Pw.data(), q, &pm, s, e, Qw.data());
                }
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    for (NL_INDEX i = s; i <= e; i++)
                        put(o, Qw[(size_t)i]);
            }
            break;
        }
        case 6: // N_BezSrfElevateDegree: r t dir f l roc pre rows cols, net[rows][cols]
        {
            NL_DEGREE r = (NL_DEGREE)rd.idx();
            NL_INDEX t = rd.idx();
            NL_FLAG dir = (NL_FLAG)rd.idx();
            NL_INDEX f = rd.idx(), l = rd.idx(), roc = rd.idx();
            int pre = (int)rd.num();
            NL_INDEX rows = rd.idx(), cols = rd.idx();
            std::vector<std::vector<NL_CPOINT>> P((size_t)rows);
            std::vector<NL_CPOINT*> Pp((size_t)rows);
            for (NL_INDEX i = 0; i < rows; i++)
            {
                P[(size_t)i] = rd.cpts(cols);
                Pp[(size_t)i] = P[(size_t)i].data();
            }
            // Output net: (l+1) x cols for u, rows x (l+1) for v (anything else: rows x cols).
            NL_INDEX qr = dir == NL_UDIR ? l + 1 : rows;
            NL_INDEX qc = dir == NL_VDIR ? l + 1 : cols;
            std::vector<std::vector<NL_CPOINT>> Q((size_t)qr, std::vector<NL_CPOINT>((size_t)qc, czero()));
            std::vector<NL_CPOINT*> Qp((size_t)qr);
            for (NL_INDEX i = 0; i < qr; i++)
                Qp[(size_t)i] = Q[(size_t)i].data();
            NL_RMATRIX dm;
            N_InitRealMatrix(&dm);
            if (pre)
                err = N_BezGetDegreeElevationMatrix(r, t, &dm, &S);
            if (err == NL_NO)
                err = N_BezSrfElevateDegree(Pp.data(), r, t, &dm, dir, f, l, roc, Qp.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (NL_INDEX k = f; k <= l; k++)
                    put(o, dir == NL_UDIR ? Q[(size_t)k][(size_t)roc] : Q[(size_t)roc][(size_t)k]);
            break;
        }
        case 7: // N_BezEvalOneBasis: p i u
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_INDEX i = rd.idx();
            NL_PARAMETER u = rd.num();
            NL_REAL B = 0.0;
            err = N_BezEvalOneBasis(p, i, u, &B);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                o.push_back(B);
            break;
        }
        case 8: // N_BezDegreeReduceCoefs: p
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_INDEX r = (p - 1) / 2;
            size_t len = (size_t)(p + 2);
            std::vector<double> alf(len, 0.0), oma(len, 0.0), bet(len, 0.0), omb(len, 0.0);
            N_BezDegreeReduceCoefs(p, alf.data(), oma.data(), bet.data(), omb.data());
            o.push_back(0.0);
            for (NL_INDEX i = 1; i <= r; i++)
                o.push_back(alf[(size_t)i]);
            for (NL_INDEX i = 1; i <= r; i++)
                o.push_back(oma[(size_t)i]);
            for (NL_INDEX i = r; i <= p - 2; i++)
                o.push_back(bet[(size_t)i]);
            for (NL_INDEX i = r; i <= p - 2; i++)
                o.push_back(omb[(size_t)i]);
            break;
        }
        case 9: // N_BezReduceDegree: p, Pw[p+1]
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            std::vector<NL_CPOINT> Pw = rd.cpts(p + 1);
            size_t len = (size_t)(p + 2);
            std::vector<double> alf(len, 0.0), oma(len, 0.0), bet(len, 0.0), omb(len, 0.0);
            N_BezDegreeReduceCoefs(p, alf.data(), oma.data(), bet.data(), omb.data());
            std::vector<NL_CPOINT> Qw((size_t)p, czero());
            NL_REAL e = 0.0;
            err = N_BezReduceDegree(Pw.data(), p, alf.data(), oma.data(), bet.data(), omb.data(), Qw.data(), &e);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                for (NL_INDEX i = 0; i < p; i++)
                    put(o, Qw[(size_t)i]);
                o.push_back(e);
            }
            break;
        }
        case 10: // N_BezSplit: p u, Pw[p+1]
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_PARAMETER u = rd.num();
            std::vector<NL_CPOINT> Pw = rd.cpts(p + 1);
            std::vector<NL_CPOINT> Qw((size_t)p + 1, czero()), Rw((size_t)p + 1, czero());
            err = N_BezSplit(Pw.data(), p, u, Qw.data(), Rw.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                for (NL_INDEX i = 0; i <= p; i++)
                    put(o, Qw[(size_t)i]);
                for (NL_INDEX i = 0; i <= p; i++)
                    put(o, Rw[(size_t)i]);
            }
            break;
        }
        case 11: // N_BezCalcArcLength: p eps len, Pw[p+1]
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_REAL eps = rd.num(), len = rd.num();
            std::vector<NL_CPOINT> Pw = rd.cpts(p + 1);
            err = N_BezCalcArcLength(Pw.data(), p, &eps, &len);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back(eps);
                o.push_back(len);
            }
            break;
        }
        case 12: // N_BezCalcLength: p tol, Pw[p+1]
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_REAL tol = rd.num();
            std::vector<NL_CPOINT> Pw = rd.cpts(p + 1);
            NL_REAL len = 0.0;
            err = N_BezCalcLength(Pw.data(), p, tol, &len);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                o.push_back(len);
            break;
        }
        case 13: // N_BezCrvExtend: p whr rev, Pw[p+1]
        {
            NL_DEGREE p = (NL_DEGREE)rd.idx();
            NL_FLAG whr = (NL_FLAG)rd.idx(), rev = (NL_FLAG)rd.idx();
            std::vector<NL_CPOINT> Pw = rd.cpts(p + 1);
            std::vector<NL_CPOINT> Qw((size_t)p + 1, czero());
            err = N_BezCrvExtend(Pw.data(), p, whr, rev, Qw.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (NL_INDEX i = 0; i <= p; i++)
                    put(o, Qw[(size_t)i]);
            break;
        }
        case 14: // N_BezSrfExtend4D: dir whr rev in_place, surface
        {
            NL_FLAG dir = (NL_FLAG)rd.idx(), whr = (NL_FLAG)rd.idx(), rev = (NL_FLAG)rd.idx();
            int in_place = (int)rd.num();
            NL_SURFACE* P = read_srf(rd, &S);
            if (P == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_SURFACE* Q = P;
            if (!in_place)
            {
                Q = N_AllocSrf(&S);
                if (Q == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                N_SrfInitArrays(Q);
            }
            err = N_BezSrfExtend4D(P, dir, whr, rev, Q, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, Q);
            break;
        }
        default:
            rc = SC_ERR_ARG;
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
