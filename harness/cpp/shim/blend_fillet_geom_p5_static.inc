// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp (after blend_fillet_geom_p4_static.inc), the
// translation unit that compiles upstream SmFilletGeom.cpp: the difftest shim of unit
// `blend.fillet_geom_p5` (SmFilletVertex::CalcFilletIntFillet, SmFilletEdge::CalcCrossSection,
// CalcFilletIntSideFace, CalcEdgeOnExtendedEdge, MakeFilletEdge, SmFilletGeom::MakeRailEdge,
// CreateEndCurve (and FindEdgeTangentAtPoint for SM_BT_BLEND), TrimRailCurves (and
// TrimCliffRails through it), UpdateSplitFace). Every op
// builds the pseudo B-rep with blend_fillet_geom_p1.cpp's Model and the base solvers of
// blend_fillet_geom_p4_static.inc. Not compiled on its own (blend_fillet_geom_p5.cpp only points
// here).
//
// One entry point, sc_blend_fillet_geom_p5_eval(op, in, n_in, out, cap, out_len). Layouts and
// the refusals (SC_ERR_ARG) are documented in crates/solidcore-ffi/src/units/
// blend_fillet_geom_p5.rs, which validates every input; reads are bounds checked here too.

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_geom_p5.rs).
enum : int32_t
{
    P5_INT_FILLET = 0,
    P5_CROSS = 1,
    P5_SIDE = 2,
    P5_EXT_EDGE = 3,
    P5_TRIM = 4,
    P5_SPLIT = 5,
    P5_MAKE_EDGE = 6,
    P5_RAIL_EDGE = 7,
    P5_END = 8,
    P5_RAIL_EU = 9,
};

bool p5_is_bspline(const SmCurve* c) { return c != NULL && c->IsKindOf(SmBSplineCurve_TYPE); }

void p5_put_sides(std::vector<double>& o, const Model& M)
{
    for (size_t g = 0; g < M.geoms.size(); ++g)
        o.push_back((double)(M.geoms[g]->*GeomPeek::sides()).GetSize());
}

// CalcCrossSection(ctx, NULL) up to its first unchecked dereference: false when upstream would
// dereference a missing solver or a missing UV curve (UB-213: sm_CreateUVCurve succeeding with
// no curve, then `*pUVCurve1`, SmFilletGeom.cpp:5759-5768).
//
// Cache warm-up: this runs sm_CreateUVCurve once before the real CalcCrossSection, which runs
// it again. The first call builds the surface's object cache (SmCacheMgr), so the measured C++
// call runs warm while the Rust side runs cold. The two are bit-identical today, so the warm-up
// is not hiding a difference; if a cache-dependent disagreement ever shows up here, look at the
// cache first (or rebuild the model for the real call).
bool p5_cross_runs(const SmContext& ctx, SmFilletEdge* e)
{
    SmFilletVertex* sv = (SmFilletVertex*)e->GetVertex();
    SmFilletVertex* ev = (SmFilletVertex*)e->GetOtherVertex(sv);
    SmFilletEdgeuse* p = (SmFilletEdgeuse*)e->GetPrimaryEdgeuse();
    SmFilletEdgeuse* m = (SmFilletEdgeuse*)p->GetMate();
    SmFilletGeom* g1 = p->GetFilletGeom();
    SmFilletGeom* g2 = m->GetFilletGeom();
    SmSurface* s1 = NULL;
    double tol = 0.0;
    if (g1)
    {
        s1 = g1->GetFilletSurface();
        if (s1 == NULL)
            return true; // NER
        if (g2 != NULL && g2 != g1)
        {
            if (g2->GetFilletSolver() == NULL || g2->GetFilletSurface() == NULL)
                return true; // NER
        }
        if (g1->GetFilletSolver() == NULL)
            return false; // dTol = pFilSolver1->GetThisApproxTol3d()
        tol = g1->GetFilletSolver()->GetThisApproxTol3d();
    }
    else
    {
        SmFace* f = e->GetOriginalFace();
        if (f == NULL || f->GetSurface() == NULL)
            return true; // NER
        s1 = f->GetSurface();
        tol = f->GetTolerance();
    }
    // the same sm_CreateUVCurve call: a success without any curve makes upstream lift NULL
    SmBSplineCurve* c3 = NULL;
    SmBSplineCurve* u1 = NULL;
    SmBSplineCurve* u2 = NULL;
    SmStatus r = sm_CreateUVCurve(ctx, sv, ev, FALSE, s1, 50.0 * tol, c3, u1, u2);
    bool ok = !(r == SM_SUCCESS && c3 == NULL && u1 == NULL);
    delete c3;
    delete u1;
    delete u2;
    return ok;
}

