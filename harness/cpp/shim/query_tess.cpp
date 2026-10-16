// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.tess`: SmTessSrfCache, SmSurfaceTessDriver,
// SmViewBasedTessDriver and SmPolyToEUVUAttr (source/SMLib/src/SmTess.cpp,
// source/SMLib/inc/SmTess.h) over an SmBSplineSurface or an SmPlane.
//
// One entry point, sc_query_tess_eval(op, in, n_in, out, cap, out_len). Layouts are documented
// in crates/solidcore-ffi/src/units/query_tess.rs, which validates every input. Every op reads
// the same blocks: a B-spline surface, the cache tolerances, the tessellation driver, an
// optional poly B-rep (one region, shell and face whose closed loops lie in the uv plane), an
// optional hand-built subdivision tree (allocated in order from the tree's node manager, every
// node an SmBezierAux2d on the cache surface), the cache state, then the op's parameters.
//
// The face is a bare SmFace whose m_pSurface is the surface (the surface keeps no owner, so
// zone tolerances are the defaults, as on the Rust side); the poly face's original face is a
// fake label pointer (never dereferenced: MarkFaceContainment only sets it back unchanged).
// Protected members are reached through derived "peek" classes. `out` receives sections
// `key kind len values...` (kind 0 floats, 1 ints).
#include "sc_shim.h"

#include "StdAfx.h"

#include <cstdint>
#include <vector>

// Read the protected members of the cache, tree, poly and attribute classes by opening the
// access of the kernel headers below (the standard headers are included above).
#define protected public
#define private public
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPoly.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTess.h>
#include <SmTree.h>
#include <SmTrimSrfCache.h>
#include <nurbs.h>

#include "sc_sm.h"

using sc::put_dom;
using sc::st;
#undef private
#undef protected

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/query_tess.rs).
enum : int32_t
{
    OP_NEW = 0,
    OP_TEST = 1,
    OP_BUILD_SUB = 2,
    OP_IMPLANT = 3,
    OP_MARK = 4,
    OP_BUILD = 5,
    OP_ATTR = 6,
};

// Section keys (keep in sync with crates/difftest/src/units/query_tess.rs).
enum : int32_t
{
    K_STATUS = 0,
    K_RES = 1,
    K_RES_I = 2,
    K_STATE = 3,
    K_DOM = 4,
    K_NODES = 5,
    K_NODE_GEOM = 6,
};

struct TreePeek : SmTree
{
    explicit TreePeek(SmObject* owner) : SmTree(owner) {}
    SmMemBlockMgr& node_mgr() { return m_sNodeMgr; }
    SmTreeNode*& top() { return m_pTopNode; }
};

struct FacePeek : SmFace
{
    FacePeek() : SmFace() {}
    ~FacePeek() { m_pSurface = NULL; }
    void set_surface(SmSurface* s) { m_pSurface = s; }
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
};

struct Out
{
    std::vector<double> o;
    void sec(int32_t key, int32_t kind, const std::vector<double>& v)
    {
        o.push_back((double)key);
        o.push_back((double)kind);
        o.push_back((double)v.size());
        o.insert(o.end(), v.begin(), v.end());
    }
};

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

void* fake(int64_t label)
{
    return label <= 0 ? NULL : (void*)(uintptr_t)(label << 4);
}

double unfake(const void* p)
{
    return p == NULL ? 0.0 : (double)((uintptr_t)p >> 4);
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

SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

// A surface `kind ...`: 0 B-spline `oob form nurb`; 1 bounded plane `o x y sx sy D` (an
// SmPlane is an SmBSplineSurface).
SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    if (rd.idx() == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double sx = rd.num();
        double sy = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
    }
    return read_bspline(rd, ctx);
}

struct Model
{
    SmPolyBrep* brep = NULL;
    SmPolyFace* face = NULL;
    std::vector<SmPolyEdge*> edges;
};

