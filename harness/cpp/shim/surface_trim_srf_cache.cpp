// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.trim_srf_cache`: SmTrimSrfCache (source/SMLib/src/SmTrimSrfCache.cpp,
// source/SMLib/inc/SmTrimSrfCache.h) over an SmBSplineSurface.
//
// One entry point, sc_surface_trim_srf_cache_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_trim_srf_cache.rs, which validates every
// input. Every op starts with a B-spline surface block `oob form n m p q r s cpts(x y z w) u v`.
// Protected members are reached through a derived "peek" class; decomposition trees are built by
// hand in the tree's own node manager (as in the surface.surface_cache_p1 shim), edge uses are bare
// SmEdgeuse objects carrying only their UV trim curve, and faces come from
// SmBrep::CreateFaceFromSurface. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus
// value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdgeuse.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmTrimSrfCache.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_trim_srf_cache.rs).
enum : int32_t
{
    OP_NEW = 0,
    OP_POINT_TEST = 1,
    OP_IMPLANT_EU = 2,
    OP_FACE = 3,
};

struct TreePeek : SmTree
{
    explicit TreePeek(SmObject* owner) : SmTree(owner) {}
    SmMemBlockMgr& node_mgr() { return m_sNodeMgr; }
    SmTreeNode*& top() { return m_pTopNode; }
};

struct TrimPeek : SmTrimSrfCache
{
    TrimPeek(const SmSurface& s, double ch, double ang, double ar, double mx, double mn, double ratio)
        : SmTrimSrfCache(s, ch, ang, ar, mx, mn, ratio)
    {
    }
    SmTree*& tree() { return m_pTree; }
    SmMemBlockMgr& bez() { return m_sBezMgr; }
    SmMemBlockMgr& aux() { return m_sAuxMgr; }
    SmMemBlockMgr& eu_mgr() { return m_sEdgeuseListMgr; }
    SmExtent2d& domain() { return m_sUVDomain; }
    double& factor() { return m_dTrimSubdivisionFactor; }
    double chord() const { return m_dChordHeightTolerance; }
    double angle() const { return m_dAngTolRad; }
    double aspect() const { return m_dAspectRatio3D; }
    double max_side() const { return m_dMaximumSideLength3D; }
    double min_side() const { return m_dMinimumSideLength3D; }
    double min_ratio() const { return m_dMinimumSideLengthRatioUV; }
};

struct EdgeusePeek : SmEdgeuse
{
    EdgeusePeek() : SmEdgeuse() {}
    void set_curve(SmBSplineCurve* c) { m_pUVTrimCurve = c; }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

gw_SURFACE* read_nurb(Rd& rd)
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
    return sur;
}

SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

// Reads `dim n p m cpts knots` into SmBSplineCurve(gw_CURVE*, dim, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
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

// Subdivision tree nodes `n` x `parent c1 c2 aux split marked class D`, allocated in order from
// the tree's node manager; patches (aux 3) from m_sBezMgr, the others from m_sAuxMgr; every node's
// mBA_pSurface is the cache surface.
std::vector<SmTreeNode*> read_tree(Rd& rd, TrimPeek* cache, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx();
    TreePeek* tree = new (ctx) TreePeek(cache);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
    cache->aux().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    cache->tree() = tree;
    std::vector<SmTreeNode*> nodes;
    for (int64_t k = 0; k < n; k++)
        nodes.push_back((SmTreeNode*)tree->node_mgr().GetNewElement());
    auto ptr = [&](int64_t k) -> SmTreeNode* { return k < 0 ? nullptr : nodes[(size_t)k]; };
    for (int64_t k = 0; k < n; k++)
    {
        SmTreeNode* nd = nodes[(size_t)k];
        nd->m_pTree = tree;
        nd->m_pParent = ptr(rd.idx());
        nd->m_pChild1 = ptr(rd.idx());
        nd->m_pChild2 = ptr(rd.idx());
        nd->m_eAuxDataType = (SmAuxDataType)rd.idx();
        nd->m_eGeomType = SM_NG_DEFAULT;
        SmBezierAux2d* a = nd->m_eAuxDataType == SM_AD_BEZIER_SURFACE
                               ? (SmBezierAux2d*)cache->bez().GetNewElement()
                               : (SmBezierAux2d*)cache->aux().GetNewElement();
        a->m_eSplitDir = (SmSurfParamType)rd.idx();
        a->m_bIsMarked = rd.flag() ? TRUE : FALSE;
        a->m_eNodeClass = (SmNodeClassType)rd.idx();
        a->m_pFace = NULL;
        a->m_sUVDomain = rd.domain();
        a->mBA_pSurface = s;
        a->m_pOwningTreeNode = nd;
        nd->m_pData = a;
    }
    tree->top() = n > 0 ? nodes[0] : nullptr;
    return nodes;
}

