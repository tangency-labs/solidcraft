// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp, after blend_fillet_geom_p4_static.inc: the
// difftest shim of unit `blend.fillet_geom_p2` (sm_SrfSrfIntersection,
// SmFilletVertex::CalcRailIntRail / CalcRailIntEdgeuse / CalcVertOnCrossSection / CalcSetBackVert,
// SmFilletEdge::CalcRailRailInterpolation, SmFilletEdge::UpdateSplitFace). sm_SrfSrfIntersection
// is defined in upstream SmFilletGeom.cpp, which only this translation unit compiles (see
// blend_fillet_geom_p1.cpp), and the topology ops build their pseudo B-rep with that file's Model
// and blend_fillet_geom_p4_static.inc's solvers and analytic surfaces. Not compiled on its own
// (blend_fillet_geom_p2.cpp only points here).
//
// One entry point, sc_blend_fillet_geom_p2_eval(op, in, n_in, out, cap, out_len). Layouts and
// the refusals (SC_ERR_ARG) are documented in crates/solidcore-ffi/src/units/
// blend_fillet_geom_p2.rs, which validates every input; reads are bounds checked here too.

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_geom_p2.rs).
enum : int32_t
{
    P2_SRF_SRF = 0,
    P2_RAIL_INT_RAIL = 1,
    P2_RAIL_INT_EU = 2,
    P2_CROSS_SECTION = 3,
    P2_SET_BACK = 4,
    P2_RAIL_RAIL = 5,
    P2_SPLIT_FACE = 6,
};

// The pCorner of CalcRailRailInterpolation: a corner with the given fillet edges
// (m_vEdges) and nothing else. Its type is SM_FCR_UNKNOWN (never SM_FCR_N_x_N_CONCAVE, whose
// TestRailRailInterpolationCurve reads the filleted vertex it does not have). The
// SmFilletCorner constructor reads pExec->m_pPseudoBrep and nothing else of the executive,
// so it gets zeroed storage standing in for one (a NULL pseudo B-rep).
alignas(SmFilletExecutive) unsigned char g_p2_exec_storage[sizeof(SmFilletExecutive)] = {};

struct P2Corner : SmFilletCorner
{
    P2Corner(const SmContext& ctx)
        : SmFilletCorner(ctx, NULL, NULL, NULL, (SmFilletExecutive*)(void*)g_p2_exec_storage, 1.0e-6, 0.01)
    {
    }
    virtual SmStatus MakeCornerTopology() { return SM_ERR; }
    virtual SmFilletCornerType GetCornerType() const { return SM_FCR_UNKNOWN; }
    virtual void GetName(TCHAR* pc, size_t n) const
    {
        if (n > 0)
            pc[0] = 0;
    }
    void add_edge(SmFilletEdge* e) { m_vEdges.Add(e); }
};

// CalcRailRailInterpolation's corner branch (SmFilletGeom.cpp:6672-6707) with fillet edges
// `ce`: `early` when it returns SM_ERR / SM_ERR_NULL_POINTER before the interpolation; false
// when it would dereference the test edge's primary edge use (NULL) or its geometry's solver
// and surface generator (the harness's solvers have none).
bool p2_rail_rail_corner_safe(SmFilletEdge* e, const std::vector<SmFilletEdge*>& ce, bool& early)
{
    early = false;
    if (ce.size() > 4)
        return true;
    size_t index = 0;
    while (index < ce.size() && ce[index] != e)
        ++index;
    if (index == ce.size())
    {
        early = true; // FindElement fails: SER(SM_ERR)
        return true;
    }
    SmFilletEdge* test = ce[(index + 2) % ce.size()];
    if (test->GetFilletEdgeType() != SM_FE_CROSS_SECTION)
    {
        early = true; // SER(SM_ERR)
        return true;
    }
    SmFilletEdgeuse* p = (SmFilletEdgeuse*)test->GetPrimaryEdgeuse();
    if (p == NULL || p->GetFilletGeom() != NULL)
        return false;
    early = true; // NER(pFilletGeom)
    return true;
}

