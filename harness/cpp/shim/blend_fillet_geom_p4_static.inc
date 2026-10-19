// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp, the translation unit that compiles upstream
// SmFilletGeom.cpp: the difftest shim of unit `blend.fillet_geom_p4` (sm_CreateUVCurve,
// sm_CalcNx2VertFilletXFillet, SmFilletVertex::CalcFilletInt2Fillets,
// SmFilletVertex::CalcRailIntExtendedEdgeuse, SmFilletGeom::GetRailIndexOfVertex,
// SmFilletGeom::MakeFaceBrep, SmFilletGeom::CalcBlendingGeom). Two of them are file-statics,
// and every op builds the pseudo B-rep with blend_fillet_geom_p1.cpp's Model, so the shim lives
// in that translation unit. Not compiled on its own (blend_fillet_geom_p4.cpp only points here).
//
// One entry point, sc_blend_fillet_geom_p4_eval(op, in, n_in, out, cap, out_len). Layouts and
// the refusals (SC_ERR_ARG) are documented in crates/solidcore-ffi/src/units/
// blend_fillet_geom_p4.rs, which validates every input; reads are bounds checked here too.

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_geom_p4.rs).
enum : int32_t
{
    P4_UV_CURVE = 0,
    P4_RAIL_INDEX = 1,
    P4_RAIL_INT_EXT = 2,
    P4_INT2 = 3,
    P4_NX2 = 4,
    P4_MAKE_FACE = 5,
    P4_BLENDING = 6,
};

// A base SmFilletSolver carrying only its tolerances and context (the members the unit's
// routines read before anything this harness cannot provide).
struct P4Solver : SmFilletSolver
{
    P4Solver(const SmContext& ctx, double tol, double ang, double tang) : SmFilletSolver(ctx, tol, ang, tang)
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
};

struct P4GeomPeek : SmFilletGeom
{
    static SmFilletSolver* SmFilletGeom::*solver() { return &P4GeomPeek::m_pFilletSolver; }
};

// Declared before the Model so that the geometries are deleted first.
struct P4Solvers
{
    std::vector<P4Solver*> v;
    ~P4Solvers()
    {
        for (size_t i = 0; i < v.size(); ++i)
            delete v[i];
    }
};

// Reads `ns (approx ang tang)..` and per geometry its solver index (-1: none).
bool p4_read_solvers(Rd& rd, const SmContext& ctx, Model& M, P4Solvers& S)
{
    int64_t ns = rd.idx();
    for (int64_t i = 0; i < ns && !rd.bad; ++i)
    {
        double tol = rd.num();
        double ang = rd.num();
        double tang = rd.num();
        S.v.push_back(new (ctx) P4Solver(ctx, tol, ang, tang));
    }
    for (size_t g = 0; g < M.geoms.size() && !rd.bad; ++g)
    {
        int64_t si = rd.idx();
        if (si >= (int64_t)S.v.size())
            return false;
        M.geoms[g]->*P4GeomPeek::solver() = si < 0 ? NULL : S.v[(size_t)si];
    }
    return !rd.bad;
}

// The surface of a UvCurve op: 0 B-spline, 1 SmPlane, 2 SmSurfOfRevolution of a line.
// `cs` is the construction status (MakeNurb / CreateCanonical).
SmSurface* p4_read_uv_surface(Rd& rd, const SmContext& ctx, SmStatus& cs)
{
    cs = SM_SUCCESS;
    int64_t kind = rd.idx();
    if (kind == 0)
        return read_surface(rd, ctx);
    if (kind == 1)
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
        if (rd.bad)
            return NULL;
        SmPlane* pl = new (ctx) SmPlane(org, x, y, SmVector2d(su, sv), SmExtent2d(d0, d1, d2, d3), &ctx);
        if (nurb)
            cs = pl->MakeNurb();
        else
        {
            // Without a NURB the plane makes one on demand, and the routine's bounding box
            // dereferences it: refuse a plane whose NURB cannot be made (tried on a copy).
            SmPlane* probe = new (ctx) SmPlane(org, x, y, SmVector2d(su, sv), SmExtent2d(d0, d1, d2, d3), &ctx);
            SmStatus made = probe->MakeNurb();
            delete probe;
            if (made != SM_SUCCESS)
            {
                delete pl;
                rd.bad = true;
                return NULL;
            }
        }
        return pl;
    }
    SmPoint3d a = rd.vec();
    SmPoint3d b = rd.vec();
    SmPoint3d ap = rd.vec();
    SmVector3d ad = rd.vec();
    bool swap = rd.flag();
    if (rd.bad)
        return NULL;
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
        return NULL;
    }
    return r;
}

