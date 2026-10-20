// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_silhouette`: the SmSurfaceSilhouette members owned by
// the unit (source/SMLib/src/SmSurfaceSilhouette.cpp, source/SMLib/inc/SmSurfaceSilhouette.h).
//
// The silhouette tracer is built on a real SmBSplineSurface or SmPlane and its public members are
// called directly: ComputePointValues (with and without a previous point, which reaches the
// file-local SmFindSilSingEFO through SmLocalSolve1d), LocalPointSolve, DoesPointLieOnCurve,
// FindSolutionsOnBoundaryCurve and BranchMayContainAnswers. A derived "peek" class only reaches
// the protected base members (tolerances, trace direction, context).
//
// BranchMayContainAnswers reads the SmBezierPatch of a surface tree node: decomposition trees are
// built by hand in the tree's own node manager (as the surface.surface_cache_p1 shim does; tree
// construction belongs to unit surface.surface_cache_p3), with optional explicit normal (polar)
// and pseudo boxes per patch.
//
// One entry point, sc_intersect_surface_silhouette_run(in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/intersect_surface_silhouette.rs, which validates
// every input; reads are bounds checked here too (a bad program is SC_ERR_ARG).
#include "sc_shim.h"

#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmSurfaceSilhouette.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_surface_silhouette.rs).
enum : int32_t
{
    OP_CPV = 1,
    OP_LPS = 2,
    OP_ON_CURVE = 3,
    OP_BOUNDARY = 4,
    OP_BRANCH = 5,
    OP_LAST = 5,
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
};

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

// `n m p q r s cpts(x y z w) u v`, sizes checked by the Rust side.
SmBSplineSurface* read_bspline_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx(1, 16);
    gw_INDEX m = (gw_INDEX)rd.idx(1, 16);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX r = (gw_INDEX)rd.idx(3, 40);
    gw_INDEX s = (gw_INDEX)rd.idx(3, 40);
    if (p > n || q > m || r != n + p + 1 || s != m + q + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) * (m + 1) + r + 1 + s + 1; i++)
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

// `0 bspline-surface` or `1 o x y scale[2] dom[4]` (SmPlane(o, x, y, scale, dom, ctx)).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
        return read_bspline_surface(rd, ctx);
    SmPoint3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector2d s = rd.uv();
    SmExtent2d d = rd.dom4();
    return new (ctx) SmPlane(o, x, y, s, d, &ctx);
}

