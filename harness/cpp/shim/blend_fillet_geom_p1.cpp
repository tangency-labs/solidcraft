// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_geom_p1`: the SmFilletVertexuse / SmFilletVertex /
// SmFilletEdgeuse / SmFilletEdge / SmFilletGeom / SmFilletBrep members owned by the unit and the
// file-static helpers of upstream source/SMLib/src/SmFilletGeom.cpp (sm_CleanupNoise,
// sm_CheckSeamOut, sm_CheckSeamOn, sm_SetSurfaceClosure, sm_IsOrderCorrect, sm_AdjustUVOnSeam,
// sm_InterpolateEndptsAndTangents, sm_SurfaceDropPoint, sm_TwoPntsIntersection, sm_EvaluateMate,
// JoinVertsByCapping, sm_ClassifyFacePoint, sm_ExamineNFixFilletGeoms).
//
// Static functions cannot be linked against, so this translation unit compiles the upstream
// source file itself (same flags as the kernel), at global scope: every header the file includes
// is included first, so its own #includes are no-ops. The file also defines the SmFillet* class
// members and a few global functions; those definitions are identical to the ones in
// libSMLib.a's SmFilletGeom.o. The shim archive precedes libSMLib.a on the link line and this
// object is pulled in by the Rust binding, so every SmFilletGeom symbol is already defined when
// the library archive is scanned and SmFilletGeom.o is never extracted (no duplicates).
//
// One entry point, sc_blend_fillet_geom_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/blend_fillet_geom_p1.rs, which validates every
// input; reads are bounds checked here too (a short program is SC_ERR_ARG). `out` receives the
// op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
//
// Pseudo-B-rep ops build an SmFilletBrep from a block: a target SmBrep of wire edges and faces
// (the objects fillet vertices classify to), a "fillet B-rep" of wire edges with given edge
// tolerances (FindFilletBrepEdge's candidates), fillet vertices (new (pseudo) SmFilletVertex, SetPoint, types, classification),
// fillet geometries (SmFilletGeom(NULL, type), centre line, surface), fillet edges
// (SmFilletEdge::MakeFilletEdge between two fillet vertices, type, curve, interval, fillet
// B-rep edges, each edge use's UV curve and fillet geometry, each vertex use's UV point), the
// geometries' rails / other rails / side edge uses and the vertices' mates. Vertices are indexed
// in creation order (the fillet corner keeps them, not the B-rep), edges in the pseudo B-rep's
// edge list order, geometries in creation order; a vertex use is (edge, 0 = primary edge use /
// 1 = its mate). The export reads raw members (protected ones through pointer-to-member
// access), never the functions under test.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmAttribute.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFilletCorner.h>
#include <SmFilletGeom.h>
#include <SmFilletIntersector.h>
#include <SmFilletSolver.h>
#include <SmGeomUtility.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSolutionArray.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <SmTopologyTraverser.h>
#include <SmTrimmingTools.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>

#include "../../../upstream/source/SMLib/src/SmFilletGeom.cpp"

#include <cstdint>
#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_geom_p1.rs).
enum : int32_t
{
    OP_CLEANUP_NOISE = 0,
    OP_CHECK_SEAM_OUT = 1,
    OP_CHECK_SEAM_ON = 2,
    OP_SURFACE_CLOSURE = 3,
    OP_IS_ORDER_CORRECT = 4,
    OP_ADJUST_UV_ON_SEAM = 5,
    OP_INTERPOLATE = 6,
    OP_SURFACE_DROP_POINT = 7,
    OP_TWO_PNTS = 8,
    OP_EVALUATE_MATE = 9,
    OP_CAPPING = 10,
    OP_CLASSIFY_FACE_POINT = 11,
    OP_EVALUATE_MATE_ANALYTIC = 12,
    OP_FB_EXPORT = 20,
    OP_FB_VERTEX_GEOMS = 21,
    OP_FB_VU_AT_RAIL_END = 22,
    OP_FB_RAIL_VERTEXUSE = 23,
    OP_FB_EDGE_NAME = 24,
    OP_FB_FIND_EDGE = 25,
    OP_FB_CLEAR_EDGES = 26,
    OP_FB_TRIM = 27,
    OP_FB_DELETE_VERTEX = 28,
    OP_FB_DELETE_EDGE = 29,
    OP_FB_VERT_ON_VERT = 30,
    OP_FB_CLIFF = 31,
    OP_FB_SPLIT_FACE = 32,
    OP_FB_EXAMINE = 33,
    OP_FB_SET_EU_GEOM = 34,
    OP_FB_SET_UV_CURVE = 35,
    OP_FB_ORIG_UV_CURVE = 36,
    OP_FB_SET_VU_PROPERTY = 37,
    OP_FB_BREP_EDGES = 38,
    OP_FB_REMOVE_EDGE = 39,
};

struct Rd
{
    const double* in;
    int64_t at;
    int64_t n;
    bool bad;
    // The op builds a surface cache from the surfaces it reads (UB-166 guard below).
    bool cache_op;
    Rd(const double* i, int64_t len) : in(i), at(0), n(len), bad(false), cache_op(false) {}
    double num()
    {
        if (at >= n)
        {
            bad = true;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    bool flag() { return num() != 0.0; }
    SmPoint3d pt3()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmPoint3d(x, y, z);
    }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double u = num();
        double v = num();
        return SmPoint2d(u, v);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(bool b) { return b ? 1.0 : 0.0; }

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface. Sizes are validated
// by the Rust wrapper; a short program leaves rd.bad set and yields a default surface.
SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    if (rd.bad || n < 0 || m < 0 || r < 0 || s < 0 || n > 4096 || m > 4096 || r > 8192 ||
        s > 8192)
    {
        rd.bad = true;
        n = m = 1;
        p = q = 1;
        r = s = 3;
    }
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
    SmBSplineSurface* srf = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    // UB-166: the constructor's sm_FixupKnotVector can raise the v start multiplicity
    // above q + 1 (snap or spread onto the start knot); sm_DecomposeSrf then writes past
    // its patch array when the surface cache is built. Read the knot vector the
    // constructor really left and refuse such a surface for the ops that build the
    // cache (in u the decomposition fails cleanly, and the other ops never decompose).
    // This refuses the precondition, not exactly the crashing runs: some paths return
    // before the cache for this surface is built (measured 28 of 169 such programs:
    // TwoPnts 1001 and Fb exits ahead of the cache fetch), and predicting them would
    // replicate upstream's control flow.
    if (rd.cache_op && !rd.bad)
    {
        const gw_SURFACE* g = srf->GetGwNurbPointer();
        if (g != NULL)
        {
            const gw_INDEX last = g->knv->m;
            gw_INDEX mult = 1;
            while (mult <= last && g->knv->U[mult] == g->knv->U[0])
                ++mult;
            if (mult > (gw_INDEX)g->q + 1)
                rd.bad = true;
        }
    }
    return srf;
}

// Reads `dim n p m cpts knots` into SmBSplineCurve(gw_CURVE*, dim, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    if (rd.bad || n < 0 || m < 0 || n > 4096 || m > 8192 || (dim != 2 && dim != 3))
    {
        rd.bad = true;
        dim = 3;
        n = 1;
        p = 1;
        m = 3;
    }
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

// `has dim n p m cpts knots` (has = 0 for a null or non-B-spline curve).
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    const SmBSplineCurve* bc =
        (c != NULL && c->IsKindOf(SmBSplineCurve_TYPE)) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = bc ? bc->GetGwNurbPointer() : NULL;
    o.push_back(flg(cur != NULL));
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)bc->GetDim());
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

