// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.loop`: SmLoop::ContainsUVPoint (source/SMLib/src/SmLoop.cpp)
// on a one-face B-rep built with SmBrepConstructor (source/SMLib/src/SmBrepData.cpp), the
// way tests/prog_test/src/smTopoTest.cpp builds its periodic-seam faces: every edge gets its
// 3D curve and interval, every edge use of the loop gets an explicit 2D B-spline UV trim
// curve (SmBrepConstructor::SetEdgeCurve(.., TRUE)), and the face and edge zone tolerances
// are set explicitly (SetTolerance(t, FALSE, FALSE)).
//
// One entry point, sc_query_loop_contains(in, n_in, out): the layout is documented in
// crates/solidcore-ffi/src/units/query_loop.rs, which validates every count, index, kind
// and value before the call. `out[5]` receives: the build status (0 = built, else the
// failing step: 1 surface, 3 constructor step, 4 edge use order or orientation differs
// from the request, 5 the surface cannot evaluate the point), the
// SmPointObjectContainmentType, the index of the on-edge (-1 = NULL), the edge parameter
// and the SmOrientType.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSurfOfRevolution.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

namespace
{
struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

// `n p m cpts(x y z w | x y w) knots` (`dim` 3 or 2; a 2D point has z = 0).
SmBSplineCurve* read_bspline_curve(Rd& rd, const SmContext& ctx, ULONG dim)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = dim == 3 ? rd.num() : 0.0;
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(cur, dim, FALSE);
}

// A 3D curve block: `0 a[3] b[3]` SmLine(a, b), `1 c[3] x[3] y[3] d0 d1 r`
// SmCircle(c, x, y, [d0, d1], r) or `2 bspline` (see read_bspline_curve).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        return new (ctx) SmLine(a, b, 3, &ctx);
    }
    if (kind == 1)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    return read_bspline_curve(rd, ctx, 3);
}

// A surface block: `0 O[3] X[3] Y[3] s[2] d[4]` SmPlane(O, X, Y, s, SmExtent2d(d0, d1, d2, d3)), `1 n m p q r s
// net((n+1)(m+1) x y z w) U V` SmBSplineSurface, `2 gen P[3] A[3] swap`
// SmSurfOfRevolution::CreateCanonical with the generator a line `0 a[3] b[3]` (its NURB
// made) or a circle `1 c[3] x[3] y[3] d0 d1 r`. NULL on failure.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double su = rd.num();
        double sv = rd.num();
        double a = rd.num();
        double b = rd.num();
        double c = rd.num();
        double d = rd.num();
        return new (ctx) SmPlane(o, x, y, SmVector2d(su, sv), SmExtent2d(a, b, c, d), &ctx);
    }
    if (kind == 1)
    {
        gw_INDEX n = (gw_INDEX)rd.idx();
        gw_INDEX m = (gw_INDEX)rd.idx();
        gw_DEGREE p = (gw_DEGREE)rd.idx();
        gw_DEGREE q = (gw_DEGREE)rd.idx();
        gw_INDEX r = (gw_INDEX)rd.idx();
        gw_INDEX s = (gw_INDEX)rd.idx();
        gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
        for (gw_INDEX i = 0; i <= n; i++)
            for (gw_INDEX j = 0; j <= m; j++)
            {
                sur->net->Pw[i][j].x = rd.num();
                sur->net->Pw[i][j].y = rd.num();
                sur->net->Pw[i][j].z = rd.num();
                sur->net->Pw[i][j].w = rd.num();
            }
        for (gw_INDEX i = 0; i <= r; i++)
            sur->knu->U[i] = rd.num();
        for (gw_INDEX i = 0; i <= s; i++)
            sur->knv->U[i] = rd.num();
        return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
    }
    int64_t gk = rd.idx();
    SmBSplineCurve* gen = NULL;
    if (gk == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        gen = l;
    }
    else
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        gen = new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    SmPoint3d pt = rd.vec();
    SmVector3d ax = rd.vec();
    bool swap = rd.num() != 0.0;
    SmSurfOfRevolution* rev = NULL;
    if (SmSurfOfRevolution::CreateCanonical(ctx, gen, pt, ax, rev, swap ? TRUE : FALSE) != SM_SUCCESS)
    {
        delete rev;
        return NULL;
    }
    return rev;
}

