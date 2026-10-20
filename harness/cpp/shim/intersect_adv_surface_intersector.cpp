// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.adv_surface_intersector`: the SmAdvSurfaceIntersector
// members (source/SMLib/src/SmAdvSurfaceIntersector.cpp, source/SMLib/inc/SmAdvSurfaceIntersector.h)
// run on a real SmAdvSurfaceIntersector over SmPlane / SmBSplineSurface objects. A derived class
// reaches the protected state; the private state (m_dClosestValue, m_sUVS, m_apNodes,
// m_bSeperateGaussMaps) is read through member pointers obtained by explicit template
// instantiation. The decomposition trees are built by hand in SmSurfaceCache objects of the
// two surfaces (the tree construction belongs to unit surface.surface_cache_p3); the solver
// trees m_apTrees are set to them, as SmSurfaceIntersector::Intersect does.
//
// One entry point, sc_intersect_adv_surface_intersector_run(in, n_in, out, cap, out_len). `in`
// is a program (layout in crates/solidcore-ffi/src/units/intersect_adv_surface_intersector.rs);
// every read is bounds checked here (a bad program is SC_ERR_ARG). Inputs upstream would crash
// on are not run: a tree whose leaves are not Bezier patches, a patch or head node without a
// polar box, a LocalSolve on a node that is not a patch, a point linked twice, and a
// ComputeSingularityPoint that would find more than 8 singular directions (upstream writes
// past aSingDirections[8]; the count is predicted by `sing_overflow` below, which repeats the
// upstream steps up to that point).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAdvSurfaceIntersector.h>
#include <SmBSplineSurface.h>
#include <SmConic.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTList.h>
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
// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_adv_surface_intersector.rs).
enum : int32_t
{
    OP_STATE = 1,
    OP_SET = 2,
    OP_PUSH = 3,
    OP_SING = 4,
    OP_DUPIN = 5,
    OP_SOLVE = 6,
    OP_BEST_SUB = 7,
    OP_GAUSS = 8,
    OP_LOCAL = 9,
    OP_INTERIOR = 10,
    OP_SOLVE_BRANCH = 11,
    OP_LAST = 11,
};

// Op status of an op the shim does not run (upstream would crash).
const double REJECTED = -2.0;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

// ---- private member access (explicit instantiation may name private members) ----------
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
struct TagNodes
{
    typedef SmTreeNode* (SmAdvSurfaceIntersector::*type)[2];
    friend type get(TagNodes);
};
struct TagSep
{
    typedef SmBoolean SmAdvSurfaceIntersector::*type;
    friend type get(TagSep);
};
template struct Rob<TagClosest, &SmAdvSurfaceIntersector::m_dClosestValue>;
template struct Rob<TagUVS, &SmAdvSurfaceIntersector::m_sUVS>;
template struct Rob<TagNodes, &SmAdvSurfaceIntersector::m_apNodes>;
template struct Rob<TagSep, &SmAdvSurfaceIntersector::m_bSeperateGaussMaps>;
struct TagCType
{
    typedef SmConicType SmConic::*type;
    friend type get(TagCType);
};
struct TagCA
{
    typedef double SmConic::*type;
    friend type get(TagCA);
};
struct TagCB
{
    typedef double SmConic::*type;
    friend type get(TagCB);
};
struct TagCOrigin
{
    typedef SmPoint2d SmConic::*type;
    friend type get(TagCOrigin);
};
struct TagCTheta
{
    typedef double SmConic::*type;
    friend type get(TagCTheta);
};
template struct Rob<TagCType, &SmConic::m_eConicType>;
template struct Rob<TagCA, &SmConic::m_dA>;
template struct Rob<TagCB, &SmConic::m_dB>;
template struct Rob<TagCOrigin, &SmConic::m_vOrigin>;
template struct Rob<TagCTheta, &SmConic::m_dThetaRad>;

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

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

// `0 o x y scale dom` (SmPlane) or `1 p q nu nv net(x y z w) u v` (SmBSplineSurface).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
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

