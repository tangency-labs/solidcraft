// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_cache_p3`: the SmSurfaceCache members owned by the unit
// (source/SMLib/src/SmSurfaceCache.cpp): FindUVNodes, SubdivideNode, SubdivideToTolerances,
// BuildTreeBase (through BuildTree), BuildTree and GetTree.
//
// Protected members are reached through derived "peek" classes; the start nodes of SubdivideNode,
// SubdivideToTolerances and FindUVNodes are built by hand in the tree's own node manager and the
// cache's patch / aux managers, as BuildTreeBase builds its first generation. The input readers
// are copies of the surface.surface_cache_p2 shim's (another translation unit).
//
// One entry point, sc_surface_surface_cache_p3_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_cache_p3.rs, which validates every
// input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's
// outputs. Every cache built here is deleted (~SmSurfaceCache) before the op returns.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmMemBlockMgr.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPseudoBox.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

namespace sc_ssc_p3
{
enum
{
    OP_SUBDIV_NODE = 0,
    OP_SUBDIV_TOL = 1,
    OP_FIND = 2,
    OP_BUILD = 3,
    OP_GET = 4,
};

struct TreePeek : SmTree
{
    explicit TreePeek(SmObject* owner) : SmTree(owner) {}
    SmMemBlockMgr& node_mgr() { return m_sNodeMgr; }
    SmTreeNode*& top() { return m_pTopNode; }
};

struct CachePeek : SmSurfaceCache
{
    CachePeek(const SmSurface& s, const SmExtent2d& d, double ch, double ang, double asp, double mx, double mn,
              double ratio)
        : SmSurfaceCache(s, d, ch, ang, asp, mx, mn, ratio)
    {
    }
    SmTree*& tree() { return m_pTree; }
    SmMemBlockMgr& bez() { return m_sBezMgr; }
    SmMemBlockMgr& aux() { return m_sAuxMgr; }
    double& start_fast() { return m_dStartFastSubdivisionAngleDeg; }
    SmBoolean& switch_fast() { return m_bSwitchToFastSubdivision; }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
    SmExtent3d box()
    {
        SmVector3d lo = vec();
        SmVector3d hi = vec();
        SmExtent3d e;
        e.SetMinMax(lo, hi);
        return e;
    }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void put_dom(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

void put_box(std::vector<double>& o, const SmExtent3d& e)
{
    put_vec(o, e.GetMin());
    put_vec(o, e.GetMax());
}

// `n m p q r s cpts(x y z w, v index fastest) u v`.
void put_nurb(std::vector<double>& o, const gw_SURFACE* s)
{
    gw_INDEX n = s->net->n;
    gw_INDEX m = s->net->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)s->p);
    o.push_back((double)s->q);
    o.push_back((double)s->knu->m);
    o.push_back((double)s->knv->m);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(s->net->Pw[i][j].x);
            o.push_back(s->net->Pw[i][j].y);
            o.push_back(s->net->Pw[i][j].z);
            o.push_back(s->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= s->knu->m; i++)
        o.push_back(s->knu->U[i]);
    for (gw_INDEX i = 0; i <= s->knv->m; i++)
        o.push_back(s->knv->U[i]);
}

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

// `oob form nurb`.
SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

// A surface `kind ...`: 0 B-spline `oob form nurb`; 1 bounded plane `o x y sx sy D`;
// 2 curve-bounded surface (implicit outer, no boundary curves) over a B-spline block.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double sx = rd.num();
        double sy = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
    }
    SmBSplineSurface* b = read_bspline(rd, ctx);
    if (kind == 0)
        return b;
    // the curve-bounded surface owns (deletes) its basis
    SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
    return new (ctx) SmCurveBoundedSurface(*b, &c3d, &cuv, TRUE);
}

// Cache options `ch ang asp max min ratio start`.
CachePeek* read_cache(Rd& rd, const SmSurface* s, const SmExtent2d& d)
{
    double ch = rd.num();
    double ang = rd.num();
    double asp = rd.num();
    double mx = rd.num();
    double mn = rd.num();
    double ratio = rd.num();
    double start = rd.num();
    CachePeek* c = new (*s->GetContext()) CachePeek(*s, d, ch, ang, asp, mx, mn, ratio);
    c->start_fast() = start;
    return c;
}