// CalcFilletIntSideFace(ctx, NULL): false when upstream would dereference the NULL corner
// (UB-214: `pCorner->IsExtendedSurfacePatch()` at SmFilletGeom.cpp:6219, tested only at :6223)
// or a missing solver.
bool p5_side_runs(SmFilletEdge* e)
{
    SmFilletVertex* sv = (SmFilletVertex*)e->GetVertex();
    SmFilletVertex* ev = (SmFilletVertex*)e->GetOtherVertex(sv);
    if (!sv->IsProcessed() || !ev->IsProcessed())
        return true;
    SmFilletEdgeuse* p = (SmFilletEdgeuse*)e->GetPrimaryEdgeuse();
    SmFilletGeom* g1 = p->GetFilletGeom();
    if (g1 == NULL)
        return true;
    if (g1->GetFilletSolver() == NULL)
        return false;
    if (!p5_is_bspline(e->GetCurve()) && g1->GetFilletSurface() != NULL)
        return false; // pCorner->IsExtendedSurfacePatch()
    return true;
}

// One side's rail pieces in TrimCliffRails' order (sAllCurvesThisRail): the main rail, then the
// other rails of that side.
std::vector<SmFilletEdge*> p5_side_rails(SmFilletGeom* g, ULONG ii)
{
    std::vector<SmFilletEdge*> all;
    all.push_back(g->GetRail(ii));
    const SmTArray<SmFilletEdge*>& other = ii == 0 ? (g->*GeomPeek::other1()) : (g->*GeomPeek::other2());
    for (ULONG k = 0; k < other.GetSize(); ++k)
        all.push_back(other[k]);
    return all;
}

// TrimRailCurves: false when upstream would dereference a missing solver or rail, or what the
// harness cannot provide in TrimCliffRails. The cliff checks are conservative: they look at every
// piece of both sides, although an earlier failure (an unsynchronised rail, a failed drop) would
// return before upstream reached a later piece.
//  - an unprocessed SM_FV_CLIFF_RAIL_X_EDGE vertex is computed by CalcCliffRailIntEdge, which
//    casts its classification object to an SmEdgeuse* unchecked (SmFilletGeom.cpp:3446): the
//    object must be an edge use (the model classifies such vertices SM_PC_EDGEUSE, as
//    SmFilletSolver.cpp:7041-7042 and 7640-7641 do);
//  - a cliff rail takes its side surface from the edge use's fillet geometry when that is another
//    geometry, else from pRail->GetOriginalFace()->GetSurface() (cpp:10931-10934), and the model
//    has no original face: the geometry must be another one.
bool p5_trim_runs(SmFilletGeom* g)
{
    bool cliff = g->GetFilletGeomType() == SM_FG_CLIFF_ROLLOVER;
    if (cliff && g->GetFilletSolver() == NULL)
        return false; // m_pFilletSolver->GetCreationContext()
    if (g->GetFilletSurface() == NULL)
        return true; // NER
    if (g->GetFilletSolver() == NULL)
        return false;
    for (ULONG ii = 0; ii < 2; ++ii)
        if (g->GetRail(ii) == NULL)
            return false;
    if (cliff)
    {
        for (ULONG ii = 0; ii < 2; ++ii)
        {
            std::vector<SmFilletEdge*> all = p5_side_rails(g, ii);
            for (size_t k = 0; k < all.size(); ++k)
            {
                SmFilletEdge* r = all[k];
                SmFilletVertex* a = (SmFilletVertex*)r->GetVertex();
                SmFilletVertex* b = (SmFilletVertex*)r->GetOtherVertex(a);
                SmFilletVertex* ends[2] = {a, b};
                for (int w = 0; w < 2; ++w)
                {
                    SmFilletVertex* v = ends[w];
                    if (!v->IsProcessed() && v->GetFilletVertexType() == SM_FV_CLIFF_RAIL_X_EDGE)
                    {
                        const SmObject* obj = v->GetPointClassObject();
                        if (obj == NULL || !obj->IsKindOf(SmEdgeuse_TYPE))
                            return false;
                    }
                }
                if (r->GetFilletEdgeType() == SM_FE_CLIFF_RAIL)
                {
                    SmFilletEdgeuse* p = (SmFilletEdgeuse*)r->GetPrimaryEdgeuse();
                    SmFilletEdgeuse* m = (SmFilletEdgeuse*)p->GetMate();
                    SmFilletGeom* side = (a == p->GetVertexuse()->GetVertex()) ? p->GetFilletGeom() : m->GetFilletGeom();
                    if (side == NULL || side == g)
                        return false;
                }
            }
        }
    }
    return true;
}

