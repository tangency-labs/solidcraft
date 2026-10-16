// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.face_props`: SmFaceProps / SmLoopProps / SmLooptreeItem
// (source/SMLib/src/SmFaceProps.cpp, SmLoopProps.cpp, inc/SmLoopProps.h).
//
// sc_heal_face_props_run(in, n_in, out, cap, out_len): a script on one SmFaceProps of a one-face
// B-rep built with SmBrepConstructor like the query.loop shim: a real SmPlane or an
// SmSurfOfRevolution (CreateCanonical, optionally with a swept angle below 360 degrees through
// AdjustSTEPUVDomain: cylinder, cone, sphere and torus kinds, open or closed) owned by the
// face, with one vertex loop or edge loops of SmLine edges (the first loop outer, the others
// holes). Layout and ops: crates/solidcore-ffi/src/units/heal_face_props.rs (which validates
// the script, including UB-131: a script that makes SetProps_Stage2 / Stage3 dereference the
// NULL m_pFace is rejected there; and UB-128: Stage 3 on a degenerate plane with edge loops). After every op the shim appends a snapshot of the face
// props (SNAP_LEN values; for op 8 the SmLoopProps of the first loops).
//
// sc_heal_face_props_tree(in, n_in, out, cap, out_len): SmLooptreeItem building and FindItem on
// fake loop pointers (small integers that are only ever compared, never dereferenced).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceProps.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopProps.h>
#include <SmLoopuse.h>
#include <SmPlane.h>
#include <SmSurfOfRevolution.h>
#include <SmTArray.h>
#include <SmVector3d.h>

#include <vector>