// `has` then `brep_tol face_tol label nloops` and per loop `etol k` then `k` x `x y z vtol`.
SmPolyBrep* read_poly(Rd& rd, const SmContext& ctx, Model& m)
{
    if (!rd.flag())
        return NULL;
    double tol = rd.num();
    SmPolyBrep* b = new (ctx) SmPolyBrep(tol);
    SmPolyRegion* r = new (b) SmPolyRegion(b);
    SmPolyShell* sh = new (b) SmPolyShell(r);
    double ftol = rd.num();
    int64_t label = rd.idx();
    SmPolyFace* f = new (b) SmPolyFace(ftol, NULL, NULL, sh, NULL);
    f->SetOriginalFace((SmFace*)fake(label), FALSE);
    int64_t nl = rd.idx();
    for (int64_t l = 0; l < nl; ++l)
    {
        double etol = rd.num();
        int64_t k = rd.idx();
        std::vector<SmPolyVertex*> vs;
        for (int64_t j = 0; j < k; ++j)
        {
            SmVector3d p;
            p.x = rd.num();
            p.y = rd.num();
            p.z = rd.num();
            double vt = rd.num();
            SmPolyVertex* v = new (b) SmPolyVertex(p, vt);
            b->m_pPolyVertexListHead->PostInsert(v);
            vs.push_back(v);
        }
        SmPolyLoop* lp = NULL;
        f->StartPolyEdgeLoop(lp);
        for (int64_t j = 0; j < k; ++j)
        {
            SmPolyVertex* v0 = vs[(size_t)j];
            SmPolyVertex* v1 = vs[(size_t)((j + 1) % k)];
            SmPolyEdge* e = NULL;
            lp->AddPolyEdge(etol, v0->GetPoint(), v1->GetPoint(), v0, NULL, b, e);
            m.edges.push_back(e);
        }
        lp->FinishPolyEdgeLoop(b);
    }
    m.brep = b;
    m.face = f;
    return b;
}

struct TessPeek : SmTessSrfCache
{
    TessPeek(SmFace* f, SmPolyBrep* pb, SmSurfaceTessDriver* d, double ch, double ang, double mx, double mn,
             double ratio, double ar)
        : SmTessSrfCache(f, pb, d, ch, ang, mx, mn, ratio, ar)
    {
    }
    SmMemBlockMgr& aux_mgr() { return m_sAuxMgr; }
    SmMemBlockMgr& pe_mgr() { return m_sPolyEdgeListMgr; }
    SmTree*& tree() { return m_pTree; }
    SmBoolean switch_fast() const { return m_bSwitchToFastSubdivision; }
    SmTArray<SmSurface*>& subs() { return m_sSubdivisionSurfaces; }
    SmExtent2d& domain() { return m_sUVDomain; }
    double chord() const { return m_dChordHeightTolerance; }
    double angle() const { return m_dAngTolRad; }
    double aspect() const { return m_dAspectRatio3D; }
    double max_side() const { return m_dMaximumSideLength3D; }
    double min_side() const { return m_dMinimumSideLength3D; }
    double min_ratio() const { return m_dMinimumSideLengthRatioUV; }
    double factor() const { return m_dTrimSubdivisionFactor; }
};

// `n` nodes `parent c1 c2 aux split marked class D`, allocated in order; every node's data
// from m_sAuxMgr with mBA_pSurface = the cache surface.
void read_tree(Rd& rd, TessPeek* c, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx();
    if (n == 0)
        return;
    TreePeek* tree = new (ctx) TreePeek(c);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    c->aux_mgr().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    c->tree() = tree;
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
        SmBezierAux2d* a = (SmBezierAux2d*)c->aux_mgr().GetNewElement();
        a->m_eSplitDir = (SmSurfParamType)rd.idx();
        a->m_bIsMarked = rd.flag() ? TRUE : FALSE;
        a->m_eNodeClass = (SmNodeClassType)rd.idx();
        a->m_pFace = NULL;
        a->m_sUVDomain = rd.domain();
        a->mBA_pSurface = s;
        a->m_pOwningTreeNode = nd;
        nd->m_pData = a;
    }
    tree->top() = nodes[0];
}

template <typename T> double ix(const std::vector<T*>& v, const void* p)
{
    if (p == NULL)
        return -1.0;
    for (size_t i = 0; i < v.size(); ++i)
        if ((const void*)v[i] == p)
            return (double)i;
    return -2.0;
}