void p5_put_rail(std::vector<double>& o, SmFilletEdge* r)
{
    o.push_back(flg(r != NULL));
    if (r == NULL)
        return;
    put_curve(o, r->GetCurve());
    o.push_back(r->GetInterval().GetMin());
    o.push_back(r->GetInterval().GetMax());
    SmFilletEdgeuse* p = (SmFilletEdgeuse*)r->GetPrimaryEdgeuse();
    SmFilletEdgeuse* m = (SmFilletEdgeuse*)p->GetMate();
    put_curve(o, p->GetUVTrimCurvePointer());
    put_curve(o, m->GetUVTrimCurvePointer());
    put_uv(o, ((SmFilletVertexuse*)p->GetVertexuse())->GetUVPoint());
    put_uv(o, ((SmFilletVertexuse*)m->GetVertexuse())->GetUVPoint());
}

// The two rails, then the other rails of side 1, then those of side 2.
void p5_put_rails(std::vector<double>& o, SmFilletGeom* g)
{
    for (ULONG ii = 0; ii < 2; ++ii)
        p5_put_rail(o, g->GetRail(ii));
    for (ULONG ii = 0; ii < 2; ++ii)
    {
        std::vector<SmFilletEdge*> all = p5_side_rails(g, ii);
        for (size_t k = 1; k < all.size(); ++k)
            p5_put_rail(o, all[k]);
    }
}

// SmFilletGeom::UpdateSplitFace(f1, f2): false when upstream would dereference what the model
// cannot provide. The edges it updates are both rails, the other rails and the edges owning the
// side edge uses; only those whose original face is f1 reach ShouldSwapFaces
// (SmFilletGeom.cpp:8462-8536), which, with the edge's interval set and not negative, evaluates
// the edge's curve (NULL curve: dereferenced; a failing evaluation leaves its test point
// uninitialised), and with no usable interval would read the corner's filleted vertex (the
// corner is an opaque tag).
bool p5_split_edge_runs(SmFilletEdge* e, SmFace* f1)
{
    if (e->GetOriginalFace() != f1)
        return true;
    SmExtent1d ivl = e->GetInterval();
    if (!(ivl.IsInit() || ivl.HasNegativeLength()))
    {
        SmCurve* c = e->GetCurve();
        if (c == NULL)
            return false;
        double params[3] = {ivl.GetMid(), ivl.GetMin(), ivl.GetMax()};
        for (int w = 0; w < 3; ++w)
        {
            SmPoint3d pt;
            if (c->EvaluatePoint(params[w], pt) != SM_SUCCESS)
                return false;
        }
        return true;
    }
    return e->GetCurveClass() == NULL && e->GetFilletCorner() == NULL;
}

