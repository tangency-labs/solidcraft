// SPDX-License-Identifier: Apache-2.0
// Unit surface.surface_assert_valid. Builds a real SmBSplineSurface, SmPlane, SmCylinder,
// SmCone, SmSphere, SmTorus, SmSurfOfRevolution or SmSurfOfExtrusion from plain data and
// runs the pinned virtual SmSurface::AssertValid on it, returning its result and every
// report.
//
// Input block: `kind level make` then, by kind,
//   0 B-spline: p q nu nv rational knots_u[nu+p+1] knots_v[nv+q+1] pts[nu*nv*4] (row i = u)
//   1 plane:    O[3] X[3] Y[3] scale[2] domain[4]
//   2 cylinder: O X Y r start end h swap inside mngc
//   3 cone:     O X Y bot top start end h swap inside mngc
//   4 sphere:   O X Y domain[4] r swap inside
//   5 torus:    O X Y domain[4] major minor swap inside
//   6 revolution: O X Y domain[4] swap, generator: p n rational pts[n*4] knots[n+p+1]
//   7 extrusion: origin[3] vec[3] domain[4] swap, generator: p n rational pts[n*4]
//                knots[n+p+1], then `stale cp[3] cv[3]` (stale != 0: SetCurvePoint(cp)
//                and SetCurveVec(cv) after construction, a stale cached plane or line)
// `make` calls MakeNurb before the check. Output: `result n` then per report
// `type list test assert_type ok reporter owner other owner_type other_type tol value
// dist3d name message message_for_file owner_string other_string` (texts as
// `len chars...`; objects: 0 null, 1 the surface or its placement, 9 another), then
// `last_add`.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCylinder.h>
#include <SmPlane.h>
#include <SmSphere.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurfOfRevolution.h>
#include <SmTol.h>
#include <SmTorus.h>
#include <nurbs.h>
#include <vector>

namespace
{
struct AvConePeek : SmCone
{
    AvConePeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double bot, double top,
               double start, double end, double h, SmBoolean swap, SmBoolean inside, SmBoolean mngc)
        : SmCone(o, x, y, bot, top, start, end, h, swap, inside, mngc)
    {
    }
};

struct AvRevolution : SmSurfOfRevolution
{
    AvRevolution(SmBSplineCurve* gen, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
                 const SmExtent2d& d, SmBoolean swap)
        : SmSurfOfRevolution(gen, o, x, y, d, swap)
    {
    }
};

struct AvExtrusion : SmSurfOfExtrusion
{
    AvExtrusion(SmBSplineCurve* gen, const SmPoint3d& o, const SmVector3d& v, const SmExtent2d& d,
                SmBoolean swap)
        : SmSurfOfExtrusion(gen, o, v, d, swap)
    {
    }
    void stale(const SmPoint3d& cp, const SmVector3d& cv)
    {
        SetCurvePoint(cp);
        SetCurveVec(cv);
    }
};