// A point: `type ptype[4] uv0 duv0 uv1 duv1 srf0[3][3] srf1[3][3] crv_pos crv_deriv param
// angle deviation user[4]` (80 values).
void read_point(Rd& rd, SmTsectPnt& t)
{
    t.m_ePointType = (SmIntersectionPointType)rd.idx(0, 7);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_eUVParamType[s][d] = (SmExtentPointType)rd.idx(0, 5);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_vUVCurvePV[s][d] = rd.uv();
    for (int s = 0; s < 2; s++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                t.m_vSurfacePV[s][i][j] = rd.vec();
    t.m_v3DCurvePV[0] = rd.vec();
    t.m_v3DCurvePV[1] = rd.vec();
    t.m_dCurveParameter = rd.num();
    t.m_dTangentPlaneAngleRad = rd.num();
    t.m_dDeviation = rd.num();
    for (int i = 0; i < SM_MAX_USER_DOUBLES; i++)
        t.m_adUserDoubles[i] = rd.num();
}

void put_point(std::vector<double>& o, const SmTsectPnt& t)
{
    o.push_back((double)t.m_ePointType);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            o.push_back((double)t.m_eUVParamType[s][d]);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            put_uv(o, t.m_vUVCurvePV[s][d]);
    for (int s = 0; s < 2; s++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                put_vec(o, t.m_vSurfacePV[s][i][j]);
    put_vec(o, t.m_v3DCurvePV[0]);
    put_vec(o, t.m_v3DCurvePV[1]);
    o.push_back(t.m_dCurveParameter);
    o.push_back(t.m_dTangentPlaneAngleRad);
    o.push_back(t.m_dDeviation);
    for (int i = 0; i < SM_MAX_USER_DOUBLES; i++)
        o.push_back(t.m_adUserDoubles[i]);
}

void put_conic(std::vector<double>& o, const SmConic& c)
{
    o.push_back((double)(c.*get(TagCType())));
    o.push_back(c.*get(TagCA()));
    o.push_back(c.*get(TagCB()));
    put_uv(o, c.*get(TagCOrigin()));
    o.push_back(c.*get(TagCTheta()));
}

// ---- hand-built decomposition trees ----------------------------------------------------

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

// Tree: `n` then per node `parent c1 c2 aux bbox[6] dom[4] npolar polar_vecs[3 npolar]` and,
// for aux 3 (SM_AD_BEZIER_SURFACE), `pseudo_basis[9] pseudo_intervals[6]`. Node 0 is the top
// (parent -1); children are both -1 or two distinct later nodes whose parent is the node; aux is
// 3, 4 (SM_AD_BEZIER_SURFACE_HEAD) or 6 (SM_AD_AUX_DATA); leaves are patches; patches and heads
// have at least one polar box vector.
struct Tree
{
    CachePeek* cache = nullptr;
    std::vector<SmTreeNode*> nodes;
    SmTreeNode* node(int64_t i) const { return nodes[(size_t)i]; }
    int64_t index_of(const SmTreeNode* p) const
    {
        if (p == nullptr)
            return -1;
        for (size_t i = 0; i < nodes.size(); i++)
            if (nodes[i] == p)
                return (int64_t)i;
        return -2;
    }
};

void read_tree(Rd& rd, Tree& t, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx(1, 64);
    CachePeek* cache = new (ctx) CachePeek(*s, s->GetNaturalUVDomain());
    t.cache = cache;
    TreePeek* tree = new (ctx) TreePeek(cache);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
    cache->aux().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    cache->tree() = tree;
    for (int64_t k = 0; k < n; k++)
        t.nodes.push_back((SmTreeNode*)tree->node_mgr().GetNewElement());
    std::vector<int64_t> par((size_t)n), c1((size_t)n), c2((size_t)n);
    for (int64_t k = 0; k < n; k++)
    {
        SmTreeNode* nd = t.node(k);
        par[(size_t)k] = rd.idx(-1, n - 1);
        c1[(size_t)k] = rd.idx(-1, n - 1);
        c2[(size_t)k] = rd.idx(-1, n - 1);
        int64_t aux = rd.idx(3, 6);
        if (aux == 5)
            throw Bad();
        nd->m_pTree = tree;
        nd->m_pParent = par[(size_t)k] < 0 ? nullptr : t.node(par[(size_t)k]);
        nd->m_pChild1 = c1[(size_t)k] < 0 ? nullptr : t.node(c1[(size_t)k]);
        nd->m_pChild2 = c2[(size_t)k] < 0 ? nullptr : t.node(c2[(size_t)k]);
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
        // (GetPolarBox() would build the box from the surface: add to the stored one)
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
        if (a < 0 && t.node(k)->m_eAuxDataType != SM_AD_BEZIER_SURFACE)
            throw Bad();
    }
    tree->top() = t.node(0);
}

// The intersector with its protected state reachable.
struct Peek : SmAdvSurfaceIntersector
{
    Peek(const SmSurface& a, const SmExtent2d& da, const SmSurface& b, const SmExtent2d& db, SmBoolean f)
        : SmAdvSurfaceIntersector(a, da, b, db, f)
    {
    }
    void set_trees(SmTree* t0, SmTree* t1)
    {
        m_apTrees[0] = t0;
        m_apTrees[1] = t1;
    }
    SmTList<SmTsectPnt>& curve_points() { return m_vCurvePoints; }
    const SmExtent2d& domain(int i) const { return m_vUVDomain[i]; }
    double trace_dir() const { return m_dCurveTraceDirection; }
    void set(double dir, double ang, double approx, double tol, int64_t op)
    {
        m_dCurveTraceDirection = dir;
        m_dThisAngTolRad = ang;
        m_dThisApproxTol3d = approx;
        m_d3dTolerance = tol;
        m_eSolverOperation = (SmSolverOperationType)op;
    }
    double ang_tol() const { return m_dThisAngTolRad; }
    double approx_tol() const { return m_dThisApproxTol3d; }
    double tol3d() const { return m_d3dTolerance; }
    int64_t op() const { return (int64_t)m_eSolverOperation; }
    SmTsectPnt* last_node() const { return m_vCurvePoints.GetLastNode(); }
    SmBoolean self_isect() const { return m_bDoingSelfIntersection; }
};

// True when ComputeSingularityPoint(t, prev) would add a 9th singular direction. Repeats the
// upstream steps that lead there (non-planar points, no early return, every status checked)
// on copies; any other outcome is false.
bool sing_overflow(const Peek& si, const SmTsectPnt& t0)
{
    SmTsectPnt t = t0;
    SmLocalSurfaceType e1 = smsurf_ComputeSurfacePointType(t.SrfNorm(0), t.SrfDuu(0), t.SrfDvv(0));
    SmLocalSurfaceType e2 = smsurf_ComputeSurfacePointType(t.SrfNorm(1), t.SrfDuu(1), t.SrfDvv(1));
    if (e1 == SM_LS_PLANAR || e2 == SM_LS_PLANAR)
        return false;
    SmTsectPnt& c = t;
    SmTArray<SmConic*> a1, b1, a2, b2;
    SmObjsDelete<SmConic*> d1(&a1), d2(&b1), d3(&a2), d4(&b2);
    double k1 = 0, k2 = 0, k1_2 = 0, k2_2 = 0;
    SmVector3d v1, v2, v1_2, v2_2, efg, lmn;
    if (si.MakeDupin(c.m_vSurfacePV[0], c.SrfNorm(0), a1, b1, v1, v2, k1, k2, efg, lmn) != SM_SUCCESS)
        return false;
    if (si.MakeDupin(c.m_vSurfacePV[1], c.SrfNorm(0), a2, b2, v1_2, v2_2, k1_2, k2_2, efg, lmn) != SM_SUCCESS)
        return false;
    const double Z = SM_EFF_ZERO_SQRT;
    if (smos_Fabs(k1 - k2) < SM_EFF_ZERO * (1.0 + smos_Fabs(k1)))
    {
        if (smos_Fabs(k1 - k1_2) < Z || smos_Fabs(k2 - k2_2) < Z)
            return false;
    }
    if (smos_Fabs(k1_2 - k2_2) < SM_EFF_ZERO * (1.0 + smos_Fabs(k1_2)))
    {
        if (smos_Fabs(k1 - k1_2) < Z || smos_Fabs(k2 - k2_2) < Z)
            return false;
    }
    if ((v1 * v1_2).Length() < Z * 100.0)
    {
        if (smos_Fabs(k1 - k1_2) < Z || smos_Fabs(k2 - k2_2) < Z)
            return false;
    }
    if ((v1 * v2_2).Length() < Z * 100.0)
    {
        if (smos_Fabs(k1 - k2_2) < Z || smos_Fabs(k2 - k1_2) < Z)
            return false;
    }
    double ang = 0.0;
    if (c.SrfNorm(0).CCWAngleBetween(v1, v1_2, ang) != SM_SUCCESS)
        return false;
    for (ULONG i = 0; i < a2.GetSize(); i++)
        if (a2[i]->Transform2D(ang, SmPoint2d(0, 0)) != SM_SUCCESS)
            return false;
    for (ULONG i = 0; i < b2.GetSize(); i++)
        if (b2[i]->Transform2D(ang, SmPoint2d(0, 0)) != SM_SUCCESS)
            return false;
    std::vector<SmVector3d> dirs;
    SmSolutionArray sols;
    for (int side = 0; side < 2; side++)
    {
        SmTArray<SmConic*>& c1 = side == 0 ? a1 : b1;
        SmTArray<SmConic*>& c2 = side == 0 ? a2 : b2;
        for (ULONG i = 0; i < c1.GetSize(); i++)
            for (ULONG j = 0; j < c2.GetSize(); j++)
            {
                if (c1[i]->IntersectConic(c1[i]->GetNaturalInterval(), *c2[j], c2[j]->GetNaturalInterval(),
                                          Z * 100.0, sols) != SM_SUCCESS)
                    return false;
                for (ULONG k = 0; k < sols.GetSize(); k++)
                {
                    SmPoint3d p;
                    if (c1[i]->EvaluatePoint(sols[k].m_vStart[0], p) != SM_SUCCESS)
                        return false;
                    if (p.Unitize() != SM_SUCCESS)
                        return false;
                    SmVector3d d = p.x * v1 + p.y * v2;
                    if (d.Unitize() != SM_SUCCESS)
                        return false;
                    bool found = false;
                    for (const SmVector3d& e : dirs)
                        if (e.Dot(d) > 0.9)
                        {
                            found = true;
                            break;
                        }
                    if (!found)
                    {
                        if (dirs.size() >= 8)
                            return true;
                        dirs.push_back(d);
                    }
                }
            }
    }
    return false;
}

struct Prog
{
    std::vector<SmTsectPnt*> pts;
    Tree trees[2];
    bool has_trees = false;
    bool same = false;
    SmSolutionArray sols; // the solution array of OP_LOCAL / OP_SOLVE_BRANCH
    ~Prog()
    {
        for (SmTsectPnt* p : pts)
            delete p;
        delete trees[0].cache;
        if (!same)
            delete trees[1].cache;
    }
    SmTsectPnt* pt(Rd& rd)
    {
        int64_t i = rd.idx(0, (int64_t)pts.size() - 1);
        return pts[(size_t)i];
    }
    static bool linked(const SmTsectPnt* p) { return p->m_pNext != NULL || p->m_pPrev != NULL; }
    const Tree& tree(int i) const { return trees[i]; }
};

void put_solutions(std::vector<double>& o, const Prog& pr, SmSolutionArray& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
    {
        SmSolution& s = a[i];
        o.push_back((double)s.m_eSolutionType);
        o.push_back((double)s.m_lNumVariables);
        o.push_back(s.m_vStart.m_dSolutionValue);
        for (int k = 0; k < SM_SA_MAX_TREES * 2; k++)
            o.push_back(s.m_vStart[k]);
        o.push_back(s.m_vEnd.m_dSolutionValue);
        for (int k = 0; k < SM_SA_MAX_TREES * 2; k++)
            o.push_back(s.m_vEnd[k]);
        o.push_back((double)pr.tree(0).index_of(s.m_apNodes[0]));
        o.push_back((double)pr.tree(1).index_of(s.m_apNodes[1]));
    }
}

void put_state(std::vector<double>& o, const Prog& pr, Peek& si)
{
    o.push_back(si.*get(TagClosest()));
    put_uv(o, (si.*get(TagUVS()))[0]);
    put_uv(o, (si.*get(TagUVS()))[1]);
    o.push_back((double)pr.tree(0).index_of((si.*get(TagNodes()))[0]));
    o.push_back((double)pr.tree(1).index_of((si.*get(TagNodes()))[1]));
    o.push_back(flg(si.*get(TagSep())));
    o.push_back(si.trace_dir());
    o.push_back(si.ang_tol());
    o.push_back(si.approx_tol());
    o.push_back(si.tol3d());
    o.push_back((double)si.op());
    SmTArray<SmTsectPnt*> pts;
    si.curve_points().GetAllNodes(pts);
    o.push_back((double)pts.GetSize());
}

void read_branch(Rd& rd, const Prog& pr, SmTreeNode* b[SM_GS_MAX_TREES])
{
    if (!pr.has_trees)
        throw Bad();
    b[0] = pr.tree(0).node(rd.idx(0, (int64_t)pr.tree(0).nodes.size() - 1));
    b[1] = pr.tree(1).node(rd.idx(0, (int64_t)pr.tree(1).nodes.size() - 1));
    b[2] = nullptr;
}

void run_op(int32_t op, Rd& rd, Prog& pr, Peek& si, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        put_state(o, pr, si);
        return;
    case OP_SET:
    {
        double dir = rd.num();
        double ang = rd.num();
        double approx = rd.num();
        double tol = rd.num();
        int64_t sop = rd.idx(0, 3);
        if (sop == 1)
            throw Bad();
        si.set(dir, ang, approx, tol, sop);
        o.push_back(0.0);
        return;
    }
    case OP_PUSH:
    {
        SmTsectPnt* p = pr.pt(rd);
        if (Prog::linked(p))
        {
            o.push_back(REJECTED);
            return;
        }
        o.push_back(st(si.AddPointToCurve(*p)));
        return;
    }
    case OP_SING:
    {
        SmTsectPnt* p = pr.pt(rd);
        bool has_prev = rd.flag();
        SmTsectPnt* prev = pr.pt(rd);
        if (sing_overflow(si, *p))
        {
            o.push_back(REJECTED);
            return;
        }
        SmPoint2d uvs[2];
        uvs[0] = p->UVPos(0);
        uvs[1] = p->UVPos(1);
        o.push_back(st(si.ComputeSingularityPoint(uvs, *p, has_prev ? prev : NULL)));
        put_point(o, *p);
        return;
    }
    case OP_DUPIN:
    {
        SmVector3d m[3][3];
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                m[i][j] = rd.vec();
        SmVector3d nrm = rd.vec();
        SmTArray<SmConic*> above, below;
        SmObjsDelete<SmConic*> d1(&above), d2(&below);
        SmVector3d v1, v2, efg, lmn;
        double k1 = 0.0, k2 = 0.0;
        SmStatus s = si.MakeDupin(m, nrm, above, below, v1, v2, k1, k2, efg, lmn);
        o.push_back(st(s));
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                put_vec(o, m[i][j]);
        if (s != SM_SUCCESS)
            return;
        put_vec(o, v1);
        put_vec(o, v2);
        o.push_back(k1);
        o.push_back(k2);
        put_vec(o, efg);
        put_vec(o, lmn);
        o.push_back((double)above.GetSize());
        for (ULONG i = 0; i < above.GetSize(); i++)
            put_conic(o, *above[i]);
        o.push_back((double)below.GetSize());
        for (ULONG i = 0; i < below.GetSize(); i++)
            put_conic(o, *below[i]);
        return;
    }
    case OP_SOLVE:
    {
        if (!pr.has_trees)
            throw Bad();
        int64_t sop = rd.idx(0, 3);
        if (sop == 1)
            throw Bad();
        int64_t req = rd.idx(0, 3);
        double tol = rd.num();
        double best = rd.num();
        SmSolutionArray a;
        SmStatus s = si.SolveIt((SmSolverOperationType)sop, (SmSolutionRequestedType)req, tol, best, NULL, a);
        o.push_back(st(s));
        put_solutions(o, pr, a);
        return;
    }
    case OP_BEST_SUB:
    {
        SmTreeNode* b[SM_GS_MAX_TREES];
        read_branch(rd, pr, b);
        ULONG idx = 99;
        SmBoolean swap = FALSE;
        SmStatus s = si.FindBestSubdivisionNode(b, idx, swap);
        o.push_back(st(s));
        o.push_back((double)idx);
        o.push_back(flg(swap));
        return;
    }
    case OP_GAUSS:
    {
        SmTreeNode* b[SM_GS_MAX_TREES];
        read_branch(rd, pr, b);
        SmBoolean sep = FALSE;
        SmStatus s = si.BranchIsGaussSeparate(b, sep);
        o.push_back(st(s));
        o.push_back(flg(sep));
        return;
    }
    case OP_LOCAL:
    {
        SmTreeNode* b[SM_GS_MAX_TREES];
        read_branch(rd, pr, b);
        if (b[0]->m_eAuxDataType != SM_AD_BEZIER_SURFACE || b[1]->m_eAuxDataType != SM_AD_BEZIER_SURFACE)
        {
            o.push_back(REJECTED);
            return;
        }
        si.SetSolutions(&pr.sols);
        SmBoolean more = TRUE;
        SmStatus s = si.LocalSolve(b, more);
        si.SetSolutions(NULL);
        o.push_back(st(s));
        o.push_back(flg(more));
        put_solutions(o, pr, pr.sols);
        put_state(o, pr, si);
        return;
    }
    case OP_INTERIOR:
    {
        if (!pr.has_trees)
            throw Bad();
        SmTArray<SmCurve*> c3, u1, u2;
        SmTArray<SmTsectCurveType> types;
        SmTArray<double> devs;
        SmStatus s = si.FindInteriorCurves(c3, u1, u2, types, devs);
        o.push_back(st(s));
        o.push_back((double)c3.GetSize());
        for (ULONG i = 0; i < c3.GetSize(); i++)
            delete c3[i];
        for (ULONG i = 0; i < u1.GetSize(); i++)
            delete u1[i];
        for (ULONG i = 0; i < u2.GetSize(); i++)
            delete u2[i];
        put_state(o, pr, si);
        return;
    }
    case OP_SOLVE_BRANCH:
    {
        SmTreeNode* b[SM_GS_MAX_TREES];
        read_branch(rd, pr, b);
        si.SetSolutions(&pr.sols);
        SmStatus s = si.SolveBranch(b);
        si.SetSolutions(NULL);
        o.push_back(st(s));
        put_solutions(o, pr, pr.sols);
        put_state(o, pr, si);
        return;
    }
    default:
        throw Bad();
    }
}
} // namespace

