// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_geom_p6`: SmFilletEdge::CalcBlendingRail,
// CalcEdgeOnEdge, CalcFilletEnd, CalcSetBackRail, ShouldSwapFaces, SmFilletGeom::Copy,
// FindEdgeTangentAtPoint, GetRailIndexOfSideEU, GetRailIndexOfVertexuse, GetRailVertex,
// SetUpRailPoint, RailEdgeuseIntersect, RecordFilletError and the file-statics
// JoinTwoFilletVertsByCurves and sm_MoveTSP of upstream source/SMLib/src/SmFilletGeom.cpp.
//
// The class members are linked from the translation unit of blend_fillet_geom_p1.cpp, which
// compiles SmFilletGeom.cpp itself; the two file-statics are reached through the wrappers that
// file includes from blend_fillet_geom_p6_static.inc (declared in blend_fillet_geom_p6.h).
//
// One entry point, sc_blend_fillet_geom_p6_eval(op, path, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/blend_fillet_geom_p6.rs, which validates the
// blocks; the shim also refuses (SC_ERR_ARG) a model on which the op would dereference NULL or
// an object of the wrong kind inside upstream (checks below).
//
// Each call builds a target SmBrep from a B-rep block of unit `topology.edgeuse_p1` (objects are
// addressed by their traversal-order export index), a fillet executive on it (corners and
// solvers point at it), fillet solvers (a minimal SmFilletSolver subclass: every virtual is the
// base version), fillet corners (a minimal SmFilletCorner subclass: the filleted vertex, the
// tolerance and a constant corner type), and a pseudo SmFilletBrep of fillet vertices,
// geometries and edges (SmFilletEdge::MakeFilletEdge). The export reads raw members, never the
// functions under test.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFilletCorner.h>
#include <SmFilletExecutive.h>
#include <SmFilletGeom.h>
#include <SmFilletSolver.h>
#include <SmOffsetSurface.h>
#include <SmPointClass.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <nurbs.h>

#include <cstdint>
#include <cstring>
#include <set>
#include <vector>

#include "blend_fillet_geom_p6.h"
#include "sc_edgeuse_graph.h"

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_geom_p6.rs).
enum : int32_t
{
    OP_EXPORT = 0,
    OP_BLENDING_RAIL = 1,
    OP_EDGE_ON_EDGE = 2,
    OP_FILLET_END = 3,
    OP_SETBACK_RAIL = 4,
    OP_SHOULD_SWAP = 5,
    OP_JOIN = 6,
    OP_COPY = 7,
    OP_FIND_TANGENT = 8,
    OP_RAIL_INDEX_SIDE_EU = 9,
    OP_RAIL_INDEX_VU = 10,
    OP_RAIL_VERTEX = 11,
    OP_SETUP_RAIL_POINT = 12,
    OP_MOVE_TSP = 13,
    OP_RAIL_EU_INTERSECT = 14,
    OP_RECORD_ERROR = 15,
};

// A base SmFilletSolver.
struct ShimFS : SmFilletSolver
{
    ShimFS(const SmContext& ctx, double tol, double ang, double tang) : SmFilletSolver(ctx, tol, ang, tang)
    {
        m_pEdgeuses[0] = m_pEdgeuses[1] = NULL;
        m_pExecutive = NULL;
        m_dOrientations[0] = m_dOrientations[1] = 1.0;
        m_dSavedOffsets[0] = m_dSavedOffsets[1] = 0.0;
        m_bExtendBefore = m_bExtendAfter = FALSE;
    }
    virtual SmFilletSolverType GetSolverType() { return SM_FS_UNKNOWN; }
    virtual void GetName(TCHAR* pc, size_t n)
    {
        if (n > 0)
            pc[0] = 0;
    }
    void set_edgeuses(SmEdgeuse* a, SmEdgeuse* b)
    {
        m_pEdgeuses[0] = a;
        m_pEdgeuses[1] = b;
    }
    void set_executive(SmFilletExecutive* e) { m_pExecutive = e; }
    void set_reverse_trim(bool r) { m_bReverseTrim = r ? TRUE : FALSE; }
};

// A fillet corner: the filleted vertex, the tolerance and a constant corner type.
struct ShimCorner : SmFilletCorner
{
    SmFilletCornerType type;
    ShimCorner(const SmContext& ctx, const SmVertex* v, SmFilletExecutive* exec, double tol, SmFilletCornerType t)
        : SmFilletCorner(ctx, v, NULL, NULL, exec, tol, 0.01), type(t)
    {
    }
    virtual SmStatus MakeCornerTopology() { return SM_ERR; }
    virtual SmFilletCornerType GetCornerType() const { return type; }
    virtual void GetName(TCHAR* pc, size_t n) const
    {
        if (n > 0)
            pc[0] = 0;
    }
};