double obj_kind(const SmObject* p)
{
    return p == NULL ? -1.0 : (double)p->GetType();
}

void op_new(Rd& rd, SmBSplineSurface* s, const SmContext& ctx, std::vector<double>& o)
{
    double a[6];
    for (double& v : a)
        v = rd.num();
    TrimPeek* c = new (ctx) TrimPeek(*s, a[0], a[1], a[2], a[3], a[4], a[5]);
    o.push_back(0.0);
    o.push_back((double)c->GetType());
    int64_t k = rd.idx();
    for (int64_t i = 0; i < k; i++)
        o.push_back(flg(c->IsKindOf((SM_TYPE)rd.idx()) == TRUE));
    o.push_back(flg(c->m_bFaceWasModified == TRUE));
    o.push_back(flg(c->m_bProcessBoundaryCurves == TRUE));
    o.push_back(flg(c->m_bPointTestEnabled == TRUE));
    o.push_back(flg(c->m_bHaveTSurfaceCache == TRUE));
    o.push_back((double)c->m_bFaceContainmentDone);
    put_dom(o, c->domain());
    o.push_back(c->factor());
    o.push_back(c->chord());
    o.push_back(c->angle());
    o.push_back(c->aspect());
    o.push_back(c->max_side());
    o.push_back(c->min_side());
    o.push_back(c->min_ratio());
    o.push_back(flg(c->tree() == NULL));
    delete c;
}

void op_point_test(Rd& rd, SmBSplineSurface* s, const SmContext& ctx, std::vector<double>& o)
{
    bool enabled = rd.flag();
    bool modified = rd.flag();
    double u = rd.num();
    double v = rd.num();
    bool has_tol = rd.flag();
    double tol = rd.num();
    TrimPeek* c = new (ctx) TrimPeek(*s, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001);
    c->m_bPointTestEnabled = enabled ? TRUE : FALSE;
    c->m_bFaceWasModified = modified ? TRUE : FALSE;
    SmBoolean ok = FALSE;
    SmZoneTol3d zt(tol);
    SmObject* obj = (SmObject*)&ok; // must be overwritten
    SmStatus r = c->PointTest(SmPoint2d(u, v), ok, has_tol ? &zt : NULL, &obj);
    o.push_back(st(r));
    o.push_back(flg(ok == TRUE));
    o.push_back(obj_kind(obj));
    delete c;
}

