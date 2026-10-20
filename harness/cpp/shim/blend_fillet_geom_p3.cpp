// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_geom_p3`: SmFilletVertex::CalcCornerVertGeom,
// SmFilletVertex::CalcMateGeom, SmFilletEdge::CalcCornerEdgeGeom,
// SmFilletEdge::CalcFilletIntFillet and SmFilletGeom::ReCalcFilletGeom of upstream
// source/SMLib/src/SmFilletGeom.cpp (class members, linked from the library).
//
// One entry point, sc_blend_fillet_geom_p3_eval(op, path, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/blend_fillet_geom_p3.rs, which validates the
// blocks; the shim also refuses (SC_ERR_ARG) a model on which the op would dereference an object
// of the wrong kind or NULL inside upstream (checks below), or recurse through a cycle of mates.
//
// Each call builds a target SmBrep from a B-rep block of unit `topology.edgeuse_p1` (objects are
// addressed by their traversal-order export index), fillet solvers (a minimal SmFilletSolver
// subclass: every virtual is the base version, except that SetupOffsetValues may succeed without
// changing anything), and a pseudo SmFilletBrep of fillet vertices, geometries and edges
// (SmFilletEdge::MakeFilletEdge). The export reads raw members, never the functions under test.
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
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <cstdint>
#include <new>
#include <set>
#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_geom_p3.rs).
enum : int32_t
{
    OP_EXPORT = 0,
    OP_CORNER_VERT = 1,
    OP_MATE = 2,
    OP_CORNER_EDGE = 3,
    OP_FILLET_INT_FILLET = 4,
    OP_RECALC = 5,
};

// A base SmFilletSolver; `setup` makes SetupOffsetValues succeed (the base fails).
struct ShimFS : SmFilletSolver
{
    bool setup = false;
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
    virtual SmStatus SetupOffsetValues(const SmTsectPnt& t, double* d = NULL)
    {
        if (!setup)
            return SmFilletSolver::SetupOffsetValues(t, d);
        return SM_SUCCESS;
    }
    void set_edgeuses(SmEdgeuse* a, SmEdgeuse* b)
    {
        m_pEdgeuses[0] = a;
        m_pEdgeuses[1] = b;
    }
    void set_extend(bool before, bool after)
    {
        m_bExtendBefore = before ? TRUE : FALSE;
        m_bExtendAfter = after ? TRUE : FALSE;
    }
};

// A fillet corner that only carries its context (CalcCornerEdgeGeom reads GetContext()).
// The SmFilletCorner constructor reads pExec->m_pPseudoBrep and nothing else of the
// executive, so it gets zeroed storage standing in for one (a NULL pseudo B-rep).
struct ShimCorner : SmFilletCorner
{
    ShimCorner(const SmContext& ctx, SmFilletExecutive* exec) : SmFilletCorner(ctx, NULL, NULL, NULL, exec, 1.0e-6, 0.01) {}
    virtual SmStatus MakeCornerTopology() { return SM_ERR; }
    virtual SmFilletCornerType GetCornerType() const { return SM_FCR_UNKNOWN; }
    virtual void GetName(TCHAR* pc, size_t n) const
    {
        if (n > 0)
            pc[0] = 0;
    }
};

alignas(SmFilletExecutive) unsigned char g_exec_storage[sizeof(SmFilletExecutive)] = {};

// The executive storage with a constructed error info: a failed fillet x fillet intersection
// records its error through the corner (RecordFilletError -> NoteFilletError ->
// GetFilletErrorInfo()->NoteError, which adds to the info's surface array).
SmFilletExecutive* shim_executive()
{
    static bool made = false;
    SmFilletExecutive* exec = (SmFilletExecutive*)(void*)g_exec_storage;
    if (!made)
    {
        ::new ((void*)exec->GetFilletErrorInfo()) SmFilletErrorInfo(exec);
        made = true;
    }
    return exec;
}

struct GeomPeek : SmFilletGeom
{
    static SmTArray<SmFilletEdge*> SmFilletGeom::*other1() { return &GeomPeek::m_vOtherRails1; }
    static SmTArray<SmFilletEdge*> SmFilletGeom::*other2() { return &GeomPeek::m_vOtherRails2; }
    static SmTArray<SmFilletEdgeuse*> SmFilletGeom::*refs() { return &GeomPeek::m_vRefEUs; }
    static SmTArray<SmFilletEdgeuse*> SmFilletGeom::*sides() { return &GeomPeek::m_vSideEUs; }
};

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