struct GeomPeek : SmFilletGeom
{
    static SmTArray<SmFilletEdge*> SmFilletGeom::*other1() { return &GeomPeek::m_vOtherRails1; }
    static SmTArray<SmFilletEdge*> SmFilletGeom::*other2() { return &GeomPeek::m_vOtherRails2; }
    static SmTArray<SmFilletEdgeuse*> SmFilletGeom::*refs() { return &GeomPeek::m_vRefEUs; }
    static SmTArray<SmFilletEdgeuse*> SmFilletGeom::*sides() { return &GeomPeek::m_vSideEUs; }
    static double (SmFilletGeom::*devs())[2] { return &GeomPeek::m_dDeviations; }
    static SmOffsetSurface* (SmFilletGeom::*offs())[2] { return &GeomPeek::m_pOffSurfs; }
    static SmSurface* SmFilletGeom::*surface() { return &GeomPeek::m_pFilletSurface; }
};

template <typename Tag, typename Tag::type M> struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};
struct StatusTag
{
    typedef SmFilStatus SmFilletErrorInfo::*type;
    friend type get(StatusTag);
};
template struct Rob<StatusTag, &SmFilletErrorInfo::m_eFilletStatus>;

double ix(int64_t i) { return (double)i; }

template <class T> T* at(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : NULL;
}

template <class T> int64_t index_of(const std::vector<T*>& v, const T* x)
{
    for (size_t i = 0; i < v.size(); ++i)
        if (v[i] == x)
            return (int64_t)i;
    return -1;
}

