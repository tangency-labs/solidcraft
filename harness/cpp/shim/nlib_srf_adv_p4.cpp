// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_adv_p4`.
// Wraps N_CreateBVectors, N_CreateCoonsSrf, N_CreateSkinSrfBoundaryDerivs,
// N_MergeKnotVectors, ST_GetRotationRateOfChange, N_SweepCrossTangentCrv,
// N_AdjustDerivSrf, N_AdjustWeightScale, N_ApproxCrossBoundaryDerivs,
// N_CreateCoonsBoundaryCrvs and N_CreateCrossBoundaryDerivCrv
// (upstream source/SMLib/src/NL_SrfAdv.cpp).
//
// One entry point, sc_nlib_srf_adv_p4_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots. A curve is: n p, then n+1 control points
// (x y z w), n+p+2 knots. Everything is allocated on one NL_STACKS. The Rust wrapper
// validates the inputs so upstream only touches the storage it allocates.
//
// A surface is dumped as n m p q r s, the control points, the u and v knots; a curve
// as n p m, the control points, the knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::dump_srf;
using sc::nl_status;
using sc::null_crv;
using sc::null_srf;
using sc::read_srf;

// Defined in NL_SrfAdv.cpp without a header declaration (C++ linkage).
NL_FLAG ST_GetRotationRateOfChange(NL_CURVE* traj, NL_PARAMETER u, NL_CURVE* curZ, NL_VECTOR* Z, NL_VECTOR* prevZ,
                                   NL_PARAMETER prev_u, NL_VECTOR* nextZ, NL_PARAMETER next_u, NL_RMATRIX* drma);

namespace
{

NL_CURVE* read_crv(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX n = (NL_INDEX)d[0];
    NL_DEGREE p = (NL_DEGREE)d[1];
    NL_INDEX m = n + p + 1;
    NL_CURVE* cur = N_AllocCrvAndArrays(n, p, m, S);
    if (cur == NULL)
        return NULL;
    const double* c = d + 2;
    NL_CPOINT* Pw;
    NL_REAL* U;
    N_CrvGetCPtsAndKnots(cur, &Pw, &U);
    for (NL_INDEX i = 0; i <= n; i++)
    {
        Pw[i].x = c[0];
        Pw[i].y = c[1];
        Pw[i].z = c[2];
        Pw[i].w = c[3];
        c += 4;
    }
    for (NL_INDEX i = 0; i <= m; i++)
        U[i] = *c++;
    *at = (int64_t)(c - in);
    return cur;
}

NL_POINT read_pt(const double* in, int64_t* at)
{
    NL_POINT P;
    P.x = in[*at];
    P.y = in[*at + 1];
    P.z = in[*at + 2];
    *at += 3;
    return P;
}

NL_CPOINT read_cpt(const double* in, int64_t* at)
{
    NL_CPOINT P;
    P.x = in[*at];
    P.y = in[*at + 1];
    P.z = in[*at + 2];
    P.w = in[*at + 3];
    *at += 4;
    return P;
}

// A 3x3 matrix, row-major.
NL_RMATRIX* read_mat(const double* in, int64_t* at, NL_STACKS* S)
{
    NL_RMATRIX* M = N_AllocRealMatrix(2, 2, NL_MT_FULL, 0, S);
    if (M == NULL)
        return NULL;
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++)
            M->RM[i][j] = in[(*at)++];
    return M;
}

void push_pt(std::vector<double>& o, NL_POINT P)
{
    o.push_back(P.x);
    o.push_back(P.y);
    o.push_back(P.z);
}

NL_KNOTVECTOR* read_knots(const double* in, int64_t* at, NL_STACKS* S)
{
    NL_INDEX cnt = (NL_INDEX)in[(*at)++];
    NL_KNOTVECTOR* kn = N_AllocKnotVectorAndArray(cnt - 1, S);
    if (kn == NULL)
        return NULL;
    for (NL_INDEX i = 0; i < cnt; i++)
        kn->U[i] = in[(*at)++];
    return kn;
}

} // namespace

