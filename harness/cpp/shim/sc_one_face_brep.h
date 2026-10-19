// SPDX-License-Identifier: Apache-2.0
// Shared one-face B-rep fixture of the heal.face_props and intersect.curve_class_p4 shims: the
// face block reader (SmBrepConstructor: one face on a real SmPlane or SmSurfOfRevolution, with one
// vertex loop or edge loops of SmLine edges) and the SmFaceProps snapshot. The Rust side rebuilds
// the same B-rep record by record (crates/difftest/src/units/heal_face_props.rs, rust_brep); the
// layouts are documented in crates/solidcore-ffi/src/units/heal_face_props.rs.
//
// R is the shim's reader: num() (next double), idx() (next integer), vec() (next three doubles).
#pragma once

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
#include <SmLoopuse.h>
#include <SmPlane.h>
#include <SmSurfOfRevolution.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <vector>

namespace sc_one_face
{
// values per SmFaceProps snapshot
const int SNAP_LEN = 82;

inline void put(std::vector<double>& o, double v) { o.push_back(v); }

// The SmFaceProps members, SNAP_LEN values (`face` is the B-rep's face: m_pFace reads 1 for it).
inline void snapshot(std::vector<double>& o, SmFaceProps& p, SmFace* face)
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

// Seeds one field of the props (field ids: crates/difftest/src/units/heal_face_props.rs, seed).
inline void seed(SmFaceProps& p, SmFace* face, int64_t field, int64_t idx, double v)
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
template <class R>
SmSurface* read_surface(R& rd, const SmContext& ctx)
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

// The topology of the built face, in the order the Rust side numbers it.
struct OneFace
{
    SmFace* face = NULL;
    // per loop: the loop, its vertex (a vertex loop) or its edge uses in order (an edge loop)
    std::vector<SmLoop*> loops;
    std::vector<SmVertex*> loop_vertex;
    std::vector<std::vector<SmEdgeuse*>> loop_edgeuses;
};

// Reads `face_tol surface loops` and builds the one-face B-rep: `brep` is set as soon as it
// exists (the caller deletes it, also on failure). Loops: `0 P[3]` one vertex loop, or `1
// n_loops` then per loop `n_pts pts[3 n_pts]` an edge loop of SmLine edges p[i] -> p[i + 1]
// (the first loop outer). False when a constructor call fails or the realised loop is not the
// requested one.
template <class R>
bool build(R& rd, const SmContext& ctx, SmBrep*& brep, OneFace& of)
{
    double face_tol = rd.num();
    SmBrepConstructor b;
    brep = b.StartBrep(ctx);
    if (brep == NULL)
        return false;
    SmShell *outer = NULL, *inner = NULL;
    if (b.StartShell(outer, inner) != SM_SUCCESS)
        return false;
    SmFace* face = b.StartFace(SM_OT_SAME);
    if (face == NULL)
        return false;
    SmSurface* srf = read_surface(rd, ctx);
    if (srf == NULL || b.SetFaceSurface(srf, srf->GetNaturalUVDomain()) != SM_SUCCESS)
        return false;
    of.face = face;

    int64_t loops_kind = rd.idx();
    if (loops_kind == 0)
    {
        SmPoint3d at = rd.vec();
        SmLoop* loop = b.StartAndEndSingleVertexLoop(at);
        if (loop == NULL)
            return false;
        SmTArray<SmVertexuse*> vus;
        loop->GetVertexuses(vus);
        if (vus.GetSize() != 1 || vus[0] == NULL || vus[0]->GetVertex() == NULL)
            return false;
        of.loops.push_back(loop);
        of.loop_vertex.push_back(vus[0]->GetVertex());
        of.loop_edgeuses.push_back(std::vector<SmEdgeuse*>());
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
                return false;
            // The loopuse starts at the edge use added last and walks the others in the
            // order added: add L[1], .., L[n-1], L[0] to get L (as the query.loop shim).
            for (int64_t k = 0; k < n; k++)
            {
                int64_t i = (k + 1) % n;
                SmEdge* edge = b.StartEdge(SM_OT_SAME, NULL, NULL, NULL);
                if (edge == NULL)
                    return false;
                SmCurve* c = new (ctx) SmLine(pts[(size_t)i], pts[(size_t)((i + 1) % n)], 3, &ctx);
                SmExtent1d ivl = c->GetNaturalInterval();
                if (b.SetEdgeCurve(c, FALSE) != SM_SUCCESS ||
                    b.SetLimits(SmVertexNode(ivl.GetMin()), SmVertexNode(ivl.GetMax())) != SM_SUCCESS ||
                    b.EndEdge() != SM_SUCCESS)
                    return false;
            }
            if (b.EndLoop() != SM_SUCCESS)
                return false;
            // the realised loop must be the requested one (the Rust B-rep is built that way)
            SmTArray<SmEdgeuse*> eus;
            loop->GetEdgeuses(eus);
            if ((int64_t)eus.GetSize() != n)
                return false;
            std::vector<SmEdgeuse*> list;
            for (int64_t i = 0; i < n; i++)
            {
                SmEdgeuse* eu = eus[(ULONG)i];
                SmPoint3d p;
                if (eu->GetOrientation() != SM_OT_SAME ||
                    eu->GetEdge()->GetCurve()->EvaluatePoint(eu->GetEdge()->GetInterval().GetMin(), p) != SM_SUCCESS ||
                    p.DistanceBetweenSquared(pts[(size_t)i]) > 1e-18)
                    return false;
                list.push_back(eu);
            }
            of.loops.push_back(loop);
            of.loop_vertex.push_back(NULL);
            of.loop_edgeuses.push_back(list);
        }
    }
    if (b.EndFace() != SM_SUCCESS || b.EndShell(FALSE) != SM_SUCCESS || b.EndBrep() != SM_SUCCESS)
        return false;
    face->SetTolerance(face_tol, FALSE, FALSE);
    return true;
}
} // namespace sc_one_face