// `has dim n p m cpts knots` (has = 0 for a null or non-B-spline curve).
void put_bcurve(std::vector<double>& o, const SmCurve* c)
{
    const SmBSplineCurve* bc =
        (c != NULL && c->IsKindOf(SmBSplineCurve_TYPE)) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = bc ? bc->GetGwNurbPointer() : NULL;
    o.push_back(cur != NULL ? 1.0 : 0.0);
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

// `has n m p q r s cpts u v` (has = 0 for a null surface or one that is not an
// SmBSplineSurface or an SmSurfOfRevolution).
void put_bsurface(std::vector<double>& o, const SmSurface* s)
{
    const SmBSplineSurface* b =
        (s != NULL && s->IsKindOf(SmBSplineSurface_TYPE) && s->GetType() != SmPlane_TYPE)
            ? (const SmBSplineSurface*)s
            : NULL;
    gw_SURFACE* sur = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(sur != NULL ? 1.0 : 0.0);
    if (!sur)
        return;
    o.push_back((double)sur->net->n);
    o.push_back((double)sur->net->m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)sur->knu->m);
    o.push_back((double)sur->knv->m);
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

struct Model
{
    const SmContext& ctx;
    Graph g;
    SmBrep* target = NULL;
    SmFilletExecutive* exec = NULL;
    SmFilletBrep* pseudo = NULL;
    std::vector<ShimFS*> solvers;
    std::vector<SmSurface*> bases; // owned here: nobody owns an offset's base surface
    std::vector<ShimCorner*> corners;
    std::vector<SmFilletVertex*> verts;
    std::vector<SmFilletGeom*> geoms;
    std::vector<SmSurface*> geomSurfaces; // owned here: SmFilletGeom does not delete its surface
    std::vector<SmFilletEdge*> edges;

    explicit Model(const SmContext& c) : ctx(c) {}

    ~Model()
    {
        for (size_t i = 0; i < edges.size(); ++i)
            delete edges[i];
        delete pseudo;
        for (size_t i = 0; i < verts.size(); ++i)
        {
            verts[i]->SetMate(0, NULL);
            verts[i]->SetMate(1, NULL);
        }
        for (size_t i = 0; i < verts.size(); ++i)
            delete verts[i];
        // fillet surfaces: the ones made here and copies made by SmFilletGeom::Copy
        std::set<SmSurface*> surfs(geomSurfaces.begin(), geomSurfaces.end());
        for (size_t i = 0; i < geoms.size(); ++i)
            if (geoms[i]->*GeomPeek::surface() != NULL)
                surfs.insert(geoms[i]->*GeomPeek::surface());
        for (size_t i = 0; i < geoms.size(); ++i)
            delete geoms[i];
        for (SmSurface* s : surfs)
            delete s;
        for (size_t i = 0; i < solvers.size(); ++i)
            delete solvers[i];
        for (size_t i = 0; i < bases.size(); ++i)
            delete bases[i];
        for (size_t i = 0; i < corners.size(); ++i)
            delete corners[i];
        delete exec;
        delete target;
    }

    static SmFilletEdgeuse* eu_of(SmFilletEdge* e, int64_t which)
    {
        if (e == NULL)
            return NULL;
        SmEdgeuse* eu = e->GetPrimaryEdgeuse();
        if (eu != NULL && which == 1)
            eu = eu->GetMate();
        return (SmFilletEdgeuse*)eu;
    }

    SmObject* pc_object(int64_t kind, int64_t index)
    {
        switch (kind)
        {
        case 1: return at(g.vertices, index);
        case 2: return at(g.edgeuses, index);
        case 3: return at(g.faces, index);
        case 4: return at(g.edges, index);
        default: return NULL;
        }
    }

    void read_tsect(Rd& rd, SmTsectPnt& t)
    {
        t.m_ePointType = (SmIntersectionPointType)rd.idx();
        t.UVPos(0) = rd.uv();
        t.UVPos(1) = rd.uv();
        t.CrvPos() = rd.vec();
        t.CrvDeriv() = rd.vec();
        t.m_dCurveParameter = rd.num();
    }

    // Reads the model after the B-rep block (layout in the Rust wrapper).
    bool build(Rd& rd)
    {
        exec = new (ctx) SmFilletExecutive(ctx, target);
        pseudo = new (ctx) SmFilletBrep();

        int64_t ns = rd.idx();
        for (int64_t i = 0; i < ns; ++i)
        {
            double tol = rd.num(), ang = rd.num(), tang = rd.num();
            ShimFS* fs = new (ctx) ShimFS(ctx, tol, ang, tang);
            solvers.push_back(fs);
            SmEdgeuse* e0 = at(g.edgeuses, rd.idx());
            SmEdgeuse* e1 = at(g.edgeuses, rd.idx());
            fs->set_edgeuses(e0, e1);
            for (ULONG k = 0; k < 2; ++k)
            {
                if (rd.flag())
                {
                    SmSurface* base = read_surface(rd, ctx);
                    bases.push_back(base);
                    double dist = rd.num();
                    fs->SetSurface(k, new (ctx) SmOffsetSurface(dist, *base));
                }
            }
            if (rd.flag())
                fs->set_executive(exec);
            fs->set_reverse_trim(rd.flag());
        }

        int64_t nc = rd.idx();
        for (int64_t i = 0; i < nc; ++i)
        {
            SmVertex* v = at(g.vertices, rd.idx());
            int64_t type = rd.idx();
            double tol = rd.num();
            corners.push_back(new (ctx) ShimCorner(ctx, v, exec, tol, (SmFilletCornerType)type));
        }

        int64_t nv = rd.idx();
        for (int64_t i = 0; i < nv; ++i)
        {
            SmPoint3d p = rd.vec();
            int64_t vtype = rd.idx();
            int64_t status = rd.idx();
            int64_t corner = rd.idx();
            int64_t pc_class = rd.idx();
            int64_t pc_kind = rd.idx();
            int64_t pc_index = rd.idx();
            int64_t pc_face = rd.idx();
            double t = rd.num();
            SmPoint2d uv = rd.uv();
            SmFilletVertex* fv = new (pseudo) SmFilletVertex(at(corners, corner));
            fv->SetPoint(p);
            fv->SetFilletVertexType((SmFilletVertexType)vtype);
            fv->SetStatus((SmFilStatus)status);
            fv->SetPointClass((SmPointClassificationType)pc_class, pc_object(pc_kind, pc_index));
            if (pc_face >= 0)
                fv->GetPointClassification().SetFace(at(g.faces, pc_face));
            fv->SetOriginalTParam(t);
            fv->SetOriginalUV(uv);
            verts.push_back(fv);
        }

        int64_t ng = rd.idx();
        for (int64_t i = 0; i < ng; ++i)
        {
            int64_t gtype = rd.idx();
            int64_t status = rd.idx();
            int64_t solver = rd.idx();
            SmFilletGeom* geom = new SmFilletGeom(at(solvers, solver), (SmFilletGeomType)gtype);
            geom->SetStatus((SmFilStatus)status);
            if (rd.flag())
            {
                SmSurface* s = read_surface(rd, ctx);
                geomSurfaces.push_back(s);
                geom->SetFilletSurface(s);
            }
            if (rd.flag())
                geom->SetCenterLineCurve(read_curve(rd, ctx));
            (geom->*GeomPeek::devs())[0] = rd.num();
            (geom->*GeomPeek::devs())[1] = rd.num();
            for (ULONG k = 0; k < 2; ++k)
            {
                if (rd.flag())
                {
                    SmSurface* base = read_surface(rd, ctx);
                    bases.push_back(base);
                    double dist = rd.num();
                    geom->SetOffsetSurface(k, new (ctx) SmOffsetSurface(dist, *base));
                }
            }
            geoms.push_back(geom);
        }

        int64_t ne = rd.idx();
        for (int64_t i = 0; i < ne; ++i)
        {
            SmFilletVertex* vs = at(verts, rd.idx());
            SmFilletVertex* ve = at(verts, rd.idx());
            int64_t etype = rd.idx();
            int64_t status = rd.idx();
            int64_t corner = rd.idx();
            if (vs == NULL || ve == NULL)
                return false;
            SmFilletEdge* e = NULL;
            if (SmFilletEdge::MakeFilletEdge(pseudo, vs, ve, e, at(corners, corner)) != SM_SUCCESS || e == NULL)
                return false;
            edges.push_back(e);
            e->SetFilletEdgeType((SmFilletEdgeType)etype);
            e->SetStatus((SmFilStatus)status);
            if (rd.flag())
            {
                // as upstream's producers: SetCurve, then the curve's owner is the edge
                SmCurve* c = read_curve(rd, ctx);
                e->SetCurve(c, TRUE);
                if (c != NULL)
                    c->SetOwner(e);
            }
            if (rd.flag())
            {
                double t0 = rd.num();
                double t1 = rd.num();
                e->SetInterval(SmExtent1d(t0, t1));
            }
            e->SetOriginalEdge(at(g.edges, rd.idx()));
            e->SetOriginalFace(at(g.faces, rd.idx()));
            for (int64_t which = 0; which < 2; ++which)
            {
                SmFilletEdgeuse* eu = eu_of(e, which);
                if (eu == NULL)
                    return false;
                if (rd.flag())
                    eu->SetUVCurve(read_curve(rd, ctx));
                SmFilletGeom* pg = at(geoms, rd.idx());
                if (pg != NULL)
                    eu->SetFilletGeom(pg);
                SmFilletVertexuse* vu = (SmFilletVertexuse*)eu->GetVertexuse();
                SmPoint2d uv = rd.uv();
                vu->SetUVPoint(uv);
                read_tsect(rd, vu->GetTsectPnt());
            }
        }

        for (int64_t i = 0; i < ng; ++i)
        {
            SmFilletGeom* geom = geoms[(size_t)i];
            for (ULONG r = 0; r < 2; ++r)
                geom->SetRail(r, at(edges, rd.idx()));
            int64_t n1 = rd.idx();
            for (int64_t k = 0; k < n1; ++k)
                (geom->*GeomPeek::other1()).Add(at(edges, rd.idx()));
            int64_t n2 = rd.idx();
            for (int64_t k = 0; k < n2; ++k)
                (geom->*GeomPeek::other2()).Add(at(edges, rd.idx()));
            int64_t nsd = rd.idx();
            for (int64_t k = 0; k < nsd; ++k)
            {
                int64_t e = rd.idx();
                int64_t which = rd.idx();
                SmFilletEdgeuse* eu = eu_of(at(edges, e), which);
                if (eu == NULL)
                    return false;
                geom->AddSideFilletEdgeuse(eu);
            }
        }

        for (int64_t i = 0; i < nv; ++i)
            for (ULONG m = 0; m < 2; ++m)
                verts[(size_t)i]->SetMate(m, at(verts, rd.idx()));
        return true;
    }

    // ---- refusal checks: inputs on which upstream reads through NULL or a blind cast ----

    // The edge's start vertex and its other vertex (always set by MakeFilletEdge).
    static void ends(SmFilletEdge* e, SmFilletVertex*& s, SmFilletVertex*& t)
    {
        s = (SmFilletVertex*)e->GetVertex();
        t = (SmFilletVertex*)e->GetOtherVertex(s);
    }

    // The first SM_FE_RAIL edge at a fillet vertex (CalcBlendingRail).
    static SmFilletEdge* rail_at(SmFilletVertex* v)
    {
        SmTArray<SmEdge*> es;
        v->GetEdges(es);
        for (ULONG j = 0; j < es.GetSize(); j++)
            if (((SmFilletEdge*)es[j])->GetFilletEdgeType() == SM_FE_RAIL)
                return (SmFilletEdge*)es[j];
        return NULL;
    }

    // CalcBlendingRail: the rails' primary edge uses' mates, curves are dereferenced.
    static bool blending_safe(SmFilletEdge* e)
    {
        SmFilletVertex *s, *t;
        ends(e, s, t);
        SmFace* f = e->GetOriginalFace();
        if (f == NULL || f->GetSurface() == NULL)
            return true; // NER
        SmFilletEdgeuse* p = eu_of(e, 0);
        if (p == NULL || p->GetMate() == NULL)
            return false;
        SmFilletEdge* rails[2] = {rail_at(s), rail_at(t)};
        for (SmFilletEdge* r : rails)
        {
            if (r == NULL)
                return true; // NER, before the other rail is read
            if (r->GetPrimaryEdgeuse() == NULL || r->GetPrimaryEdgeuse()->GetMate() == NULL ||
                r->GetCurve() == NULL)
                return false;
        }
        return true;
    }

    // CalcEdgeOnEdge: the original edge and its curve are dereferenced.
    static bool on_edge_safe(SmFilletEdge* e)
    {
        SmEdge* o = e->GetOriginalEdge();
        return o != NULL && o->GetCurve() != NULL;
    }

    // CalcFilletEnd: the vertices' classification objects are cast to edge uses and their
    // edges dereferenced; the solver of the primary edge use's geometry is dereferenced.
    static bool fillet_end_safe(SmFilletEdge* e)
    {
        SmFilletVertex *s, *t;
        ends(e, s, t);
        SmFilletEdgeuse* p = eu_of(e, 0);
        if (p == NULL)
            return false;
        SmFilletGeom* fg = p->GetFilletGeom();
        if (fg != NULL && fg->GetFilletSolver() == NULL)
            return false;
        SmFilletVertex* vs[2] = {s, t};
        for (SmFilletVertex* v : vs)
        {
            if (v->GetPointClass() != SM_PC_EDGEUSE)
                continue;
            SmObject* o = v->GetPointClassObject();
            if (o == NULL)
                continue; // NER
            if (!o->IsKindOf(SmEdgeuse_TYPE) || ((SmEdgeuse*)o)->GetEdge() == NULL ||
                ((SmEdgeuse*)o)->GetEdge()->GetPrimaryEdgeuse() == NULL)
                return false;
        }
        return true;
    }

    // CalcSetBackRail: the geometry's solver, the rail (by FindIndexOfRailOnFace) and the
    // rail's edge uses are dereferenced.
    static bool setback_safe(SmFilletEdge* e)
    {
        SmFilletEdgeuse* p = eu_of(e, 0);
        if (e->GetOriginalFace() == NULL || p == NULL)
            return true; // NER
        if (p->GetMate() == NULL)
            return false;
        SmFilletGeom* fg = p->GetFilletGeom();
        if (fg == NULL)
            return true; // NER
        SmFilletSolver* fs = fg->GetFilletSolver();
        if (fs == NULL)
            return false;
        SmEdgeuse* eu0 = fs->GetEdgeuse(0);
        if (eu0 != NULL && (eu0->GetLoopuse() == NULL || eu0->GetLoopuse()->GetFaceuse() == NULL))
            return false;
        ULONG idx = fs->FindIndexOfRailOnFace(e->GetOriginalFace());
        SmFilletEdge* r = fg->GetRail(idx);
        if (r == NULL)
            return false;
        if (r->GetCurve() == NULL || !r->GetCurve()->IsKindOf(SmBSplineCurve_TYPE))
            return true; // NER
        return r->GetPrimaryEdgeuse() != NULL && r->GetPrimaryEdgeuse()->GetMate() != NULL;
    }

    // ShouldSwapFaces: the curve (with an interval or a classification) or the corner's
    // vertex is dereferenced.
    static bool swap_safe(SmFilletEdge* e)
    {
        SmExtent1d ivl = e->GetInterval();
        if (!(ivl.IsInit() || ivl.HasNegativeLength()) || e->GetCurveClass() != NULL)
            return e->GetCurve() != NULL;
        if (e->GetFilletCorner() != NULL)
            return e->GetFilletCorner()->GetFilletedVertex() != NULL;
        return true;
    }

    // SmFilletGeom::Copy: the solver and both geometries' rails (and primary edge uses and
    // mates) are dereferenced past the surface/centre-line checks.
    static bool copy_safe(SmFilletGeom* g, SmFilletGeom* other)
    {
        if (g->GetFilletSolver() != other->GetFilletSolver())
            return true; // SM_ERR
        if (g->GetFilletSolver() == NULL)
            return false;
        for (ULONG i = 0; i < 2; ++i)
        {
            SmFilletEdge* a = other->GetRail(i);
            SmFilletEdge* b = g->GetRail(i);
            if (a == NULL || b == NULL || a->GetPrimaryEdgeuse() == NULL || a->GetPrimaryEdgeuse()->GetMate() == NULL)
                return false;
        }
        return true;
    }

    // The rail-end vertex use of GetRailVertexuse: the rail, its primary edge use and the
    // mate taken are dereferenced.
    static bool rail_vu_safe(SmFilletGeom* g, ULONG rail)
    {
        SmFilletEdge* r = rail < 2 ? g->GetRail(rail) : NULL;
        if (r == NULL || r->GetPrimaryEdgeuse() == NULL || r->GetPrimaryEdgeuse()->GetMate() == NULL)
            return false;
        return true;
    }

    // ---- export (layout in the Rust wrapper) ----

    void put_eu(std::vector<double>& o, const SmEdgeuse* eu) const
    {
        for (size_t i = 0; i < edges.size(); ++i)
        {
            const SmEdgeuse* p = edges[i]->GetPrimaryEdgeuse();
            if (p != NULL && p == eu)
            {
                o.push_back(ix((int64_t)i));
                o.push_back(0.0);
                return;
            }
            if (p != NULL && p->GetMate() == eu)
            {
                o.push_back(ix((int64_t)i));
                o.push_back(1.0);
                return;
            }
        }
        o.push_back(-1.0);
        o.push_back(-1.0);
    }

    void put(std::vector<double>& o) const
    {
        o.push_back(ix((int64_t)verts.size()));
        for (SmFilletVertex* v : verts)
        {
            SmPoint3d p = v->GetPoint();
            o.push_back(p.x);
            o.push_back(p.y);
            o.push_back(p.z);
            o.push_back((double)v->GetFilletVertexType());
            o.push_back((double)v->GetStatus());
            SmPoint2d uv = v->GetOriginalUV();
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)v->GetPointClass());
            o.push_back(v->GetOriginalTParam());
        }
        o.push_back(ix((int64_t)edges.size()));
        for (SmFilletEdge* e : edges)
        {
            o.push_back((double)e->GetFilletEdgeType());
            o.push_back((double)e->GetStatus());
            put_bcurve(o, e->GetCurve());
            SmExtent1d ivl = e->GetInterval();
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
            o.push_back(ix(g.find(7, e->GetOriginalEdge())));
            o.push_back(ix(g.find(3, e->GetOriginalFace())));
            for (int64_t which = 0; which < 2; ++which)
            {
                SmFilletEdgeuse* eu = eu_of(e, which);
                put_bcurve(o, eu->GetUVTrimCurvePointer());
                o.push_back(ix(index_of(geoms, eu->GetFilletGeom())));
                SmFilletVertexuse* vu = (SmFilletVertexuse*)eu->GetVertexuse();
                SmPoint2d vuv = vu->GetUVPoint();
                o.push_back(vuv.x);
                o.push_back(vuv.y);
            }
        }
        o.push_back(ix((int64_t)geoms.size()));
        for (SmFilletGeom* geom : geoms)
        {
            o.push_back((double)geom->GetStatus());
            const SmTArray<SmFilletEdgeuse*>& sides = geom->*GeomPeek::sides();
            o.push_back((double)sides.GetSize());
            for (ULONG k = 0; k < sides.GetSize(); ++k)
                put_eu(o, sides[k]);
            const SmTArray<SmFilletEdgeuse*>& refs = geom->*GeomPeek::refs();
            o.push_back((double)refs.GetSize());
            for (ULONG k = 0; k < refs.GetSize(); ++k)
                put_eu(o, refs[k]);
            put_bsurface(o, geom->*GeomPeek::surface());
            put_bcurve(o, geom->GetCenterLineCurve());
            o.push_back((geom->*GeomPeek::devs())[0]);
            o.push_back((geom->*GeomPeek::devs())[1]);
            for (ULONG k = 0; k < 2; ++k)
            {
                SmOffsetSurface* off = (geom->*GeomPeek::offs())[k];
                o.push_back(off != NULL ? 1.0 : 0.0);
                if (off != NULL)
                {
                    o.push_back(off->GetOffsetDistance());
                    put_bsurface(o, off->GetBaseSurface());
                }
            }
        }
    }
};

