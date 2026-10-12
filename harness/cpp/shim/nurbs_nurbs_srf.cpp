// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.nurbs_srf`.
// Wraps sm_ComputeNetConstants, sm_ComputeSurfConstants, sm_AllocateNurbSurface (with
// sm_ComputeNurbSurfaceSize / sm_InitNurbSurfaceMemory), sm_AllocateAndCopyNurbSurface,
// sm_DecomposeSrf, sm_SplitSrf and sm_IsNSrfDegenerate (upstream
// source/SMLib/src/SmNurbsSrf.cpp).
//
// One entry point, sc_nurbs_nurbs_srf_eval(op, in, n_in, out, cap, out_len). Except for
// op 2, `in` starts with a surface: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots; it is built with sm_AllocateNurbSurface.
// The op's parameters follow. `out` receives the status (0 = ok; for NLib routines
// NL_ERROR.eno, cleared before the call, or 1000 for a flag without a number; for
// SmStatus routines the SmStatus value), then the results. A surface is dumped as
// n m p q r s, the control points, the u and v knots.
//
// The Rust wrapper (crates/solidcore-ffi/src/units/nurbs_nurbs_srf.rs) validates every
// input so upstream only touches the memory it allocates.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent2d.h>
#include <SmNurbsSrf.h>
#include <SmTArray.h>

#include <vector>

namespace
{
double nl_status(gw_FLAG err)
{
    if (err == NL_NO)
        return 0.0;
    return NL_ERROR.eno != 0 ? (double)NL_ERROR.eno : 1000.0;
}

double sm_status(SmStatus st)
{
    return st == SM_SUCCESS ? 0.0 : (double)st;
}

// Reads one surface at in[*at] into a single sm_AllocateNurbSurface block; advances *at.
gw_SURFACE* read_srf(const double* in, int64_t* at)
{
    const double* d = in + *at;
    gw_INDEX n = (gw_INDEX)d[0], m = (gw_INDEX)d[1];
    gw_DEGREE p = (gw_DEGREE)d[2], q = (gw_DEGREE)d[3];
    gw_INDEX r = n + p + 1, s = m + q + 1;
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    if (sur == NULL)
        return NULL;
    const double* c = d + 4;
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            gw_CPOINT& pw = sur->net->Pw[i][j];
            pw.x = c[0];
            pw.y = c[1];
            pw.z = c[2];
            pw.w = c[3];
            c += 4;
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = *c++;
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = *c++;
    *at = (int64_t)(c - in);
    return sur;
}

void dump_srf(std::vector<double>& o, const gw_SURFACE* sur)
{
    gw_INDEX n = sur->net->n, m = sur->net->m, r = sur->knu->m, s = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX j = 0; j <= s; j++)
        o.push_back(sur->knv->U[j]);
}

// Five requested outputs (flags at f[0..5]: u_ch v_ch u_ang v_ang uv_ch); unrequested
// ones are passed as NULL and dumped as 0.
struct Outs
{
    double v[5] = {0.0, 0.0, 0.0, 0.0, 0.0};
    double* ptr(const double* f, int k) { return f[k] != 0.0 ? &v[k] : NULL; }
};
} // namespace