// `type has dim n p m cpts knots`: the curve's SM type (0 for none) and its B-spline data.
void p2_put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(c == NULL ? 0.0 : (double)c->GetType());
    put_curve(o, c);
}

// A surface of `kind [data]` (p4_read_uv_surface); NULL with cs set when its construction
// failed, NULL with rd.bad on a bad program.
SmSurface* p2_read_surface(Rd& rd, const SmContext& ctx, SmStatus& cs) { return p4_read_uv_surface(rd, ctx, cs); }

int32_t p2_srf_srf(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmStatus cs1 = SM_SUCCESS;
    SmStatus cs2 = SM_SUCCESS;
    SmSurface* s1 = p2_read_surface(rd, ctx, cs1);
    SmSurface* s2 = p2_read_surface(rd, ctx, cs2);
    bool has_start = rd.flag();
    SmPoint3d sp = rd.pt3();
    bool has_end = rd.flag();
    SmPoint3d ep = rd.pt3();
    bool has_dir = rd.flag();
    SmVector3d dir = rd.vec();
    bool has_end_dir = rd.flag();
    SmVector3d end_dir = rd.vec();
    double tol = rd.num();
    double ang = rd.num();
    bool skip = rd.flag();
    bool has_ref = rd.flag();
    SmPoint3d ref = rd.pt3();
    if (rd.bad || (s1 == NULL && cs1 == SM_SUCCESS) || (s2 == NULL && cs2 == SM_SUCCESS))
    {
        delete s1;
        delete s2;
        return SC_ERR_ARG;
    }
    o.push_back(st(cs1));
    o.push_back(st(cs2));
    if (cs1 != SM_SUCCESS || cs2 != SM_SUCCESS)
    {
        delete s1;
        delete s2;
        return SC_OK;
    }
    SmBrep* b = new (ctx) SmBrep();
    SmVertex* vs = has_start ? lone_vertex(b, sp) : NULL;
    SmVertex* ve = has_end ? lone_vertex(b, ep) : NULL;
    if ((has_start && vs == NULL) || (has_end && ve == NULL))
    {
        delete b;
        delete s1;
        delete s2;
        return SC_ERR_ARG;
    }
    SmBSplineCurve* c3 = NULL;
    SmBSplineCurve* u1 = NULL;
    SmBSplineCurve* u2 = NULL;
    SmStatus r = sm_SrfSrfIntersection(ctx, vs, ve, has_dir ? &dir : NULL, has_end_dir ? &end_dir : NULL, s1, s2,
                                       tol, ang, c3, u1, u2, skip ? TRUE : FALSE, has_ref ? &ref : NULL, 0);
    o.push_back(st(r));
    // on failure upstream may leave (and leak) pointers it did not return: report only a
    // success's curves
    p2_put_curve(o, r == SM_SUCCESS ? c3 : NULL);
    p2_put_curve(o, r == SM_SUCCESS ? u1 : NULL);
    p2_put_curve(o, r == SM_SUCCESS ? u2 : NULL);
    put_pt(o, dir); // unitised in place by the two-point intersection
    if (r == SM_SUCCESS)
    {
        delete c3;
        delete u1;
        delete u2;
    }
    delete b;
    delete s1;
    delete s2;
    return SC_OK;
}

void p2_put_vertex(std::vector<double>& o, SmFilletVertex* v)
{
    put_pt(o, v->GetPoint());
    o.push_back((double)v->GetStatus());
}