void op_implant_eu(Rd& rd, SmBSplineSurface* s, const SmContext& ctx, std::vector<double>& o)
{
    TrimPeek* c = new (ctx) TrimPeek(*s, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001);
    c->domain() = rd.domain();
    c->factor() = rd.num();
    c->m_bPointTestEnabled = FALSE; // ImplantEdgeuse asserts point testing is off (BuildTree)
    c->eu_mgr().Initialize(ALIGN_SIZE(sizeof(SmEdgeuseList)), 30);
    std::vector<SmTreeNode*> nodes = read_tree(rd, c, s, ctx);
    SmExtent2d fe;
    bool init = rd.flag();
    SmExtent2d given = rd.domain();
    if (!init)
        fe = given;
    int64_t k = rd.idx();
    std::vector<EdgeusePeek*> eus;
    o.push_back(0.0);
    bool failed = false;
    for (int64_t i = 0; i < k; i++)
    {
        SmBSplineCurve* crv = read_curve(rd, ctx);
        bool subdiv = rd.flag();
        int64_t node = rd.idx();
        EdgeusePeek* eu = new (static_cast<SmObject*>(c)) EdgeusePeek();
        eu->set_curve(crv);
        eus.push_back(eu);
        if (failed)
            continue;
        SmStatus r = c->ImplantEdgeuse(eu, subdiv ? TRUE : FALSE, fe, node < 0 ? NULL : nodes[(size_t)node]);
        o.push_back(st(r));
        if (r != SM_SUCCESS)
            failed = true;
    }
    put_dom(o, fe);
    // every node of the tree (the input nodes, then the children subdivisions made)
    SmTArray<void*> act;
    ((TreePeek*)c->tree())->node_mgr().GetActiveElements(act);
    o.push_back((double)act.GetSize());
    for (ULONG ni = 0; ni < act.GetSize(); ni++)
    {
        const SmTreeNode* nd = (const SmTreeNode*)act[ni];
        const SmBezierAux2d* a = (const SmBezierAux2d*)nd->m_pData;
        o.push_back(flg(a->m_bIsMarked == TRUE));
        o.push_back((double)a->m_eNodeClass);
        SmTArray<SmEdgeuseList*> l;
        a->m_sEdgeuseList.GetAllNodes(l);
        o.push_back((double)l.GetSize());
        for (ULONG j = 0; j < l.GetSize(); j++)
        {
            double id = -2.0;
            for (size_t e = 0; e < eus.size(); e++)
                if (eus[e] == l[j]->m_pEU)
                    id = (double)e;
            o.push_back(id);
        }
    }
    delete c;
    for (EdgeusePeek* eu : eus)
        delete eu;
}

// The cache of a face made from the surface (consumed by the brep).
void op_face(Rd& rd, SmBSplineSurface* s, const SmContext& ctx, std::vector<double>& o)
{
    bool build = rd.flag();
    SmBrep* brep = new (ctx) SmBrep();
    SmFace* face = NULL;
    SmStatus r = brep->CreateFaceFromSurface(s, s->GetNaturalUVDomain(), face);
    o.push_back(st(r));
    if (r != SM_SUCCESS || face == NULL)
    {
        delete brep;
        return;
    }
    TrimPeek* c = new (ctx) TrimPeek(*face->GetSurface(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.001);
    if (build)
    {
        SmStatus b = c->BuildTree(NULL);
        o.push_back(st(b));
        o.push_back(flg(c->m_bProcessBoundaryCurves == TRUE));
        o.push_back(flg(c->m_bPointTestEnabled == TRUE));
        o.push_back(flg(c->m_bHaveTSurfaceCache == TRUE));
        o.push_back((double)c->m_bFaceContainmentDone);
        put_dom(o, c->domain());
        SmTArray<void*> act;
        if (c->tree())
            ((TreePeek*)c->tree())->node_mgr().GetActiveElements(act);
        o.push_back((double)act.GetSize());
    }
    int64_t k = rd.idx();
    for (int64_t i = 0; i < k; i++)
    {
        double u = rd.num();
        double v = rd.num();
        bool has_tol = rd.flag();
        double tol = rd.num();
        SmZoneTol3d zt(tol);
        SmBoolean ok = FALSE;
        SmObject* obj = NULL;
        SmStatus a = c->PointTest(SmPoint2d(u, v), ok, has_tol ? &zt : NULL, &obj);
        o.push_back(st(a));
        o.push_back(flg(ok == TRUE));
        o.push_back(obj_kind(obj));
        SmPointClassificationType cls = SM_PC_UNKNOWN;
        SmObject* obj2 = NULL;
        SmStatus b = c->PointClassify(SmPoint2d(u, v), SmZoneTol3d(tol), cls, obj2);
        o.push_back(st(b));
        o.push_back((double)cls);
        o.push_back(obj_kind(obj2));
    }
    delete c;
    delete brep;
}
} // namespace

SC_EXPORT int32_t sc_surface_trim_srf_cache_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                 int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    SmBSplineSurface* s = read_bspline(rd, ctx);
    switch (op)
    {
    case OP_NEW: op_new(rd, s, ctx, o); break;
    case OP_POINT_TEST: op_point_test(rd, s, ctx, o); break;
    case OP_IMPLANT_EU: op_implant_eu(rd, s, ctx, o); break;
    case OP_FACE:
        op_face(rd, s, ctx, o);
        s = NULL; // consumed by the brep
        break;
    default: break;
    }
    delete s;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
