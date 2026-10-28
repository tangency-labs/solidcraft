// SPDX-License-Identifier: Apache-2.0
// The "const query makes the NURB" sequence shared by the sphere, torus and surface of
// revolution shims (the cone's is shim op 19 of surface_cone_p1.cpp).
//
// The surface comes from its constructor with no MakeNurb. Output, after the build status
// the shim already wrote: has-NURB, the const GetNaturalUVDomain (GetOrCreateGwNurbPointer
// makes the NURB and keeps it), has-NURB, the NURB, HavePolarConversion, EvaluatePoint at
// the domain's (0.3, 0.6), the point and normal at its four corners, Copy (status, the copy's has-NURB and NURB, *copy == *this, its
// EvaluatePoint at (0.7, 0.2)), then Transform by a fixed rotation and move (status,
// has-NURB, the natural domain, EvaluatePoint at (0.3, 0.6)) and the NURB again.
#ifndef SC_CONST_MAKE_H
#define SC_CONST_MAKE_H

#include <vector>

#include <SmAxis2Placement.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent2d.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmNurbsSrf.h>
#include <nurbs.h>

#include "sc_sm.h"

namespace sc
{
inline void put_gw_surface(std::vector<double>& o, const gw_SURFACE* sur)
{
    o.push_back(sur != NULL ? 1.0 : 0.0);
    if (!sur)
        return;
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)(sur->net->n + 1));
    o.push_back((double)(sur->net->m + 1));
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    o.push_back((double)(sur->knu->m + 1));
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    o.push_back((double)(sur->knv->m + 1));
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

inline void put_const_make(const SmContext& ctx, SmSurfOfRevolution* s, std::vector<double>& o)
{
    const SmSurface* cs = s;
    o.push_back(s->GetGwNurbPointer() != NULL ? 1.0 : 0.0);
    const SmExtent2d d = cs->GetNaturalUVDomain();
    o.push_back(d.GetMin().x); o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x); o.push_back(d.GetMax().y);
    o.push_back(s->GetGwNurbPointer() != NULL ? 1.0 : 0.0);
    put_gw_surface(o, s->GetGwNurbPointer());
    o.push_back(s->HavePolarConversion() ? 1.0 : 0.0);
    SmPoint3d p(0.0, 0.0, 0.0);
    SmStatus status = cs->EvaluatePoint(d.Evaluate(0.3, 0.6), p);
    o.push_back((double)status);
    if (status == SM_SUCCESS)
        put_vec(o, p);
    // the natural domain's corners (poles and seams): point and normal
    for (int c = 0; c < 4; c++)
    {
        const SmPoint2d uv = d.Evaluate((double)(c & 1), (double)(c >> 1));
        SmPoint3d cp3(0.0, 0.0, 0.0);
        status = cs->EvaluatePoint(uv, cp3);
        o.push_back((double)status);
        if (status == SM_SUCCESS)
            put_vec(o, cp3);
        SmVector3d nrm(0.0, 0.0, 0.0);
        status = cs->EvaluateNormal(uv, TRUE, TRUE, nrm);
        o.push_back((double)status);
        if (status == SM_SUCCESS)
            put_vec(o, nrm);
    }
    SmSurface* cp = NULL;
    status = cs->Copy(ctx, cp);
    o.push_back((double)status);
    if (status == SM_SUCCESS && cp)
    {
        const SmBSplineSurface* cb = static_cast<SmBSplineSurface*>(cp);
        o.push_back(cb->GetGwNurbPointer() != NULL ? 1.0 : 0.0);
        put_gw_surface(o, cb->GetGwNurbPointer());
        o.push_back((*cp == *cs) ? 1.0 : 0.0);
        SmPoint3d q(0.0, 0.0, 0.0);
        SmStatus qs = cp->EvaluatePoint(d.Evaluate(0.7, 0.2), q);
        o.push_back((double)qs);
        if (qs == SM_SUCCESS)
            put_vec(o, q);
    }
    delete cp;
    const SmAxis2Placement rot(SmPoint3d(1.0, 2.0, 3.0), SmVector3d(0.0, 1.0, 0.0),
                               SmVector3d(-1.0, 0.0, 0.0));
    status = s->Transform(rot, NULL);
    o.push_back((double)status);
    o.push_back(s->GetGwNurbPointer() != NULL ? 1.0 : 0.0);
    put_gw_surface(o, s->GetGwNurbPointer());
    const SmExtent2d t = cs->GetNaturalUVDomain();
    o.push_back(t.GetMin().x); o.push_back(t.GetMin().y);
    o.push_back(t.GetMax().x); o.push_back(t.GetMax().y);
    status = cs->EvaluatePoint(t.Evaluate(0.3, 0.6), p);
    o.push_back((double)status);
    if (status == SM_SUCCESS)
        put_vec(o, p);
}
} // namespace sc

#endif
