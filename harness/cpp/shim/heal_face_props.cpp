// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.face_props`: SmFaceProps / SmLoopProps / SmLooptreeItem
// (source/SMLib/src/SmFaceProps.cpp, SmLoopProps.cpp, inc/SmLoopProps.h).
//
// sc_heal_face_props_run(in, n_in, out, cap, out_len): a script on one SmFaceProps of a one-face
// B-rep (a plane with one vertex loop, built with SmBrepConstructor like the query.loop shim).
// Layout and ops: crates/solidcore-ffi/src/units/heal_face_props.rs (which validates the script).
// After every op the shim appends a snapshot of the face props (SNAP_LEN values).
//
// sc_heal_face_props_tree(in, n_in, out, cap, out_len): SmLooptreeItem building and FindItem on
// fake loop pointers (small integers that are only ever compared, never dereferenced).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmContext.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceProps.h>
#include <SmLoop.h>
#include <SmLoopProps.h>
#include <SmPlane.h>
#include <SmVector3d.h>

#include <vector>

namespace
{
const int SNAP_LEN = 70;

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
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
    SmPoint3d o(rd.num(), rd.num(), rd.num());
    SmVector3d x(rd.num(), rd.num(), rd.num());
    SmVector3d y(rd.num(), rd.num(), rd.num());
    double a = rd.num(), bb = rd.num(), c = rd.num(), d = rd.num();
    SmSurface* srf = new (ctx) SmPlane(o, x, y, SmVector2d(1.0, 1.0), SmExtent2d(a, bb, c, d), &ctx);
    if (srf == NULL || b.SetFaceSurface(srf, srf->GetNaturalUVDomain()) != SM_SUCCESS)
        return SC_ERR_ARG;
    SmLoop* loop = b.StartAndEndSingleVertexLoop(SmPoint3d(0, 0, 0));
    if (loop == NULL || b.EndFace() != SM_SUCCESS || b.EndShell(FALSE) != SM_SUCCESS || b.EndBrep() != SM_SUCCESS)
        return SC_ERR_ARG;
    face->SetTolerance(face_tol, FALSE, FALSE);

    SmFaceProps props(&ctx);
    int64_t n_ops = rd.idx();
    for (int64_t s = 0; s < n_ops; s++)
    {
        int64_t op = rd.idx();
        double p0 = rd.num(), p1 = rd.num(), p2 = rd.num(), p3 = rd.num();
        double res = 0.0;
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
        default: break;
        }
        (void)p3;
        out.push_back(res);
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