TreePeek* new_tree(CachePeek* cache, const SmContext& ctx)
{
    TreePeek* tree = new (ctx) TreePeek(cache);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
    cache->aux().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    cache->tree() = tree;
    return tree;
}

// A start node, set as BuildTreeBase sets a first generation node:
// `aux D box class chsat angsat surf [aux 3: nurb pseudo(basis 9, ivl 6) uconts(2) vconts(2)]`.
// `surf` 1 puts the cache's surface on the node (mBA_pSurface), 0 none.
SmTreeNode* read_start_node(Rd& rd, TreePeek* tree, CachePeek* cache, const SmSurface* s, gw_SURFACE** bez)
{
    *bez = NULL;
    SmAuxDataType aux = (SmAuxDataType)rd.idx();
    SmExtent2d d = rd.domain();
    SmExtent3d box = rd.box();
    SmNodeClassType cls = (SmNodeClassType)rd.idx();
    bool chsat = rd.flag();
    bool angsat = rd.flag();
    bool surf = rd.flag();
    SmTreeNode* nd = (SmTreeNode*)tree->node_mgr().GetNewElement();
    nd->m_pTree = tree;
    nd->m_pParent = NULL;
    nd->m_pChild1 = NULL;
    nd->m_pChild2 = NULL;
    nd->m_eAuxDataType = aux;
    nd->m_eGeomType = SM_NG_DEFAULT;
    nd->m_sBBox = box;
    SmBezierAux2d* a = aux == SM_AD_BEZIER_SURFACE ? (SmBezierAux2d*)cache->bez().GetNewElement()
                                                   : (SmBezierAux2d*)cache->aux().GetNewElement();
    a->m_eSplitDir = SM_SP_U;
    a->m_bIsMarked = FALSE;
    a->m_eNodeClass = cls;
    a->m_pFace = NULL;
    a->mBA_pSurface = surf ? s : NULL;
    a->m_bChordHeightSatisfied = chsat ? TRUE : FALSE;
    a->m_bAngleTolSatisfied = angsat ? TRUE : FALSE;
    a->m_sUVDomain = d;
    a->m_vVertexList.Init();
    a->m_sEdgeuseList.Init();
    a->m_sPolyEdgeList.Init();
    a->m_pOwningTreeNode = nd;
    nd->m_pData = a;
    if (aux == SM_AD_BEZIER_SURFACE)
    {
        SmBezierPatch* bp = (SmBezierPatch*)a;
        *bez = read_nurb(rd);
        bp->m_pBezier = *bez;
        SmVector3d b1 = rd.vec(), b2 = rd.vec(), b3 = rd.vec();
        bp->GetPseudoBox().SetBasis(b1, b2, b3);
        double l0 = rd.num(), h0 = rd.num(), l1 = rd.num(), h1 = rd.num(), l2 = rd.num(), h2 = rd.num();
        bp->GetPseudoBox().SetIntervals(SmExtent1d(l0, h0), SmExtent1d(l1, h1), SmExtent1d(l2, h2));
        bp->m_eUCurveConts[0] = (SmContinuityType)rd.idx();
        bp->m_eUCurveConts[1] = (SmContinuityType)rd.idx();
        bp->m_eVCurveConts[0] = (SmContinuityType)rd.idx();
        bp->m_eVCurveConts[1] = (SmContinuityType)rd.idx();
    }
    return nd;
}