// op 0 net constants: surface, zone_tol, 5 flags, use_surface, ul vl uh vh -> status, 5 values.
// op 1 surf constants: surface, ul vl uh vh, 5 flags -> status, 5 values.
// op 2 allocate: n m p q r s (no surface) -> status 0, surface.
// op 3 allocate and copy: surface -> status 0, surface.
// op 4 decompose: surface -> status, nsu nsv, nsu*nsv surfaces (only on success).
// op 5 split: surface, t dir -> status, left, right (only on success).
// op 6 degenerate: surface, tol -> status 0, flag.
SC_EXPORT int32_t sc_nurbs_nurbs_srf_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    std::vector<double> o;
    if (op == 2)
    {
        gw_SURFACE* sur = sm_AllocateNurbSurface((gw_INDEX)in[0], (gw_INDEX)in[1], (gw_DEGREE)in[2],
                                                 (gw_DEGREE)in[3], (gw_INDEX)in[4], (gw_INDEX)in[5]);
        if (sur == NULL)
            return SC_ERR_ARG;
        o.push_back(0.0);
        dump_srf(o, sur);
        smos_Free(sur);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }

    int64_t at = 0;
    gw_SURFACE* sur = read_srf(in, &at);
    if (sur == NULL)
        return SC_ERR_ARG;
    const double* prm = in + at;
    int32_t rc = SC_OK;
    N_ErrClear();
    switch (op)
    {
        case 0:
        {
            double zone = prm[0];
            const double* f = prm + 1;
            bool use_surface = prm[6] != 0.0;
            SmExtent2d dom(prm[7], prm[8], prm[9], prm[10]);
            Outs res;
            SmZoneTol3d ztol(zone);
            SmStatus st;
            if (use_surface)
            {
                SmContext ctx;
                SmBSplineSurface* bs = new (ctx) SmBSplineSurface(sur, TRUE, &ctx);
                st = sm_ComputeNetConstants(bs, &dom, sur, ztol, res.ptr(f, 0), res.ptr(f, 1), res.ptr(f, 2),
                                            res.ptr(f, 3), res.ptr(f, 4));
                delete bs;
            }
            else
                st = sm_ComputeNetConstants(NULL, NULL, sur, ztol, res.ptr(f, 0), res.ptr(f, 1), res.ptr(f, 2),
                                            res.ptr(f, 3), res.ptr(f, 4));
            o.push_back(sm_status(st));
            for (int k = 0; k < 5; k++)
                o.push_back(res.v[k]);
            break;
        }
        case 1:
        {
            SmExtent2d dom(prm[0], prm[1], prm[2], prm[3]);
            const double* f = prm + 4;
            Outs res;
            SmContext ctx;
            SmBSplineSurface* bs = new (ctx) SmBSplineSurface(sur, TRUE, &ctx);
            SmStatus st = sm_ComputeSurfConstants(bs, dom, res.ptr(f, 0), res.ptr(f, 1), res.ptr(f, 2),
                                                  res.ptr(f, 3), res.ptr(f, 4));
            delete bs;
            o.push_back(sm_status(st));
            for (int k = 0; k < 5; k++)
                o.push_back(res.v[k]);
            break;
        }
        case 3:
        {
            gw_SURFACE* cp = sm_AllocateAndCopyNurbSurface(sur);
            if (cp == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(0.0);
            dump_srf(o, cp);
            smos_Free(cp);
            break;
        }
        case 4:
        {
            gw_INDEX nsu = 0, nsv = 0;
            N_BasisGetSpanCount(sur->knu, sur->p, &nsu);
            N_BasisGetSpanCount(sur->knv, sur->q, &nsv);
            if (nsu < 1 || nsv < 1)
            {
                rc = SC_ERR_ARG;
                break;
            }
            // As sm_CreateBezierPatchBlock: nsu*nsv patches of degrees (p, q), knots 2p+1, 2q+1.
            SmTArray<gw_SURFACE*> bez;
            bez.SetSize((ULONG)(nsu * nsv));
            for (gw_INDEX k = 0; k < nsu * nsv; k++)
                bez[(ULONG)k] = sm_AllocateNurbSurface(sur->p, sur->q, sur->p, sur->q, 2 * sur->p + 1, 2 * sur->q + 1);
            gw_FLAG err = sm_DecomposeSrf(sur, bez);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)nsu);
                o.push_back((double)nsv);
                for (gw_INDEX k = 0; k < nsu * nsv; k++)
                    dump_srf(o, bez[(ULONG)k]);
            }
            for (gw_INDEX k = 0; k < nsu * nsv; k++)
                smos_Free(bez[(ULONG)k]);
            break;
        }
        case 5:
        {
            gw_SURFACE* L = NULL;
            gw_SURFACE* R = NULL;
            gw_FLAG err = sm_SplitSrf(sur, prm[0], (gw_FLAG)prm[1], L, R);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                if (L == NULL || R == NULL)
                    rc = SC_ERR_ARG;
                else
                {
                    dump_srf(o, L);
                    dump_srf(o, R);
                }
            }
            if (L != NULL)
                smos_Free(L);
            if (R != NULL)
                smos_Free(R);
            break;
        }
        case 6:
        {
            SmBoolean deg = sm_IsNSrfDegenerate(sur, prm[0]);
            o.push_back(0.0);
            o.push_back(deg ? 1.0 : 0.0);
            break;
        }
        default:
            rc = SC_ERR_ARG;
    }
    smos_Free(sur);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
