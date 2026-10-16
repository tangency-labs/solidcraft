// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p8`: the SmSurface members owned by the unit
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h), called through `SmSurface*` on
// SmBSplineSurface, SmPlane and SmSurfOfRevolution objects (virtual dispatch as upstream):
// FindKnotsByMaxContinuity, CoincidenceCheck, GlobalLineIntersect, GlobalCurveIntersect,
// LocalLineIntersect, InvertPointNearSingularity and CreateBlendSurface.
//
// The statics `sm_ExtractIsoCurve` and `sm_IntersectSrfBndryCrv` cannot be linked against and
// are reached only through unported callers (LiftCurve, the surface intersector); they are
// covered by Rust unit tests. SmSPGlobalSolver::LocalSolve is a file-local class: its
// declaration is reproduced from SmSurface.cpp:19523 (as unit surface.surface_p9 does for
// SmCSIGlobalSolver) and the unmodified member is called on a one-leaf surface cache.
//
// One entry point, sc_surface_surface_p8_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p8.rs, which validates every
// input.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmGlobalSolver.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::put_vec;

// SmSPGlobalSolver (SmSurface.cpp:19523): the constructor and virtuals are linked from SMLib.
class SmSPGlobalSolver : public SmGlobalSolver
{
  public:
    const SmSurface* m_pSurf;
    const SmExtent2d& m_crUVDomain;
    SmSPGlobalSolver(SmTree* pPointTree, SmTree* pSurfaceTree, const SmExtent2d& crUVDomain);
    virtual SmBoolean BranchMayContainAnswers(SmTreeNode* apBranch[SM_GS_MAX_TREES]);
    virtual SmStatus LocalSolve(SmTreeNode* apBranch[SM_GS_MAX_TREES], SmBoolean& rbNeedsMoreSubdivision);
    virtual SmSolutionPairType IdenticalSolutions(const SmSolution& crSol1, const SmSolution& crSol2,
                                                  const SmSolution*& pPreferredSol) const;
};

namespace
{
enum : int32_t
{
    OP_KNOTS = 0,
    OP_COINC = 1,
    OP_LINE = 2,
    OP_CURVE = 3,
    OP_LOCAL_LINE = 4,
    OP_INVERT = 5,
    OP_BLEND = 6,
    OP_SPLOCAL = 7,
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmBoolean bflag() { return flag() ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double u = num();
        double v = num();
        return SmPoint2d(u, v);
    }
    SmExtent1d ivl()
    {
        double a = num();
        double b = num();
        return SmExtent1d(a, b);
    }
    SmExtent2d dom()
    {
        double u0 = num();
        double v0 = num();
        double u1 = num();
        double v1 = num();
        return SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

SmBSplineCurve* read_bspline_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = rd.num();
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(cur, dim, FALSE);
}

// `0 bspline` (SmBSplineCurve) or `1 A[3] B[3]` (SmLine::CreateLineSegment, 3D).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, l) != SM_SUCCESS)
            return NULL;
        return l;
    }
    return read_bspline_curve(rd, ctx);
}

// `0 oob form n m p q r s cpts(x y z w) u v` (SmBSplineSurface), `1 O[3] N[3]` (SmPlane) or
// `3 A[3] B[3] P[3] axis[3] swap` (SmSurfOfRevolution::CreateCanonical of an SmLine generator).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 3)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmPoint3d p = rd.vec();
        SmVector3d ax = rd.vec();
        bool swap = rd.flag();
        SmLine* gen = new (ctx) SmLine(a, b, 3, &ctx);
        gen->GetOrCreateGwNurbPointer();
        SmSurfOfRevolution* s = NULL;
        SmStatus stt = SmSurfOfRevolution::CreateCanonical(ctx, gen, p, ax, s, swap ? TRUE : FALSE);
        if (stt != SM_SUCCESS || s == NULL)
        {
            if (s)
                delete s;
            else
                delete gen;
            return NULL;
        }
        return s;
    }
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
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
    SmBSplineSurface* b = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    b->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    b->SetBSplineSurfaceForm(form);
    return b;
}