// Every node of the tree's node manager: `count`, then per node
// `parent c1 c2 aux split D box class chsat angsat marked tess hassurf`
// and for aux 3 `uconts(2) vconts(2) pseudo(basis 9, ivl 6) hasbez [nurb]`.
// `hassurf`: 0 no mBA_pSurface, 1 the cache's surface, 2 another surface.
void put_tree(std::vector<double>& o, SmTree* tree, CachePeek* cache)
{
    TreePeek* tp = (TreePeek*)tree;
    SmTArray<void*> all;
    tp->node_mgr().GetActiveElements(all);
    o.push_back((double)all.GetSize());
    auto index_of = [&](const SmTreeNode* p) -> double {
        if (p == NULL)
            return -1.0;
        for (ULONG i = 0; i < all.GetSize(); i++)
            if (all[i] == (const void*)p)
                return (double)i;
        return -2.0;
    };
    for (ULONG i = 0; i < all.GetSize(); i++)
    {
        const SmTreeNode* nd = (const SmTreeNode*)all[i];
        o.push_back(index_of(nd->m_pParent));
        o.push_back(index_of(nd->m_pChild1));
        o.push_back(index_of(nd->m_pChild2));
        o.push_back((double)nd->m_eAuxDataType);
        const SmBezierAux2d* a = (const SmBezierAux2d*)nd->m_pData;
        o.push_back((double)a->m_eSplitDir);
        put_dom(o, a->m_sUVDomain);
        put_box(o, nd->m_sBBox);
        o.push_back((double)a->m_eNodeClass);
        o.push_back(flg(a->m_bChordHeightSatisfied == TRUE));
        o.push_back(flg(a->m_bAngleTolSatisfied == TRUE));
        o.push_back(flg(a->m_bIsMarked == TRUE));
        o.push_back(flg(a->m_bIsTessOnly == TRUE));
        o.push_back(a->mBA_pSurface == NULL ? 0.0 : (a->mBA_pSurface == cache->GetSurface() ? 1.0 : 2.0));
        if (nd->m_eAuxDataType == SM_AD_BEZIER_SURFACE)
        {
            const SmBezierPatch* bp = (const SmBezierPatch*)a;
            o.push_back((double)bp->m_eUCurveConts[0]);
            o.push_back((double)bp->m_eUCurveConts[1]);
            o.push_back((double)bp->m_eVCurveConts[0]);
            o.push_back((double)bp->m_eVCurveConts[1]);
            SmPseudoBox pb = ((SmBezierPatch*)bp)->GetPseudoBox();
            for (ULONG k = 0; k < 3; k++)
                put_vec(o, pb.GetBasis(k));
            for (ULONG k = 0; k < 3; k++)
            {
                o.push_back(pb.GetInterval(k).GetMin());
                o.push_back(pb.GetInterval(k).GetMax());
            }
            o.push_back(flg(bp->m_pBezier != NULL));
            if (bp->m_pBezier != NULL)
                put_nurb(o, bp->m_pBezier);
        }
    }
    o.push_back(tree->GetTopNode() == NULL ? -1.0 : index_of(tree->GetTopNode()));
}

// Element size of the construction Bezier block: the start patch's (SubdivideNode allocates its
// halves with the parent's sizes), else a bicubic patch's (approximations).
ULONG block_size(const gw_SURFACE* bez)
{
    ULONG cubic = sm_ComputeNurbSurfaceSize(3, 3, 7, 7);
    if (bez == NULL)
        return cubic;
    ULONG own = sm_ComputeNurbSurfaceSize(bez);
    return own > cubic ? own : cubic;
}

// `S opts start dir only`.
void op_subdiv_node(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_surface(rd, ctx);
    CachePeek* cache = read_cache(rd, s, s->GetNaturalUVDomain());
    TreePeek* tree = new_tree(cache, ctx);
    gw_SURFACE* bez = NULL;
    SmTreeNode* nd = read_start_node(rd, tree, cache, s, &bez);
    tree->top() = nd;
    SmSurfParamType dir = rd.flag() ? SM_SP_V : SM_SP_U;
    bool only = rd.flag();
    SmMemBlockMgr block;
    block.Initialize(block_size(bez), 25);
    SmStatus r = cache->SubdivideNode(nd, dir, only ? TRUE : FALSE, &block);
    o.push_back(st(r));
    put_tree(o, tree, cache);
    if (bez)
        smos_Free(bez);
    delete cache;
    delete s;
}