// `has n m p q r s cpts u v` (has = 0 for a null or non-B-spline surface).
void put_surface(std::vector<double>& o, const SmSurface* s)
{
    const SmBSplineSurface* bs =
        (s != NULL && s->IsKindOf(SmBSplineSurface_TYPE)) ? (const SmBSplineSurface*)s : NULL;
    const gw_SURFACE* sur = bs ? bs->GetGwNurbPointer() : NULL;
    o.push_back(flg(sur != NULL));
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX ss = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)ss);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= ss; i++)
        o.push_back(sur->knv->U[i]);
}

void put_pt(std::vector<double>& o, const SmPoint3d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

void put_uv(std::vector<double>& o, const SmPoint2d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
}

// ---- raw member access (pointer-to-member through a derived class) ----------------------
struct GeomPeek : SmFilletGeom
{
    static SmTArray<SmFilletEdge*> SmFilletGeom::*other1() { return &GeomPeek::m_vOtherRails1; }
    static SmTArray<SmFilletEdge*> SmFilletGeom::*other2() { return &GeomPeek::m_vOtherRails2; }
    static SmTArray<SmFilletEdgeuse*> SmFilletGeom::*refs() { return &GeomPeek::m_vRefEUs; }
    static SmTArray<SmFilletEdgeuse*> SmFilletGeom::*sides() { return &GeomPeek::m_vSideEUs; }
};

// Builds a wire edge on a copy of `curve` over its natural interval.
SmEdge* wire_edge(SmBrep* b, SmBSplineCurve* curve)
{
    SmEdge* e = NULL;
    if (b->CreateWireEdgeFromCurve(curve, curve->GetNaturalInterval(), e) != SM_SUCCESS)
        return NULL;
    return e;
}

SmFace* face_of(SmBrep* b, SmSurface* surface)
{
    SmFace* f = NULL;
    if (b->CreateFaceFromSurface(surface, surface->GetNaturalUVDomain(), f) != SM_SUCCESS)
        return NULL;
    return f;
}

// A standalone vertex (shell vertex topology in the infinite region) at `p`.
SmVertex* lone_vertex(SmBrep* b, const SmPoint3d& p)
{
    SmShell* shell = NULL;
    SmVertex* v = NULL;
    if (b->MakeShellVertexTopology(b->GetInfiniteRegion(), shell, v) != SM_SUCCESS || v == NULL)
        return NULL;
    v->SetPoint(p);
    return v;
}

bool tstr_eq(const TCHAR* a, const TCHAR* b)
{
    while (*a != 0 && *b != 0)
    {
        if (*a != *b)
            return false;
        ++a;
        ++b;
    }
    return *a == 0 && *b == 0;
}

template <class T> int64_t index_of(const std::vector<T*>& v, const T* x)
{
    for (size_t i = 0; i < v.size(); ++i)
        if (v[i] == x)
            return (int64_t)i;
    return -1;
}

// The pseudo B-rep and everything it refers to.
struct Model
{
    const SmContext& ctx;
    SmBrep* target;
    SmBrep* filletBrep;
    SmFilletBrep* pseudo;
    std::vector<SmEdge*> targetEdges;
    std::vector<SmFace*> targetFaces;
    std::vector<SmEdge*> filletBrepEdges;
    std::vector<SmFilletVertex*> verts;
    std::vector<SmFilletGeom*> geoms;
    std::vector<SmSurface*> geomSurfaces; // owned here: SmFilletGeom does not delete its surface
    bool ok;

    explicit Model(const SmContext& c)
        : ctx(c), target(NULL), filletBrep(NULL), pseudo(NULL), ok(true)
    {
    }

    ~Model()
    {
        // edges first (their destructors unlink their uses from the vertices), then the
        // pseudo B-rep, the vertices, the geometries and their surfaces
        if (pseudo)
        {
            SmTArray<SmEdge*> edges;
            pseudo->GetEdges(edges);
            for (ULONG i = 0; i < edges.GetSize(); i++)
                delete (SmFilletEdge*)edges[i];
            delete pseudo;
        }
        // ~SmFilletVertex unlinks itself from its mates, assuming upstream's symmetric
        // pairing; the model input may pair asymmetrically, which would leave a mate
        // pointing at an already-deleted vertex. Clear every mate first so no destructor
        // follows one.
        for (size_t i = 0; i < verts.size(); ++i)
        {
            verts[i]->SetMate(0, NULL);
            verts[i]->SetMate(1, NULL);
        }
        for (size_t i = 0; i < verts.size(); ++i)
            delete verts[i];
        for (size_t i = 0; i < geoms.size(); ++i)
            delete geoms[i];
        for (size_t i = 0; i < geomSurfaces.size(); ++i)
            delete geomSurfaces[i];
        delete target;
        delete filletBrep;
    }

    // edges in the pseudo B-rep's edge list order
    std::vector<SmFilletEdge*> edges() const
    {
        std::vector<SmFilletEdge*> out;
        SmTArray<SmEdge*> es;
        pseudo->GetEdges(es);
        for (ULONG i = 0; i < es.GetSize(); i++)
            out.push_back((SmFilletEdge*)es[i]);
        return out;
    }

    SmFilletEdge* edge(int64_t i) const
    {
        std::vector<SmFilletEdge*> es = edges();
        return (i >= 0 && (size_t)i < es.size()) ? es[(size_t)i] : NULL;
    }

    SmFilletVertex* vertex(int64_t i) const
    {
        return (i >= 0 && (size_t)i < verts.size()) ? verts[(size_t)i] : NULL;
    }

    SmFilletGeom* geom(int64_t i) const
    {
        return (i >= 0 && (size_t)i < geoms.size()) ? geoms[(size_t)i] : NULL;
    }

    static SmFilletEdgeuse* eu_of(SmFilletEdge* e, int64_t which)
    {
        if (e == NULL)
            return NULL;
        SmEdgeuse* eu = e->GetPrimaryEdgeuse();
        if (eu == NULL)
            return NULL;
        if (which == 1)
            eu = eu->GetMate();
        return (SmFilletEdgeuse*)eu;
    }

    SmFilletEdgeuse* edgeuse(int64_t e, int64_t which) const { return eu_of(edge(e), which); }

    // The classification object (kind, index, sub): 1 = target vertex (wire edge index, 0
    // start / 1 end), 2 = target edge use (wire edge index, primary), 3 = target face,
    // 4 = target edge (the SmEdge itself). Upstream pairs the classes with their objects:
    // SM_PC_VERTEX with a vertex, SM_PC_EDGE with an SmEdge (SmFilletSolver.cpp:7605-7619,
    // read back as SmEdge* by FindEdgeTangentAtPoint and SmFilletSolver.cpp:7665),
    // SM_PC_EDGEUSE with an SmEdgeuse (SM_FV_CLIFF_RAIL_X_EDGE, SmFilletSolver.cpp:7041-7042,
    // read back as SmEdgeuse* by CalcCliffRailIntEdge), SM_PC_FACE with a face; the class
    // is read independently of the kind, so a mismatched pair is a type confusion the ops
    // that read the object as another type must refuse.
    SmObject* pc_object(int64_t kind, int64_t index, int64_t sub) const
    {
        if (kind == 4 && index >= 0 && (size_t)index < targetEdges.size())
            return targetEdges[(size_t)index];
        if (kind == 1 && index >= 0 && (size_t)index < targetEdges.size())
        {
            SmVertex* vs = NULL;
            SmVertex* ve = NULL;
            targetEdges[(size_t)index]->GetVertices(vs, ve);
            return sub == 0 ? vs : ve;
        }
        if (kind == 2 && index >= 0 && (size_t)index < targetEdges.size())
            return targetEdges[(size_t)index]->GetPrimaryEdgeuse();
        if (kind == 3 && index >= 0 && (size_t)index < targetFaces.size())
            return targetFaces[(size_t)index];
        return NULL;
    }

    void put_pc_object(std::vector<double>& o, const SmObject* obj) const
    {
        if (obj != NULL)
        {
            for (size_t i = 0; i < targetEdges.size(); ++i)
            {
                SmVertex* vs = NULL;
                SmVertex* ve = NULL;
                targetEdges[i]->GetVertices(vs, ve);
                if (obj == vs || obj == ve)
                {
                    o.push_back(1.0);
                    o.push_back((double)i);
                    o.push_back(obj == vs ? 0.0 : 1.0);
                    return;
                }
                if (obj == targetEdges[i])
                {
                    o.push_back(4.0);
                    o.push_back((double)i);
                    o.push_back(0.0);
                    return;
                }
                if (obj == targetEdges[i]->GetPrimaryEdgeuse())
                {
                    o.push_back(2.0);
                    o.push_back((double)i);
                    o.push_back(0.0);
                    return;
                }
            }
            int64_t f = index_of(targetFaces, (const SmFace*)obj);
            if (f >= 0)
            {
                o.push_back(3.0);
                o.push_back((double)f);
                o.push_back(0.0);
                return;
            }
            o.push_back(-1.0); // an object not in the model (should not happen)
            o.push_back(-1.0);
            o.push_back(0.0);
            return;
        }
        o.push_back(0.0);
        o.push_back(-1.0);
        o.push_back(0.0);
    }

    static SmFilletCorner* corner_tag(int64_t i)
    {
        return i < 0 ? NULL : (SmFilletCorner*)(uintptr_t)(i + 1);
    }
    static double corner_index(const SmFilletCorner* c)
    {
        return c == NULL ? -1.0 : (double)((int64_t)(uintptr_t)c - 1);
    }

    // The (edge index, 0/1) of a vertex use's edge use, or (-1, -1).
    void put_vu(std::vector<double>& o, const SmVertexuse* vu) const
    {
        std::vector<SmFilletEdge*> es = edges();
        if (vu != NULL)
        {
            for (size_t i = 0; i < es.size(); ++i)
            {
                SmEdgeuse* p = es[i]->GetPrimaryEdgeuse();
                if (p != NULL && p->GetVertexuse() == vu)
                {
                    o.push_back((double)i);
                    o.push_back(0.0);
                    return;
                }
                if (p != NULL && p->GetMate() != NULL && p->GetMate()->GetVertexuse() == vu)
                {
                    o.push_back((double)i);
                    o.push_back(1.0);
                    return;
                }
            }
        }
        o.push_back(-1.0);
        o.push_back(-1.0);
    }

    void put_eu(std::vector<double>& o, const SmEdgeuse* eu) const
    {
        std::vector<SmFilletEdge*> es = edges();
        if (eu != NULL)
        {
            for (size_t i = 0; i < es.size(); ++i)
            {
                SmEdgeuse* p = es[i]->GetPrimaryEdgeuse();
                if (p == eu)
                {
                    o.push_back((double)i);
                    o.push_back(0.0);
                    return;
                }
                if (p != NULL && p->GetMate() == eu)
                {
                    o.push_back((double)i);
                    o.push_back(1.0);
                    return;
                }
            }
        }
        o.push_back(-1.0);
        o.push_back(-1.0);
    }

    double edge_index(const SmEdge* e) const
    {
        std::vector<SmFilletEdge*> es = edges();
        for (size_t i = 0; i < es.size(); ++i)
            if (es[i] == e)
                return (double)i;
        return -1.0;
    }

    double fillet_brep_edge_index(const SmEdge* e) const
    {
        return (double)index_of(filletBrepEdges, e);
    }

    // Reads the pseudo-B-rep block (layout in the Rust wrapper). Returns false on a bad program.
    bool build(Rd& rd)
    {
        target = new (ctx) SmBrep();
        filletBrep = new (ctx) SmBrep();
        pseudo = new (ctx) SmFilletBrep();

        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt && !rd.bad; ++i)
        {
            SmEdge* e = wire_edge(target, read_curve(rd, ctx));
            if (e == NULL)
                return false;
            targetEdges.push_back(e);
        }
        int64_t nf = rd.idx();
        for (int64_t i = 0; i < nf && !rd.bad; ++i)
        {
            SmFace* f = face_of(target, read_surface(rd, ctx));
            if (f == NULL)
                return false;
            targetFaces.push_back(f);
        }
        int64_t nfb = rd.idx();
        for (int64_t i = 0; i < nfb && !rd.bad; ++i)
        {
            SmEdge* e = wire_edge(filletBrep, read_curve(rd, ctx));
            double tol = rd.num();
            if (e == NULL)
                return false;
            e->SetTolerance(tol, FALSE, FALSE);
            filletBrepEdges.push_back(e);
        }

        int64_t nv = rd.idx();
        for (int64_t i = 0; i < nv && !rd.bad; ++i)
        {
            SmPoint3d p = rd.pt3();
            int64_t vtype = rd.idx();
            int64_t status = rd.idx();
            int64_t corner = rd.idx();
            int64_t pc_class = rd.idx();
            int64_t pc_kind = rd.idx();
            int64_t pc_index = rd.idx();
            int64_t pc_sub = rd.idx();
            int64_t pc_face = rd.idx();
            double t = rd.num();
            SmPoint2d uv = rd.uv();
            SmFilletVertex* fv = new (pseudo) SmFilletVertex(corner_tag(corner));
            fv->SetPoint(p);
            fv->SetFilletVertexType((SmFilletVertexType)vtype);
            fv->SetStatus((SmFilStatus)status);
            fv->SetPointClass((SmPointClassificationType)pc_class,
                              pc_object(pc_kind, pc_index, pc_sub));
            if (pc_face >= 0 && (size_t)pc_face < targetFaces.size())
                fv->GetPointClassification().SetFace(targetFaces[(size_t)pc_face]);
            fv->SetOriginalTParam(t);
            fv->SetOriginalUV(uv);
            verts.push_back(fv);
        }

        int64_t ng = rd.idx();
        for (int64_t i = 0; i < ng && !rd.bad; ++i)
        {
            int64_t gtype = rd.idx();
            int64_t status = rd.idx();
            SmFilletGeom* g = new SmFilletGeom(NULL, (SmFilletGeomType)gtype);
            g->SetStatus((SmFilStatus)status);
            if (rd.flag())
                g->SetCenterLineCurve(read_curve(rd, ctx));
            if (rd.flag())
            {
                SmSurface* s = read_surface(rd, ctx);
                geomSurfaces.push_back(s);
                g->SetFilletSurface(s);
            }
            geoms.push_back(g);
        }

        int64_t ne = rd.idx();
        for (int64_t i = 0; i < ne && !rd.bad; ++i)
        {
            SmFilletVertex* vs = vertex(rd.idx());
            SmFilletVertex* ve = vertex(rd.idx());
            int64_t etype = rd.idx();
            int64_t status = rd.idx();
            int64_t corner = rd.idx();
            if (vs == NULL || ve == NULL)
                return false;
            SmFilletEdge* e = NULL;
            if (SmFilletEdge::MakeFilletEdge(pseudo, vs, ve, e, corner_tag(corner)) != SM_SUCCESS ||
                e == NULL)
                return false;
            e->SetFilletEdgeType((SmFilletEdgeType)etype);
            e->SetStatus((SmFilStatus)status);
            if (rd.flag())
                e->SetCurve(read_curve(rd, ctx), TRUE);
            if (rd.flag())
            {
                double t0 = rd.num();
                double t1 = rd.num();
                e->SetInterval(SmExtent1d(t0, t1));
            }
            int64_t fbe1 = rd.idx();
            int64_t fbe2 = rd.idx();
            e->m_pFilletBrepEdge1 =
                (fbe1 >= 0 && (size_t)fbe1 < filletBrepEdges.size()) ? filletBrepEdges[(size_t)fbe1] : NULL;
            e->m_pFilletBrepEdge2 =
                (fbe2 >= 0 && (size_t)fbe2 < filletBrepEdges.size()) ? filletBrepEdges[(size_t)fbe2] : NULL;
            for (int64_t which = 0; which < 2; ++which)
            {
                SmFilletEdgeuse* eu = eu_of(e, which);
                if (eu == NULL)
                    return false;
                if (rd.flag())
                    eu->SetUVCurve(read_curve(rd, ctx));
                int64_t g = rd.idx();
                if (g >= 0)
                {
                    SmFilletGeom* pg = geom(g);
                    if (pg == NULL)
                        return false;
                    eu->SetFilletGeom(pg);
                }
            }
            for (int64_t which = 0; which < 2; ++which)
            {
                SmFilletEdgeuse* eu = eu_of(e, which);
                SmPoint2d uv = rd.uv();
                ((SmFilletVertexuse*)eu->GetVertexuse())->SetUVPoint(uv);
            }
        }

        for (int64_t i = 0; i < ng && !rd.bad; ++i)
        {
            SmFilletGeom* g = geoms[(size_t)i];
            for (int64_t r = 0; r < 2; ++r)
            {
                int64_t ei = rd.idx();
                g->SetRail((ULONG)r, ei < 0 ? NULL : edge(ei));
            }
            int64_t n1 = rd.idx();
            for (int64_t k = 0; k < n1 && !rd.bad; ++k)
            {
                SmFilletEdge* e = edge(rd.idx());
                if (e == NULL)
                    return false;
                (g->*GeomPeek::other1()).Add(e);
            }
            int64_t n2 = rd.idx();
            for (int64_t k = 0; k < n2 && !rd.bad; ++k)
            {
                SmFilletEdge* e = edge(rd.idx());
                if (e == NULL)
                    return false;
                (g->*GeomPeek::other2()).Add(e);
            }
            int64_t ns = rd.idx();
            for (int64_t k = 0; k < ns && !rd.bad; ++k)
            {
                int64_t ei = rd.idx();
                int64_t which = rd.idx();
                SmFilletEdgeuse* eu = edgeuse(ei, which);
                if (eu == NULL)
                    return false;
                g->AddSideFilletEdgeuse(eu);
            }
        }

        for (int64_t i = 0; i < nv && !rd.bad; ++i)
        {
            SmFilletVertex* fv = verts[(size_t)i];
            for (int64_t m = 0; m < 2; ++m)
            {
                int64_t mi = rd.idx();
                fv->SetMate((ULONG)m, mi < 0 ? NULL : vertex(mi));
            }
        }
        return !rd.bad;
    }

    // The whole state (layout in the Rust wrapper).
    void put(std::vector<double>& o) const
    {
        std::vector<SmFilletEdge*> es = edges();

        o.push_back((double)verts.size());
        for (size_t i = 0; i < verts.size(); ++i)
        {
            SmFilletVertex* v = verts[i];
            put_pt(o, v->GetPoint());
            o.push_back((double)v->GetFilletVertexType());
            o.push_back((double)v->GetStatus());
            for (ULONG m = 0; m < 2; ++m)
                o.push_back((double)index_of(verts, v->GetMate(m)));
            o.push_back(corner_index(v->GetFilletCorner()));
            o.push_back((double)v->GetPointClass());
            put_pc_object(o, v->GetPointClassObject());
            o.push_back((double)index_of(targetFaces, v->GetPointClassification().GetFace()));
            o.push_back(v->GetOriginalTParam());
            put_uv(o, v->GetOriginalUV());
            SmTArray<SmVertexuse*> vus;
            v->GetVertexuses(vus);
            o.push_back((double)vus.GetSize());
            for (ULONG k = 0; k < vus.GetSize(); ++k)
            {
                put_vu(o, vus[k]);
                put_uv(o, ((SmFilletVertexuse*)vus[k])->GetUVPoint());
            }
        }

        o.push_back((double)es.size());
        for (size_t i = 0; i < es.size(); ++i)
        {
            SmFilletEdge* e = es[i];
            SmVertex* vs = NULL;
            SmVertex* ve = NULL;
            e->GetVertices(vs, ve);
            o.push_back((double)index_of(verts, (SmFilletVertex*)vs));
            o.push_back((double)index_of(verts, (SmFilletVertex*)ve));
            o.push_back((double)e->GetFilletEdgeType());
            o.push_back((double)e->GetStatus());
            o.push_back(corner_index(e->GetFilletCorner()));
            o.push_back(fillet_brep_edge_index(e->m_pFilletBrepEdge1));
            o.push_back(fillet_brep_edge_index(e->m_pFilletBrepEdge2));
            put_curve(o, e->GetCurve());
            SmExtent1d ivl = e->GetInterval();
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
            SmTArray<SmEdgeuse*> eus;
            e->GetEdgeuses(eus);
            o.push_back((double)eus.GetSize());
            for (ULONG k = 0; k < eus.GetSize(); ++k)
            {
                SmFilletEdgeuse* eu = (SmFilletEdgeuse*)eus[k];
                o.push_back((double)eu->GetOrientation());
                SmVertexuse* vu = eu->GetVertexuse();
                o.push_back((double)index_of(verts, (SmFilletVertex*)(vu ? vu->GetVertex() : NULL)));
                put_curve(o, eu->GetUVTrimCurvePointer());
                o.push_back((double)index_of(geoms, eu->GetFilletGeom()));
            }
        }

        o.push_back((double)geoms.size());
        for (size_t i = 0; i < geoms.size(); ++i)
        {
            SmFilletGeom* g = geoms[i];
            o.push_back((double)g->GetFilletGeomType());
            o.push_back((double)g->GetStatus());
            for (ULONG r = 0; r < 2; ++r)
                o.push_back(edge_index(g->GetRail(r)));
            const SmTArray<SmFilletEdge*>& o1 = g->*GeomPeek::other1();
            o.push_back((double)o1.GetSize());
            for (ULONG k = 0; k < o1.GetSize(); ++k)
                o.push_back(edge_index(o1[k]));
            const SmTArray<SmFilletEdge*>& o2 = g->*GeomPeek::other2();
            o.push_back((double)o2.GetSize());
            for (ULONG k = 0; k < o2.GetSize(); ++k)
                o.push_back(edge_index(o2[k]));
            put_curve(o, g->GetCenterLineCurve());
            put_surface(o, g->GetFilletSurface());
            const SmTArray<SmFilletEdgeuse*>& refs = g->*GeomPeek::refs();
            o.push_back((double)refs.GetSize());
            for (ULONG k = 0; k < refs.GetSize(); ++k)
                put_eu(o, refs[k]);
            const SmTArray<SmFilletEdgeuse*>& sides = g->*GeomPeek::sides();
            o.push_back((double)sides.GetSize());
            for (ULONG k = 0; k < sides.GetSize(); ++k)
                put_eu(o, sides[k]);
        }
    }
};