void dump(TessPeek* c, Model& m, Out& out)
{
    std::vector<double> state;
    state.push_back(flg(c->m_bPointTestEnabled));
    state.push_back(flg(c->m_bHaveTSurfaceCache));
    state.push_back((double)c->m_bFaceContainmentDone);
    state.push_back(flg(c->m_bProcessBoundaryCurves));
    state.push_back(flg(c->switch_fast()));
    state.push_back((double)c->subs().GetSize());
    state.push_back(m.face ? unfake(m.face->GetOriginalFace()) : -1.0);
    out.sec(K_STATE, 1, state);

    std::vector<double> dom;
    put_dom(dom, c->domain());
    dom.push_back(c->chord());
    dom.push_back(c->angle());
    out.sec(K_DOM, 0, dom);

    std::vector<double> ni, nf;
    if (c->tree())
    {
        SmTArray<void*> act;
        ((TreePeek*)c->tree())->node_mgr().GetActiveElements(act);
        std::vector<const SmTreeNode*> nodes;
        for (ULONG i = 0; i < act.GetSize(); i++)
            nodes.push_back((const SmTreeNode*)act[i]);
        ni.push_back((double)nodes.size());
        ni.push_back(ix(nodes, c->tree()->GetTopNode()));
        std::vector<const SmSurface*> subs;
        for (ULONG i = 0; i < c->subs().GetSize(); i++)
            subs.push_back(c->subs()[i]);
        for (const SmTreeNode* nd : nodes)
        {
            const SmBezierAux2d* a = (const SmBezierAux2d*)nd->m_pData;
            ni.push_back(ix(nodes, nd->m_pParent));
            ni.push_back(ix(nodes, nd->m_pChild1));
            ni.push_back(ix(nodes, nd->m_pChild2));
            ni.push_back((double)nd->m_eAuxDataType);
            ni.push_back((double)a->m_eSplitDir);
            ni.push_back(flg(a->m_bIsMarked));
            ni.push_back((double)a->m_eNodeClass);
            ni.push_back(unfake(a->m_pFace));
            double surf = a->mBA_pSurface == NULL ? -1.0
                          : a->mBA_pSurface == c->GetSurface()
                              ? -3.0
                              : ix(subs, a->mBA_pSurface);
            ni.push_back(surf);
            ni.push_back(flg(a->m_bChordHeightSatisfied));
            ni.push_back(flg(a->m_bAngleTolSatisfied));
            SmTArray<SmPolyEdgeList*> l;
            a->m_sPolyEdgeList.GetAllNodes(l);
            ni.push_back((double)l.GetSize());
            for (ULONG j = 0; j < l.GetSize(); j++)
                ni.push_back(ix(m.edges, l[j]->m_pPolyEdge));
            put_dom(nf, a->m_sUVDomain);
            const SmExtent3d& b = nd->m_sBBox;
            nf.push_back(b.GetMin().x);
            nf.push_back(b.GetMin().y);
            nf.push_back(b.GetMin().z);
            nf.push_back(b.GetMax().x);
            nf.push_back(b.GetMax().y);
            nf.push_back(b.GetMax().z);
        }
    }
    out.sec(K_NODES, 1, ni);
    out.sec(K_NODE_GEOM, 0, nf);
}

struct PolyToEuVuPeek : SmPolyToEUVUAttr
{
    PolyToEuVuPeek(ULONG id, SmAttributeBehaviorType b) : SmPolyToEUVUAttr(id, b) {}
};

// `id behavior eu_label vu_label`: the attribute and its MakeCopy.
void op_attr(Rd& rd, const SmContext& ctx, Out& out)
{
    ULONG id = (ULONG)rd.idx();
    SmAttributeBehaviorType b = (SmAttributeBehaviorType)rd.idx();
    int64_t eu = rd.idx();
    int64_t vu = rd.idx();
    SmPolyToEUVUAttr* a = new (ctx) SmPolyToEUVUAttr(id, b);
    std::vector<double> r;
    r.push_back(unfake(a->m_pEdgeuse));
    r.push_back(unfake(a->m_pVertexuse));
    a->m_pEdgeuse = (SmEdgeuse*)fake(eu);
    a->m_pVertexuse = (SmVertexuse*)fake(vu);
    SmPolyToEUVUAttr* cp = (SmPolyToEUVUAttr*)a->MakeCopy(ctx);
    for (SmPolyToEUVUAttr* x : {a, cp})
    {
        r.push_back((double)x->GetAttributeID());
        r.push_back((double)x->GetBehavior());
        r.push_back((double)x->GetType());
        r.push_back(unfake(x->m_pEdgeuse));
        r.push_back(unfake(x->m_pVertexuse));
    }
    out.sec(K_RES_I, 1, r);
    delete cp;
    delete a;
}
} // namespace

