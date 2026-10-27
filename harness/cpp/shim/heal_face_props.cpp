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
#include "sc_one_face_brep.h"

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
using sc_one_face::SNAP_LEN;
using sc_one_face::seed;
using sc_one_face::snapshot;

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

int32_t run(Rd& rd, std::vector<double>& out)
{
    SmContext ctx;
    SmBrep* brep = NULL;
    sc_one_face::OneFace of;
    bool built = sc_one_face::build(rd, ctx, brep, of);
    SmObjDelete clean(brep);
    if (!built)
        return SC_ERR_ARG;
    SmFace* face = of.face;

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
        case 9: // the loop props SetProps_Stage3 left in m_sLoopProps (first 4)
        {
            SmTArray<SmLoop*> loops;
            face->GetLoops(loops);
            res = (double)props.m_sLoopProps.GetSize();
            use_alt = true;
            for (ULONG li = 0; li < props.m_sLoopProps.GetSize() && li < 4; li++)
            {
                const SmLoopProps& lp = props.m_sLoopProps[li];
                double at = -1.0;
                for (ULONG lj = 0; lj < loops.GetSize(); lj++)
                    if (loops[lj] == lp.m_cpLoop)
                        at = (double)lj;
                alt.push_back(at);
                alt.push_back((double)lp.m_tLoopuseType);
                alt.push_back((double)lp.m_lEdgeCnt);
                alt.push_back((double)lp.m_bClosed3d);
                alt.push_back((double)lp.m_bClosedPtrs);
                alt.push_back((double)lp.m_bGoodClosed);
                alt.push_back((double)lp.m_bArea_Loop);
                alt.push_back((double)lp.m_eContainmentType);
                alt.push_back((double)lp.m_eLoopOrient);
                alt.push_back((double)lp.m_eDesiredLoopOrient);
                alt.push_back((double)lp.m_bGoodOrient);
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
