// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_basic`.
// Wraps the NL_SURFACE routines of upstream source/SMLib/src/NL_SrfBasic.cpp owned by
// the unit, plus N_SrfGetAverageLen (NL_SrfGeom.cpp).
//
// One entry point, sc_nlib_srf_basic_eval(op, in, n_in, out, cap, out_len): `op`
// selects a group of upstream calls, `in` holds its inputs, `out` receives a flat
// record of doubles (layouts in crates/difftest/src/units/nlib_srf_basic.rs).
// Statuses of upstream calls are written into the record: 0 = ok, else NL_ERROR.eno
// (cleared before each call; 1000 if the flag was set without a number). Outputs
// upstream leaves unwritten are NaN.
//
// A surface in `in` is: N M R S n m p q r s, then (N+1)*(M+1) control points
// (x y z w, row-major), R+1 u knots, S+1 v knots. N, M, R, S size the storage; n, m,
// r, s (<= the storage) are the indices in use. Every surface is allocated on one
// NL_STACKS (as upstream callers do), so N_SrfCompress / N_SwapUV can free its arrays.
// The Rust wrapper validates shapes and index ranges so upstream only touches the
// storage it is given.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <cmath>
#include <limits>
#include <vector>

#include "sc_nlib.h"

using sc::dump_srf;
using sc::nl_status;

namespace
{
const double kNaN = std::numeric_limits<double>::quiet_NaN();

double flag(NL_BOOLEAN b) { return b ? 1.0 : 0.0; }

// Reads one surface at in[*at], allocating it on S; advances *at.
NL_SURFACE* read_srf(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX N = (NL_INDEX)d[0], M = (NL_INDEX)d[1], R = (NL_INDEX)d[2], SS = (NL_INDEX)d[3];
    NL_INDEX n = (NL_INDEX)d[4], m = (NL_INDEX)d[5];
    NL_DEGREE p = (NL_DEGREE)d[6], q = (NL_DEGREE)d[7];
    NL_INDEX r = (NL_INDEX)d[8], s = (NL_INDEX)d[9];
    NL_SURFACE* sur = N_AllocSrfAndArrays(N, M, p, q, R, SS, S);
    if (sur == NULL)
        return NULL;
    const double* c = d + 10;
    for (NL_INDEX i = 0; i <= N; i++)
        for (NL_INDEX j = 0; j <= M; j++)
        {
            NL_CPOINT& pw = sur->net->Pw[i][j];
            pw.x = c[0];
            pw.y = c[1];
            pw.z = c[2];
            pw.w = c[3];
            c += 4;
        }
    for (NL_INDEX i = 0; i <= R; i++)
        sur->knu->U[i] = *c++;
    for (NL_INDEX i = 0; i <= SS; i++)
        sur->knv->U[i] = *c++;
    N_SrfSetSizeIndices(sur, n, m, p, q, r, s);
    *at = (int64_t)(c - in);
    return sur;
}

// Op 0: predicates, checks and measures of one surface.
// Params after the surface: eps np mp rk sk.
void op_inspect(std::vector<double>& o, NL_SURFACE* sur, const double* prm)
{
    NL_FLAG err;
    TCHAR rname[] = _T("sc_nlib_srf_basic");

    N_ErrClear();
    err = N_SrfCountsAreValid(sur, rname);
    o.push_back(nl_status(err));
    N_ErrClear();
    err = N_SrfWeightsAreValid(sur, rname);
    o.push_back(nl_status(err));
    N_ErrClear();
    err = N_SrfIsValid(sur, rname);
    o.push_back(nl_status(err));
    N_ErrClear();
    err = N_SrfIsSized(sur, (NL_INDEX)prm[1], (NL_INDEX)prm[2], (NL_INDEX)prm[3], (NL_INDEX)prm[4], rname);
    o.push_back(nl_status(err));

    o.push_back(flag(N_IsSrfRat(sur)));
    o.push_back(flag(N_SrfIsClosed(sur, NL_UDIR)));
    o.push_back(flag(N_SrfIsClosed(sur, NL_VDIR)));
    o.push_back(flag(N_SrfIsClosed(sur, NL_UVDIR)));
    o.push_back(flag(N_SrfAreWeightsEqual(sur)));

    NL_PARAMETER ul, ur, vb, vt;
    N_SrfGetParameterBounds(sur, &ul, &ur, &vb, &vt);
    o.push_back(ul);
    o.push_back(ur);
    o.push_back(vb);
    o.push_back(vt);

    NL_INDEX n, m, r, s;
    N_SrfGetArraySizes(sur, &n, &m, &r, &s);
    NL_DEGREE p, q;
    N_SrfGetDegrees(sur, &p, &q);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)r);
    o.push_back((double)s);
    o.push_back((double)p);
    o.push_back((double)q);

    NL_REAL A = kNaN, lu = kNaN, lv = kNaN;
    N_ErrClear();
    err = N_SrfGetAverageLen(sur, &A, &lu, &lv);
    o.push_back(nl_status(err));
    o.push_back(A);
    o.push_back(lu);
    o.push_back(lv);

    NL_FLAG flt = -7, dir = -7;
    N_ErrClear();
    err = N_SrfIsFlatCheap(sur, prm[0], &flt, &dir);
    o.push_back(nl_status(err));
    o.push_back(flt == -7 ? kNaN : (double)flt);
    o.push_back(dir == -7 ? kNaN : (double)dir);
}
} // namespace