namespace
{
const int SNAP_LEN = 82;

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

void put(std::vector<double>& o, double v) { o.push_back(v); }

void snapshot(std::vector<double>& o, SmFaceProps& p, SmFace* face)
{
    size_t start = o.size();
    put(o, (double)p.m_ePropStage);
    put(o, (double)p.m_eNotYetFaceType);
    put(o, (double)p.m_bBadHasNotYetProb);
    put(o, (double)p.m_bBadDegenFace);
    put(o, p.m_pFace == NULL ? 0.0 : (p.m_pFace == face ? 1.0 : 2.0));
    put(o, (double)p.m_lFaceIndx);
    put(o, (double)p.m_sZoneTol3d.val);
    put(o, (double)p.m_sOrigZoneTol3d.val);
    put(o, (double)p.m_bBadSmallZoneTol3d);
    put(o, (double)p.m_bBadLargeZoneTol3d);
    put(o, p.m_cpSurface != NULL ? 1.0 : 0.0);
    put(o, (double)p.m_eSheet);
    put(o, (double)p.m_bClosedU);
    put(o, (double)p.m_bClosedV);
    put(o, (double)p.m_bClosedSurf);
    put(o, (double)p.m_eClosedValU);
    put(o, (double)p.m_eClosedValV);
    put(o, (double)p.m_ePoles);
    put(o, (double)p.m_lPoles);
    put(o, (double)p.m_lApproxPoles);
    put(o, (double)p.m_lNoVertexPoles);
    put(o, (double)p.m_lBadFlatCorners);
    put(o, p.m_pCrvClassU != NULL ? 1.0 : 0.0);
    put(o, p.m_pCrvClassV != NULL ? 1.0 : 0.0);
    put(o, (double)p.m_bBadCrossedSeam);
    put(o, (double)p.m_eBadCrossedSeam);
    put(o, (double)p.m_bBadMissingSeam);
    put(o, (double)p.m_eBadMissingSeam);
    put(o, (double)p.m_bBadNearMissSeam);
    put(o, (double)p.m_eBadNearMissSeam);
    put(o, (double)p.m_eBeenThroughMoveSeam);
    put(o, (double)p.m_eBeenThroughSplitEdgesAtSeam);
    put(o, (double)p.m_eBeenThroughSplitFacesAtSeam);
    put(o, (double)p.m_lClosedLoopCnt);
    put(o, (double)p.m_lOuterLoopCnt);
    put(o, (double)p.m_bBadLoopProblems);
    put(o, (double)p.m_bBadOuterLoopOrder);
    put(o, (double)p.m_lBadOrient_LoopCnt);
    put(o, (double)p.m_lBadNoArea_LoopCnt);
    put(o, (double)p.m_lBadNested_LoopCnt);
    put(o, (double)p.m_bBadUnpairedNoArea_Loop);
    put(o, (double)p.m_lBadClosed3d_LoopCnt);
    put(o, (double)p.m_lBadClosedPtr_LoopCnt);
    put(o, (double)p.m_lBadMissingPoles);
    put(o, (double)p.m_sLoopProps.GetSize());
    put(o, (double)p.m_sPolePoints.GetSize());
    for (int i = 0; i < 4; i++)
    {
        bool ok = (ULONG)i < p.m_sPolePoints.GetSize();
        put(o, ok ? p.m_sPolePoints[i].x : 0.0);
        put(o, ok ? p.m_sPolePoints[i].y : 0.0);
        put(o, ok ? p.m_sPolePoints[i].z : 0.0);
    }
    put(o, p.m_sNaturalUVDomain.GetUMin());
    put(o, p.m_sNaturalUVDomain.GetVMin());
    put(o, p.m_sNaturalUVDomain.GetUMax());
    put(o, p.m_sNaturalUVDomain.GetVMax());
    put(o, (double)p.m_sFlatCornerPoints.GetSize());
    // the flat corner points (an unset corner stays SetUninitialized)
    for (int i = 0; i < 4; i++)
    {
        bool ok = (ULONG)i < p.m_sFlatCornerPoints.GetSize();
        put(o, ok ? p.m_sFlatCornerPoints[i].x : 0.0);
        put(o, ok ? p.m_sFlatCornerPoints[i].y : 0.0);
        put(o, ok ? p.m_sFlatCornerPoints[i].z : 0.0);
    }
    while (o.size() < start + SNAP_LEN)
        o.push_back(0.0);
}

// seeds one field of the props (field ids: see heal_face_props.rs)
void seed(SmFaceProps& p, SmFace* face, int64_t field, int64_t idx, double v)
{
    switch (field)
    {
    case 0: p.m_ePropStage = (SmPropStageType)(int)v; break;
    case 1: p.m_sZoneTol3d.val = v; break;
    case 2: p.m_sOrigZoneTol3d.val = v; break;
    case 3: p.m_lPoles = (ULONG)v; break;
    case 4:
        if ((ULONG)idx < p.m_sPolePoints.GetSize())
            p.m_sPolePoints[(ULONG)idx] = SmPoint3d(v, v * 0.5, -v);
        break;
    case 5: p.m_bBadSmallZoneTol3d = (SmBooleanUL)v; break;
    case 6: p.m_bBadLargeZoneTol3d = (SmBooleanUL)v; break;
    case 7: p.m_eSheet = (SmMixedLinkType)(int)v; break;
    case 8: p.m_ePoles = (SmMixedLinkType)(int)v; break;
    case 9: p.m_lBadFlatCorners = (ULONG)v; break;
    case 10: p.m_bBadCrossedSeam = (SmBooleanUL)v; break;
    case 11: p.m_bBadMissingSeam = (SmBooleanUL)v; break;
    case 12: p.m_lOuterLoopCnt = (ULONG)v; break;
    case 13: p.m_lBadOrient_LoopCnt = (ULONG)v; break;
    case 14: p.m_lBadNoArea_LoopCnt = (ULONG)v; break;
    case 15: p.m_lBadNested_LoopCnt = (ULONG)v; break;
    case 16: p.m_bBadUnpairedNoArea_Loop = (SmBooleanUL)v; break;
    case 17: p.m_lBadClosed3d_LoopCnt = (ULONG)v; break;
    case 18: p.m_lBadClosedPtr_LoopCnt = (ULONG)v; break;
    case 19: p.m_lBadMissingPoles = (ULONG)v; break;
    case 20: p.m_bBadOuterLoopOrder = (SmBooleanUL)v; break;
    case 21: p.m_eNotYetFaceType = (SmHealNotYetFaceType)(int)v; break;
    case 22: p.m_bClosedU = (SmBoolean)v; break;
    case 23: p.m_bClosedV = (SmBoolean)v; break;
    case 24: p.m_pFace = v != 0.0 ? face : NULL; break;
    case 25: p.m_bBadHasNotYetProb = (SmBooleanUL)v; break;
    default: break;
    }
}

// The face's surface block: `0 O[3] X[3] Y[3] umin vmin umax vmax` SmPlane, or `1 gen P[3] A[3]
// swap sweep` SmSurfOfRevolution::CreateCanonical (generator `0 a[3] b[3]` line with its NURB
// made, or `1 c[3] x[3] y[3] d0 d1 r` circle), the angle domain then adjusted to `sweep` degrees
// when that is below 360. NULL on failure.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double a = rd.num(), bb = rd.num(), c = rd.num(), d = rd.num();
        return new (ctx) SmPlane(o, x, y, SmVector2d(1.0, 1.0), SmExtent2d(a, bb, c, d), &ctx);
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
    double sweep = rd.num();
    SmSurfOfRevolution* rev = NULL;
    if (SmSurfOfRevolution::CreateCanonical(ctx, gen, pt, ax, rev, swap ? TRUE : FALSE) != SM_SUCCESS || rev == NULL)
    {
        delete rev;
        return NULL;
    }
    if (sweep < 360.0)
    {
        SmExtent2d d = rev->GetSTEPUVDomain();
        SmExtent2d nd(d.GetUMin(), d.GetVMin(), d.GetUMin() + sweep, d.GetVMax());
        if (rev->AdjustSTEPUVDomain(nd) != SM_SUCCESS)
        {
            delete rev;
            return NULL;
        }
    }
    return rev;
}