// Fillet edge type name code (GetName): the index in this table, -1 when the buffer is left
// untouched.
int64_t name_code(SmFilletEdge* e)
{
    TCHAR buf[64];
    buf[0] = 0;
    e->GetName(buf, 64);
    static const TCHAR* names[] = {_T("SM_FE_CROSS_SECTION"),
                                   _T("SM_FE_FILLET_X_FILLET"),
                                   _T("SM_FE_FILLET_X_SIDE_FACE"),
                                   _T("SM_FE_RAIL_RAIL_INTERPOLATION"),
                                   _T("SM_FE_ON_EDGE"),
                                   _T("SM_FE_ON_EXTEND_EDGE"),
                                   _T("SM_FE_FILLET_END"),
                                   _T("SM_FE_SETBACK_RAIL"),
                                   _T("SM_FE_RAIL_EXTENSION"),
                                   _T("SM_FE_RAIL")};
    for (int64_t i = 0; i < 10; ++i)
        if (tstr_eq(buf, names[i]))
            return i;
    return -1;
}

int32_t run_geometry(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_CLEANUP_NOISE:
    {
        SmPoint2d uv = rd.uv();
        if (rd.bad)
            return SC_ERR_ARG;
        sm_CleanupNoise(uv);
        o.push_back(0.0);
        put_uv(o, uv);
        return SC_OK;
    }
    case OP_CHECK_SEAM_OUT:
    case OP_CHECK_SEAM_ON:
    {
        SmBSplineSurface* s = read_surface(rd, ctx);
        double tol = rd.num();
        int64_t closure = rd.idx();
        SmPoint2d uv = rd.uv();
        if (rd.bad)
        {
            delete s;
            return SC_ERR_ARG;
        }
        SmBoolean ret = (op == OP_CHECK_SEAM_OUT)
                            ? sm_CheckSeamOut(s, uv, tol, (SmSurfParamType)closure)
                            : sm_CheckSeamOn(s, uv, tol, (SmSurfParamType)closure);
        delete s;
        o.push_back(0.0);
        o.push_back(flg(ret != FALSE));
        put_uv(o, uv);
        return SC_OK;
    }
    case OP_SURFACE_CLOSURE:
    {
        SmBSplineSurface* s = read_surface(rd, ctx);
        double umin = rd.num();
        double vmin = rd.num();
        double umax = rd.num();
        double vmax = rd.num();
        if (rd.bad)
        {
            delete s;
            return SC_ERR_ARG;
        }
        SmExtent2d dom(SmPoint2d(umin, vmin), SmPoint2d(umax, vmax));
        SmSurfParamType c = sm_SetSurfaceClosure(s, dom);
        delete s;
        o.push_back(0.0);
        o.push_back((double)c);
        return SC_OK;
    }
    case OP_IS_ORDER_CORRECT:
    {
        int64_t n = rd.idx();
        if (rd.bad || n < 0 || n > 64)
            return SC_ERR_ARG;
        std::vector<SmTsectPnt> pts((size_t)n);
        SmTArray<SmTsectPnt*> arr;
        for (int64_t i = 0; i < n; ++i)
        {
            pts[(size_t)i].CrvPos() = rd.pt3();
            pts[(size_t)i].CrvDeriv() = rd.vec();
            arr.Add(&pts[(size_t)i]);
        }
        int64_t i1 = rd.idx();
        int64_t i2 = rd.idx();
        if (rd.bad || i1 < 0 || i1 >= n || i2 < 0 || i2 >= n)
            return SC_ERR_ARG;
        SmBoolean ret = sm_IsOrderCorrect(arr, (ULONG)i1, (ULONG)i2);
        o.push_back(0.0);
        o.push_back(flg(ret != FALSE));
        return SC_OK;
    }
    case OP_ADJUST_UV_ON_SEAM:
    {
        SmBSplineSurface* s = read_surface(rd, ctx);
        SmVector3d ref = rd.vec();
        SmPoint2d uv = rd.uv();
        if (rd.bad)
        {
            delete s;
            return SC_ERR_ARG;
        }
        SmStatus e = sm_AdjustUVOnSeam(s, ref, uv);
        delete s;
        o.push_back(st(e));
        put_uv(o, uv);
        return SC_OK;
    }
    case OP_INTERPOLATE:
    {
        SmPoint3d sp = rd.pt3();
        SmPoint3d ep = rd.pt3();
        SmVector3d sv = rd.vec();
        SmVector3d ev = rd.vec();
        int64_t dim = rd.idx();
        if (rd.bad || dim < 0 || dim > 16)
            return SC_ERR_ARG;
        SmBSplineCurve* c = NULL;
        SmStatus e = sm_InterpolateEndptsAndTangents(ctx, sp, ep, sv, ev, (ULONG)dim, c, 0);
        o.push_back(st(e));
        put_pt(o, sv);
        put_pt(o, ev);
        put_curve(o, c);
        delete c;
        return SC_OK;
    }
    case OP_SURFACE_DROP_POINT:
    {
        SmBSplineSurface* s = read_surface(rd, ctx);
        SmPoint3d p = rd.pt3();
        double tol = rd.num();
        if (rd.bad)
        {
            delete s;
            return SC_ERR_ARG;
        }
        SmSolution data[16];
        SmSolutionArray sols(16, data);
        SmStatus e = sm_SurfaceDropPoint(s, p, tol, sols, 0);
        o.push_back(st(e));
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); ++i)
        {
            o.push_back(sols[i].m_vStart[0]);
            o.push_back(sols[i].m_vStart[1]);
            o.push_back(sols[i].m_vStart.m_dSolutionValue);
        }
        delete s;
        return SC_OK;
    }
    case OP_TWO_PNTS:
    {
        SmBSplineSurface* s1 = read_surface(rd, ctx);
        SmBSplineSurface* s2 = read_surface(rd, ctx);
        SmPoint3d sp = rd.pt3();
        bool has_end = rd.flag();
        SmPoint3d ep = rd.pt3();
        SmVector3d dir = rd.vec();
        bool has_end_dir = rd.flag();
        SmVector3d end_dir = rd.vec();
        double tol = rd.num();
        if (rd.bad)
        {
            delete s1;
            delete s2;
            return SC_ERR_ARG;
        }
        SmBrep* b = new (ctx) SmBrep();
        SmVertex* vs = lone_vertex(b, sp);
        SmVertex* ve = has_end ? lone_vertex(b, ep) : NULL;
        if (vs == NULL || (has_end && ve == NULL))
        {
            delete b;
            delete s1;
            delete s2;
            return SC_ERR_ARG;
        }
        SmBSplineCurve* c3 = NULL;
        SmBSplineCurve* uv1 = NULL;
        SmBSplineCurve* uv2 = NULL;
        SmStatus e = sm_TwoPntsIntersection(ctx, vs, ve, &dir, has_end_dir ? &end_dir : NULL, s1,
                                            s2, tol, c3, uv1, uv2, 0);
        o.push_back(st(e));
        put_curve(o, c3);
        put_curve(o, uv1);
        put_curve(o, uv2);
        put_pt(o, dir); // unitised in place on success
        delete c3;
        delete uv1;
        delete uv2;
        delete b;
        delete s1;
        delete s2;
        return SC_OK;
    }
    case OP_EVALUATE_MATE:
    {
        SmBSplineSurface* s = read_surface(rd, ctx);
        SmPoint2d uv = rd.uv();
        if (rd.bad)
        {
            delete s;
            return SC_ERR_ARG;
        }
        SmBrep* b = new (ctx) SmBrep();
        SmFace* f = face_of(b, s); // the face owns the surface
        if (f == NULL)
        {
            delete b;
            return SC_ERR_ARG;
        }
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus e = sm_EvaluateMate(f, uv, p);
        o.push_back(st(e));
        put_pt(o, p);
        delete b;
        return SC_OK;
    }
    case OP_EVALUATE_MATE_ANALYTIC:
    {
        // a real SmPlane (kind 0: SmPlane(O, X, Y, UVScale, UVDomain), MakeNurb when asked)
        // or SmSurfOfRevolution (kind 1: CreateCanonical of a line, its NURB made, about
        // an axis), so sm_EvaluateMate's virtual EvaluatePoint and the inherited
        // EvaluatePointOnExtendedSurface run on those kinds
        int64_t kind = rd.idx();
        SmBSplineSurface* s = NULL;
        SmStatus cs = SM_SUCCESS;
        if (kind == 0)
        {
            SmPoint3d org = rd.vec();
            SmVector3d x = rd.vec();
            SmVector3d y = rd.vec();
            double su = rd.num();
            double sv = rd.num();
            double d0 = rd.num();
            double d1 = rd.num();
            double d2 = rd.num();
            double d3 = rd.num();
            bool nurb = rd.flag();
            if (!rd.bad)
            {
                SmPlane* pl = new (ctx) SmPlane(org, x, y, SmVector2d(su, sv),
                                                SmExtent2d(d0, d1, d2, d3), &ctx);
                if (nurb)
                    cs = pl->MakeNurb();
                s = pl;
            }
        }
        else
        {
            SmPoint3d a = rd.vec();
            SmPoint3d b = rd.vec();
            SmPoint3d ap = rd.vec();
            SmVector3d ad = rd.vec();
            bool swap = rd.flag();
            if (!rd.bad)
            {
                SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
                l->GetOrCreateGwNurbPointer();
                SmSurfOfRevolution* r = NULL;
                cs = SmSurfOfRevolution::CreateCanonical(ctx, l, ap, ad, r, swap ? TRUE : FALSE);
                if (cs != SM_SUCCESS)
                {
                    if (r)
                        delete r; // owns the line
                    else
                        delete l;
                    r = NULL;
                }
                s = r;
            }
        }
        SmPoint2d uv = rd.uv();
        if (rd.bad)
        {
            delete s;
            return SC_ERR_ARG;
        }
        o.push_back(st(cs));
        if (s == NULL || cs != SM_SUCCESS)
        {
            delete s;
            o.push_back(0.0);
            put_pt(o, SmPoint3d(0.0, 0.0, 0.0));
            return SC_OK;
        }
        SmBrep* b = new (ctx) SmBrep();
        SmFace* f = face_of(b, s); // the face owns the surface
        if (f == NULL)
        {
            delete b; // as OP_EVALUATE_MATE: a face the B-rep refuses is an input error
            return SC_ERR_ARG;
        }
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus e = sm_EvaluateMate(f, uv, p);
        o.push_back(st(e));
        put_pt(o, p);
        delete b;
        return SC_OK;
    }
    case OP_CAPPING:
    {
        double tol = rd.num();
        SmBSplineSurface* s = read_surface(rd, ctx);
        SmBSplineCurve* c1 = read_curve(rd, ctx);
        int64_t o1 = rd.idx();
        SmBSplineCurve* c2 = read_curve(rd, ctx);
        int64_t o2 = rd.idx();
        if (rd.bad)
        {
            delete s;
            delete c1;
            delete c2;
            return SC_ERR_ARG;
        }
        SmTArray<SmCurve*> c3d;
        SmTArray<SmBSplineCurve*> uvs;
        SmTArray<SmOrientType> orients;
        SmStatus e = JoinVertsByCapping(ctx, tol, s, c1, (SmOrientType)o1, c2, (SmOrientType)o2,
                                        c3d, uvs, orients, 0);
        o.push_back(st(e));
        o.push_back((double)uvs.GetSize());
        for (ULONG i = 0; i < uvs.GetSize(); ++i)
        {
            put_curve(o, uvs[i]);
            put_curve(o, i < c3d.GetSize() ? c3d[i] : NULL);
            o.push_back(i < orients.GetSize() ? (double)orients[i] : -1.0);
        }
        for (ULONG i = 0; i < uvs.GetSize(); ++i)
            delete uvs[i];
        for (ULONG i = 0; i < c3d.GetSize(); ++i)
            delete c3d[i];
        delete s;
        delete c1;
        delete c2;
        return SC_OK;
    }
    case OP_CLASSIFY_FACE_POINT:
    {
        SmBSplineSurface* s1 = read_surface(rd, ctx);
        SmBSplineSurface* s2 = read_surface(rd, ctx);
        SmPoint3d p = rd.pt3();
        if (rd.bad)
        {
            delete s1;
            delete s2;
            return SC_ERR_ARG;
        }
        SmBrep* b = new (ctx) SmBrep();
        SmFace* f1 = face_of(b, s1);
        SmFace* f2 = face_of(b, s2);
        if (f1 == NULL || f2 == NULL)
        {
            delete b;
            return SC_ERR_ARG;
        }
        int which = sm_ClassifyFacePoint(p, f1, f2);
        o.push_back(0.0);
        o.push_back((double)which);
        delete b;
        return SC_OK;
    }
    default:
        return SC_ERR_ARG;
    }
}