// A solution `type nvars nobjs value end_value start[3] end[3] obj0 obj1`; an object is -1 for
// NULL, 0 for `o0`, 1 for `o1` and -2 for any other pointer. Range ends are written for range
// solutions only.
void put_solution(std::vector<double>& o, const SmSolution& s, const SmObject* o0, const SmObject* o1)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_vStart.m_dSolutionValue);
    const bool range = s.m_eSolutionType == SM_ST_RANGE_OF_VALUES;
    o.push_back(range ? s.m_vEnd.m_dSolutionValue : 0.0);
    for (ULONG i = 0; i < 3; i++)
        o.push_back(s.m_vStart[i]);
    for (ULONG i = 0; i < 3; i++)
        o.push_back(range ? s.m_vEnd[i] : 0.0);
    for (int j = 0; j < 2; j++)
    {
        const SmObject* p = s.m_apObjects[j];
        o.push_back(p == NULL ? -1.0 : (p == o0 ? 0.0 : (p == o1 ? 1.0 : -2.0)));
    }
}

void put_solutions(std::vector<double>& o, SmSolutionArray& sols, const SmObject* o0, const SmObject* o1)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
        put_solution(o, sols[i], o0, o1);
}

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    switch (op)
    {
    case OP_KNOTS:
    {
        SmContinuityType c = (SmContinuityType)rd.idx();
        bool at = rd.flag();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        SmTArray<double> uk, vk;
        SmStatus r = s->FindKnotsByMaxContinuity(c, at ? TRUE : FALSE, uk, vk);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)uk.GetSize());
            for (ULONG i = 0; i < uk.GetSize(); i++)
                o.push_back(uk[i]);
            o.push_back((double)vk.GetSize());
            for (ULONG i = 0; i < vk.GetSize(); i++)
                o.push_back(vk[i]);
        }
        break;
    }
    case OP_COINC:
    {
        bool same = rd.flag();
        SmSurface* s2 = read_surface(rd, ctx);
        double tol = rd.num();
        bool whole = rd.flag();
        if (s == NULL || s2 == NULL)
        {
            o.push_back(-3.0);
            delete s2;
            break;
        }
        const SmSurface& other = same ? *s : *s2;
        SmBoolean coincident = FALSE;
        double dist = 0.0;
        SmSurface* contain = NULL;
        SmStatus r = s->CoincidenceCheck(other, tol, coincident, dist, whole ? TRUE : FALSE, contain);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(coincident));
            o.push_back(dist);
        }
        delete s2;
        break;
    }
    case OP_LINE:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        bool has_ivl = rd.flag();
        SmExtent1d ivl = rd.ivl();
        bool ray = rd.flag();
        double tol = rd.num();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        SmSolutionArray sols;
        SmStatus r = s->GlobalLineIntersect(d, p, v, has_ivl ? &ivl : NULL, ray ? TRUE : FALSE, tol, sols);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_solutions(o, sols, NULL, s);
        break;
    }
    case OP_CURVE:
    {
        SmExtent2d d = rd.dom();
        SmCurve* c = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        double tol = rd.num();
        if (s == NULL || c == NULL)
        {
            o.push_back(-3.0);
            delete c;
            break;
        }
        SmSolutionArray sols;
        SmStatus r = s->GlobalCurveIntersect(d, *c, ivl, tol, sols);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_solutions(o, sols, c, s);
        delete c;
        break;
    }
    case OP_LOCAL_LINE:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        SmExtent1d ivl = rd.ivl();
        double dtol = rd.num();
        double atol = rd.num();
        SmPoint2d g = rd.uv();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        SmBoolean found = FALSE;
        SmPoint2d uv;
        double t = 0.0, dev = 0.0;
        SmPoint3d pt;
        SmVector3d nrm;
        SmStatus r = s->LocalLineIntersect(d, p, v, ivl, dtol, atol, g, found, uv, t, dev, pt, nrm);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                o.push_back(uv.x);
                o.push_back(uv.y);
                o.push_back(t);
                o.push_back(dev);
                put_vec(o, pt);
                put_vec(o, nrm);
            }
        }
        break;
    }
    case OP_INVERT:
    {
        SmPoint3d pt = rd.vec();
        SmExtent2d d = rd.dom();
        SmPoint2d g = rd.uv();
        double tol = rd.num();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        SmTerminationReasonType reason = SM_TR_UNDEFINED;
        SmStatus r = s->InvertPointNearSingularity(pt, d, g, tol, reason);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)reason);
            o.push_back(g.x);
            o.push_back(g.y);
        }
        break;
    }
    case OP_BLEND:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        SmExtent1d ivl1 = rd.ivl();
        bool right = rd.flag();
        SmSurface* s2 = read_surface(rd, ctx);
        SmCurve* c2 = read_curve(rd, ctx);
        SmExtent1d ivl2 = rd.ivl();
        bool left = rd.flag();
        double tol = rd.num();
        if (s == NULL || s2 == NULL || c1 == NULL || c2 == NULL)
        {
            o.push_back(-3.0);
            delete s2;
            delete c1;
            delete c2;
            break;
        }
        // Upstream ignores the status of both GetSampleValues calls and goes on with the partly
        // filled sample arrays (uninitialised parameters, zero vectors), which corrupts the heap in
        // the skinning that follows (UB-157, plan/upstream-issues.md). Probe the same two calls with
        // CreateBlendSurface's default definition and report SM_ERR, as the port does at the first
        // failing sample, instead of running the undefined part.
        {
            SmDerivSurfDefinition def;
            def.m_lNumSamples = 100;
            def.m_eTangencyType = SM_TT_DIR_TO_REFERENCE_CURVE;
            def.m_dStartDerivDirAngleDeg = 0.0;
            def.m_dEndDerivDirAngleDeg = 0.0;
            def.m_dStartTanLength = 1.0;
            def.m_dEndTanScale = 1.0;
            def.m_dStartAngleDeg = 0.0;
            def.m_dEndAngleDeg = 0.0;
            def.m_vVector.Set(0, 0, 0);
            def.m_eCurvatureType = SM_CT_G2_FROM_SURFACE;
            def.m_eBlendEndType = SM_BE_NO_CUSP;
            def.m_dStartRadius = 0.0;
            def.m_dEndRadScale = 0.0;
            def.m_pCurvatureReference = NULL;
            def.m_bFlipCurvature = FALSE;
            ULONG nd = def.GetNumDerivs();
            SmTArray<double> params;
            SmTArray<SmVector3d> p0, d0, p1, d1, s20, s30, s21, s31;
            SmExtent1d i1 = ivl1;
            SmExtent1d i2 = ivl2;
            def.m_pTangentReference = c2;
            SmStatus r1 = def.GetSampleValues(*s, *c1, i1, right ? TRUE : FALSE, TRUE, params, p0, d0,
                                              nd >= 2 ? &s20 : NULL, nd >= 3 ? &s30 : NULL);
            def.m_pTangentReference = c1;
            SmStatus r2 = def.GetSampleValues(*s2, *c2, i2, left ? TRUE : FALSE, FALSE, params, p1, d1,
                                              nd >= 2 ? &s21 : NULL, nd >= 3 ? &s31 : NULL);
            if (r1 != SM_SUCCESS || r2 != SM_SUCCESS)
            {
                o.push_back(st(SM_ERR));
                delete s2;
                delete c1;
                delete c2;
                break;
            }

            // CreateSkinnedSurface (SmBSplineSurfaceNLib.cpp:5305) copies the 101 rail parameters
            // CreateBlendSurface passes and, when SyncronizeKnotsOfCurves changes the natural
            // interval of rail 2, reads and writes sIntParams[numSections + ii]: past the end of the
            // array (heap corruption). The port reports InvalidInput ("too few intersection
            // parameters"); the probe runs the synchronisation on copies and reports 1007.
            SmCurve* k1 = NULL;
            SmCurve* k2 = NULL;
            bool moves = false;
            if (c1->Copy(ctx, k1) == SM_SUCCESS && c2->Copy(ctx, k2) == SM_SUCCESS && k1 != NULL && k2 != NULL)
            {
                SmBSplineCurve* b1 = (SmBSplineCurve*)k1;
                SmBSplineCurve* b2 = (SmBSplineCurve*)k2;
                SmExtent1d before = b2->GetNaturalInterval();
                SmTArray<SmBSplineCurve*> rails;
                rails.Add(b1);
                rails.Add(b2);
                if (SmBSplineCurve::SyncronizeKnotsOfCurves(rails, SM_EFF_ZERO * 100) == SM_SUCCESS)
                {
                    SmExtent1d after = b2->GetNaturalInterval();
                    moves = fabs(after.GetMin() - before.GetMin()) > SM_EFF_ZERO ||
                            fabs(after.GetMax() - before.GetMax()) > SM_EFF_ZERO;
                }
            }
            delete k1;
            delete k2;
            if (moves)
            {
                o.push_back(1007.0);
                delete s2;
                delete c1;
                delete c2;
                break;
            }
        }
        SmSurface* blend = NULL;
        SmStatus r = s->CreateBlendSurface(ctx, *c1, ivl1, right ? TRUE : FALSE, *s2, *c2, ivl2, left ? TRUE : FALSE, tol,
                                           NULL, blend);
        o.push_back(st(r));
        if (r == SM_SUCCESS && blend != NULL)
        {
            SmExtent2d nd = blend->GetNaturalUVDomain();
            o.push_back(nd.GetUMin());
            o.push_back(nd.GetVMin());
            o.push_back(nd.GetUMax());
            o.push_back(nd.GetVMax());
            for (int i = 0; i <= 4; i++)
                for (int j = 0; j <= 4; j++)
                {
                    SmPoint3d p;
                    SmPoint2d uv = nd.Evaluate(i / 4.0, j / 4.0);
                    if (blend->EvaluatePoint(uv, p) == SM_SUCCESS)
                        put_vec(o, p);
                    else
                        put_vec(o, SmVector3d(NAN, NAN, NAN));
                }
        }
        delete blend;
        delete s2;
        delete c1;
        delete c2;
        break;
    }
    case OP_SPLOCAL:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d pt = rd.vec();
        int64_t sop = rd.idx();
        double tol = rd.num();
        double best_sq = rd.num();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        bool failed = false;
        {
            SmSurfaceCache sc(*s, s->GetNaturalUVDomain(), 0.01);
            SmStatus r = sc.BuildTree(NULL);
            if (r != SM_SUCCESS)
                failed = true;
            else
            {
                SmTreeNode point_node;
                SmTree point_tree(pt, 0, &point_node);
                SmSPGlobalSolver solver(&point_tree, sc.GetTree(), d);
                SmSolutionArray solutions;
                solver.SetSolutions(&solutions);
                solver.SetSolverOperation((SmSolverOperationType)sop);
                solver.Set3DTolerance(tol);
                solver.SetSolutionRequested(SM_SR_ALL);
                solver.SetBestAnswerSoFarSq(best_sq);
                SmTreeNode* branch[SM_GS_MAX_TREES] = {&point_node, sc.GetTree()->GetFirstLeafNode(), NULL};
                SmBoolean more = FALSE;
                r = solver.LocalSolve(branch, more);
                o.push_back(st(r));
                if (r == SM_SUCCESS)
                {
                    o.push_back(flg(more));
                    o.push_back((double)solutions.GetSize());
                    for (ULONG i = 0; i < solutions.GetSize(); i++)
                    {
                        const SmSolution& sol = solutions[i];
                        o.push_back((double)sol.m_eSolutionType);
                        o.push_back((double)sol.m_lNumVariables);
                        o.push_back(sol.m_vStart.m_dSolutionValue);
                        o.push_back(sol.m_vStart[0]);
                        o.push_back(sol.m_vStart[1]);
                    }
                }
            }
        }
        if (failed)
            o.push_back(-3.0);
        break;
    }
    default:
        o.push_back((double)SC_ERR_ARG);
        break;
    }
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_p8_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, o);
    if (rd.at != n_in)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
