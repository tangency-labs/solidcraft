// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.spiral`.
// Wraps upstream N_CrvApproxSpiral (source/SMLib/src/NL_Spiral.cpp, with its static
// helpers ST_GetSpiralAngle / ST_GetPointAtAngle / ST_GetPointAtAngleD) and the
// volume memory routines sm_ComputeNurbVolumeSize, sm_InitNurbVolumeMemory,
// sm_AllocateNurbVolume, sm_AllocateAndCopyNurbVolume (source/SMLib/src/SmNurbsVol.cpp).
//
// One entry point, sc_nlib_spiral_eval(op, in, n_in, out, cap, out_len). `in` is a
// flat list of doubles (layouts per op below and in
// crates/solidcore-ffi/src/units/nlib_spiral.rs, which validates them).
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>
#include <SmMemory.h>
#include <SmNurbsVol.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;

namespace
{
// l m n p q r ku kv kw, knots (U V W), control points (x y z w, w index fastest).
void dump_vol(std::vector<double>& o, const gw_VOLUME* v)
{
    const NL_CMESH* mesh = v->mesh;
    o.push_back((double)mesh->m);
    o.push_back((double)mesh->n);
    o.push_back((double)mesh->o);
    o.push_back((double)v->p);
    o.push_back((double)v->q);
    o.push_back((double)v->r);
    o.push_back((double)v->knu->m);
    o.push_back((double)v->knv->m);
    o.push_back((double)v->knw->m);
    for (long i = 0; i <= v->knu->m; i++)
        o.push_back(v->knu->U[i]);
    for (long i = 0; i <= v->knv->m; i++)
        o.push_back(v->knv->U[i]);
    for (long i = 0; i <= v->knw->m; i++)
        o.push_back(v->knw->U[i]);
    for (long i = 0; i <= mesh->m; i++)
        for (long j = 0; j <= mesh->n; j++)
            for (long k = 0; k <= mesh->o; k++)
            {
                const NL_CPOINT& c = mesh->Pw[i][j][k];
                o.push_back(c.x);
                o.push_back(c.y);
                o.push_back(c.z);
                o.push_back(c.w);
            }
}
} // namespace

SC_EXPORT int32_t sc_nlib_spiral_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                      int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    std::vector<double> o;
    switch (op)
    {
    case 0: // N_CrvApproxSpiral: height rad0 rad1 turns rorl num_points tol
            // -> status, then on success num_points tol curve
    {
        NL_STACKS S;
        N_InitNurbs(&S);
        NL_CURVE cur;
        N_CrvInitArrays(&cur);
        NL_INDEX np = (NL_INDEX)in[5];
        NL_REAL tol = in[6];
        N_ErrClear();
        NL_FLAG err = N_CrvApproxSpiral(in[0], in[1], in[2], in[3], (NL_INDEX)in[4], &np, &tol, &cur, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            o.push_back((double)np);
            o.push_back(tol);
            dump_crv(o, &cur);
        }
        N_EndNurbs(&S);
        break;
    }
    case 1: // sm_AllocateNurbVolume then sm_AllocateAndCopyNurbVolume:
            // l m n p q r ku kv kw, knots, control points
            // -> fresh volume (zero filled), copy of the filled volume
    {
        long l = (long)in[0], m = (long)in[1], n = (long)in[2];
        gw_DEGREE p = (gw_DEGREE)in[3], q = (gw_DEGREE)in[4], r = (gw_DEGREE)in[5];
        long ku = (long)in[6], kv = (long)in[7], kw = (long)in[8];
        gw_VOLUME* v = sm_AllocateNurbVolume(l, m, n, p, q, r, ku, kv, kw);
        if (v == NULL)
            return SC_ERR_ARG;
        dump_vol(o, v);
        int64_t at = 9;
        for (long i = 0; i <= ku; i++)
            v->knu->U[i] = in[at++];
        for (long i = 0; i <= kv; i++)
            v->knv->U[i] = in[at++];
        for (long i = 0; i <= kw; i++)
            v->knw->U[i] = in[at++];
        for (long i = 0; i <= l; i++)
            for (long j = 0; j <= m; j++)
                for (long k = 0; k <= n; k++)
                {
                    NL_CPOINT& c = v->mesh->Pw[i][j][k];
                    c.x = in[at++];
                    c.y = in[at++];
                    c.z = in[at++];
                    c.w = in[at++];
                }
        gw_VOLUME* c = sm_AllocateAndCopyNurbVolume(v);
        smos_Free(v);
        if (c == NULL)
            return SC_ERR_ARG;
        dump_vol(o, c);
        smos_Free(c);
        break;
    }
    default:
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