// Whether sm_ExamineNFixFilletGeoms(g1, a1, g2, a2, sv, ev, tol) would reach its adjusting
// path: upstream's own pre-checks (SmFilletGeom.cpp:4844-4940) repeated verbatim. Surfaces
// missing (NER), a low U degree or a non-B-spline kind return early; otherwise exactly one
// surface failing to meet the vertices within tol means adjustment.
bool sc_examine_reaches_adjust(SmFilletGeom* g1, bool a1, SmFilletGeom* g2, bool a2,
                               SmFilletVertex* sv, SmFilletVertex* ev, double tol)
{
    SM_FILLETSURF_TYPE* s[2] = { g1->GetFilletSurface(), g2->GetFilletSurface() };
    if (s[0] == NULL || s[1] == NULL)
        return false;
    if (s[0]->GetDegree(SM_SP_U) < 3 || s[1]->GetDegree(SM_SP_U) < 3
        || !s[0]->IsKindOf(SmBSplineSurface_TYPE) || !s[1]->IsKindOf(SmBSplineSurface_TYPE))
        return false;
    SmPoint3d sp = sv->GetPoint();
    SmPoint3d ep = ev->GetPoint();
    bool at[2] = { a1, a2 };
    bool adjust[2] = { true, true };
    for (int i = 0; i < 2; ++i)
    {
        SmExtent2d dom = s[i]->GetNaturalUVDomain();
        SmPoint2d uv1 = at[i] ? dom.Evaluate(0.0, 0.0) : dom.Evaluate(1.0, 0.0);
        SmPoint2d uv2 = at[i] ? dom.Evaluate(0.0, 1.0) : dom.Evaluate(1.0, 1.0);
        SmPoint3d p1, p2;
        if (s[i]->EvaluatePoint(uv1, p1) != SM_SUCCESS || s[i]->EvaluatePoint(uv2, p2) != SM_SUCCESS)
            return false;
        double dStart = sp.DistanceBetween(p1);
        double d2 = sp.DistanceBetween(p2);
        double dEnd;
        if (dStart < d2)
            dEnd = ep.DistanceBetween(p2);
        else
        {
            dStart = d2;
            dEnd = ep.DistanceBetween(p1);
        }
        if (dStart < tol && dEnd < tol)
            adjust[i] = false;
    }
    return adjust[0] != adjust[1];
}