// A 3D B-spline curve `n p m cpts(x y z w) knots`.
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t n = rd.idx(1, 32);
    int64_t p = rd.idx(1, 8);
    int64_t m = rd.idx(3, 64);
    if (p > n || m != n + p + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) + m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)n, (gw_DEGREE)p, (gw_INDEX)m);
    for (int64_t i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[4 * i];
        cur->pol->Pw[i].y = v[4 * i + 1];
        cur->pol->Pw[i].z = v[4 * i + 2];
        cur->pol->Pw[i].w = v[4 * i + 3];
    }
    for (int64_t i = 0; i <= m; i++)
        cur->knt->U[i] = v[4 * (n + 1) + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(3, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

// Access to the protected base members of the silhouette tracer.
class Sil : public SmSurfaceSilhouette
{
public:
    Sil(const SmContext& ctx, const SmSurface* s, const SmExtent2d& d, SmBoolean persp, const SmVector3d& eye)
        : SmSurfaceSilhouette(s, d, persp, eye)
    {
        m_cpContext = &ctx;
    }
    double& ApproxTol() { return m_dThisApproxTol3d; }
    double& AngTol() { return m_dThisAngTolRad; }
    double& Direction() { return m_dCurveTraceDirection; }
};

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
};

// A trace point with every vector zeroed (SmTracePnt leaves them uninitialised).
void zero_point(SmTracePnt& p)
{
    SmVector3d z(0.0, 0.0, 0.0);
    SmVector2d z2(0.0, 0.0);
    for (int i = 0; i < 3; i++)
    {
        p.m_v3DCurvePV[i] = z;
        p.m_vUVCurvePV[i] = z2;
        for (int j = 0; j < 3; j++)
            p.m_vSurfacePV[i][j] = z;
    }
}

// `type c3d[0] c3d[1] cuv[0] cuv[1] S00 S10 S01 S11 S20 S02 S22`.
void put_point(std::vector<double>& o, const SmTracePnt& p)
{
    o.push_back((double)p.m_ePointType);
    put_vec(o, p.m_v3DCurvePV[0]);
    put_vec(o, p.m_v3DCurvePV[1]);
    put_uv(o, p.m_vUVCurvePV[0]);
    put_uv(o, p.m_vUVCurvePV[1]);
    put_vec(o, p.m_vSurfacePV[0][0]);
    put_vec(o, p.m_vSurfacePV[1][0]);
    put_vec(o, p.m_vSurfacePV[0][1]);
    put_vec(o, p.m_vSurfacePV[1][1]);
    put_vec(o, p.m_vSurfacePV[2][0]);
    put_vec(o, p.m_vSurfacePV[0][2]);
    put_vec(o, p.m_vSurfacePV[2][2]);
}

// `type nvars nobjs has_obj0 start_value start[np] end_value end[np]`, np = clamp(nvars, 2, 10).
void put_sol(std::vector<double>& o, const SmSolution& s)
{
    ULONG np = s.m_lNumVariables > 2 ? s.m_lNumVariables : 2;
    if (np > 10)
        np = 10;
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_apObjects[0] != NULL ? 1.0 : 0.0);
    o.push_back(s.m_vStart.m_dSolutionValue);
    for (ULONG i = 0; i < np; i++)
        o.push_back(s.m_vStart[i]);
    o.push_back(s.m_vEnd.m_dSolutionValue);
    for (ULONG i = 0; i < np; i++)
        o.push_back(s.m_vEnd[i]);
}

// Tree nodes `n` x `parent c1 c2 aux null dom[4] polar pseudo`, allocated in order from the
// tree's node manager (node 0 the top); patches (aux SM_AD_BEZIER_SURFACE) from m_sBezMgr, the
// others from m_sAuxMgr (`null`: m_pData = NULL); every mBA_pSurface is the cache surface.
// `polar` is `0` (uninitialised: built on demand) or `1 k vecs[k]` (AddVector3d of each);
// `pseudo` (patches only) is `0` (SmPseudoBox()) or `1 basis[3] k pts[k]` (SmPseudoBox(), SetBasis,
// AddPoint3d of each).
std::vector<SmTreeNode*> read_tree(Rd& rd, CachePeek* cache, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx(1, 16);
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
        nd->m_pParent = ptr(rd.idx(-1, n - 1));
        nd->m_pChild1 = ptr(rd.idx(-1, n - 1));
        nd->m_pChild2 = ptr(rd.idx(-1, n - 1));
        nd->m_eAuxDataType = (SmAuxDataType)rd.idx(0, 7);
        nd->m_eGeomType = SM_NG_DEFAULT;
        bool patch = nd->m_eAuxDataType == SM_AD_BEZIER_SURFACE;
        bool null_data = rd.flag();
        SmBezierAux2d* a =
            patch ? (SmBezierAux2d*)cache->bez().GetNewElement() : (SmBezierAux2d*)cache->aux().GetNewElement();
        a->m_eSplitDir = SM_SP_UNKNOWN;
        a->m_bIsMarked = FALSE;
        a->m_eNodeClass = SM_NC_UNKNOWN;
        a->m_pFace = NULL;
        a->m_sUVDomain = rd.dom4();
        a->mBA_pSurface = s;
        a->m_pOwningTreeNode = nd;
        if (rd.flag())
        {
            SmPolarBox b;
            int64_t kv = rd.idx(0, 16);
            for (int64_t i = 0; i < kv; i++)
                b.AddVector3d(rd.vec());
            *const_cast<SmPolarBox*>(a->GetPolarBoxPtr()) = b;
        }
        if (patch)
        {
            SmPseudoBox b;
            if (rd.flag())
            {
                SmVector3d b1 = rd.vec();
                SmVector3d b2 = rd.vec();
                SmVector3d b3 = rd.vec();
                b.SetBasis(b1, b2, b3);
                int64_t kp = rd.idx(0, 16);
                for (int64_t i = 0; i < kp; i++)
                    b.AddPoint3d(rd.vec());
            }
            ((SmBezierPatch*)a)->GetPseudoBox() = b;
        }
        nd->m_pData = null_data ? NULL : a;
    }
    tree->top() = nodes[0];
    return nodes;
}