// op 0 bvec: curve, b0, k, t[0..=k] -> status, B[0..=k].
// op 1 coons: curves U0 U1 V0 V1 -> status, surface, the four curves after.
// op 2 skin derivs: curve, V, bndy -> status, surface, curve after.
// op 3 merge: p, knots R (count + values), knots S -> status, |XR|, XR, |XS|, XS.
// op 4 rotation rate: traj, u, mode (0: curve Z; 1: Z, has_prev, prevZ, prev_u,
//      has_next, nextZ, next_u) -> status, RM row-major.
// op 5 sweep cross tangent: dtraj, scale, dscale, rma, drma, curve -> status, curve.
// op 6 adjust deriv: surface, dir, cst, usg, inplace -> status, surface.
// op 7 adjust weight: Pw, Qw, Rw -> status, wr, alf.
// op 8 approx: curve, Ds, De, eps, inplace -> status, result curve, derP after.
// op 9 coons boundary: curves L dL R dR B dB T dT, cpf -> status, surface, 8 curves.
// op 10 cross deriv: curve, Ns, Ne, Bs, Be -> status, curve.
SC_EXPORT int32_t sc_nlib_srf_adv_p4_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (op)
    {
        case 0:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_VECTOR B0 = read_pt(in, &at);
            NL_INDEX k = (NL_INDEX)in[at++];
            std::vector<NL_PARAMETER> t(in + at, in + at + k + 1);
            at += k + 1;
            std::vector<NL_VECTOR> B((size_t)k + 1);
            err = N_CreateBVectors(cur, B0, t.data(), k, B.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (NL_INDEX i = 0; i <= k; i++)
                    push_pt(o, B[(size_t)i]);
            break;
        }
        case 1:
        {
            NL_CURVE* c[4];
            for (int i = 0; i < 4 && rc == SC_OK; i++)
            {
                c[i] = read_crv(in, &at, &S);
                if (c[i] == NULL)
                    rc = SC_ERR_ARG;
            }
            NL_SURFACE* res = null_srf(&S);
            if (rc != SC_OK || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_CURVE* curU[2] = {c[0], c[1]};
            NL_CURVE* curV[2] = {c[2], c[3]};
            err = N_CreateCoonsSrf(curU, curV, res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                dump_srf(o, res);
                for (int i = 0; i < 4; i++)
                    dump_crv(o, c[i]);
            }
            break;
        }
        case 2:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            NL_SURFACE* res = null_srf(&S);
            if (cur == NULL || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_VECTOR V = read_pt(in, &at);
            NL_FLAG bndy = (NL_FLAG)in[at++];
            err = N_CreateSkinSrfBoundaryDerivs(cur, V, bndy, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                dump_srf(o, res);
                dump_crv(o, cur);
            }
            break;
        }
        case 3:
        {
            NL_DEGREE p = (NL_DEGREE)in[at++];
            NL_KNOTVECTOR* knr = read_knots(in, &at, &S);
            NL_KNOTVECTOR* kns = knr == NULL ? NULL : read_knots(in, &at, &S);
            if (kns == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            // Every knot inserted into one vector consumes a knot of the other.
            std::vector<NL_REAL> XR((size_t)kns->m + 2), XS((size_t)knr->m + 2);
            NL_INDEX rr = -1, ss = -1;
            err = N_MergeKnotVectors(knr, kns, p, XR.data(), &rr, XS.data(), &ss);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)(rr + 1));
                for (NL_INDEX i = 0; i <= rr; i++)
                    o.push_back(XR[(size_t)i]);
                o.push_back((double)(ss + 1));
                for (NL_INDEX i = 0; i <= ss; i++)
                    o.push_back(XS[(size_t)i]);
            }
            break;
        }
        case 4:
        {
            NL_CURVE* traj = read_crv(in, &at, &S);
            NL_RMATRIX* drma = N_AllocRealMatrix(2, 2, NL_MT_FULL, 0, &S);
            if (traj == NULL || drma == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_PARAMETER u = in[at++];
            int mode = (int)in[at++];
            if (mode == 0)
            {
                NL_CURVE* curZ = read_crv(in, &at, &S);
                if (curZ == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                err = ST_GetRotationRateOfChange(traj, u, curZ, NULL, NULL, 0.0, NULL, 0.0, drma);
            }
            else
            {
                NL_VECTOR Z = read_pt(in, &at);
                bool has_prev = in[at++] != 0.0;
                NL_VECTOR prevZ = read_pt(in, &at);
                NL_PARAMETER prev_u = in[at++];
                bool has_next = in[at++] != 0.0;
                NL_VECTOR nextZ = read_pt(in, &at);
                NL_PARAMETER next_u = in[at++];
                err = ST_GetRotationRateOfChange(traj, u, NULL, &Z, has_prev ? &prevZ : NULL, prev_u,
                                                 has_next ? &nextZ : NULL, next_u, drma);
            }
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (int i = 0; i < 3; i++)
                    for (int j = 0; j < 3; j++)
                        o.push_back(drma->RM[i][j]);
            break;
        }
        case 5:
        {
            NL_VECTOR dtraj = read_pt(in, &at);
            NL_POINT scale = read_pt(in, &at);
            NL_VECTOR dscale = read_pt(in, &at);
            NL_RMATRIX* rma = read_mat(in, &at, &S);
            NL_RMATRIX* drma = rma == NULL ? NULL : read_mat(in, &at, &S);
            NL_CURVE* cur = drma == NULL ? NULL : read_crv(in, &at, &S);
            NL_CURVE* res = null_crv(&S);
            if (cur == NULL || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SweepCrossTangentCrv(&dtraj, &scale, &dscale, rma, drma, cur, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, res);
            break;
        }
        case 6:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_FLAG dir = (NL_FLAG)in[at++];
            NL_FLAG cst = (NL_FLAG)in[at++];
            NL_FLAG usg = (NL_FLAG)in[at++];
            bool inplace = in[at++] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_AdjustDerivSrf(sur, dir, cst, usg, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 7:
        {
            NL_CPOINT Pw = read_cpt(in, &at);
            NL_CPOINT Qw = read_cpt(in, &at);
            NL_CPOINT Rw = read_cpt(in, &at);
            NL_REAL wr = 0.0, alf = 0.0;
            err = N_AdjustWeightScale(Pw, Qw, Rw, &wr, &alf);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back(wr);
                o.push_back(alf);
            }
            break;
        }
        case 8:
        {
            NL_CURVE* der = read_crv(in, &at, &S);
            if (der == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_VECTOR Ds = read_pt(in, &at);
            NL_VECTOR De = read_pt(in, &at);
            NL_REAL eps = in[at++];
            bool inplace = in[at++] != 0.0;
            NL_CURVE* res = inplace ? der : null_crv(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_ApproxCrossBoundaryDerivs(der, Ds, De, eps, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                dump_crv(o, res);
                if (!inplace)
                    dump_crv(o, der);
            }
            break;
        }
        case 9:
        {
            NL_CURVE* c[8];
            for (int i = 0; i < 8 && rc == SC_OK; i++)
            {
                c[i] = read_crv(in, &at, &S);
                if (c[i] == NULL)
                    rc = SC_ERR_ARG;
            }
            NL_SURFACE* res = null_srf(&S);
            if (rc != SC_OK || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_FLAG cpf = in[at++] != 0.0 ? NL_YES : NL_NO;
            err = N_CreateCoonsBoundaryCrvs(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], cpf, res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                dump_srf(o, res);
                for (int i = 0; i < 8; i++)
                    dump_crv(o, c[i]);
            }
            break;
        }
        case 10:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            NL_CURVE* res = null_crv(&S);
            if (cur == NULL || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_VECTOR Ns = read_pt(in, &at);
            NL_VECTOR Ne = read_pt(in, &at);
            NL_VECTOR Bs = read_pt(in, &at);
            NL_VECTOR Be = read_pt(in, &at);
            err = N_CreateCrossBoundaryDerivCrv(cur, Ns, Ne, Bs, Be, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, res);
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