// sm_CalcNx2VertFilletXFillet's edge loop up to its first exit: 1 when it exits (SM_ERR or
// NER), 0 when it finishes (upstream then asks the vertex's corner for its context), -1 when a
// cross-section geometry has no solver (dereferenced).
int p4_nx2_loop_exits(SmFilletVertex* v)
{
    SmTArray<SmEdge*> es;
    v->GetEdges(es);
    for (ULONG i = 0; i < es.GetSize(); ++i)
    {
        SmFilletEdge* fe = (SmFilletEdge*)es[i];
        switch (fe->GetFilletEdgeType())
        {
        case SM_FE_CROSS_SECTION:
        {
            SmFilletEdgeuse* p = (SmFilletEdgeuse*)fe->GetPrimaryEdgeuse();
            SmFilletGeom* g = p ? p->GetFilletGeom() : NULL;
            if (g == NULL)
                return 1;
            if (g->GetFilletSolver() == NULL)
                return -1;
            break;
        }
        case SM_FE_FILLET_X_SIDE_FACE:
        {
            SmFilletVertex* other = (SmFilletVertex*)fe->GetOtherVertex(v);
            if (!other->IsProcessed())
                return 1;
            break;
        }
        default:
            break;
        }
    }
    return 0;
}

bool p4_curve_missing(SmEdge* e) { return e == NULL || e->GetCurve() == NULL; }

// MakeFaceBrep up to JoinTwoFilletVertsByCurves: 0 when it exits before, 1 when it gets
// there with every rail, other rail and side edge carrying a 3D curve, -1 when it would
// dereference a missing rail or solver, or reach the join with a missing 3D curve (which the
// join or the degenerate-curve pass after it dereferences).
int p4_make_face_reach(SmFilletGeom* g)
{
    if (g->GetFilletSurface() == NULL)
        return 0;
    if (g->GetFilletGeomType() != SM_FG_CLIFF_SIDE_PATCH)
    {
        for (ULONG ii = 0; ii < 2; ++ii)
        {
            SmFilletEdge* rail = g->GetRail(ii);
            if (rail == NULL)
                return -1;
            SmEdgeuse* p = rail->GetPrimaryEdgeuse();
            if (p == NULL)
                return 0;
            SmBSplineCurve* uv = p->GetUVTrimCurve();
            if (uv == NULL)
                return 0;
            SmPoint3d pt;
            if (uv->EvaluatePoint(rail->GetInterval().Evaluate(0.5), pt) != SM_SUCCESS)
                return 0;
        }
    }
    if (g->GetFilletSolver() == NULL)
        return -1;
    for (ULONG ii = 0; ii < 2; ++ii)
    {
        SmFilletEdge* rail = g->GetRail(ii);
        if (rail == NULL)
            return -1;
        if (rail->GetPrimaryEdgeuse() == NULL)
            return 0;
    }
    const SmTArray<SmFilletEdge*>* lists[2] = {&(g->*GeomPeek::other1()), &(g->*GeomPeek::other2())};
    for (int l = 0; l < 2; ++l)
        for (ULONG k = 0; k < lists[l]->GetSize(); ++k)
            if ((*lists[l])[k]->GetPrimaryEdgeuse() == NULL)
                return 0;
    if (p4_curve_missing(g->GetRail(0)) || p4_curve_missing(g->GetRail(1)))
        return -1;
    for (int l = 0; l < 2; ++l)
        for (ULONG k = 0; k < lists[l]->GetSize(); ++k)
            if (p4_curve_missing((*lists[l])[k]))
                return -1;
    const SmTArray<SmFilletEdgeuse*>& sides = g->*GeomPeek::sides();
    for (ULONG k = 0; k < sides.GetSize(); ++k)
        if (p4_curve_missing(sides[k]->GetEdge()))
            return -1;
    return 1;
}

void p4_put_vertex(std::vector<double>& o, SmFilletVertex* v)
{
    put_pt(o, v->GetPoint());
    o.push_back((double)v->GetStatus());
}