// `S opts start`.
void op_subdiv_tol(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_surface(rd, ctx);
    CachePeek* cache = read_cache(rd, s, s->GetNaturalUVDomain());
    TreePeek* tree = new_tree(cache, ctx);
    gw_SURFACE* bez = NULL;
    SmTreeNode* nd = read_start_node(rd, tree, cache, s, &bez);
    tree->top() = nd;
    SmMemBlockMgr block;
    block.Initialize(block_size(bez), 25);
    SmStatus r = cache->SubdivideToTolerances(tree, nd, &block);
    o.push_back(st(r));
    o.push_back(flg(cache->switch_fast() == TRUE));
    put_tree(o, tree, cache);
    if (bez)
        smos_Free(bez);
    delete cache;
    delete s;
}

// `S opts start nsplits (node dir only)* qD poles`: the start node split by SubdivideNode as
// listed (node = index in the node manager), then FindUVNodes(qD, poles).
void op_find(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_surface(rd, ctx);
    CachePeek* cache = read_cache(rd, s, s->GetNaturalUVDomain());
    TreePeek* tree = new_tree(cache, ctx);
    gw_SURFACE* bez = NULL;
    SmTreeNode* nd = read_start_node(rd, tree, cache, s, &bez);
    tree->top() = nd;
    SmMemBlockMgr block;
    block.Initialize(block_size(bez), 25);
    int64_t ns = rd.idx();
    std::vector<double> stats;
    for (int64_t i = 0; i < ns; i++)
    {
        int64_t which = rd.idx();
        SmSurfParamType dir = rd.flag() ? SM_SP_V : SM_SP_U;
        bool only = rd.flag();
        SmTArray<void*> all;
        tree->node_mgr().GetActiveElements(all);
        SmTreeNode* target = (SmTreeNode*)all[(ULONG)which];
        stats.push_back(st(cache->SubdivideNode(target, dir, only ? TRUE : FALSE, &block)));
    }
    SmExtent2d qd = rd.domain();
    bool poles = rd.flag();
    SmTArray<SmTreeNode*> found;
    SmStatus r = cache->FindUVNodes(qd, found, poles ? TRUE : FALSE);
    o.push_back(st(r));
    o.push_back((double)stats.size());
    for (double v : stats)
        o.push_back(v);
    SmTArray<void*> all;
    tree->node_mgr().GetActiveElements(all);
    o.push_back((double)found.GetSize());
    for (ULONG i = 0; i < found.GetSize(); i++)
    {
        double at = -2.0;
        for (ULONG j = 0; j < all.GetSize(); j++)
            if (all[j] == (void*)found[i])
                at = (double)j;
        o.push_back(at);
    }
    put_tree(o, tree, cache);
    if (bez)
        smos_Free(bez);
    delete cache;
    delete s;
}

// `S opts`: BuildTree(NULL) on a new cache.
void op_build(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_surface(rd, ctx);
    CachePeek* cache = read_cache(rd, s, s->GetNaturalUVDomain());
    SmStatus r = cache->BuildTree(NULL);
    o.push_back(st(r));
    if (r == SM_SUCCESS && cache->tree() != NULL)
        put_tree(o, cache->tree(), cache);
    delete cache;
    delete s;
}

// `S opts`: GetTree() on a new cache (0 = a tree, 1 = NULL).
void op_get(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_surface(rd, ctx);
    CachePeek* cache = read_cache(rd, s, s->GetNaturalUVDomain());
    SmTree* t = cache->GetTree();
    o.push_back(t == NULL ? 1.0 : 0.0);
    if (t != NULL)
        put_tree(o, t, cache);
    delete cache;
    delete s;
}
} // namespace sc_ssc_p3

SC_EXPORT int32_t sc_surface_surface_cache_p3_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    sc_ssc_p3::Rd rd{in, 0};
    switch (op)
    {
    case sc_ssc_p3::OP_SUBDIV_NODE: sc_ssc_p3::op_subdiv_node(rd, ctx, o); break;
    case sc_ssc_p3::OP_SUBDIV_TOL: sc_ssc_p3::op_subdiv_tol(rd, ctx, o); break;
    case sc_ssc_p3::OP_FIND: sc_ssc_p3::op_find(rd, ctx, o); break;
    case sc_ssc_p3::OP_BUILD: sc_ssc_p3::op_build(rd, ctx, o); break;
    case sc_ssc_p3::OP_GET: sc_ssc_p3::op_get(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
