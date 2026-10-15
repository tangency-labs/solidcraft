// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_intersector_p5`: SmSurfaceIntersector::DoIntersection
// (with SmSurfaceIntersector::GrazingBoxProcess, source/SMLib/src/SmSurfaceIntersector.cpp), run as
// every upstream caller runs it: on an SmAdvSurfaceIntersector over SmPlane / SmBSplineSurface /
// SmCurveBoundedSurface objects, built as SmSurface::GlobalSurfaceIntersect builds it.
//
// DoIntersection takes the surface caches from smsurf_GetSurfaceCache. A case either uses them as
// upstream builds them, or replaces their decomposition trees by hand-built ones (the tree
// construction belongs to unit surface.surface_cache_p3): the real cache's m_pTree is swapped for
// the hand tree (kept in its own SmSurfaceCache, as the intersect.adv_surface_intersector shim
// builds it) for the call and restored afterwards, so the real cache deletes its own tree.
//
// One entry point, sc_intersect_surface_intersector_p5_run(in, n_in, out, cap, out_len). `in` is
// the case (layout in crates/solidcore-ffi/src/units/intersect_surface_intersector_p5.rs); every
// read is bounds checked here (a bad case is SC_ERR_ARG, as is a surface without a cache in the
// hand-tree mode).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAdvSurfaceIntersector.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTList.h>
#include <SmTol.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

namespace
{
struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad case") {}
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
    SmPoint2d uv()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
    }
    SmExtent2d dom4()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

// `0 o x y scale dom` (SmPlane) or `1 p q nu nv net(x y z w) u v` (SmBSplineSurface).
SmSurface* read_basic_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        return new (ctx) SmPlane(o, x, y, s, d, &ctx);
    }
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX nu = (gw_INDEX)rd.idx(2, 17);
    gw_INDEX nv = (gw_INDEX)rd.idx(2, 17);
    if (p >= nu || q >= nv)
        throw Bad();
    gw_INDEX n = nu - 1;
    gw_INDEX m = nv - 1;
    gw_INDEX r = n + p + 1;
    gw_INDEX s = m + q + 1;
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (int64_t)nu * nv + r + 1 + s + 1; i++)
        v.push_back(rd.num());
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    size_t k = 0;
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = v[k++];
            sur->net->Pw[i][j].y = v[k++];
            sur->net->Pw[i][j].z = v[k++];
            sur->net->Pw[i][j].w = v[k++];
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = v[k++];
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = v[k++];
    return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
}

// A basic surface (`0 ..` / `1 ..`) or `3 basic`: an SmCurveBoundedSurface (implicit outer, no
// boundary curves) over it, which owns (deletes) its basis. (`2` marks B = A.)
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    if (rd.at < rd.n && rd.in[rd.at] == 3.0)
    {
        rd.at++;
        SmSurface* basis = read_basic_surface(rd, ctx);
        SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
        return new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, TRUE);
    }
    return read_basic_surface(rd, ctx);
}

// ---- hand-built trees ---------------------------------------------------------------------

struct TreePeek : SmTree
{
    explicit TreePeek(SmObject* owner) : SmTree(owner) {}
    SmMemBlockMgr& node_mgr() { return m_sNodeMgr; }
    SmTreeNode*& top() { return m_pTopNode; }
};

struct CachePeek : SmSurfaceCache
{
    CachePeek(const SmSurface& s, const SmExtent2d& d) : SmSurfaceCache(s, d) {}
    SmTree*& tree() { return m_pTree; }
    SmMemBlockMgr& bez() { return m_sBezMgr; }
    SmMemBlockMgr& aux() { return m_sAuxMgr; }
    // The m_pTree member of any surface cache (a pointer to a protected base member).
    static SmTree* SmSurfaceCache::*tree_member() { return &CachePeek::m_pTree; }
};