int32_t run(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Model m(ctx);
    SmStatus bst = SM_SUCCESS;
    m.target = build(rd, path, bst);
    if (m.target == NULL)
    {
        o.push_back(1.0); // the target B-rep cannot be built
        o.push_back(st(bst));
        return SC_OK;
    }
    walk(m.target, m.g);
    if (!m.build(rd))
        return SC_ERR_ARG;
    o.push_back(0.0);

    // op results that come before the model export (layout in the Rust wrapper)
    std::vector<double> r;
    SmStatus s = SM_SUCCESS;
    switch (op)
    {
    case OP_EXPORT: break;
    case OP_BLENDING_RAIL:
    case OP_EDGE_ON_EDGE:
    case OP_FILLET_END:
    case OP_SETBACK_RAIL:
    {
        SmFilletEdge* e = at(m.edges, rd.idx());
        bool use_corner = rd.flag();
        if (e == NULL)
            return SC_ERR_ARG;
        SmFilletCorner* corner = use_corner ? e->GetFilletCorner() : NULL;
        if (op == OP_BLENDING_RAIL)
        {
            if (!Model::blending_safe(e))
                return SC_ERR_ARG;
            s = e->CalcBlendingRail(ctx);
        }
        else if (op == OP_EDGE_ON_EDGE)
        {
            if (!Model::on_edge_safe(e))
                return SC_ERR_ARG;
            s = e->CalcEdgeOnEdge(ctx, corner);
        }
        else if (op == OP_FILLET_END)
        {
            if (!Model::fillet_end_safe(e))
                return SC_ERR_ARG;
            s = e->CalcFilletEnd(ctx, corner);
        }
        else
        {
            if (!Model::setback_safe(e))
                return SC_ERR_ARG;
            s = e->CalcSetBackRail(ctx, corner);
        }
        break;
    }
    case OP_SHOULD_SWAP:
    {
        SmFilletEdge* e = at(m.edges, rd.idx());
        SmFace* f1 = at(m.g.faces, rd.idx());
        SmFace* f2 = at(m.g.faces, rd.idx());
        if (e == NULL || f1 == NULL || f2 == NULL || !Model::swap_safe(e))
            return SC_ERR_ARG;
        r.push_back(flg(e->ShouldSwapFaces(f1, f2)));
        break;
    }
    case OP_JOIN:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        SmFilletVertex* vs = at(m.verts, rd.idx());
        SmFilletVertex* ve = at(m.verts, rd.idx());
        double uv_tol = rd.num();
        int64_t n = rd.idx();
        SmTArray<SmCurve*> c3;
        SmTArray<SmBSplineCurve*> cuv;
        SmTArray<SmOrientType> orients;
        for (int64_t k = 0; k < n; ++k)
        {
            SmFilletEdge* e = at(m.edges, rd.idx());
            int64_t which = rd.idx();
            bool with_uv = rd.flag();
            int64_t orient = rd.idx();
            SmFilletEdgeuse* eu = Model::eu_of(e, which);
            if (eu == NULL)
                return SC_ERR_ARG;
            c3.Add(e->GetCurve());
            cuv.Add(with_uv ? eu->GetUVTrimCurvePointer() : NULL);
            orients.Add((SmOrientType)orient);
        }
        if (geom == NULL || n < 1 || geom->GetFilletSolver() == NULL)
            return SC_ERR_ARG;
        SmTArray<SmFilletEdgeuse*>& sides = geom->*GeomPeek::sides();
        // the last curve of the list tested (UV or 3D) is evaluated, and every side edge use's
        // edge; with UV testing a gap above uv_tol evaluates the fillet surface
        bool all_uv = true;
        for (ULONG k = 0; k < cuv.GetSize(); ++k)
            all_uv = all_uv && cuv[k] != NULL;
        if ((all_uv ? (SmCurve*)cuv.GetLast() : c3.GetLast()) == NULL)
            return SC_ERR_ARG;
        for (ULONG k = 0; k < sides.GetSize(); ++k)
            if (sides[k]->GetEdge() == NULL)
                return SC_ERR_ARG;
        if (all_uv && geom->GetFilletSurface() == NULL)
            return SC_ERR_ARG;
        SmBoolean made = FALSE;
        s = sc_p6_join_two_fillet_verts_by_curves(geom, vs, ve, sides, uv_tol, c3, cuv, orients, made);
        r.push_back(flg(made));
        r.push_back((double)c3.GetSize());
        for (ULONG k = 0; k < c3.GetSize(); ++k)
        {
            put_bcurve(r, c3[k]);
            put_bcurve(r, cuv[k]);
            r.push_back((double)orients[k]);
        }
        break;
    }
    case OP_COPY:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        SmFilletGeom* other = at(m.geoms, rd.idx());
        if (geom == NULL || other == NULL || !Model::copy_safe(geom, other))
            return SC_ERR_ARG;
        s = geom->Copy(other);
        break;
    }
    case OP_FIND_TANGENT:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        int64_t rail = rd.idx();
        int64_t end = rd.idx();
        if (geom == NULL || rail > 1 || geom->GetRail((ULONG)rail) == NULL)
            return SC_ERR_ARG;
        SmFilletEdge* re = geom->GetRail((ULONG)rail);
        SmFilletVertex* fv = (SmFilletVertex*)re->GetVertex();
        if (end != 0)
            fv = (SmFilletVertex*)re->GetOtherVertex(fv);
        if (fv == NULL)
            return SC_ERR_ARG;
        if (fv->GetPointClass() == SM_PC_EDGE)
        {
            SmObject* ob = fv->GetPointClassObject();
            if (ob != NULL && !ob->IsKindOf(SmEdge_TYPE))
                return SC_ERR_ARG;
        }
        SmBoolean found = FALSE;
        SmVector3d tan(0.0, 0.0, 0.0);
        s = geom->FindEdgeTangentAtPoint((ULONG)rail, (ULONG)end, found, tan);
        r.push_back(flg(found));
        put_vec(r, tan);
        break;
    }
    case OP_RAIL_INDEX_SIDE_EU:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        SmEdgeuse* eu = at(m.g.edgeuses, rd.idx());
        if (geom == NULL || eu == NULL)
            return SC_ERR_ARG;
        if (geom->GetRail(0) == NULL || geom->GetRail(1) == NULL)
        {
            SmFilletSolver* fs = geom->GetFilletSolver();
            SmEdgeuse* eu0 = fs == NULL ? NULL : fs->GetEdgeuse(0);
            if (fs == NULL || eu0 == NULL || eu->GetEdge() == NULL)
                return SC_ERR_ARG;
            if (eu->GetEdge()->IsClosed())
            {
                if (eu0->GetVertexuse() == NULL || eu->GetVertexuse() == NULL)
                    return SC_ERR_ARG;
                if (eu->GetVertexuse()->GetVertex() != eu0->GetVertexuse()->GetVertex() &&
                    eu0->GetCCWEdgeuse() == NULL)
                    return SC_ERR_ARG;
            }
        }
        ULONG idx = 99;
        s = geom->GetRailIndexOfSideEU(eu, idx);
        r.push_back(s == SM_SUCCESS ? (double)idx : 99.0); // the out-param only on success
        break;
    }
    case OP_RAIL_INDEX_VU:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        int64_t ei = rd.idx();
        int64_t which = rd.idx();
        SmFilletEdgeuse* eu = Model::eu_of(at(m.edges, ei), which);
        if (geom == NULL || eu == NULL)
            return SC_ERR_ARG;
        SmFilletVertexuse* vu = (SmFilletVertexuse*)eu->GetVertexuse();
        // GetRailVertexuse dereferences rail 0, and rail 1 when the use is not at rail 0
        if (!Model::rail_vu_safe(geom, 0))
            return SC_ERR_ARG;
        if (geom->GetRailVertexuse(0, 0) != vu && geom->GetRailVertexuse(0, 1) != vu &&
            !Model::rail_vu_safe(geom, 1))
            return SC_ERR_ARG;
        ULONG idx = 99;
        s = geom->GetRailIndexOfVertexuse(vu, idx);
        r.push_back(s == SM_SUCCESS ? (double)idx : 99.0); // the out-param only on success
        break;
    }
    case OP_RAIL_VERTEX:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        int64_t rail = rd.idx();
        int64_t end = rd.idx();
        if (geom == NULL || rail > 1 || end > 1 || !Model::rail_vu_safe(geom, (ULONG)rail))
            return SC_ERR_ARG;
        SmFilletVertex* v = geom->GetRailVertex((ULONG)rail, (ULONG)end);
        r.push_back(ix(index_of(m.verts, v)));
        break;
    }
    case OP_SETUP_RAIL_POINT:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        int64_t rail = rd.idx();
        int64_t end = rd.idx();
        double param = rd.num();
        bool has_pc = rd.flag();
        int64_t pc_class = rd.idx();
        int64_t pc_kind = rd.idx();
        int64_t pc_index = rd.idx();
        double pc_t = rd.num();
        if (geom == NULL || rail > 1 || end > 1 || !Model::rail_vu_safe(geom, (ULONG)rail))
            return SC_ERR_ARG;
        SmFilletVertexuse* vu = geom->GetRailVertexuse((ULONG)rail, (ULONG)end);
        SmFilletVertex* fv = (SmFilletVertex*)vu->GetVertex();
        if (!fv->IsProcessed() && geom->GetRail((ULONG)rail)->GetCurve() == NULL)
            return SC_ERR_ARG;
        SmPointClassification pc(SmTol::GetZoneTol3d());
        pc.SetClassObject((SmPointClassificationType)pc_class, m.pc_object(pc_kind, pc_index));
        pc.SetTParam(pc_t);
        s = geom->SetUpRailPoint((ULONG)rail, (ULONG)end, param, has_pc ? &pc : NULL);
        break;
    }
    case OP_MOVE_TSP:
    {
        int64_t n = rd.idx();
        std::vector<SmTsectPnt> pnts((size_t)n);
        SmTArray<SmTsectPnt*> arr;
        for (int64_t k = 0; k < n; ++k)
        {
            pnts[(size_t)k].CrvPos() = rd.vec();
            pnts[(size_t)k].CrvDeriv() = rd.vec();
        }
        for (int64_t k = 0; k < n; ++k)
            arr.Add(&pnts[(size_t)k]);
        int64_t from = rd.idx();
        int64_t to = rd.idx();
        s = sc_p6_move_tsp(arr, (ULONG)from, (ULONG)to);
        r.push_back((double)arr.GetSize());
        for (ULONG k = 0; k < arr.GetSize(); ++k)
            r.push_back((double)(arr[k] - pnts.data()));
        break;
    }
    case OP_RAIL_EU_INTERSECT:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        int64_t rail = rd.idx();
        SmEdgeuse* eu = at(m.g.edgeuses, rd.idx());
        double guess = rd.num();
        if (geom == NULL || eu == NULL || rail > 1)
            return SC_ERR_ARG;
        SmFilletSolver* fs = geom->GetFilletSolver();
        // without a solver: NER(pFS); else the base solver reads both of its surfaces
        // (through the geometry: missing offset surfaces recurse without bound, UB-204),
        // the side edge use's edge and its UV trim curve (made on demand)
        if (fs != NULL &&
            (eu->GetEdge() == NULL || eu->GetFace() == NULL ||
             (geom->*GeomPeek::offs())[0] == NULL || (geom->*GeomPeek::offs())[1] == NULL))
            return SC_ERR_ARG;
        SmBoolean found = FALSE;
        SmTsectPnt t;
        double param = 0.0;
        s = geom->RailEdgeuseIntersect((ULONG)rail, eu, guess, found, t, param);
        r.push_back(flg(found));
        r.push_back(param);
        break;
    }
    case OP_RECORD_ERROR:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        int64_t type = rd.idx();
        if (geom == NULL || geom->GetFilletSolver() == NULL || geom->GetFilletSolver()->GetFilletExecutive() == NULL)
            return SC_ERR_ARG;
        SmEdge* filleted_edge = (!m.g.edges.empty() && (type & 1) != 0) ? m.g.edges[0] : NULL;
        SmFace* target_face = (!m.g.faces.empty() && (type & 2) != 0) ? m.g.faces[0] : NULL;
        SmFilletEdge* rail = ((type & 4) != 0) ? geom->GetRail(0) : NULL;
        SmSurface* surf = geom->*GeomPeek::surface();
        const TCHAR* comment = (type & 1) != 0 ? _T("SmFilletGeom::RecordFilletError") : NULL;
        geom->RecordFilletError((SmFilletErrorType)type, filleted_edge, target_face, rail, surf, comment);
        SmFilletErrorInfo* info = m.exec->GetFilletErrorInfo();
        r.push_back((double)info->GetErrorCode());
        r.push_back((double)(info->*get(StatusTag())));
        r.push_back((double)info->m_eEdgeType);
        r.push_back(ix(info->m_pFilletedEdge == NULL ? -1 : index_of(m.g.edges, (SmEdge*)info->m_pFilletedEdge)));
        r.push_back(ix(info->m_pSideFace == NULL ? -1 : index_of(m.g.faces, (SmFace*)info->m_pSideFace)));
        r.push_back((double)info->m_pFilletEdges.GetSize());
        for (ULONG k = 0; k < info->m_pFilletEdges.GetSize(); ++k)
            r.push_back(ix(info->m_pFilletEdges[k] == NULL ? -1 : index_of(m.edges, info->m_pFilletEdges[k])));
        r.push_back((double)info->m_pFilletSurfaces.GetSize());
        for (ULONG k = 0; k < info->m_pFilletSurfaces.GetSize(); ++k)
            r.push_back(flg(info->m_pFilletSurfaces[k] != NULL));
        r.push_back((double)std::strlen(info->m_cComment));
        break;
    }
    default: return SC_ERR_ARG;
    }
    o.push_back(st(s));
    o.insert(o.end(), r.begin(), r.end());
    m.put(o);
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_geom_p6_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                               double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    int32_t r = run(op, rd, path, o);
    if (r != SC_OK)
        return r;
    (void)n_in;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