struct Last
{
    bool has = false;
    SmTracePnt pnt;
};

void run_op(int32_t op, Rd& rd, const SmContext& ctx, const SmSurface* srf, Sil& t, Last& last,
            std::vector<double>& o)
{
    switch (op)
    {
    case OP_CPV:
    {
        double dir = rd.num();
        bool use_prev = rd.flag();
        SmTracePnt p;
        zero_point(p);
        p.m_ePointType = (SmTracePointType)rd.idx(0, 4);
        p.m_v3DCurvePV[1] = rd.vec();
        SmPoint2d uv = rd.uv();
        t.Direction() = dir;
        SmTracePnt prev;
        SmTracePnt* pprev = NULL;
        if (use_prev && last.has)
        {
            prev = last.pnt;
            pprev = &prev;
        }
        SmStatus s = t.ComputePointValues(uv, p, pprev, NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_point(o, p);
            last.has = true;
            last.pnt = p;
        }
        break;
    }
    case OP_LPS:
    {
        SmPoint2d guess = rd.uv();
        SmTracePnt a, b;
        zero_point(a);
        zero_point(b);
        SmBoolean found = FALSE;
        SmPoint2d res(0.0, 0.0);
        SmStatus s = t.LocalPointSolve(a, guess, b, found, res);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
                put_uv(o, res);
        }
        break;
    }
    case OP_ON_CURVE:
    {
        bool refine = rd.flag();
        SmPoint2d uv = rd.uv();
        SmBoolean r = t.DoesPointLieOnCurve(uv, refine ? TRUE : FALSE);
        o.push_back(0.0);
        o.push_back(flg(r));
        put_uv(o, uv);
        break;
    }
    case OP_BOUNDARY:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        {
            SmSolutionArray sols;
            SmStatus s = t.FindSolutionsOnBoundaryCurve(*c, SmExtent1d(a, b), 0, SM_SP_U, sols);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back((double)sols.GetSize());
                for (ULONG i = 0; i < sols.GetSize(); i++)
                    put_sol(o, sols[i]);
            }
        }
        delete c;
        break;
    }
    case OP_BRANCH:
    {
        CachePeek* cache = new (ctx) CachePeek(*srf, srf->GetNaturalUVDomain());
        {
            std::vector<SmTreeNode*> nodes = read_tree(rd, cache, srf, ctx);
            int64_t k = rd.idx(0, 16);
            o.push_back(0.0);
            for (int64_t i = 0; i < k; i++)
            {
                SmTreeNode* br[SM_GS_MAX_TREES] = {NULL};
                br[0] = nodes[(size_t)rd.idx(0, (int64_t)nodes.size() - 1)];
                o.push_back(flg(t.BranchMayContainAnswers(br)));
            }
        }
        delete cache;
        break;
    }
    default:
        throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_intersect_surface_silhouette_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                      int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SC_NATIVE_LOCK;
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* srf = read_surface(rd, ctx);
        SmExtent2d dom = rd.dom4();
        bool persp = rd.flag();
        SmVector3d eye = rd.vec();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool has_ang = rd.flag();
        double ang = rd.num();
        {
            Sil t(ctx, srf, dom, persp ? TRUE : FALSE, eye);
            o.push_back(t.ApproxTol());
            o.push_back(t.AngTol());
            if (has_tol)
                t.ApproxTol() = tol;
            if (has_ang)
                t.AngTol() = ang;
            Last last;
            int64_t n_ops = rd.idx(0, 64);
            for (int64_t i = 0; i < n_ops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, ctx, srf, t, last, o);
            }
            o.push_back(t.Direction());
        }
        delete srf;
        if (rd.at != rd.n)
            return SC_ERR_ARG;
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