// Tree: `n` then per node `parent c1 c2 aux bbox[6] dom[4] npolar polar_vecs[3 npolar]` and,
// for aux 3 (SM_AD_BEZIER_SURFACE), `pseudo_basis[9] pseudo_intervals[6]` (the rules of the
// intersect.adv_surface_intersector shim). The nodes live in a cache of their own (`holder`).
CachePeek* read_tree(Rd& rd, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx(1, 64);
    CachePeek* cache = new (ctx) CachePeek(*s, s->GetNaturalUVDomain());
    TreePeek* tree = new (ctx) TreePeek(cache);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
    cache->aux().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    cache->tree() = tree;
    std::vector<SmTreeNode*> nodes;
    for (int64_t k = 0; k < n; k++)
        nodes.push_back((SmTreeNode*)tree->node_mgr().GetNewElement());
    std::vector<int64_t> par((size_t)n), c1((size_t)n), c2((size_t)n);
    try
    {
        for (int64_t k = 0; k < n; k++)
        {
            SmTreeNode* nd = nodes[(size_t)k];
            par[(size_t)k] = rd.idx(-1, n - 1);
            c1[(size_t)k] = rd.idx(-1, n - 1);
            c2[(size_t)k] = rd.idx(-1, n - 1);
            int64_t aux = rd.idx(3, 6);
            if (aux == 5)
                throw Bad();
            nd->m_pTree = tree;
            nd->m_pParent = par[(size_t)k] < 0 ? nullptr : nodes[(size_t)par[(size_t)k]];
            nd->m_pChild1 = c1[(size_t)k] < 0 ? nullptr : nodes[(size_t)c1[(size_t)k]];
            nd->m_pChild2 = c2[(size_t)k] < 0 ? nullptr : nodes[(size_t)c2[(size_t)k]];
            nd->m_eAuxDataType = (SmAuxDataType)aux;
            nd->m_eGeomType = SM_NG_DEFAULT;
            SmVector3d lo = rd.vec();
            SmVector3d hi = rd.vec();
            nd->m_sBBox = SmExtent3d(lo, hi);
            SmBezierAux2d* a = aux == 3 ? (SmBezierAux2d*)cache->bez().GetNewElement()
                                        : (SmBezierAux2d*)cache->aux().GetNewElement();
            a->m_sUVDomain = rd.dom4();
            a->mBA_pSurface = s;
            a->m_pOwningTreeNode = nd;
            int64_t npolar = rd.idx(0, 8);
            if (npolar == 0 && aux != 6)
                throw Bad();
            SmPolarBox* pb = const_cast<SmPolarBox*>(a->GetPolarBoxPtr());
            for (int64_t i = 0; i < npolar; i++)
                pb->AddVector3d(rd.vec(), NULL);
            if (aux == 3)
            {
                SmBezierPatch* p = (SmBezierPatch*)a;
                SmVector3d b0 = rd.vec();
                SmVector3d b1 = rd.vec();
                SmVector3d b2 = rd.vec();
                double i0 = rd.num(), i1 = rd.num(), i2 = rd.num(), i3 = rd.num(), i4 = rd.num(), i5 = rd.num();
                if (p->GetPseudoBox().SetBasis(b0, b1, b2) != SM_SUCCESS)
                    throw Bad();
                p->GetPseudoBox().SetIntervals(SmExtent1d(i0, i1), SmExtent1d(i2, i3), SmExtent1d(i4, i5));
            }
            nd->m_pData = a;
        }
        // shape checks
        for (int64_t k = 0; k < n; k++)
        {
            int64_t p = par[(size_t)k], a = c1[(size_t)k], b = c2[(size_t)k];
            if ((k == 0) != (p < 0))
                throw Bad();
            if (p >= 0 && !(p < k && (c1[(size_t)p] == k || c2[(size_t)p] == k)))
                throw Bad();
            if ((a < 0) != (b < 0))
                throw Bad();
            if (a >= 0 && !(a > k && b > k && a != b && par[(size_t)a] == k && par[(size_t)b] == k))
                throw Bad();
            if (a < 0 && nodes[(size_t)k]->m_eAuxDataType != SM_AD_BEZIER_SURFACE)
                throw Bad();
        }
    }
    catch (...)
    {
        delete cache;
        throw;
    }
    tree->top() = nodes[0];
    return cache;
}

// ---- outputs ------------------------------------------------------------------------------

struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
};

// `type dim knot_type form has [n p m cpts(x y z w) knots] ivl[2]` of a B-spline curve.
void put_bspline(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    if (!c->IsKindOf(SmBSplineCurve_TYPE))
        throw Bad();
    const SmBSplineCurve* b = (const SmBSplineCurve*)c;
    o.push_back((double)(b->*PeekC::kt()));
    o.push_back((double)(b->*PeekC::form()));
    const gw_CURVE* g = b->GetGwNurbPointer();
    o.push_back(g ? 1.0 : 0.0);
    if (g)
    {
        o.push_back((double)g->pol->n);
        o.push_back((double)g->p);
        o.push_back((double)g->knt->m);
        for (gw_INDEX i = 0; i <= g->pol->n; i++)
        {
            o.push_back(g->pol->Pw[i].x);
            o.push_back(g->pol->Pw[i].y);
            o.push_back(g->pol->Pw[i].z);
            o.push_back(g->pol->Pw[i].w);
        }
        for (gw_INDEX i = 0; i <= g->knt->m; i++)
            o.push_back(g->knt->U[i]);
    }
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
}

// `has [curve]`.
void put_opt(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(c ? 1.0 : 0.0);
    if (c)
        put_bspline(o, c);
}

// ---- intersector state ---------------------------------------------------------------------

// private member access (explicit instantiation may name private members)
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};
struct TagClosest
{
    typedef double SmAdvSurfaceIntersector::*type;
    friend type get(TagClosest);
};
struct TagUVS
{
    typedef SmPoint2d (SmAdvSurfaceIntersector::*type)[2];
    friend type get(TagUVS);
};
struct TagSep
{
    typedef SmBoolean SmAdvSurfaceIntersector::*type;
    friend type get(TagSep);
};
template struct Rob<TagClosest, &SmAdvSurfaceIntersector::m_dClosestValue>;
template struct Rob<TagUVS, &SmAdvSurfaceIntersector::m_sUVS>;
template struct Rob<TagSep, &SmAdvSurfaceIntersector::m_bSeperateGaussMaps>;