// Curves not (yet) handed to the B-rep; deleted on every return.
struct Owned
{
    std::vector<SmCurve*> curves;
    std::vector<SmBSplineCurve*> trims;
    ~Owned()
    {
        for (size_t i = 0; i < curves.size(); i++)
            delete curves[i];
        for (size_t i = 0; i < trims.size(); i++)
            delete trims[i];
    }
};

int32_t run(Rd& rd, double* out)
{
    SmContext ctx;
    double face_tol = rd.num();
    bool vertex_loop = rd.num() != 0.0;
    SmPoint2d uv;
    uv.x = rd.num();
    uv.y = rd.num();
    double tol3d = rd.num();

    SmBrepConstructor b;
    SmBrep* brep = b.StartBrep(ctx);
    if (brep == NULL)
        return SC_ERR_ARG;
    SmObjDelete clean(brep);
    out[0] = 3.0;

    SmShell *outer = NULL, *inner = NULL;
    if (b.StartShell(outer, inner) != SM_SUCCESS)
        return SC_OK;
    SmFace* face = b.StartFace(SM_OT_SAME);
    if (face == NULL)
        return SC_OK;
    SmSurface* srf = read_surface(rd, ctx);
    if (srf == NULL)
    {
        out[0] = 1.0;
        return SC_OK;
    }
    if (b.SetFaceSurface(srf, srf->GetNaturalUVDomain()) != SM_SUCCESS)
        return SC_OK;
    {
        // ContainsUVPoint ignores this status and reads the point: reject a failure
        SmPoint3d chk;
        if (srf->EvaluatePoint(uv, chk) != SM_SUCCESS)
        {
            out[0] = 5.0;
            return SC_OK;
        }
    }

    // edges: curve, interval, tolerance
    int64_t n_edges = rd.idx();
    Owned owned;
    std::vector<SmCurve*>& curves = owned.curves;
    std::vector<double> ivl, tols;
    for (int64_t i = 0; i < n_edges; i++)
    {
        curves.push_back(read_curve(rd, ctx));
        ivl.push_back(rd.num());
        ivl.push_back(rd.num());
        tols.push_back(rd.num());
    }
    std::vector<SmEdge*> edges((size_t)n_edges, (SmEdge*)NULL);

    // edge uses in loop order: edge index, orientation, UV trim curve
    int64_t n_eus = rd.idx();
    std::vector<int64_t> eu_edge;
    std::vector<SmOrientType> eu_or;
    std::vector<SmBSplineCurve*>& trims = owned.trims;
    for (int64_t i = 0; i < n_eus; i++)
    {
        eu_edge.push_back(rd.idx());
        eu_or.push_back(rd.idx() == 1 ? SM_OT_SAME : SM_OT_OPPOSITE);
        trims.push_back(read_bspline_curve(rd, ctx, 2));
    }

    SmLoop* loop = NULL;
    std::vector<SmEdgeuse*> made((size_t)n_eus, (SmEdgeuse*)NULL);
    if (vertex_loop)
    {
        loop = b.StartAndEndSingleVertexLoop(SmPoint3d(0, 0, 0));
        if (loop == NULL)
            return SC_OK;
    }
    else
    {
        loop = b.StartLoop(SM_OT_SAME);
        if (loop == NULL)
            return SC_OK;
        // The loopuse starts at the edge use added last and walks the others in the
        // order added: add L[1], .., L[n-1], L[0] to get L.
        for (int64_t k = 0; k < n_eus; k++)
        {
            int64_t i = (k + 1) % n_eus;
            int64_t e = eu_edge[(size_t)i];
            SmEdge* existing = edges[(size_t)e];
            SmEdge* edge = b.StartEdge(eu_or[(size_t)i], existing, NULL, NULL);
            if (edge == NULL)
                return SC_OK;
            if (existing == NULL)
            {
                edges[(size_t)e] = edge;
                SmCurve* c = curves[(size_t)e];
                curves[(size_t)e] = NULL;
                if (b.SetEdgeCurve(c, FALSE) != SM_SUCCESS)
                    return SC_OK;
                if (b.SetLimits(SmVertexNode(ivl[(size_t)(2 * e)]), SmVertexNode(ivl[(size_t)(2 * e + 1)])) !=
                    SM_SUCCESS)
                    return SC_OK;
            }
            SmBSplineCurve* t = trims[(size_t)i];
            trims[(size_t)i] = NULL;
            if (b.SetEdgeCurve(t, TRUE) != SM_SUCCESS)
                return SC_OK;
            SmTArray<SmEdgeuse*> eus;
            loop->GetEdgeuses(eus);
            made[(size_t)i] = NULL;
            for (ULONG j = 0; j < eus.GetSize(); j++)
                if (eus[j]->GetUVTrimCurvePointer() == t)
                    made[(size_t)i] = eus[j];
            if (b.EndEdge() != SM_SUCCESS)
                return SC_OK;
        }
    }
    // (StartAndEndSingleVertexLoop already ended its loop)
    if ((!vertex_loop && b.EndLoop() != SM_SUCCESS) || b.EndFace() != SM_SUCCESS || b.EndShell(FALSE) != SM_SUCCESS ||
        b.EndBrep() != SM_SUCCESS)
        return SC_OK;

    // the realised loop must be the requested one
    out[0] = 4.0;
    if (!vertex_loop)
    {
        SmTArray<SmEdgeuse*> eus;
        loop->GetEdgeuses(eus);
        if ((int64_t)eus.GetSize() != n_eus)
            return SC_OK;
        for (int64_t i = 0; i < n_eus; i++)
        {
            SmEdgeuse* eu = eus[(ULONG)i];
            if (eu != made[(size_t)i] || eu->GetOrientation() != eu_or[(size_t)i] ||
                eu->GetEdge() != edges[(size_t)eu_edge[(size_t)i]] || eu->GetUVTrimCurve() == NULL)
                return SC_OK;
        }
    }
    else
    {
        SmTArray<SmEdgeuse*> eus;
        loop->GetEdgeuses(eus);
        if (eus.GetSize() != 0)
            return SC_OK;
    }

    // tolerances (SM_USE_OLDTOL: stored on the face and the edges)
    face->SetTolerance(face_tol, FALSE, FALSE);
    for (int64_t e = 0; e < n_edges; e++)
        if (edges[(size_t)e] != NULL)
            edges[(size_t)e]->SetTolerance(tols[(size_t)e], FALSE, FALSE);
    out[0] = 0.0;

    SmEdge* on = NULL;
    double param = 0.0;
    SmOrientType orient = SM_OT_UNKNOWN;
    SmPointObjectContainmentType cls = loop->ContainsUVPoint(uv, tol3d, on, param, orient);
    int64_t on_idx = -1;
    for (int64_t e = 0; e < n_edges; e++)
        if (on != NULL && edges[(size_t)e] == on)
        {
            on_idx = e;
            break;
        }
    if (on != NULL && on_idx < 0)
        on_idx = -2;
    out[1] = (double)cls;
    out[2] = (double)on_idx;
    out[3] = param;
    out[4] = (double)orient;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_query_loop_contains(const double* in, int64_t n_in, double* out)
{
    if (in == NULL || out == NULL || n_in < 1)
        return SC_ERR_ARG;
    SC_TRY
    for (int i = 0; i < 5; i++)
        out[i] = 0.0;
    Rd rd{in, 0};
    return run(rd, out);
    SC_CATCH
}
