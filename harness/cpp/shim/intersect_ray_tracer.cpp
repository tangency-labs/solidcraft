// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.ray_tracer`: SmRayTracer and SmLSI3LocalSolver
// (source/SMLib/src/SmRayTracer.cpp, source/SMLib/inc/SmRayTracer.h).
//
// One entry point, sc_intersect_ray_tracer_run(in, n_in, out, cap, out_len): builds real
// SmPlane / SmBSplineSurface surfaces, an SmGrid, and an SmRayTracer owning the grid, then
// runs ops against them and returns one flat record. The layout is documented in
// crates/solidcore-ffi/src/units/intersect_ray_tracer.rs, which validates every input; reads are
// bounds checked here too (a bad program is SC_ERR_ARG).
//
// Surface leaves: SmSurfaceGridElement::Init needs a surface subdivision tree node
// (SmBezierPatch), which the Rust port does not have yet. ADD_LEAF therefore fills an
// SmSurfaceGridElement the way SmGrid::AddElement + Init do, from the leaf's domain, box,
// pseudo box and polar box given as data, computing the mid-point data from the real surface
// exactly as Init does. SmGrid's protected members are reached by exposing them while SmGrid.h
// is included.
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmMemBlockMgr.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSArray.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#define protected public
#include <SmGrid.h>
#undef protected

#include <SmRayTracer.h>

#include "sc_sm.h"

using sc::st;

namespace
{
enum : int32_t
{
    OP_SOLVER_TOL = 1,
    OP_ADD_LEAF = 2,
    OP_FIRE = 3,
    OP_LOCAL = 4,
    OP_ADD_SURFACE = 5,
    OP_RENDERER = 6,
    OP_LIGHT = 7,
    OP_DO_RENDER = 8,
    OP_RENDER_PIXEL = 9,
    OP_RENDER_RAY = 10,
};

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmVector2d uv()
    {
        double x = num();
        double y = num();
        return SmVector2d(x, y);
    }
    SmExtent2d dom4()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
    SmExtent3d ext6()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        double e = num();
        double f = num();
        return SmExtent3d(a, b, c, d, e, f);
    }
};

// `1 n m p q cpts(x y z w)` clamped Bezier-knot B-spline surface: degree p x q, (p+1)(q+1) points,
// knots 0..0 1..1; or `kind 1 o x y scale[2] dom[4]`, an SmPlane.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        gw_DEGREE p = (gw_DEGREE)rd.idx(1, 3);
        gw_DEGREE q = (gw_DEGREE)rd.idx(1, 3);
        gw_INDEX n = p;
        gw_INDEX m = q;
        gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, n + p + 1, m + q + 1);
        for (gw_INDEX i = 0; i <= n; i++)
            for (gw_INDEX j = 0; j <= m; j++)
            {
                sur->net->Pw[i][j].x = rd.num();
                sur->net->Pw[i][j].y = rd.num();
                sur->net->Pw[i][j].z = rd.num();
                sur->net->Pw[i][j].w = rd.num();
            }
        for (gw_INDEX i = 0; i <= n + p + 1; i++)
            sur->knu->U[i] = i <= p ? 0.0 : 1.0;
        for (gw_INDEX i = 0; i <= m + q + 1; i++)
            sur->knv->U[i] = i <= q ? 0.0 : 1.0;
        return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
    }
    SmPoint3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector2d s = rd.uv();
    SmExtent2d d = rd.dom4();
    return new (ctx) SmPlane(o, x, y, s, d, &ctx);
}

void put3(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// The solution fields of FireRay / LocalSolve: type, variable and object counts, solution value,
// the ten start parameters, the surface index and whether object 1 and node 0 are set.
void put_solution(std::vector<double>& o, const SmSolution& s, const std::vector<SmSurface*>& surfs)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_vStart.m_dSolutionValue);
    for (int i = 0; i < 10; i++)
        o.push_back(s.m_vStart[i]);
    double surf = -1.0;
    for (size_t i = 0; i < surfs.size(); i++)
        if (s.m_apObjects[0] == (SmObject*)surfs[i])
            surf = (double)i;
    if (surf < 0.0 && s.m_apObjects[0] != nullptr)
        surf = -2.0;
    o.push_back(surf);
    o.push_back(s.m_apObjects[1] == nullptr ? -1.0 : -2.0);
    o.push_back(s.m_apNodes[0] == nullptr ? 0.0 : 1.0);
}