// The intersector with its protected state reachable.
struct Peek : SmAdvSurfaceIntersector
{
    Peek(const SmSurface& a, const SmExtent2d& da, const SmSurface& b, const SmExtent2d& db)
        : SmAdvSurfaceIntersector(a, da, b, db)
    {
    }
    static double count(SmTList<SmTsectPnt>& l)
    {
        SmTArray<SmTsectPnt*> pts;
        l.GetAllNodes(pts);
        return (double)pts.GetSize();
    }
    // `closest uv1[2] uv2[2] separate trace_dir ang_tol approx_tol tol3d op closed
    // ncurve nstart nthrough`
    void put_state(std::vector<double>& o)
    {
        o.push_back(this->*get(TagClosest()));
        const SmPoint2d* uvs = this->*get(TagUVS());
        for (int i = 0; i < 2; i++)
        {
            o.push_back(uvs[i].x);
            o.push_back(uvs[i].y);
        }
        o.push_back((this->*get(TagSep())) ? 1.0 : 0.0);
        o.push_back(m_dCurveTraceDirection);
        o.push_back(m_dThisAngTolRad);
        o.push_back(m_dThisApproxTol3d);
        o.push_back(m_d3dTolerance);
        o.push_back((double)m_eSolverOperation);
        o.push_back(m_bCurveIsClosed ? 1.0 : 0.0);
        o.push_back(count(m_vCurvePoints));
        o.push_back(count(m_vStartPoints));
        o.push_back(count(m_vThroughPoints));
    }
};

template <typename T>
void free_all(SmTArray<T*>& a)
{
    for (ULONG i = 0; i < a.GetSize(); i++)
        delete a[i];
    a.ReSet();
}

// The hand trees swapped into the real caches; restored (and the holders deleted) on exit.
struct Swap
{
    SmSurfaceCache* real[2] = {nullptr, nullptr};
    SmTree* saved[2] = {nullptr, nullptr};
    CachePeek* holder[2] = {nullptr, nullptr};
    int count = 0;
    ~Swap()
    {
        for (int i = 0; i < count; i++)
        {
            if (real[i])
                real[i]->*CachePeek::tree_member() = saved[i];
            delete holder[i];
        }
    }
};
} // namespace

SC_EXPORT int32_t sc_intersect_surface_intersector_p5_run(const double* in, int64_t n_in, double* out,
                                                          int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    SmSurface* a = NULL;
    SmSurface* b = NULL;
    bool same = false;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        a = read_surface(rd, ctx);
        if (rd.at < rd.n && rd.in[rd.at] == 2.0)
        {
            rd.at++;
            same = true;
            b = a;
        }
        else
            b = read_surface(rd, ctx);
        SmExtent2d d1 = rd.dom4();
        SmExtent2d d2 = rd.dom4();
        bool e1 = rd.flag();
        bool e2 = rd.flag();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool has_ang = rd.flag();
        double ang = rd.num();
        bool hand = rd.flag();
        {
            Swap sw;
            if (hand)
            {
                const SmSurface* surfs[2] = {a, b};
                sw.count = same ? 1 : 2;
                for (int i = 0; i < sw.count; i++)
                {
                    sw.holder[i] = read_tree(rd, surfs[i], ctx);
                    SmSurfaceCache* sc = smsurf_GetSurfaceCache(surfs[i]);
                    if (sc == NULL)
                        throw Bad();
                    sw.real[i] = sc;
                    sw.saved[i] = sc->*CachePeek::tree_member();
                    sc->*CachePeek::tree_member() = sw.holder[i]->tree();
                }
            }
            if (rd.at != rd.n)
                throw Bad();

            SmApproxTol3d approx;
            double& approx_val = approx; // the raw value (the constructor takes |tol|)
            approx_val = tol;
            SmTArray<SmCurve*> c3(ctx), uv1(ctx), uv2(ctx);
            SmTArray<SmTsectCurveType> types(ctx);
            SmTArray<double> devs(ctx);
            SmStatus st;
            std::vector<double> state;
            {
                Peek si(*a, d1, *b, d2);
                si.SetUseSurfaceEdges(e1 ? TRUE : FALSE, e2 ? TRUE : FALSE);
                st = si.DoIntersection(ctx, has_tol ? &approx : NULL, has_ang ? &ang : NULL, &c3, &uv1, &uv2,
                                       &types, &devs);
                if (hand)
                    si.put_state(state);
            }
            o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
            o.insert(o.end(), state.begin(), state.end());
            if (st == SM_SUCCESS)
            {
                ULONG nc = c3.GetSize();
                if (uv1.GetSize() != nc || uv2.GetSize() != nc || types.GetSize() != nc || devs.GetSize() != nc)
                    throw Bad();
                o.push_back((double)nc);
                for (ULONG i = 0; i < nc; i++)
                {
                    put_bspline(o, c3[i]);
                    put_opt(o, uv1[i]);
                    put_opt(o, uv2[i]);
                    o.push_back((double)types[i]);
                    o.push_back(devs[i]);
                }
            }
            free_all(c3);
            free_all(uv1);
            free_all(uv2);
        }
        if (!same)
            delete b;
        delete a;
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