// Program: `surfA surfB` (surfB `2` = the same object as A), `dom1[4] dom2[4] fillet`, `trees`
// (0, or 1 then tree A and, unless B is A, tree B), `npoints points`, `nops ops`. Output: per op
// `op status [values]`, then the final state.
SC_EXPORT int32_t sc_intersect_adv_surface_intersector_run(const double* in, int64_t n_in, double* out,
                                                           int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SC_NATIVE_LOCK;
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* a = read_surface(rd, ctx);
        SmSurface* b = NULL;
        bool same = false;
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
        bool fillet = rd.flag();
        {
            Prog pr;
            pr.same = same;
            if (rd.flag())
            {
                pr.has_trees = true;
                read_tree(rd, pr.trees[0], a, ctx);
                if (same)
                    pr.trees[1] = pr.trees[0];
                else
                    read_tree(rd, pr.trees[1], b, ctx);
            }
            int64_t np = rd.idx(0, 64);
            for (int64_t i = 0; i < np; i++)
            {
                SmTsectPnt* t = new SmTsectPnt();
                pr.pts.push_back(t);
                read_point(rd, *t);
            }
            int64_t nops = rd.idx(0, 64);
            {
                Peek si(*a, d1, *b, d2, fillet ? TRUE : FALSE);
                if (pr.has_trees)
                    si.set_trees(pr.trees[0].cache->tree(), pr.trees[1].cache->tree());
                for (int64_t i = 0; i < nops; i++)
                {
                    int32_t op = (int32_t)rd.idx(1, OP_LAST);
                    o.push_back((double)op);
                    run_op(op, rd, pr, si, o);
                }
                put_state(o, pr, si);
                // unlink every point before the lists go away
                for (SmTsectPnt* t : pr.pts)
                    t->ReSet();
            }
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