struct AvRd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool ok = true;
    double num()
    {
        if (at >= n)
        {
            ok = false;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    SmBoolean b() { return num() != 0.0 ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmExtent2d dom()
    {
        double a = num();
        double b2 = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b2, c, d);
    }
};

void av_text(std::vector<double>& out, const TCHAR* text)
{
    std::vector<double> s;
    for (const TCHAR* p = text; p && *p; p++)
        s.push_back((double)*p);
    out.push_back((double)s.size());
    out.insert(out.end(), s.begin(), s.end());
}

double av_id(const void* p, const SmSurface* s, const void* placement)
{
    if (p == NULL)
        return 0.0;
    if (p == s || (placement != NULL && p == placement))
        return 1.0;
    return 9.0;
}

gw_CPOINT av_point(AvRd& rd, bool rational)
{
    gw_CPOINT p;
    p.x = rd.num();
    p.y = rd.num();
    p.z = rd.num();
    double w = rd.num();
    p.w = rational ? w : NL_NOW;
    return p;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_assert_valid_eval(const double* input, int64_t n, double* out,
                                                       int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmContext ctx;
    AvRd rd{input, n, 0};
    int64_t kind = rd.idx();
    int64_t level = rd.idx();
    bool make = rd.num() != 0.0;
    SmSurface* surface = NULL;
    const void* placement = NULL;
    if (kind == 0)
    {
        int64_t p = rd.idx(), q = rd.idx(), nu = rd.idx(), nv = rd.idx();
        bool rational = rd.num() != 0.0;
        if (p < 1 || q < 1 || nu < 2 || nv < 2 || nu > 64 || nv > 64)
            return SC_ERR_ARG;
        std::vector<double> ku(nu + p + 1), kv(nv + q + 1);
        for (auto& v : ku)
            v = rd.num();
        for (auto& v : kv)
            v = rd.num();
        std::vector<gw_CPOINT> pts(nu * nv);
        for (auto& c : pts)
            c = av_point(rd, rational);
        if (!rd.ok)
            return SC_ERR_ARG;
        std::vector<gw_CPOINT*> rows(nu);
        for (int64_t i = 0; i < nu; i++)
            rows[i] = &pts[i * nv];
        surface = new (ctx) SmBSplineSurface((ULONG)(nu - 1), (ULONG)(nv - 1), (short)p, (short)q,
                                             (ULONG)(nu + p), (ULONG)(nv + q), rows.data(),
                                             ku.data(), kv.data());
    }
    else if (kind >= 1 && kind <= 6)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        if (kind == 1)
        {
            double sx = rd.num(), sy = rd.num();
            SmExtent2d d = rd.dom();
            if (!rd.ok)
                return SC_ERR_ARG;
            SmPlane* pl = new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
            placement = &pl->GetPosition();
            surface = pl;
        }
        else if (kind == 2)
        {
            double r = rd.num(), s0 = rd.num(), s1 = rd.num(), h = rd.num();
            SmBoolean swap = rd.b(), inside = rd.b(), mngc = rd.b();
            if (!rd.ok)
                return SC_ERR_ARG;
            SmCylinder* c = new (ctx) SmCylinder(o, x, y, r, s0, s1, h, swap, inside, mngc, &ctx);
            placement = &c->GetPosition();
            surface = c;
        }
        else if (kind == 3)
        {
            double bot = rd.num(), top = rd.num(), s0 = rd.num(), s1 = rd.num(), h = rd.num();
            SmBoolean swap = rd.b(), inside = rd.b(), mngc = rd.b();
            if (!rd.ok)
                return SC_ERR_ARG;
            AvConePeek* c = new (ctx) AvConePeek(o, x, y, bot, top, s0, s1, h, swap, inside, mngc);
            placement = &c->GetPosition();
            surface = c;
        }
        else if (kind == 4)
        {
            SmExtent2d d = rd.dom();
            double r = rd.num();
            SmBoolean swap = rd.b(), inside = rd.b();
            if (!rd.ok)
                return SC_ERR_ARG;
            SmSphere* s = new (ctx) SmSphere(o, x, y, d, r, swap, inside, NULL, &ctx);
            placement = &s->GetPosition();
            surface = s;
        }
        else if (kind == 5)
        {
            SmExtent2d d = rd.dom();
            double major = rd.num(), minor = rd.num();
            SmBoolean swap = rd.b(), inside = rd.b();
            if (!rd.ok)
                return SC_ERR_ARG;
            SmTorus* t = new (ctx) SmTorus(o, x, y, d, major, minor, swap, inside, NULL, &ctx);
            placement = &t->GetPosition();
            surface = t;
        }
        else
        {
            SmExtent2d d = rd.dom();
            SmBoolean swap = rd.b();
            int64_t p = rd.idx(), cn = rd.idx();
            bool rational = rd.num() != 0.0;
            if (p < 1 || cn < 2 || cn > 64)
                return SC_ERR_ARG;
            std::vector<gw_CPOINT> pts(cn);
            for (auto& c : pts)
                c = av_point(rd, rational);
            std::vector<double> k(cn + p + 1);
            for (auto& v : k)
                v = rd.num();
            if (!rd.ok)
                return SC_ERR_ARG;
            SmBSplineCurve* gen = new (ctx)
                SmBSplineCurve(3, (ULONG)(cn - 1), (ULONG)(cn + p), (short)p, pts.data(), k.data());
            AvRevolution* r = new (ctx) AvRevolution(gen, o, x, y, d, swap);
            placement = &r->GetPosition();
            surface = r;
        }
    }
    else if (kind == 7)
    {
        SmPoint3d o = rd.vec();
        SmVector3d v = rd.vec();
        SmExtent2d d = rd.dom();
        SmBoolean swap = rd.b();
        int64_t p = rd.idx(), cn = rd.idx();
        bool rational = rd.num() != 0.0;
        if (p < 1 || cn < 2 || cn > 64)
            return SC_ERR_ARG;
        std::vector<gw_CPOINT> pts(cn);
        for (auto& c : pts)
            c = av_point(rd, rational);
        std::vector<double> k(cn + p + 1);
        for (auto& x : k)
            x = rd.num();
        bool stale = rd.num() != 0.0;
        SmPoint3d cp = rd.vec();
        SmVector3d cv = rd.vec();
        if (!rd.ok)
            return SC_ERR_ARG;
        SmBSplineCurve* gen = new (ctx)
            SmBSplineCurve(3, (ULONG)(cn - 1), (ULONG)(cn + p), (short)p, pts.data(), k.data());
        AvExtrusion* e = new (ctx) AvExtrusion(gen, o, v, d, swap);
        if (stale)
            e->stale(cp, cv);
        surface = e;
    }
    else
        return SC_ERR_ARG;
    if (!rd.ok || rd.at != n)
    {
        delete surface;
        return SC_ERR_ARG;
    }
    if (make)
    {
        SmBSplineSurface* b = SM_CAST_PTR(SmBSplineSurface, surface);
        if (b)
            b->MakeNurb(); // status ignored, as the Rust side
    }
    // sm_AssertTestSurface6 reads uninitialised iso curves when CreateIsoBoundaries
    // fails (undefined behaviour): reject the inputs where the base check would reach it.
    bool base_runs = level != 4 && (kind <= 1 || make);
    if (base_runs)
    {
        SmApproxTol3d tol = SmTol::GetApproxTol3d(surface);
        for (int ii = 0; ii < 2; ii++)
        {
            SmSurfParamType dir = ii == 1 ? SM_SP_V : SM_SP_U;
            if (surface->IsClosed(surface->GetNaturalUVDomain(), dir))
                continue;
            SmBSplineCurve *c1 = NULL, *c2 = NULL;
            SmStatus st = surface->CreateIsoBoundaries(ctx, dir, tol, c1, c2);
            bool good = st == SM_SUCCESS && c1 != NULL && c2 != NULL;
            delete c1;
            delete c2;
            if (!good)
            {
                delete surface;
                return SC_ERR_ARG;
            }
        }
    }
    SmAssertArray reports;
    SmBoolean ok = surface->AssertValid(&reports, (SmAssertTestLevel)level, SM_NO_WALK);
    std::vector<double> v;
    v.push_back(ok ? 1.0 : 0.0);
    v.push_back((double)reports.GetSize());
    for (ULONG i = 0; i < reports.GetSize(); i++)
    {
        SmAssertReport* r = reports[i];
        v.insert(v.end(), {(double)r->m_lReportingType, (double)r->m_lListIndex,
                           (double)r->m_lTestIndex, (double)r->m_eAssertType, r->m_bOK ? 1.0 : 0.0});
        v.push_back(av_id(r->m_pReporter, surface, placement));
        v.push_back(av_id(r->m_pOwner, surface, placement));
        v.push_back(av_id(r->m_pOther, surface, placement));
        v.insert(v.end(), {(double)r->m_lOwnerType, (double)r->m_lOtherType, (double)r->m_sTol3d,
                           r->m_dValue, r->m_dDist3d});
        av_text(v, r->m_pName);
        av_text(v, r->m_pMessage);
        av_text(v, r->m_pMessageForFile);
        av_text(v, r->m_sOwnerTypeString);
        av_text(v, r->m_sOtherTypeString);
    }
    v.push_back(reports.GetLastAdd() ? 1.0 : 0.0);
    delete surface;
    return sc_copy_out(v.data(), (int64_t)v.size(), out, cap, out_len);
    SC_CATCH
}