struct Model
{
    const SmContext& ctx;
    Graph g;
    SmBrep* target = NULL;
    SmFilletBrep* pseudo = NULL;
    std::vector<ShimFS*> solvers;
    std::vector<SmSurface*> bases; // owned here: nobody owns an offset's base surface
    std::vector<SmFilletVertex*> verts;
    std::vector<SmFilletGeom*> geoms;
    std::vector<SmSurface*> geomSurfaces; // owned here: SmFilletGeom does not delete its surface
    std::vector<SmFilletEdge*> edges;
    std::vector<ShimCorner*> corners; // by tag index, made on first use

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
        for (size_t i = 0; i < geoms.size(); ++i)
            delete geoms[i];
        for (size_t i = 0; i < geomSurfaces.size(); ++i)
            delete geomSurfaces[i];
        for (size_t i = 0; i < solvers.size(); ++i)
            delete solvers[i];
        for (size_t i = 0; i < bases.size(); ++i)
            delete bases[i];
        for (size_t i = 0; i < corners.size(); ++i)
            delete corners[i];
        delete target;
    }

    SmFilletCorner* corner_tag(int64_t i)
    {
        if (i < 0)
            return NULL;
        while ((int64_t)corners.size() <= i)
            corners.push_back(NULL);
        if (corners[(size_t)i] == NULL)
            corners[(size_t)i] = new (ctx) ShimCorner(ctx, shim_executive());
        return corners[(size_t)i];
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
            bool before = rd.flag(), after = rd.flag();
            fs->set_extend(before, after);
            fs->setup = rd.flag();
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
            SmFilletVertex* fv = new (pseudo) SmFilletVertex(corner_tag(corner));
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
            if (SmFilletEdge::MakeFilletEdge(pseudo, vs, ve, e, corner_tag(corner)) != SM_SUCCESS || e == NULL)
                return false;
            edges.push_back(e);
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

    // FindIndexOfRailXSideEdgeuse(side) of `fs` dereferences: the side edge use's edge, edge
    // use 0, on a closed side edge both vertex uses and (vertices differing) edge use 0's
    // counter-clockwise neighbour.
    static bool rail_x_side_safe(SmFilletSolver* fs, SmEdgeuse* side)
    {
        SmEdgeuse* eu0 = fs->GetEdgeuse(0);
        if (side == NULL || side->GetEdge() == NULL || eu0 == NULL)
            return false;
        if (side->GetEdge()->IsClosed())
        {
            if (eu0->GetVertexuse() == NULL || side->GetVertexuse() == NULL)
                return false;
            if (side->GetVertexuse()->GetVertex() != eu0->GetVertexuse()->GetVertex() &&
                eu0->GetCCWEdgeuse() == NULL)
                return false;
        }
        return true;
    }

    // The vertex calculator of an unported unit (part 5): it reads corner and executive data
    // the shim does not build.
    static bool other_unit_vertex(SmFilletVertexType t) { return t == SM_FV_FILLET_X_FILLET; }

    // The vertex calculators of parts 2 and 4 (ported, called by the dispatcher). Each asks
    // the vertex's corner for its filleted vertex, which the shim's corners do not have, so
    // they run only on vertices without a corner, as in the shims of those units.
    static bool ported_unit_vertex(SmFilletVertexType t)
    {
        switch (t)
        {
        case SM_FV_RAIL_X_RAIL:
        case SM_FV_RAIL_X_EDGEUSE:
        case SM_FV_RAIL_X_EXTENDED_EDGEUSE:
        case SM_FV_FILLET_X2_FILLETS:
        case SM_FV_ON_CROSS_SECTION:
        case SM_FV_SETBACK: return true;
        default: return false;
        }
    }

    // The edge calculators of unported units (parts 5, 6), likewise. CalcRailRailInterpolation
    // (part 2) runs: the shim's corners have no fillet edges, so its corner branch returns
    // SM_ERR before it reads anything else of them.
    static bool other_unit_edge(SmFilletEdgeType t)
    {
        switch (t)
        {
        case SM_FE_CROSS_SECTION:
        case SM_FE_FILLET_X_SIDE_FACE:
        case SM_FE_ON_EDGE:
        case SM_FE_ON_EXTEND_EDGE:
        case SM_FE_FILLET_END:
        case SM_FE_SETBACK_RAIL:
        case SM_FE_RAIL_EXTENSION: return true;
        default: return false;
        }
    }

    // CalcCornerVertGeom(v) / CalcMateGeom(v) on the vertex types this unit handles.
    bool vertex_safe(SmFilletVertex* v, bool as_mate, std::set<SmFilletVertex*>& seen)
    {
        if (!as_mate && v->IsProcessed())
            return true;
        SmFilletVertexType t = v->GetFilletVertexType();
        if (!as_mate && t == SM_FV_ON_VERTEX)
        {
            // CalcVertOnVert dereferences the classification object as an SmVertex
            SmObject* o = v->GetPointClassObject();
            return o != NULL && o->IsKindOf(SmVertex_TYPE);
        }
        if (!as_mate && ported_unit_vertex(t))
            return v->GetFilletCorner() == NULL;
        if (!as_mate && t != SM_FV_MATE)
            return !other_unit_vertex(t);
        if (seen.count(v) != 0)
            return false; // a cycle of mates recurses without end
        seen.insert(v);
        SmFilletVertex* main = v->GetMate(0);
        if (main == NULL)
            return true;
        if (!main->IsProcessed() && !vertex_safe(main, false, seen))
            return false;
        // the main vertex's object is cast to an SmEdgeuse, ours to an SmFace
        SmObject* side = main->GetPointClassObject();
        if (side != NULL && !side->IsKindOf(SmEdgeuse_TYPE))
            return false;
        SmObject* face = v->GetPointClassObject();
        if (face != NULL && !face->IsKindOf(SmFace_TYPE))
            return false;
        SmTArray<SmFilletGeom*> fgs;
        v->GetFilletGeoms(fgs);
        if (fgs.GetSize() < 1 || side == NULL)
            return true;
        SmFilletSolver* fs = fgs[0]->GetFilletSolver();
        if (fs == NULL)
            return true;
        if (!rail_x_side_safe(fs, (SmEdgeuse*)side))
            return false;
        // the main vertex's rail-end vertex use is dereferenced
        return main->GetVUAtRailEnd(fgs[0]) != NULL;
    }

    // CalcFilletIntFillet(e): the solvers of both edge uses' geometries are dereferenced, on
    // the trimming path both UV trim curves, and without a 3D curve the edge's corner.
    bool fif_safe(SmFilletEdge* e)
    {
        SmFilletEdgeuse* p = eu_of(e, 0);
        SmFilletEdgeuse* m = eu_of(e, 1);
        if (p == NULL || m == NULL)
            return false;
        SmFilletGeom* g1 = p->GetFilletGeom();
        SmFilletGeom* g2 = m->GetFilletGeom();
        if (g1 == NULL || g2 == NULL)
            return true;
        SmFilletVertex* vs = (SmFilletVertex*)e->GetVertex();
        SmFilletVertex* ve = (SmFilletVertex*)e->GetOtherVertex(vs);
        if (vs == NULL || ve == NULL)
            return false;
        if (!vs->IsProcessed() || !ve->IsProcessed())
            return true;
        if (g1->GetFilletSolver() == NULL || g2->GetFilletSolver() == NULL)
            return false;
        if (g1->GetFilletSurface() == NULL || g2->GetFilletSurface() == NULL)
            return true;
        // without a B-spline 3D curve the surface/surface intersection is called with
        // m_cpCorner->GetContext() (UB-210)
        if (e->GetCurve() == NULL || !e->GetCurve()->IsKindOf(SmBSplineCurve_TYPE))
            return e->GetFilletCorner() != NULL;
        return p->GetUVTrimCurvePointer() != NULL && m->GetUVTrimCurvePointer() != NULL;
    }

    // ReCalcFilletGeom without a march direction, when rail 0 has no curve, reads
    // sTsectPnts[0]->CrvDeriv() unchecked (SmFilletGeom.cpp:11648). True when its sort loop
    // (:11431-11507) runs to the end without sorting a rail-end point: every vertex there is
    // unprocessed, a mate or an extended-edge-use end, or its tsect point is unset
    // (-SM_BIG_DOUBLE). A loop that returns an error first (NER, a rail without two edge uses)
    // never gets there, and a point that reaches the curve solve is either sorted or an error.
    static bool recalc_sorts_nothing(SmFilletGeom* geom, SmFilletSolver* fs)
    {
        SmFilletGeomType t = geom->GetFilletGeomType();
        bool b3d = t == SM_FG_CORNER_FILLET || t == SM_FG_TANGENT_ROLLOVER;
        for (ULONG jj = 0; jj < 2; ++jj)
        {
            if (!b3d && (fs->GetEdgeuse(jj) == NULL || fs->GetEdgeuse(jj)->GetUVTrimCurve() == NULL))
                return false;
            SmTArray<SmFilletEdge*> rails;
            rails.Add(geom->GetRail(jj));
            rails.Append(jj == 0 ? geom->*GeomPeek::other1() : geom->*GeomPeek::other2());
            for (ULONG r = 0; r < rails.GetSize(); ++r)
            {
                if (rails[r] == NULL)
                    return false;
                SmTArray<SmEdgeuse*> eus;
                rails[r]->GetEdgeuses(eus);
                if (eus.GetSize() != 2)
                    return false;
                for (ULONG k = 0; k < 2; ++k)
                {
                    SmFilletVertexuse* vu = (SmFilletVertexuse*)eus[k]->GetVertexuse();
                    SmFilletVertex* fv = vu == NULL ? NULL : (SmFilletVertex*)vu->GetVertex();
                    if (fv == NULL)
                        return false;
                    if (fv->GetStatus() == SM_FIL_UNPROCESSED || fv->GetFilletVertexType() == SM_FV_MATE ||
                        fv->GetFilletVertexType() == SM_FV_RAIL_X_EXTENDED_EDGEUSE)
                        continue;
                    SmTsectPnt& tp = vu->GetTsectPnt();
                    if (!b3d)
                    {
                        SmPoint2d uv = tp.UVPos(jj);
                        if (uv.x == -SM_BIG_DOUBLE || uv.y == -SM_BIG_DOUBLE)
                            continue;
                        return false;
                    }
                    if (tp.m_ePointType == SM_IP_UNKNOWN || tp.CrvPos().IsUndef())
                    {
                        SmPoint2d uv = tp.UVPos(0);
                        if (uv.x == -SM_BIG_DOUBLE || uv.y == -SM_BIG_DOUBLE)
                            continue;
                    }
                    return false;
                }
            }
        }
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
            for (int64_t which = 0; which < 2; ++which)
            {
                SmFilletEdgeuse* eu = eu_of(e, which);
                put_bcurve(o, eu->GetUVTrimCurvePointer());
                o.push_back(ix(index_of(geoms, eu->GetFilletGeom())));
                SmFilletVertexuse* vu = (SmFilletVertexuse*)eu->GetVertexuse();
                SmTsectPnt& t = vu->GetTsectPnt();
                o.push_back((double)t.m_ePointType);
                o.push_back(t.UVPos(0).x);
                o.push_back(t.UVPos(0).y);
                o.push_back(t.UVPos(1).x);
                o.push_back(t.UVPos(1).y);
                o.push_back(t.CrvPos().x);
                o.push_back(t.CrvPos().y);
                o.push_back(t.CrvPos().z);
                o.push_back(t.m_dCurveParameter);
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

    SmStatus s = SM_SUCCESS;
    switch (op)
    {
    case OP_EXPORT: break;
    case OP_CORNER_VERT:
    case OP_MATE:
    {
        SmFilletVertex* v = at(m.verts, rd.idx());
        if (v == NULL)
            return SC_ERR_ARG;
        std::set<SmFilletVertex*> seen;
        if (!m.vertex_safe(v, op == OP_MATE, seen))
            return SC_ERR_ARG;
        s = op == OP_MATE ? v->CalcMateGeom() : v->CalcCornerVertGeom();
        break;
    }
    case OP_CORNER_EDGE:
    case OP_FILLET_INT_FILLET:
    {
        SmFilletEdge* e = at(m.edges, rd.idx());
        if (e == NULL)
            return SC_ERR_ARG;
        if (op == OP_CORNER_EDGE && Model::other_unit_edge(e->GetFilletEdgeType()))
            return SC_ERR_ARG;
        bool fif = op == OP_FILLET_INT_FILLET || e->GetFilletEdgeType() == SM_FE_FILLET_X_FILLET;
        if (fif && !m.fif_safe(e))
            return SC_ERR_ARG;
        if (op == OP_FILLET_INT_FILLET)
            s = e->CalcFilletIntFillet(ctx, e->GetFilletCorner());
        else
            s = e->CalcCornerEdgeGeom();
        break;
    }
    case OP_RECALC:
    {
        SmFilletGeom* geom = at(m.geoms, rd.idx());
        bool analytic = rd.flag();
        bool has_dir = rd.flag();
        SmVector3d dir = rd.vec();
        SmBSplineCurve* ref = NULL;
        if (rd.flag())
            ref = read_curve(rd, ctx);
        // upstream dereferences the solver, its offset surfaces and (but for a corner
        // fillet) its first edge use; the analytic fillets (CalcConeFilletGeom, unit
        // blend.fillet_solver_p2) read solver data this shim does not build; without a
        // march direction it may read the first of no sorted points (UB-208)
        SmFilletSolver* fs = geom == NULL ? NULL : geom->GetFilletSolver();
        if (fs == NULL || fs->GetSurface(0) == NULL || fs->GetSurface(1) == NULL ||
            (geom->GetFilletGeomType() != SM_FG_CORNER_FILLET && fs->GetEdgeuse(0) == NULL) ||
            (analytic && geom->GetFilletGeomType() != SM_FG_TANGENT_ROLLOVER) ||
            (!has_dir && geom->GetRail(0) != NULL && geom->GetRail(0)->GetCurve() == NULL &&
             Model::recalc_sorts_nothing(geom, fs)))
        {
            delete ref;
            return SC_ERR_ARG;
        }
        s = geom->ReCalcFilletGeom(analytic ? TRUE : FALSE, has_dir ? &dir : NULL, ref);
        delete ref;
        break;
    }
    default: return SC_ERR_ARG;
    }
    o.push_back(st(s));
    m.put(o);
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_geom_p3_eval(int32_t op, const char* path, const double* in, int64_t n_in,
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