SC_EXPORT int32_t sc_query_tess_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                     int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    Out o;
    Rd rd{in, 0};
    if (op == OP_ATTR)
    {
        op_attr(rd, ctx, o);
        return sc_copy_out(o.o.data(), (int64_t)o.o.size(), out, cap, out_len);
    }
    SmBSplineSurface* s = read_surface(rd, ctx);

    // cache tolerances: ch ang max_edge min_edge ratio aspect
    double t[6];
    for (double& v : t)
        v = rd.num();

    // driver: kind vx vy vz sil_ch sil_ang ch ang max_edge min_edge ratio aspect
    int64_t dk = rd.idx();
    double d[11];
    for (double& v : d)
        v = rd.num();
    SmSurfaceTessDriver* driver = NULL;
    if (dk == 1)
        driver = new SmSurfaceTessDriver(d[5], d[6], d[7], d[8], d[9], d[10]);
    else if (dk == 2)
    {
        SmVector3d vv;
        vv.x = d[0];
        vv.y = d[1];
        vv.z = d[2];
        driver = new SmViewBasedTessDriver(vv, d[3], d[4], d[5], d[6], d[7], d[8], d[9], d[10]);
    }

    Model m;
    SmPolyBrep* pb = read_poly(rd, ctx, m);

    FacePeek* face = new (static_cast<SmObject*>(s)) FacePeek();
    face->set_surface(s);
    TessPeek* c = new (ctx) TessPeek(face, pb, driver, t[0], t[1], t[2], t[3], t[4], t[5]);

    read_tree(rd, c, s, ctx);

    // state: point_test have_t containment has_dom D
    c->m_bPointTestEnabled = rd.flag() ? TRUE : FALSE;
    c->m_bHaveTSurfaceCache = rd.flag() ? TRUE : FALSE;
    c->m_bFaceContainmentDone = (SmBoolean)rd.idx();
    bool has_dom = rd.flag();
    SmExtent2d dom = rd.domain();
    if (has_dom)
        c->domain() = dom;

    std::vector<double> status, res;
    switch (op)
    {
    case OP_NEW:
    {
        res.push_back((double)c->GetType());
        int64_t k = rd.idx();
        for (int64_t i = 0; i < k; i++)
            res.push_back(flg(c->IsKindOf((SM_TYPE)rd.idx())));
        res.push_back(flg(c->GetFace() == face));
        res.push_back(flg(c->GetTS_PolyBrep() == pb));
        res.push_back(flg(c->m_bFaceWasModified));
        res.push_back(c->aspect());
        res.push_back(c->max_side());
        res.push_back(c->min_side());
        res.push_back(c->min_ratio());
        res.push_back(c->factor());
        if (driver)
        {
            res.push_back((double)driver->GetType());
            res.push_back(flg(driver->m_bEvalBasedTessellation));
            res.push_back(driver->m_dChordHeight);
            res.push_back(driver->m_dAngTolDeg);
            res.push_back(driver->m_dMaxAspectRatio);
            res.push_back(driver->m_dMaxEdgeLength3D);
            res.push_back(driver->m_dMinEdgeLength3D);
            res.push_back(driver->m_dMinEdgeLengthRatioUV);
            if (driver->GetType() == SmViewBasedTessDriver_TYPE)
            {
                SmViewBasedTessDriver* v = (SmViewBasedTessDriver*)driver;
                res.push_back(v->GetViewVector().x);
                res.push_back(v->GetViewVector().y);
                res.push_back(v->GetViewVector().z);
                res.push_back(v->GetSilhouetteChordHeight());
                res.push_back(v->GetSilhouetteAngleTolerance());
            }
        }
        status.push_back(0.0);
        break;
    }
    case OP_TEST:
    {
        int64_t k = rd.idx();
        for (int64_t i = 0; i < k; i++)
        {
            int64_t node = rd.idx();
            SmTArray<void*> act;
            ((TreePeek*)c->tree())->node_mgr().GetActiveElements(act);
            SmBoolean needs = FALSE;
            SmSurfParamType dir = SM_SP_U;
            SmStatus r = c->TestAgainstTolerances((SmTreeNode*)act[(ULONG)node], needs, dir);
            status.push_back(st(r));
            if (r != SM_SUCCESS)
                break;
            res.push_back(flg(needs));
            res.push_back((double)dir);
        }
        break;
    }
    case OP_BUILD_SUB:
        status.push_back(st(c->BuildTreeWithSubdivision(s)));
        break;
    case OP_IMPLANT:
    {
        c->pe_mgr().Initialize(ALIGN_SIZE(sizeof(SmPolyEdgeList)), 30);
        int64_t k = rd.idx();
        for (int64_t i = 0; i < k; i++)
        {
            int64_t e = rd.idx();
            SmStatus r = c->ImplantUVSegment(m.edges[(size_t)e]);
            status.push_back(st(r));
            if (r != SM_SUCCESS)
                break;
        }
        break;
    }
    case OP_MARK:
        status.push_back(st(c->MarkFaceContainment()));
        break;
    case OP_BUILD:
        status.push_back(st(c->BuildTree(NULL)));
        break;
    default:
        break;
    }
    o.sec(K_STATUS, 1, status);
    o.sec(K_RES, 0, res);
    dump(c, m, o);

    delete c; // deletes the poly brep
    delete face;
    delete driver;
    delete s;
    return sc_copy_out(o.o.data(), (int64_t)o.o.size(), out, cap, out_len);
    SC_CATCH
}