int32_t run(Rd& rd, std::vector<double>& out)
{
    SmContext ctx;
    double face_tol = rd.num();
    SmBrepConstructor b;
    SmBrep* brep = b.StartBrep(ctx);
    if (brep == NULL)
        return SC_ERR_ARG;
    SmObjDelete clean(brep);
    SmShell *outer = NULL, *inner = NULL;
    if (b.StartShell(outer, inner) != SM_SUCCESS)
        return SC_ERR_ARG;
    SmFace* face = b.StartFace(SM_OT_SAME);
    if (face == NULL)
        return SC_ERR_ARG;
    SmSurface* srf = read_surface(rd, ctx);
    if (srf == NULL || b.SetFaceSurface(srf, srf->GetNaturalUVDomain()) != SM_SUCCESS)
        return SC_ERR_ARG;

    // loops: `0 P[3]` one vertex loop, or `1 n_loops` then per loop `n_pts pts[3 n_pts]` an edge
    // loop of SmLine edges p[i] -> p[i + 1]
    int64_t loops_kind = rd.idx();
    if (loops_kind == 0)
    {
        SmPoint3d at = rd.vec();
        SmLoop* loop = b.StartAndEndSingleVertexLoop(at);
        if (loop == NULL)
            return SC_ERR_ARG;
    }
    else
    {
        int64_t n_loops = rd.idx();
        for (int64_t li = 0; li < n_loops; li++)
        {
            int64_t n = rd.idx();
            std::vector<SmPoint3d> pts;
            for (int64_t i = 0; i < n; i++)
                pts.push_back(rd.vec());
            SmLoop* loop = b.StartLoop(SM_OT_SAME);
            if (loop == NULL)
                return SC_ERR_ARG;
            // The loopuse starts at the edge use added last and walks the others in the
            // order added: add L[1], .., L[n-1], L[0] to get L (as the query.loop shim).
            for (int64_t k = 0; k < n; k++)
            {
                int64_t i = (k + 1) % n;
                SmEdge* edge = b.StartEdge(SM_OT_SAME, NULL, NULL, NULL);
                if (edge == NULL)
                    return SC_ERR_ARG;
                SmCurve* c = new (ctx) SmLine(pts[(size_t)i], pts[(size_t)((i + 1) % n)], 3, &ctx);
                SmExtent1d ivl = c->GetNaturalInterval();
                if (b.SetEdgeCurve(c, FALSE) != SM_SUCCESS ||
                    b.SetLimits(SmVertexNode(ivl.GetMin()), SmVertexNode(ivl.GetMax())) != SM_SUCCESS ||
                    b.EndEdge() != SM_SUCCESS)
                    return SC_ERR_ARG;
            }
            if (b.EndLoop() != SM_SUCCESS)
                return SC_ERR_ARG;
            // the realised loop must be the requested one (the Rust B-rep is built that way)
            SmTArray<SmEdgeuse*> eus;
            loop->GetEdgeuses(eus);
            if ((int64_t)eus.GetSize() != n)
                return SC_ERR_ARG;
            for (int64_t i = 0; i < n; i++)
            {
                SmEdgeuse* eu = eus[(ULONG)i];
                SmPoint3d p;
                if (eu->GetOrientation() != SM_OT_SAME ||
                    eu->GetEdge()->GetCurve()->EvaluatePoint(eu->GetEdge()->GetInterval().GetMin(), p) != SM_SUCCESS ||
                    p.DistanceBetweenSquared(pts[(size_t)i]) > 1e-18)
                    return SC_ERR_ARG;
            }
        }
    }
    if (b.EndFace() != SM_SUCCESS || b.EndShell(FALSE) != SM_SUCCESS || b.EndBrep() != SM_SUCCESS)
        return SC_ERR_ARG;
    face->SetTolerance(face_tol, FALSE, FALSE);

    SmFaceProps props(&ctx);
    int64_t n_ops = rd.idx();
    for (int64_t s = 0; s < n_ops; s++)
    {
        int64_t op = rd.idx();
        double p0 = rd.num(), p1 = rd.num(), p2 = rd.num(), p3 = rd.num();
        double res = 0.0;
        std::vector<double> alt;
        bool use_alt = false;
        switch (op)
        {
        case 0: // SetProps(stage, face, index, healOp)
            res = (double)props.SetProps((SmPropStageType)(int)p0, face, (ULONG)p2, (SmHealerOpType)(int)p1);
            break;
        case 1: props.ReSet((SmPropStageType)(int)p0); break;
        case 2: res = (double)props.HasProps((SmPropStageType)(int)p0, p1 != 0.0 ? face : NULL); break;
        case 3: res = (double)props.HasProblems((ULONG)p0); break;
        case 4:
        {
            SmPoint3d pt(p0, p1, p2);
            res = (double)props.IsPoint3dOnPole(pt);
            break;
        }
        case 5: seed(props, face, (int64_t)p0, (int64_t)p1, p2); break;
        case 6: // SetProps_Gaps / Stage2 / Stage3 directly: p0 = which (1,2,3), p1 healOp, p2 index
            if ((int)p0 == 1)
                res = (double)props.SetProps_Gaps(face, (ULONG)p2, (SmHealerOpType)(int)p1);
            else if ((int)p0 == 2)
                res = (double)props.SetProps_Stage2(face, (ULONG)p2, (SmHealerOpType)(int)p1);
            else
                res = (double)props.SetProps_Stage3(face, (ULONG)p2, (SmHealerOpType)(int)p1);
            break;
        case 7: res = (double)props.IsPeriodicU() + 2.0 * (double)props.IsPeriodicV(); break;
        case 8: // SmLoopProps::SetProps of every loop of the face (SetProps_Stage3's loop)
        {
            SmTArray<SmLoop*> loops;
            face->GetLoops(loops);
            res = (double)loops.GetSize();
            use_alt = true;
            for (ULONG li = 0; li < loops.GetSize() && li < 4; li++)
            {
                SmLoopProps lp(&ctx);
                lp.SetProps(loops[li], props);
                alt.push_back((double)lp.m_tLoopuseType);
                alt.push_back((double)lp.m_bVertexLoopOnPole);
                alt.push_back((double)lp.m_lEdgeCnt);
                alt.push_back((double)lp.m_lEdgeuseCnt);
                alt.push_back((double)lp.m_bClosed3d);
                alt.push_back((double)lp.m_bClosedPtrs);
                alt.push_back((double)lp.m_bGoodClosed);
                alt.push_back((double)lp.m_lSeamCrossingCntU);
                alt.push_back((double)lp.m_lSeamWindingCntU);
                alt.push_back((double)lp.m_eWindingOrientU);
                alt.push_back((double)lp.m_lSeamCrossingCntV);
                alt.push_back((double)lp.m_lSeamWindingCntV);
                alt.push_back((double)lp.m_eWindingOrientV);
                alt.push_back((double)lp.m_bArea_Loop);
            }
            break;
        }
        default: break;
        }
        (void)p3;
        out.push_back(res);
        if (use_alt)
        {
            alt.resize((size_t)SNAP_LEN, 0.0);
            out.insert(out.end(), alt.begin(), alt.end());
        }
        else
            snapshot(out, props, face);
    }
    return SC_OK;
}