int32_t p4_run(int32_t op, Rd& rd, std::vector<double>& o)
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
    case P4_UV_CURVE:
    {
        SmFilletVertex* sv = M.vertex(rd.idx());
        SmFilletVertex* ev = M.vertex(rd.idx());
        bool two = rd.flag();
        SmStatus cs = SM_SUCCESS;
        SmSurface* s = p4_read_uv_surface(rd, ctx, cs);
        double tol = rd.num();
        if (rd.bad || sv == NULL || ev == NULL || (s == NULL && cs == SM_SUCCESS))
        {
            delete s;
            return SC_ERR_ARG;
        }
        o.push_back(st(cs));
        if (cs != SM_SUCCESS)
        {
            delete s;
            return SC_OK;
        }
        SmBSplineCurve* c3 = NULL;
        SmBSplineCurve* u1 = NULL;
        SmBSplineCurve* u2 = NULL;
        SmStatus r = sm_CreateUVCurve(ctx, sv, ev, two ? TRUE : FALSE, s, tol, c3, u1, u2);
        o.push_back(st(r));
        put_curve(o, c3);
        put_curve(o, u1);
        put_curve(o, u2);
        delete c3;
        delete u1;
        delete u2;
        delete s;
        return SC_OK;
    }
    case P4_RAIL_INDEX:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        int64_t vi = rd.idx();
        SmFilletVertex* v = vi < 0 ? NULL : M.vertex(vi);
        if (rd.bad || g == NULL || (vi >= 0 && v == NULL))
            return SC_ERR_ARG;
        // GetRailVertexuse dereferences both rails
        if (v != NULL && (g->GetRail(0) == NULL || g->GetRail(1) == NULL))
            return SC_ERR_ARG;
        ULONG idx = 0;
        SmStatus r = g->GetRailIndexOfVertex(v, idx);
        o.push_back(st(r));
        o.push_back((double)idx);
        return SC_OK;
    }
    case P4_RAIL_INT_EXT:
    case P4_INT2:
    case P4_NX2:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        if (rd.bad || v == NULL)
            return SC_ERR_ARG;
        SmStatus r = SM_SUCCESS;
        if (op == P4_NX2)
        {
            if (p4_nx2_loop_exits(v) != 1)
                return SC_ERR_ARG; // a solver or corner is dereferenced
            r = sm_CalcNx2VertFilletXFillet(v);
        }
        else
        {
            if (v->GetFilletCorner() != NULL)
                return SC_ERR_ARG; // the corner is an opaque tag
            r = op == P4_INT2 ? v->CalcFilletInt2Fillets() : v->CalcRailIntExtendedEdgeuse();
        }
        o.push_back(st(r));
        p4_put_vertex(o, v);
        return SC_OK;
    }
    case P4_MAKE_FACE:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        if (rd.bad || g == NULL || p4_make_face_reach(g) < 0)
            return SC_ERR_ARG;
        int reach = p4_make_face_reach(g);
        if (reach == 1)
        {
            // Past the join the face makers consume the fillet surface (a new face owns
            // it, or a failure deletes it): the Model must no longer delete it.
            SmSurface* fs = g->GetFilletSurface();
            for (size_t i = 0; i < M.geomSurfaces.size(); ++i)
                if (M.geomSurfaces[i] == fs)
                    M.geomSurfaces[i] = NULL;
        }
        SmBrep* nb = new (ctx) SmBrep();
        SmStatus r = g->MakeFaceBrep(nb);
        // a new face may own the surface: the B-rep is left alive (leaked) then
        if (reach == 0)
            delete nb;
        o.push_back(st(r));
        return SC_OK;
    }
    case P4_BLENDING:
    {
        SmFilletGeom* g = M.geom(rd.idx());
        if (rd.bad || g == NULL || g->GetFilletSolver() == NULL || g->GetRail(0) == NULL)
            return SC_ERR_ARG;
        SmStatus r = g->CalcBlendingGeom();
        o.push_back(st(r));
        return SC_OK;
    }
    default:
        return SC_ERR_ARG;
    }
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_geom_p4_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                               int64_t cap, int64_t* out_len)
{
    if (in == NULL && n_in > 0)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    // the ops that build surface caches (UB-166 guard in read_surface)
    rd.cache_op = op == P4_UV_CURVE || op == P4_MAKE_FACE;
    std::vector<double> o;
    int32_t rc = p4_run(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