// op 0 inspect: surface, eps np mp rk sk.
// op 1 enet: surface, ku lu kv lv -> status, points (3 per point).
// op 2 modify: surface, kind, params -> status, surface dump.
//   kind 0 compress; 1 swap uv; 2 non-rat -> rat; 3 make non-rat; 4 prune rat;
//   5 reparam (ul ur vb vt dir); 6 set size indices (n m p q r s);
//   7 copy into a NULL surface; 8 copy into the second surface (follows the params);
//   9 size arrays of the surface to (n m p q r s) -> status, n m p q r s only.
// op 3 equal: surface P, surface Q, tol3d tol1d -> flag.
// op 4 rat compatible: count k, k surfaces -> each dump.
SC_EXPORT int32_t sc_nlib_srf_basic_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                         int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    switch (op)
    {
        case 0:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            op_inspect(o, sur, in + at);
            break;
        }
        case 1:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            NL_INDEX ku = (NL_INDEX)prm[0], lu = (NL_INDEX)prm[1], kv = (NL_INDEX)prm[2], lv = (NL_INDEX)prm[3];
            NL_ENET ntl;
            N_ErrClear();
            NL_FLAG err = N_SrfGetENet(sur, ku, lu, kv, lv, &ntl, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                for (NL_INDEX i = 0; i <= ntl.n; i++)
                    for (NL_INDEX j = 0; j <= ntl.m; j++)
                    {
                        o.push_back(ntl.P[i][j].x);
                        o.push_back(ntl.P[i][j].y);
                        o.push_back(ntl.P[i][j].z);
                    }
            }
            break;
        }
        case 2:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            int kind = (int)prm[0];
            NL_SURFACE* res = sur;
            NL_FLAG err = NL_NO;
            N_ErrClear();
            switch (kind)
            {
                case 0:
                    err = N_SrfCompress(sur, &S);
                    break;
                case 1:
                    err = N_SwapUV(sur, &S);
                    break;
                case 2:
                    N_SrfNonRatToRat(sur);
                    break;
                case 3:
                    N_SrfMakeNonRat(sur);
                    break;
                case 4:
                    N_SrfPruneRat(sur);
                    break;
                case 5:
                {
                    NL_RECTANGLE R;
                    N_CreateRectangle(&R, prm[1], prm[2], prm[3], prm[4]);
                    N_SrfReparamToInterval(sur, R, (NL_FLAG)prm[5]);
                    break;
                }
                case 6:
                    N_SrfSetSizeIndices(sur, (NL_INDEX)prm[1], (NL_INDEX)prm[2], (NL_DEGREE)prm[3],
                                        (NL_DEGREE)prm[4], (NL_INDEX)prm[5], (NL_INDEX)prm[6]);
                    break;
                case 7:
                {
                    res = N_AllocSrf(&S);
                    if (res == NULL)
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    N_SrfInitArrays(res);
                    if (!N_SrfAreArraysNULL(res))
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    err = N_SrfCopy(sur, res, &S);
                    break;
                }
                case 8:
                {
                    int64_t at2 = at + 1;
                    res = read_srf(in, &at2, &S);
                    if (res == NULL || N_SrfAreArraysNULL(res))
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    err = N_SrfCopy(sur, res, &S);
                    break;
                }
                case 9:
                {
                    TCHAR rname[] = _T("sc_nlib_srf_basic");
                    err = N_SrfSizeArrays(sur, (NL_INDEX)prm[1], (NL_INDEX)prm[2], (NL_DEGREE)prm[3],
                                          (NL_DEGREE)prm[4], (NL_INDEX)prm[5], (NL_INDEX)prm[6], rname, &S);
                    o.push_back(nl_status(err));
                    NL_INDEX n, m, r, s;
                    NL_DEGREE p, q;
                    N_SrfGetArraySizes(sur, &n, &m, &r, &s);
                    N_SrfGetDegrees(sur, &p, &q);
                    o.push_back((double)n);
                    o.push_back((double)m);
                    o.push_back((double)p);
                    o.push_back((double)q);
                    o.push_back((double)r);
                    o.push_back((double)s);
                    break;
                }
                default:
                    rc = SC_ERR_ARG;
            }
            if (rc != SC_OK || kind == 9)
                break;
            o.push_back(nl_status(err));
            dump_srf(o, res);
            break;
        }
        case 3:
        {
            NL_SURFACE* P = read_srf(in, &at, &S);
            NL_SURFACE* Q = P == NULL ? NULL : read_srf(in, &at, &S);
            if (Q == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(flag(N_SrfsAreEqual(P, Q, in[at], in[at + 1])));
            break;
        }
        case 4:
        {
            int64_t k = (int64_t)in[0];
            at = 1;
            std::vector<NL_SURFACE*> srfs;
            for (int64_t i = 0; i < k; i++)
            {
                NL_SURFACE* sur = read_srf(in, &at, &S);
                if (sur == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                srfs.push_back(sur);
            }
            if (rc != SC_OK)
                break;
            N_MakeSrfsRatCompatible(srfs.empty() ? NULL : srfs.data(), (NL_INDEX)(k - 1));
            for (NL_SURFACE* sur : srfs)
                dump_srf(o, sur);
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