int32_t run(const double* p, int64_t n, std::vector<double>& o)
{
    Rd rd{p, n, 0};
    SmContext sCtx;
    std::vector<SmSurface*> surfs;
    SmRayTracer* rt = nullptr;
    SmRayRenderer* renderer = nullptr;
    std::vector<SmSurfaceGridElement*> leaves;
    int32_t rc = SC_OK;
    try
    {
        int64_t nsurf = rd.idx(1, 4);
        for (int64_t i = 0; i < nsurf; i++)
            surfs.push_back(read_surface(rd, sCtx));
        SmExtent3d gbox = rd.ext6();
        ULONG lSize[3];
        for (int i = 0; i < 3; i++)
            lSize[i] = (ULONG)rd.idx(1, 20);
        double rayTol = rd.num();
        double rayAcc = rd.num();
        SmGrid* g = new (sCtx) SmGrid(gbox, lSize);
        rt = new (sCtx) SmRayTracer(sCtx, g, rayTol, rayAcc);

        int64_t nops = rd.idx(0, 64);
        for (int64_t k = 0; k < nops; k++)
        {
            int op = (int)rd.idx(1, 10);
            switch (op)
            {
            case OP_SOLVER_TOL:
                rt->SetSolverTolerance(rd.num());
                break;
            case OP_ADD_LEAF:
            {
                SmSurface* surf = surfs.at((size_t)rd.idx(0, nsurf - 1));
                SmExtent2d dom = rd.dom4();
                SmExtent3d bbox = rd.ext6();
                SmExtent3d pseudoExt = rd.ext6();
                bool rotated = rd.flag();
                SmPseudoBox pseudo(pseudoExt);
                if (rotated)
                {
                    SmVector3d bx = rd.vec();
                    SmVector3d by = rd.vec();
                    SmVector3d bz = rd.vec();
                    SmExtent1d ix(pseudoExt.GetMin(0), pseudoExt.GetMax(0));
                    SmExtent1d iy(pseudoExt.GetMin(1), pseudoExt.GetMax(1));
                    SmExtent1d iz(pseudoExt.GetMin(2), pseudoExt.GetMax(2));
                    pseudo = SmPseudoBox(bx, by, bz, ix, iy, iz);
                }
                int64_t np = rd.idx(1, 8);
                SmPolarBox polar(rd.vec());
                for (int64_t i = 1; i < np; i++)
                    polar.AddVector3d(rd.vec(), NULL);
                SmExtent3d elemBox = rd.ext6();
                if (leaves.size() >= 64)
                    throw Bad();

                // SmGrid::AddElement(SM_GE_SURFACE) with SmSurfaceGridElement::Init
                SmSurfaceGridElement* pSGE = (SmSurfaceGridElement*)g->m_sSurfaceElementMgr.GetNewElement();
                pSGE->m_lMark = 0;
                pSGE->m_lType = SmSurfaceGridElement_TYPE;
                pSGE->m_pSurface = surf;
                pSGE->m_sUVDomain = dom;
                pSGE->m_sBBox = bbox;
                pSGE->m_sPseudoBox = pseudo;
                pSGE->m_sPolarBox = polar;
                SmPoint2d sUVMid = pSGE->m_sUVDomain.Evaluate(0.5, 0.5);
                SmVector3d sEval[2][2];
                surf->Evaluate(sUVMid, 1, 1, TRUE, TRUE, TRUE, sEval[0]);
                pSGE->m_sMidPoint = sEval[0][0];
                pSGE->m_sMidDU = sEval[1][0];
                pSGE->m_sMidDV = sEval[0][1];
                pSGE->m_sMidNormal = pSGE->m_sMidDU * pSGE->m_sMidDV;
                if (pSGE->m_sMidNormal.LengthSquared() > SM_EFF_ZERO)
                    pSGE->m_sMidNormal.Unitize();
                else
                    surf->EvaluateNormal(sUVMid, TRUE, TRUE, pSGE->m_sMidNormal);

                SmGridElement* pGE = pSGE;
                elemBox.ComputeSphereBound(pGE->m_sSphereCenter, pGE->m_dSphereRadius);
                pGE->m_sBBox = elemBox;
                SmVoxel* sVData[256];
                SmTArray<SmVoxel*> sVoxels(256, sVData);
                g->FindVoxelsInBox(elemBox, sVoxels);
                for (ULONG i = 0; i < sVoxels.GetSize(); i++)
                {
                    SmVoxel* pVoxel = sVoxels[i];
                    if (pVoxel->m_pElements == NULL)
                        pVoxel->m_pElements = new (sCtx) SmTArray<SmGridElement*>(sCtx);
                    pVoxel->m_pElements->Add(pGE);
                }
                leaves.push_back(pSGE);
                break;
            }
            case OP_FIRE:
            {
                SmPoint3d rp = rd.vec();
                SmVector3d rv = rd.vec();
                double maxT = rd.num();
                rt->m_bHitAnyThing = rd.flag();
                int64_t skip = rd.idx(-1, 63);
                if (skip >= (int64_t)leaves.size())
                    throw Bad();
                rt->m_pSkipElement = skip < 0 ? NULL : leaves[(size_t)skip];
                SmBoolean bHit = FALSE;
                SmSolution sol;
                SmTArray<SmGridElement*> ge;
                SmTArray<double> mn, mx;
                SmStatus s = rt->FireRay(rp, rv, maxT, bHit, sol, ge, mn, mx);
                o.push_back(st(s));
                o.push_back(bHit ? 1.0 : 0.0);
                put_solution(o, sol, surfs);
                o.push_back((double)ge.GetSize());
                for (ULONG i = 0; i < ge.GetSize(); i++)
                {
                    double id = -1.0;
                    for (size_t j = 0; j < leaves.size(); j++)
                        if (ge[i] == (SmGridElement*)leaves[j])
                            id = (double)j;
                    o.push_back(id);
                }
                for (ULONG i = 0; i < mn.GetSize(); i++)
                    o.push_back(mn[i]);
                for (ULONG i = 0; i < mx.GetSize(); i++)
                    o.push_back(mx[i]);
                break;
            }
            case OP_LOCAL:
            {
                int64_t li = rd.idx(0, 63);
                if (li >= (int64_t)leaves.size())
                    throw Bad();
                SmPoint3d lp = rd.vec();
                SmVector3d lv = rd.vec();
                double d3d = rd.num();
                double distTol = rd.num();
                double acc = rd.num();
                bool rayFire = rd.flag();
                bool haveT = rd.flag();
                double tmin = rd.num();
                double tmax = rd.num();
                bool hasIvl = rd.flag();
                double i0 = rd.num();
                double i1 = rd.num();
                SmSurfaceGridElement* leaf = leaves[(size_t)li];
                SmExtent1d ivl(i0, i1);
                SmLSI3LocalSolver solver(NULL, leaf->m_sUVDomain, lp, lv, hasIvl ? &ivl : NULL, rayFire, distTol,
                                         acc);
                solver.m_d3dTolerance = d3d;
                solver.m_bHaveTMinTMax = haveT;
                solver.m_dTMinOfBBox = tmin;
                solver.m_dTMaxOfBBox = tmax;
                SmBoolean bNeeds = TRUE, bFound = TRUE;
                SmSolution sol;
                SmStatus s = solver.LocalSolve(leaf, bNeeds, bFound, sol);
                o.push_back(st(s));
                o.push_back(bFound ? 1.0 : 0.0);
                o.push_back(bNeeds ? 1.0 : 0.0);
                put_solution(o, sol, surfs);
                break;
            }
            case OP_ADD_SURFACE:
                // needs the surface cache; the Rust side is deferred, nothing to compare
                rd.idx(0, nsurf - 1);
                break;
            case OP_RENDERER:
            {
                if (renderer != nullptr)
                    throw Bad();
                SmPoint3d o = rd.vec();
                SmVector3d x = rd.vec();
                SmVector3d y = rd.vec();
                SmAxis2Placement place(o, x, y);
                SmExtent3d view = rd.ext6();
                ULONG nx = (ULONG)rd.idx(1, 8);
                ULONG ny = (ULONG)rd.idx(1, 8);
                ULONG rate = (ULONG)rd.idx(1, 3);
                renderer = new (sCtx) SmRayRenderer(rt, place, view, nx, ny);
                renderer->SetSampleRate(rate);
                break;
            }
            case OP_LIGHT:
            {
                if (renderer == nullptr)
                    throw Bad();
                SmVector3d v = rd.vec();
                bool pt = rd.flag();
                renderer->AddLightSource(new (sCtx) SmLightSource(v, pt));
                break;
            }
            case OP_DO_RENDER:
            {
                if (renderer == nullptr)
                    throw Bad();
                bool shadows = rd.flag();
                SmVector3d bg = rd.vec();
                SmVector3d obj = rd.vec();
                SmStatus s = renderer->DoRender(shadows, bg, obj);
                o.push_back(st(s));
                o.push_back((double)renderer->GetPixelsShaded());
                break;
            }
            case OP_RENDER_PIXEL:
            {
                if (renderer == nullptr)
                    throw Bad();
                ULONG px = (ULONG)rd.idx(0, 7);
                ULONG py = (ULONG)rd.idx(0, 7);
                SmVector3d c;
                SmStatus s = renderer->RenderPixel(px, py, c);
                o.push_back(st(s));
                put3(o, c);
                o.push_back((double)renderer->GetPixelsShaded());
                break;
            }
            case OP_RENDER_RAY:
            {
                if (renderer == nullptr)
                    throw Bad();
                SmVector3d rp = rd.vec();
                SmVector3d rv = rd.vec();
                SmVector3d c;
                SmStatus s = renderer->RenderRay(rp, rv, c);
                o.push_back(st(s));
                put3(o, c);
                o.push_back((double)renderer->GetPixelsShaded());
                break;
            }
            default:
                throw Bad();
            }
        }
    }
    catch (const Bad&)
    {
        rc = SC_ERR_ARG;
    }
    if (renderer != nullptr)
        delete renderer; // deletes the tracer and the lights
    else
        delete rt; // deletes the grid, not the surfaces
    for (SmSurface* s : surfs)
        delete s;
    return rc;
}
} // namespace

SC_EXPORT int32_t sc_intersect_ray_tracer_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                              int64_t* out_len)
{
    SC_TRY
    std::vector<double> o;
    int32_t s = run(in, n_in, o);
    if (s != SC_OK)
        return s;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