// ---- looptree ----------------------------------------------------------------------------
SmLoop* L(int64_t i) { return (SmLoop*)(intptr_t)(0x1000 + 16 * i); }

int32_t run_tree(Rd& rd, std::vector<double>& out)
{
    int64_t n = rd.idx();
    std::vector<SmLooptreeItem*> items;
    for (int64_t i = 0; i < n; i++)
    {
        int64_t loop = rd.idx();     // -1 = pole vertex
        int64_t partner = rd.idx();  // -2 = none, -1 = partner pole vertex
        SmLooptreeItem* it = new SmLooptreeItem(loop < 0 ? NULL : L(loop), NULL, NULL);
        if (partner != -2)
            it->AddPartner(partner < 0 ? NULL : L(partner), NULL, NULL);
        items.push_back(it);
    }
    int64_t n_links = rd.idx();
    for (int64_t i = 0; i < n_links; i++)
    {
        int64_t kind = rd.idx(), a = rd.idx(), c = rd.idx();
        if (kind == 0)
            items[(size_t)a]->AddChild(items[(size_t)c]);
        else
            items[(size_t)a]->AddSibling(items[(size_t)c]);
    }
    int64_t n_find = rd.idx();
    for (int64_t i = 0; i < n_find; i++)
    {
        int64_t start = rd.idx(), loop = rd.idx();
        SmLooptreeItem* parent = NULL;
        SmLooptreeItem* f = items[(size_t)start]->FindItem(loop < 0 ? NULL : L(loop), parent);
        int64_t fi = -1, pi = -1;
        for (size_t j = 0; j < items.size(); j++)
        {
            if (items[j] == f)
                fi = (int64_t)j;
            if (items[j] == parent)
                pi = (int64_t)j;
        }
        out.push_back((double)fi);
        out.push_back((double)pi);
    }
    for (int64_t i = 0; i < n; i++)
        out.push_back(items[(size_t)i]->IsNoArea_Pair() ? 1.0 : 0.0);
    // the items link each other: free them without the recursive destructor
    for (size_t i = 0; i < items.size(); i++)
    {
        items[i]->m_pChild = NULL;
        items[i]->m_pSibling = NULL;
        delete items[i];
    }
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_heal_face_props_run(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> res;
    int32_t st = run(rd, res);
    if (st != SC_OK)
        return st;
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_heal_face_props_tree(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> res;
    int32_t st = run_tree(rd, res);
    if (st != SC_OK)
        return st;
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}