// CalcRailRailInterpolation without a corner up to its drops: false when upstream would
// dereference a NULL rail curve (it drops the vertex onto pCurve1 / pCurve2 unchecked).
bool p2_rail_rail_curves_present(const SmContext& ctx, SmFilletEdge* e)
{
    SmFilletVertex* sv = (SmFilletVertex*)e->GetVertex();
    SmFilletVertex* ev = (SmFilletVertex*)e->GetOtherVertex(sv);
    SmFace* f = e->GetOriginalFace();
    if (f == NULL)
        return true; // NER
    SmBSplineSurface* s = SM_CAST_PTR(SmBSplineSurface, f->GetSurface());
    if (s == NULL)
        return true; // NER
    bool on3d = false;
    SmPlane* pl = NULL;
    if (SmPlane::IsNurbSurfacePlane(ctx, s, pl))
    {
        delete pl;
        on3d = true;
    }
    else if (sv->GetFilletVertexType() != SM_FV_MATE || ev->GetFilletVertexType() != SM_FV_MATE)
        on3d = true;
    SmFilletVertex* ends[2] = {sv, ev};
    SmCurve* curves[2] = {NULL, NULL};
    for (int i = 0; i < 2; ++i)
    {
        SmTArray<SmEdge*> es;
        ends[i]->GetEdges(es);
        SmFilletEdge* rail = NULL;
        for (ULONG j = 0; j < es.GetSize(); ++j)
            if (((SmFilletEdge*)es[j])->GetFilletEdgeType() == SM_FE_RAIL)
            {
                rail = (SmFilletEdge*)es[j];
                break;
            }
        if (rail == NULL)
            return true; // NER
        SmEdgeuse* p = rail->GetPrimaryEdgeuse();
        if (p == NULL || p->GetMate() == NULL)
            return false; // dereferenced
        curves[i] = p->GetMate()->GetUVTrimCurve();
        if (!on3d && curves[i] == NULL)
            on3d = true;
        if (on3d)
            curves[i] = rail->GetCurve();
        // dropped onto unchecked (rail 0's curve is picked before rail 1 can switch
        // the mode, as upstream)
        if (curves[i] == NULL)
            return false;
    }
    return true;
}