bool p5_split_runs(SmFilletGeom* g, SmFace* f1)
{
    for (ULONG ii = 0; ii < 2; ++ii)
    {
        std::vector<SmFilletEdge*> all = p5_side_rails(g, ii);
        for (size_t k = 0; k < all.size(); ++k)
            if (all[k] == NULL || !p5_split_edge_runs(all[k], f1))
                return false;
    }
    const SmTArray<SmFilletEdgeuse*>& sides = g->*GeomPeek::sides();
    for (ULONG ii = 0; ii < sides.GetSize(); ++ii)
    {
        SmFilletEdge* fe = SM_CAST_PTR(SmFilletEdge, sides[ii]->GetOwner());
        if (fe != NULL && !p5_split_edge_runs(fe, f1))
            return false;
    }
    return true;
}

int32_t p5_run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    P4Solvers S;
    Model M(ctx);
    if (!M.build(rd))
        return SC_ERR_ARG;
    if (!p4_read_solvers(rd, ctx, M, S))
        return SC_ERR_ARG;

    switch (op)
    {
    case P5_INT_FILLET:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        if (rd.bad || v == NULL)
            return SC_ERR_ARG;
        if (v->GetFilletCorner() != NULL)
            return SC_ERR_ARG; // the corner is an opaque tag
        SmStatus r = v->CalcFilletIntFillet();
        o.push_back(st(r));
        p4_put_vertex(o, v);
        return SC_OK;
    }
    case P5_CROSS:
    case P5_SIDE:
    case P5_EXT_EDGE:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        if (rd.bad || e == NULL || e->GetPrimaryEdgeuse() == NULL)
            return SC_ERR_ARG;
        SmStatus r = SM_SUCCESS;
        if (op == P5_CROSS)
        {
            if (!p5_cross_runs(ctx, e))
                return SC_ERR_ARG;
            r = e->CalcCrossSection(ctx, NULL);
            o.push_back(st(r));
            SmFilletEdgeuse* p = (SmFilletEdgeuse*)e->GetPrimaryEdgeuse();
            put_curve(o, e->GetCurve());
            put_curve(o, p->GetUVTrimCurvePointer());
            put_curve(o, p->GetMate()->GetUVTrimCurvePointer());
            p5_put_sides(o, M);
        }
        else if (op == P5_SIDE)
        {
            if (!p5_side_runs(e))
                return SC_ERR_ARG;
            r = e->CalcFilletIntSideFace(ctx, NULL);
            o.push_back(st(r));
            o.push_back((double)e->GetStatus());
            p5_put_sides(o, M);
        }
        else
        {
            r = e->CalcEdgeOnExtendedEdge(ctx, NULL);
            o.push_back(st(r));
        }
        return SC_OK;
    }
    case P5_TRIM:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        if (rd.bad || g == NULL || !p5_trim_runs(g))
            return SC_ERR_ARG;
        SmStatus r = g->TrimRailCurves();
        o.push_back(st(r));
        p5_put_rails(o, g);
        return SC_OK;
    }
    case P5_SPLIT:
    {
        // g f1 f2 then, per edge in edge-list order, its original face (-1: none): the model has
        // no original faces, so the op sets them before the call (on both sides). The two faces
        // are dereferenced by sm_ClassifyFacePoint: they must be real target faces.
        SmFilletGeom* g = M.geom(rd.idx());
        int64_t i1 = rd.idx();
        int64_t i2 = rd.idx();
        std::vector<SmFilletEdge*> es = M.edges();
        std::vector<int64_t> orig;
        for (size_t i = 0; i < es.size() && !rd.bad; ++i)
            orig.push_back(rd.idx());
        if (rd.bad || g == NULL || g->GetRail(0) == NULL || g->GetRail(1) == NULL)
            return SC_ERR_ARG;
        if (i1 < 0 || (size_t)i1 >= M.targetFaces.size() || i2 < 0 || (size_t)i2 >= M.targetFaces.size())
            return SC_ERR_ARG;
        SmFace* f1 = M.targetFaces[(size_t)i1];
        SmFace* f2 = M.targetFaces[(size_t)i2];
        for (size_t i = 0; i < es.size(); ++i)
        {
            if (orig[i] >= (int64_t)M.targetFaces.size())
                return SC_ERR_ARG;
            es[i]->SetOriginalFace(orig[i] < 0 ? NULL : M.targetFaces[(size_t)orig[i]]);
        }
        if (!p5_split_runs(g, f1))
            return SC_ERR_ARG;
        SmStatus r = g->UpdateSplitFace(f1, f2);
        o.push_back(st(r));
        for (size_t i = 0; i < es.size(); ++i)
        {
            o.push_back((double)index_of(M.targetFaces, es[i]->GetOriginalFace()));
            const SmTArray<SmFace*>& sp = es[i]->GetSplitFaces();
            o.push_back((double)sp.GetSize());
            for (ULONG k = 0; k < sp.GetSize(); ++k)
                o.push_back((double)index_of(M.targetFaces, sp[k]));
        }
        return SC_OK;
    }
    case P5_MAKE_EDGE:
    {
        int64_t a = rd.idx();
        int64_t b = rd.idx();
        if (rd.bad)
            return SC_ERR_ARG;
        SmFilletVertex* vs = a < 0 ? NULL : M.vertex(a);
        SmFilletVertex* ve = b < 0 ? NULL : M.vertex(b);
        if ((a >= 0 && vs == NULL) || (b >= 0 && ve == NULL))
            return SC_ERR_ARG;
        SmFilletEdge* ne = NULL;
        SmStatus r = SmFilletEdge::MakeFilletEdge(M.pseudo, vs, ve, ne, NULL);
        o.push_back(st(r));
        o.push_back((double)M.edges().size());
        // after a success the whole pseudo B-rep (the new edge's type, both uses' orientations
        // and list order, each vertex use's vertex, the vertices' use lists)
        if (r == SM_SUCCESS)
            M.put(o);
        return SC_OK;
    }
    case P5_RAIL_EDGE:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        int64_t rail = rd.idx();
        if (rd.bad || g == NULL || rail < 0 || rail > 1)
            return SC_ERR_ARG;
        SmFilletEdge* ne = NULL;
        SmStatus r = g->MakeRailEdge(M.pseudo, (ULONG)rail, ne);
        o.push_back(st(r));
        o.push_back((double)M.edges().size());
        if (r == SM_SUCCESS)
        {
            // the geometry's rail and its two new vertices (appended to the model's vertex list
            // in creation order, as the Rust model lists every live vertex) are in the export
            SmVertex* nvs = NULL;
            SmVertex* nve = NULL;
            if (ne != NULL)
                ne->GetVertices(nvs, nve);
            size_t nverts = M.verts.size();
            if (nvs != NULL)
                M.verts.push_back((SmFilletVertex*)nvs);
            if (nve != NULL)
                M.verts.push_back((SmFilletVertex*)nve);
            M.put(o);
            // the geometry owns the two new vertices (MakeRailEdge adds them to m_vVertices and
            // ~SmFilletGeom deletes them): take them back out of the model's own list, or the
            // model deletes them twice
            M.verts.resize(nverts);
        }
        return SC_OK;
    }
    case P5_RAIL_EU:
    {
        // MakeRailEdgeuse(pseudo, rail, fv, eu, orient, belonging): its checks are
        // NER(pFilletSolver) and NER(pFilletVertex), then it allocates; the base solvers carry
        // no edge uses, so past the allocation upstream fails NER(pBaseEU) and its SmObjDelete
        // clean-up deletes the new vertex use while it is still linked in fv's use list (a
        // vertex use's destructor does not unlink it): ~SmFilletVertex then deletes it a second
        // time (UB-238). A solver without both base edge uses is also a null dereference
        // (pBaseEU1->GetFace()). Both are refused, so only the two defined early exits run.
        SmFilletGeom* g = M.geom(rd.idx());
        int64_t rail = rd.idx();
        int64_t fvi = rd.idx();
        int64_t orient = rd.idx();
        int64_t bi = rd.idx();
        if (rd.bad || g == NULL || rail < 0 || rail > 1 || orient < 0 || orient > 2)
            return SC_ERR_ARG;
        SmFilletVertex* fv = fvi < 0 ? NULL : M.vertex(fvi);
        SmFilletEdge* belonging = bi < 0 ? NULL : M.edge(bi);
        if ((fvi >= 0 && fv == NULL) || (bi >= 0 && belonging == NULL))
            return SC_ERR_ARG;
        // past NER(pFilletSolver) and NER(pFilletVertex) the call allocates (UB-238)
        {
            SmFilletSolver* fs = g->GetFilletSolver();
            if (fs != NULL && fv != NULL && (fs->GetEdgeuse(0) == NULL || fs->GetEdgeuse(1) == NULL))
                return SC_ERR_ARG;
        }
        SmFilletEdgeuse* neu = NULL;
        SmStatus r = g->MakeRailEdgeuse(M.pseudo, (ULONG)rail, fv, neu, (SmOrientType)orient, belonging);
        o.push_back(st(r));
        o.push_back((double)M.edges().size());
        if (r == SM_SUCCESS)
            M.put(o);
        return SC_OK;
    }
    case P5_END:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        int64_t end = rd.idx();
        int64_t trim = rd.idx();
        if (rd.bad || g == NULL || end < 0 || end > 1 || trim < 0 || trim > 4)
            return SC_ERR_ARG;
        if (g->GetFilletSurface() != NULL &&
            (g->GetRail(0) == NULL || g->GetRail(1) == NULL || g->GetFilletSolver() == NULL))
            return SC_ERR_ARG; // dereferenced unchecked
        // SM_BT_BLEND runs FindEdgeTangentAtPoint on each rail's end vertex in turn; for a vertex
        // classified SM_PC_EDGE it casts the classification object to an SmEdge* and reads it
        // (NER(pE) catches NULL, SmFilletGeom.cpp:9220-9223). Upstream always pairs SM_PC_EDGE
        // with an SmEdge (SmFilletSolver.cpp:7605-7619), which the model can build (pc kind 4);
        // an object of another type is the type confusion to refuse. The first vertex's
        // NULL object returns before the second is read.
        if (trim == SM_BT_BLEND && g->GetFilletSurface() != NULL)
        {
            for (ULONG ri = 0; ri < 2; ++ri)
            {
                SmFilletVertex* fv = (SmFilletVertex*)g->GetRail(ri)->GetVertex();
                if (end != 0)
                    fv = (SmFilletVertex*)g->GetRail(ri)->GetOtherVertex(fv);
                if (fv->GetPointClass() != SM_PC_EDGE)
                    continue;
                const SmObject* obj = fv->GetPointClassObject();
                if (obj == NULL)
                    break; // NER(pE) returns SM_ERR_NULL_POINTER
                if (!obj->IsKindOf(SmEdge_TYPE))
                    return SC_ERR_ARG;
            }
        }
        SmStatus r = g->CreateEndCurve((ULONG)end, (SmBoundaryTrimmingType)trim, M.pseudo);
        o.push_back(st(r));
        o.push_back((double)M.edges().size());
        // the new cross-section edge: its 3D curve and interval, the primary use's UV curve,
        // the geometry's side uses, everything else in the export
        if (r == SM_SUCCESS)
            M.put(o);
        return SC_OK;
    }
    default:
        return SC_ERR_ARG;
    }
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_geom_p5_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                               int64_t cap, int64_t* out_len)
{
    if (in == NULL && n_in > 0)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    // the ops that build surface caches (UB-166 guard in read_surface)
    rd.cache_op = op == P5_CROSS || op == P5_TRIM || op == P5_END || op == P5_SIDE || op == P5_SPLIT;
    std::vector<double> o;
    int32_t rc = p5_run(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