int32_t run_pseudo(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Model M(ctx);
    if (!M.build(rd))
        return SC_ERR_ARG;

    switch (op)
    {
    case OP_FB_EXPORT:
    {
        o.push_back(0.0);
        M.put(o);
        return SC_OK;
    }
    case OP_FB_VERTEX_GEOMS:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        if (rd.bad || v == NULL)
            return SC_ERR_ARG;
        SmTArray<SmFilletGeom*> gs;
        v->GetFilletGeoms(gs);
        o.push_back(0.0);
        o.push_back((double)gs.GetSize());
        for (ULONG i = 0; i < gs.GetSize(); ++i)
            o.push_back((double)index_of(M.geoms, gs[i]));
        return SC_OK;
    }
    case OP_FB_VU_AT_RAIL_END:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        int64_t g = rd.idx();
        int64_t xe = rd.idx();
        int64_t xw = rd.idx();
        int64_t r = rd.idx();
        if (rd.bad || v == NULL)
            return SC_ERR_ARG;
        SmFilletGeom* pg = g < 0 ? NULL : M.geom(g);
        SmFilletVertexuse* xvu = NULL;
        if (xe >= 0)
        {
            SmFilletEdgeuse* xeu = M.edgeuse(xe, xw);
            if (xeu == NULL)
                return SC_ERR_ARG;
            xvu = (SmFilletVertexuse*)xeu->GetVertexuse();
        }
        SmFilletEdge* rail = r < 0 ? NULL : M.edge(r);
        if ((g >= 0 && pg == NULL) || (r >= 0 && rail == NULL))
            return SC_ERR_ARG;
        SmFilletVertexuse* vu = v->GetVUAtRailEnd(pg, xvu, rail);
        o.push_back(0.0);
        o.push_back(flg(vu != NULL));
        M.put_vu(o, vu);
        return SC_OK;
    }
    case OP_FB_RAIL_VERTEXUSE:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        int64_t rail = rd.idx();
        int64_t end = rd.idx();
        if (rd.bad || g == NULL || rail < 0 || rail > 1 || end < 0 || end > 1)
            return SC_ERR_ARG;
        // upstream asserts on a missing rail / primary edge use / mate; the Rust wrapper
        // rejects such programs
        SmFilletEdge* r = g->GetRail((ULONG)rail);
        if (r == NULL || r->GetPrimaryEdgeuse() == NULL ||
            r->GetPrimaryEdgeuse()->GetMate() == NULL)
            return SC_ERR_ARG;
        SmFilletVertexuse* vu = g->GetRailVertexuse((ULONG)rail, (ULONG)end);
        o.push_back(0.0);
        o.push_back(flg(vu != NULL));
        M.put_vu(o, vu);
        return SC_OK;
    }
    case OP_FB_EDGE_NAME:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        if (rd.bad || e == NULL)
            return SC_ERR_ARG;
        o.push_back(0.0);
        o.push_back((double)name_code(e));
        return SC_OK;
    }
    case OP_FB_FIND_EDGE:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        bool multi = rd.flag();
        int64_t n = rd.idx();
        if (rd.bad || e == NULL || n < 0 || n > 256)
            return SC_ERR_ARG;
        SmTArray<SmEdge*> cands;
        for (int64_t i = 0; i < n; ++i)
        {
            int64_t k = rd.idx();
            if (rd.bad || k < 0 || (size_t)k >= M.filletBrepEdges.size())
                return SC_ERR_ARG;
            cands.Add(M.filletBrepEdges[(size_t)k]);
        }
        if (e->GetCurve() == NULL)
            return SC_ERR_ARG; // upstream NER
        SmStatus s = e->FindFilletBrepEdge(cands, multi ? TRUE : FALSE);
        o.push_back(st(s));
        o.push_back(M.fillet_brep_edge_index(e->m_pFilletBrepEdge1));
        o.push_back(M.fillet_brep_edge_index(e->m_pFilletBrepEdge2));
        return SC_OK;
    }
    case OP_FB_CLEAR_EDGES:
    case OP_FB_BREP_EDGES:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        if (rd.bad || e == NULL)
            return SC_ERR_ARG;
        if (op == OP_FB_CLEAR_EDGES)
        {
            int64_t n = rd.idx();
            if (rd.bad || n < 0 || n > 256)
                return SC_ERR_ARG;
            SmTArray<SmEdge*> dels;
            for (int64_t i = 0; i < n; ++i)
            {
                int64_t k = rd.idx();
                if (rd.bad || k < 0 || (size_t)k >= M.filletBrepEdges.size())
                    return SC_ERR_ARG;
                dels.Add(M.filletBrepEdges[(size_t)k]);
            }
            SmStatus s = e->ClearFilletBrepEdges(dels);
            o.push_back(st(s));
        }
        else
        {
            o.push_back(0.0);
        }
        SmTArray<SmEdge*> got;
        e->GetFilletBrepEdges(got);
        o.push_back((double)got.GetSize());
        for (ULONG i = 0; i < got.GetSize(); ++i)
            o.push_back(M.fillet_brep_edge_index(got[i]));
        o.push_back(M.fillet_brep_edge_index(e->GetFilletBrepEdge()));
        return SC_OK;
    }
    case OP_FB_TRIM:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        double t0 = rd.num();
        double t1 = rd.num();
        if (rd.bad || g == NULL || g->GetRail(0) == NULL || g->GetRail(1) == NULL)
            return SC_ERR_ARG; // upstream dereferences the rails
        SmExtent1d ivl(t0, t1);
        SmStatus s = g->Trim(ivl);
        o.push_back(st(s));
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        M.put(o);
        return SC_OK;
    }
    case OP_FB_DELETE_VERTEX:
    {
        int64_t vi = rd.idx();
        SmFilletVertex* v = M.vertex(vi);
        if (rd.bad || v == NULL)
            return SC_ERR_ARG;
        // its vertex uses are deleted but their edge uses keep dangling pointers; the
        // wrapper only allows deleting a vertex whose edges were deleted first
        delete v;
        M.verts.erase(M.verts.begin() + vi);
        o.push_back(0.0);
        M.put(o);
        return SC_OK;
    }
    case OP_FB_DELETE_EDGE:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        if (rd.bad || e == NULL)
            return SC_ERR_ARG;
        delete e;
        o.push_back(0.0);
        M.put(o);
        return SC_OK;
    }
    case OP_FB_VERT_ON_VERT:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        if (rd.bad || v == NULL)
            return SC_ERR_ARG;
        SmObject* obj = v->GetPointClassObject();
        if (obj == NULL || !obj->IsKindOf(SmVertex_TYPE))
            return SC_ERR_ARG; // upstream casts blindly
        SmStatus s = v->CalcVertOnVert();
        o.push_back(st(s));
        put_pt(o, v->GetPoint());
        o.push_back((double)v->GetStatus());
        return SC_OK;
    }
    case OP_FB_CLIFF:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        SmBSplineSurface* s = read_surface(rd, ctx);
        double tol = rd.num();
        if (rd.bad || v == NULL)
        {
            delete s;
            return SC_ERR_ARG;
        }
        SmObject* obj = v->GetPointClassObject();
        if (obj == NULL || !obj->IsKindOf(SmEdgeuse_TYPE) ||
            ((SmEdgeuse*)obj)->GetEdge()->GetCurve() == NULL)
        {
            delete s;
            return SC_ERR_ARG; // upstream casts blindly / dereferences
        }
        SmStatus e = v->CalcCliffRailIntEdge(s, tol);
        o.push_back(st(e));
        put_pt(o, v->GetPoint());
        o.push_back((double)v->GetStatus());
        delete s;
        return SC_OK;
    }
    case OP_FB_SPLIT_FACE:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        int64_t f1 = rd.idx();
        int64_t f2 = rd.idx();
        if (rd.bad || v == NULL || f1 < 0 || (size_t)f1 >= M.targetFaces.size() || f2 < 0 ||
            (size_t)f2 >= M.targetFaces.size())
            return SC_ERR_ARG;
        SmStatus s = v->UpdateSplitFace(M.targetFaces[(size_t)f1], M.targetFaces[(size_t)f2]);
        o.push_back(st(s));
        o.push_back((double)v->GetPointClass());
        M.put_pc_object(o, v->GetPointClassObject());
        o.push_back((double)index_of(M.targetFaces, v->GetPointClassification().GetFace()));
        return SC_OK;
    }
    case OP_FB_EXAMINE:
    {
        SmFilletGeom* g1 = M.geom(rd.idx());
        bool a1 = rd.flag();
        SmFilletGeom* g2 = M.geom(rd.idx());
        bool a2 = rd.flag();
        SmFilletVertex* sv = M.vertex(rd.idx());
        SmFilletVertex* ev = M.vertex(rd.idx());
        double tol = rd.num();
        if (rd.bad || g1 == NULL || g2 == NULL || sv == NULL || ev == NULL)
            return SC_ERR_ARG;
        // The geometries here have no SmFilletSolver (new SmFilletGeom(NULL, type)); the
        // adjusting path dereferences it (SmFilletGeom.cpp:5071 GetFilletSolver(), :5092
        // GetSurface(k)), so an input that would reach it is refused.
        if (sc_examine_reaches_adjust(g1, a1, g2, a2, sv, ev, tol))
            return SC_ERR_ARG;
        SmStatus s = sm_ExamineNFixFilletGeoms(g1, a1 ? TRUE : FALSE, g2, a2 ? TRUE : FALSE, sv,
                                               ev, tol, 0);
        o.push_back(st(s));
        M.put(o);
        return SC_OK;
    }
    case OP_FB_SET_EU_GEOM:
    {
        int64_t ei = rd.idx();
        int64_t which = rd.idx();
        int64_t g = rd.idx();
        SmFilletEdgeuse* eu = M.edgeuse(ei, which);
        if (rd.bad || eu == NULL || (g >= 0 && M.geom(g) == NULL))
            return SC_ERR_ARG;
        eu->SetFilletGeom(g < 0 ? NULL : M.geom(g));
        o.push_back(0.0);
        M.put(o);
        return SC_OK;
    }
    case OP_FB_SET_UV_CURVE:
    {
        int64_t ei = rd.idx();
        int64_t which = rd.idx();
        SmFilletEdgeuse* eu = M.edgeuse(ei, which);
        bool has = rd.flag();
        SmBSplineCurve* c = has ? read_curve(rd, ctx) : NULL;
        bool del = rd.flag();
        bool warn = rd.flag();
        if (rd.bad || eu == NULL)
        {
            delete c;
            return SC_ERR_ARG;
        }
        SmBSplineCurve* old = eu->GetUVTrimCurvePointer();
        eu->SetUVCurve(c, del ? TRUE : FALSE, warn ? TRUE : FALSE);
        if (!del && old != NULL && old != c)
            delete old; // upstream leaks it
        o.push_back(0.0);
        M.put(o);
        return SC_OK;
    }
    case OP_FB_ORIG_UV_CURVE:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        if (rd.bad || e == NULL || e->GetPrimaryEdgeuse() == NULL ||
            e->GetPrimaryEdgeuse()->GetMate() == NULL)
            return SC_ERR_ARG;
        o.push_back(0.0);
        put_curve(o, e->GetOriginalUVCurve());
        return SC_OK;
    }
    case OP_FB_SET_VU_PROPERTY:
    {
        int64_t ei = rd.idx();
        int64_t which = rd.idx();
        int64_t te = rd.idx();
        int64_t tw = rd.idx();
        SmFilletEdgeuse* eu = M.edgeuse(ei, which);
        SmFilletEdgeuse* teu = M.edgeuse(te, tw);
        if (rd.bad || eu == NULL || teu == NULL)
            return SC_ERR_ARG;
        ((SmFilletVertexuse*)eu->GetVertexuse())->SetProperty(teu);
        o.push_back(0.0);
        M.put(o);
        return SC_OK;
    }
    case OP_FB_REMOVE_EDGE:
    {
        int64_t ei = rd.idx();
        SmFilletEdge* e = M.edge(ei);
        bool readd = rd.flag();
        if (rd.bad || e == NULL)
            return SC_ERR_ARG;
        M.pseudo->RemoveEdge(e);
        if (readd)
            M.pseudo->AddEdge(e);
        o.push_back(0.0);
        M.put(o);
        if (!readd)
            delete e; // no longer in the list the destructor walks
        return SC_OK;
    }
    default:
        return SC_ERR_ARG;
    }
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_geom_p1_eval(int32_t op, const double* in, int64_t n_in,
                                               double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL && n_in > 0)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    rd.cache_op = op == OP_SURFACE_DROP_POINT || op == OP_TWO_PNTS ||
                  op == OP_CLASSIFY_FACE_POINT || op == OP_FB_VERT_ON_VERT ||
                  op == OP_FB_CLIFF || op == OP_FB_SPLIT_FACE;
    std::vector<double> o;
    int32_t rc = op >= OP_FB_EXPORT ? run_pseudo(op, rd, o) : run_geometry(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// The difftest shim of unit blend.fillet_geom_p4 (it needs this file's statics and Model).
#include "blend_fillet_geom_p4_static.inc"

// The difftest shim of unit blend.fillet_geom_p2 (sm_SrfSrfIntersection is defined in this
// translation unit; it needs this file's Model and the p4 shim's solvers and surfaces).
#include "blend_fillet_geom_p2_static.inc"

// The difftest shim of unit blend.fillet_corner_p3, corner operations (it needs this file's Model).
#include "blend_fillet_corner_p3_static.inc"

// The difftest shim of unit blend.fillet_geom_p5 (this file's Model, p4's base solvers).
#include "blend_fillet_geom_p5_static.inc"
// Wrappers of SmFilletGeom.cpp file-statics for the shim of unit blend.fillet_geom_p6.
#include "blend_fillet_geom_p6_static.inc"