int32_t p2_run(int32_t op, Rd& rd, std::vector<double>& o)
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
    case P2_RAIL_INT_RAIL:
    case P2_RAIL_INT_EU:
    case P2_CROSS_SECTION:
    case P2_SET_BACK:
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        int64_t gi = op == P2_RAIL_INT_EU ? rd.idx() : -1;
        bool has_param = op == P2_CROSS_SECTION ? rd.flag() : false;
        double param = op == P2_CROSS_SECTION ? rd.num() : 0.0;
        if (rd.bad || v == NULL || (gi >= 0 && M.geom(gi) == NULL))
            return SC_ERR_ARG;
        // the corner is an opaque tag: every routine asks it for its filleted vertex
        if (v->GetFilletCorner() != NULL)
            return SC_ERR_ARG;
        SmStatus r = SM_SUCCESS;
        switch (op)
        {
        case P2_RAIL_INT_RAIL:
            r = v->CalcRailIntRail();
            break;
        case P2_RAIL_INT_EU:
            r = v->CalcRailIntEdgeuse(gi < 0 ? NULL : M.geom(gi));
            break;
        case P2_CROSS_SECTION:
            r = v->CalcVertOnCrossSection(has_param ? &param : NULL);
            break;
        default:
            r = v->CalcSetBackVert();
            break;
        }
        o.push_back(st(r));
        p2_put_vertex(o, v);
        return SC_OK;
    }
    case P2_RAIL_RAIL:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        int64_t face_kind = rd.idx();
        SmStatus cs = SM_SUCCESS;
        SmSurface* s = NULL;
        if (face_kind >= 0)
        {
            // p4_read_uv_surface reads the kind itself
            rd.at -= 1;
            s = p2_read_surface(rd, ctx, cs);
        }
        double face_tol = face_kind >= 0 ? rd.num() : 0.0;
        // the corner's fillet edges (`-1`: pCorner NULL)
        int64_t nc = rd.idx();
        std::vector<SmFilletEdge*> ce;
        for (int64_t i = 0; i < nc && !rd.bad; ++i)
        {
            SmFilletEdge* c = M.edge(rd.idx());
            if (c == NULL)
                rd.bad = true;
            ce.push_back(c);
        }
        if (rd.bad || e == NULL || (face_kind >= 0 && s == NULL && cs == SM_SUCCESS))
        {
            delete s;
            return SC_ERR_ARG;
        }
        bool early = false;
        if (nc >= 0 && !p2_rail_rail_corner_safe(e, ce, early))
        {
            delete s;
            return SC_ERR_ARG;
        }
        o.push_back(st(cs));
        if (cs != SM_SUCCESS)
            return SC_OK;
        if (s != NULL)
        {
            SmFace* f = face_of(M.target, s);
            if (f == NULL)
            {
                delete s;
                return SC_ERR_ARG;
            }
            f->SetTolerance(SmZoneTol3d(face_tol), FALSE, FALSE);
            e->SetOriginalFace(f);
        }
        if (!early && !p2_rail_rail_curves_present(ctx, e))
            return SC_ERR_ARG; // a NULL rail curve or edge use would be dereferenced
        P2Corner* corner = NULL;
        if (nc >= 0)
        {
            corner = new (ctx) P2Corner(ctx);
            for (size_t i = 0; i < ce.size(); ++i)
                corner->add_edge(ce[i]);
        }
        SmStatus r = e->CalcRailRailInterpolation(ctx, corner);
        delete corner;
        o.push_back(st(r));
        o.push_back((double)e->GetStatus());
        p2_put_curve(o, e->GetCurve());
        SmEdgeuse* p = e->GetPrimaryEdgeuse();
        p2_put_curve(o, p ? p->GetUVTrimCurve() : NULL);
        p2_put_curve(o, (p && p->GetMate()) ? p->GetMate()->GetUVTrimCurve() : NULL);
        return SC_OK;
    }
    case P2_SPLIT_FACE:
    {
        SmFilletEdge* e = M.edge(rd.idx());
        int64_t set = rd.idx();
        int64_t f1 = rd.idx();
        int64_t f2 = rd.idx();
        int64_t nf = (int64_t)M.targetFaces.size();
        if (rd.bad || e == NULL || set >= nf || f1 >= nf || f2 >= nf)
            return SC_ERR_ARG;
        SmFace* fs = set < 0 ? NULL : M.targetFaces[(size_t)set];
        SmFace* of = f1 < 0 ? NULL : M.targetFaces[(size_t)f1];
        SmFace* nw = f2 < 0 ? NULL : M.targetFaces[(size_t)f2];
        // the split face is this edge's: ShouldSwapFaces (unit blend.fillet_geom_p6) drops a
        // point of the edge (its curve, curve classification or the corner's vertex, each
        // dereferenced unchecked) and classifies it against both faces (NULL faces are
        // dereferenced too); the block cannot make that safe, so it is refused
        if (fs == of)
            return SC_ERR_ARG;
        e->SetOriginalFace(fs);
        SmStatus r = e->UpdateSplitFace(of, nw);
        o.push_back(st(r));
        o.push_back((double)index_of(M.targetFaces, (const SmFace*)e->GetOriginalFace()));
        const SmTArray<SmFace*>& split = e->GetSplitFaces();
        o.push_back((double)split.GetSize());
        for (ULONG i = 0; i < split.GetSize(); ++i)
            o.push_back((double)index_of(M.targetFaces, (const SmFace*)split[i]));
        return SC_OK;
    }
    default:
        return SC_ERR_ARG;
    }
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_geom_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                               int64_t cap, int64_t* out_len)
{
    if (in == NULL && n_in > 0)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    // every op may build surface caches (UB-166 guard in read_surface)
    rd.cache_op = true;
    std::vector<double> o;
    int32_t rc = op == P2_SRF_SRF ? p2_srf_srf(rd, o) : p2_run(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
