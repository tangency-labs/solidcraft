// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp: the difftest shim of unit
// `blend.fillet_corner_p3`, part 2 (the corner operations). Every op builds the pseudo B-rep
// with blend_fillet_geom_p1.cpp's Model (anonymous namespace there), so this shim lives in that
// translation unit; part 1 (SmFilletCorner::Create) is blend_fillet_corner_p3.cpp.
//
// One entry point, sc_blend_fillet_corner_p3_corner(op, in, n_in, out, cap, out_len): a real
// fillet corner of the given derived class (its upstream constructor) on the Model, with the
// given fillet vertices, fillet edges, flags and tolerances, the intersection points of some
// vertex uses set (curve parameter, UV positions, m_apUserPointer surfaces), then one of
// SmFilletCorner::CalcCornerGeom, the virtual CreateCornerPatch, AdjustTopology or
// AdjustTangentCorner. Layout in crates/solidcore-ffi/src/units/blend_fillet_corner_p3.rs.
//
// Output: `-2` (refused: upstream would dereference null, read past an array or free the
// Model's objects), `-3` (not run: upstream would reach a routine whose Rust port is a stub,
// the case is deferred), else the op's status (`0` = SM_SUCCESS), the corner's members and the
// Model's export. The pre-checks repeat upstream's own code up to the point where it would
// call into what the harness cannot provide, as blend_fillet_geom_p4_static.inc does.

namespace
{
enum : int32_t
{
    P3_CALC_GEOM = 0,
    P3_PATCH = 1,
    P3_ADJ_TOPO = 2,
    P3_ADJ_TANGENT = 3,
};

const double P3C_REFUSE = -2.0;
const double P3C_NOT_RUN = -3.0;

struct P3ExecPeek : SmFilletExecutive
{
    P3ExecPeek(const SmContext& c) : SmFilletExecutive(c) {}
    static SmFilletBrep* SmFilletExecutive::*pseudo() { return &P3ExecPeek::m_pPseudoBrep; }
};

struct P3CornerPeek : SmFilletCorner
{
    static SmTArray<SmFilletVertex*> SmFilletCorner::*verts() { return &P3CornerPeek::m_vVertices; }
    static SmTArray<SmFilletEdge*> SmFilletCorner::*edges() { return &P3CornerPeek::m_vEdges; }
    static SmTArray<SmSurface*> SmFilletCorner::*surfs() { return &P3CornerPeek::m_vSurfaces; }
    static SmTArray<SmEdgeuse*>* SmFilletCorner::*solver() { return &P3CornerPeek::m_pSolverEUs; }
    static SmTArray<SmEdgeuse*>* SmFilletCorner::*all() { return &P3CornerPeek::m_pAllEUs; }
    static SmBoolean SmFilletCorner::*bevel() { return &P3CornerPeek::m_bBevel; }
    static SmBoolean SmFilletCorner::*blending() { return &P3CornerPeek::m_bBlending; }
    static SmBoolean SmFilletCorner::*chamfer() { return &P3CornerPeek::m_bChamfer; }
    static SmBoolean SmFilletCorner::*degenerate() { return &P3CornerPeek::m_bDegenerate; }
    static SmBoolean SmFilletCorner::*constrad() { return &P3CornerPeek::m_bCalcAsConstRad; }
    static SmBoolean SmFilletCorner::*adjusted() { return &P3CornerPeek::m_bTopologyAdjusted; }
    static SmBoolean SmFilletCorner::*ext() { return &P3CornerPeek::m_bExtendedSurfacePatch; }
    static double SmFilletCorner::*setback() { return &P3CornerPeek::m_dSetBackDist; }
    static double SmFilletCorner::*tol() { return &P3CornerPeek::m_dThisApproxTol3d; }
};

SmFilletCorner* p3_make_corner(int64_t kind, const SmContext& ctx, const SmVertex* v, SmTArray<SmEdgeuse*>* s,
                               SmTArray<SmEdgeuse*>* a, SmFilletExecutive* x, double tol, double tan)
{
    switch (kind)
    {
    case 0: return new (ctx) SmFillet1x1Corner(ctx, v, s, a, x, tol, tan);
    case 1: return new (ctx) SmFillet2x2Corner(ctx, v, s, a, x, tol, tan);
    case 2: return new (ctx) SmFilletNx1ClosedCorner(ctx, v, s, a, x, tol, tan);
    case 3: return new (ctx) SmFilletNx1Corner(ctx, v, s, a, x, tol, tan);
    case 4: return new (ctx) SmFilletDegenerateCorner(ctx, v, s, a, x, tol, tan);
    case 5: return new (ctx) SmFilletOpenCorner(ctx, v, s, a, x, tol, tan);
    case 6: return new (ctx) SmFillet3x2MixedConvexityCorner(ctx, v, s, a, x, tol, tan);
    case 7: return new (ctx) SmFilletNx2Corner(ctx, v, s, a, x, tol, tan);
    case 8: return new (ctx) SmFillet4x3Corner(ctx, v, s, a, x, tol, tan);
    case 9: return new (ctx) SmFilletConvexNxNCorner(ctx, v, s, a, x, tol, tan);
    case 10: return new (ctx) SmFilletConcaveNxNCorner(ctx, v, s, a, x, tol, tan);
    default: return NULL;
    }
}

// Reads `has n (edge which)..` into a new edge use array (NULL when !has).
bool p3_read_eus(Rd& rd, const SmContext& ctx, Model& M, SmTArray<SmEdgeuse*>*& out)
{
    out = NULL;
    bool has = rd.flag();
    int64_t n = rd.idx();
    if (has)
        out = new (ctx) SmTArray<SmEdgeuse*>(ctx);
    for (int64_t i = 0; i < n && !rd.bad; ++i)
    {
        int64_t e = rd.idx();
        int64_t which = rd.idx();
        if (e < 0 || (size_t)e >= M.targetEdges.size())
            return false;
        SmEdgeuse* eu = M.targetEdges[(size_t)e]->GetPrimaryEdgeuse();
        if (eu != NULL && which == 1)
            eu = eu->GetMate();
        if (eu == NULL)
            return false;
        if (out)
            out->Add(eu);
    }
    return !rd.bad;
}

SmFilletVertexuse* p3_vu(Model& M, int64_t e, int64_t which)
{
    SmFilletEdgeuse* eu = M.edgeuse(e, which);
    return eu ? (SmFilletVertexuse*)eu->GetVertexuse() : NULL;
}

// sm-style point of a user surface with out-of-bounds evaluation enabled (as upstream's
// SmTemporaryChangeValue does around AdjustTangentCorner's evaluations).
SmPoint3d p3_eval_oob(SmSurface* s, const SmPoint2d& uv)
{
    SmBSplineSurface* b = SM_CAST_PTR(SmBSplineSurface, s);
    SmBoolean tmp = FALSE;
    SmTemporaryChangeValue<SmBoolean> clean(b ? b->GetOutOfBoundsEnabled() : tmp, TRUE);
    SmPoint3d p;
    s->EvaluatePoint(uv, p);
    return p;
}

// A user surface of an intersection point: target face `s`, or (1000, 1001) a real SmPlane
// (an SmBSplineSurface-derived analytic kind), else NULL. The planes are owned here and
// freed when the run ends (P3PlaneGuard).
std::vector<SmSurface*>& p3_planes()
{
    static std::vector<SmSurface*> v;
    return v;
}
struct P3PlaneGuard
{
    P3PlaneGuard() = default;
    P3PlaneGuard(const P3PlaneGuard&) = delete;
    P3PlaneGuard& operator=(const P3PlaneGuard&) = delete;
    ~P3PlaneGuard()
    {
        for (SmSurface* s : p3_planes())
            delete s;
        p3_planes().clear();
    }
};
void* p3_user_srf(Model& M, int64_t s)
{
    if (s >= 0 && (size_t)s < M.targetFaces.size())
        return M.targetFaces[(size_t)s]->GetSurface();
    if (s == 1000 || s == 1001)
    {
        SmPlane* pl = s == 1000
                          ? new (M.ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                                                SmVector2d(1, 1), SmExtent2d(0, 0, 1, 1), &M.ctx)
                          : new (M.ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(0, 0, 1), SmVector3d(1, 0, 0),
                                                SmVector2d(2, 0.5), SmExtent2d(0, 0, 1, 1), &M.ctx);
        p3_planes().push_back(pl);
        return pl;
    }
    return NULL;
}

int64_t p3_index_of_vertex(Model& M, SmFilletVertex* v) { return index_of(M.verts, v); }

// ---- pre-checks (upstream's own code up to the unported or unbuildable callees) ----

double p3_check_patch(int64_t kind, SmFilletCorner* c, Model& M)
{
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    switch (kind)
    {
    case 6: // 3x2 mixed convexity
    {
        if (E.GetSize() != 4)
            return 0.0;
        SmFilletEdge* onEdge = NULL;
        SmFilletEdge* onExt = NULL;
        for (ULONG i = 0; i < 4; ++i)
        {
            if (E[i]->GetFilletEdgeType() == SM_FE_ON_EXTEND_EDGE)
                onExt = E[i];
            else if (E[i]->GetFilletEdgeType() == SM_FE_ON_EDGE)
                onEdge = E[i];
        }
        if (onEdge == NULL || onExt == NULL)
            return 0.0;
        SmFilletVertex* cornerV = (SmFilletVertex*)onEdge->GetVertex();
        if (cornerV == NULL)
            return P3C_REFUSE;
        if (cornerV->GetFilletVertexType() != SM_FV_ON_VERTEX || onExt->GetVertex() == cornerV)
            return 0.0;
        SmBSplineCurve* c1 = SM_CAST_PTR(SmBSplineCurve, onExt->GetCurve());
        SmBSplineCurve* c2 = SM_CAST_PTR(SmBSplineCurve, onEdge->GetCurve());
        if (c1 == NULL || c2 == NULL)
            return 0.0;
        // JoinWith on copies: a failure returns before anything changes. The virtual
        // Copy(ctx, SmCurve*&) keeps the derived kind (SmLine, SmEllipse), so the copy runs
        // the same JoinWith override as upstream; Copy(ctx, SmBSplineCurve*&) would slice
        // it to an SmBSplineCurve.
        SmCurve* k1 = NULL;
        SmCurve* k2 = NULL;
        SmStatus cs1 = c1->Copy(M.ctx, k1);
        SmStatus cs2 = c2->Copy(M.ctx, k2);
        SmBSplineCurve* b1 = SM_CAST_PTR(SmBSplineCurve, k1);
        SmBSplineCurve* b2 = SM_CAST_PTR(SmBSplineCurve, k2);
        SmStatus js = (cs1 == SM_SUCCESS && cs2 == SM_SUCCESS && b1 && b2) ? b1->JoinWith(1, b2, 0) : SM_ERR;
        delete k1;
        delete k2;
        if (js != SM_SUCCESS)
            return 0.0;
        ULONG idx = 0;
        if (V.FindElement(cornerV, idx) == FALSE)
            return 0.0; // SER(SM_ERR) before any deletion
        if (onExt->GetOriginalFace() == NULL)
            return P3C_REFUSE; // deletes Model objects, then NER
        return P3C_NOT_RUN;    // SmSurface::DropCurve
    }
    case 3:
    case 5: // Nx1, open
        return E.GetSize() == 3 ? P3C_NOT_RUN : 0.0;
    case 7: // Nx2
        if (E.GetSize() > 3 && E.GetSize() != 6 && V.GetSize() == 0)
            return P3C_REFUSE; // unsigned index wraps around
        return P3C_NOT_RUN;
    default:
        return P3C_NOT_RUN; // MakeAnalyticCornerSurface
    }
}

double p3_check_calc(int64_t kind, SmFilletCorner* c, Model& M)
{
    ULONG total = (c->*P3CornerPeek::edges()).GetSize();
    SmTArray<SmEdgeuse*>* s = c->*P3CornerPeek::solver();
    ULONG fil = s == NULL ? 0 : s->GetSize();
    if ((c->IsBevelCorner() && fil > 1) || total < 3)
        return 0.0;
    return p3_check_patch(kind, c, M);
}

double p3_check_topo(int64_t kind, SmFilletCorner* c, Model& M)
{
    if (kind == 9 || kind == 10)
        return P3C_NOT_RUN; // SmFilletConvexNxNCorner::AdjustTopology
    if (kind != 7)
        return 0.0;
    SmTArray<SmEdgeuse*>* all = c->*P3CornerPeek::all();
    if (all == NULL)
        return P3C_REFUSE;
    if (all->GetSize() > 4)
        return 0.0;
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    if (E.GetSize() == 3)
        return P3C_NOT_RUN; // AdjustConcaveCornerTopology
    for (ULONG ii = 0; ii < V.GetSize(); ++ii)
    {
        SmFilletVertex* v = V[ii];
        if (v->GetFilletVertexType() != SM_FV_RAIL_X_EDGEUSE)
            continue;
        SmTArray<SmFilletGeom*> gs;
        v->GetFilletGeoms(gs);
        if (gs.GetSize() != 2)
            continue;
        SmObject* obj = v->GetPointClassObject();
        SmEdgeuse* side = SM_CAST_PTR(SmEdgeuse, obj);
        if (side == NULL)
            return P3C_REFUSE;
        SmFilletVertexuse* vu1 = v->GetVUAtRailEnd(gs[0]);
        SmFilletVertexuse* vu2 = v->GetVUAtRailEnd(gs[1], vu1);
        if (vu1 == NULL || vu2 == NULL || side->GetEdge() == NULL || side->GetEdge()->GetCurve() == NULL)
            return P3C_REFUSE;
        SmPoint3d p1, p2;
        side->GetEdge()->GetCurve()->EvaluatePoint(vu1->GetTsectPnt().m_dCurveParameter, p1);
        side->GetEdge()->GetCurve()->EvaluatePoint(vu2->GetTsectPnt().m_dCurveParameter, p2);
        double dist = p1.DistanceBetween(p2);
        if (c->IsBlendingCorner())
            return P3C_NOT_RUN; // TestAndConvertBigRadFillet
        if (dist < c->*P3CornerPeek::tol())
            continue;
        const SmVertex* cv = c->GetFilletedVertex();
        if (cv == NULL)
            return P3C_REFUSE;
        SmPoint3d vp = cv->GetPoint();
        if (!(p1.DistanceBetween(vp) < p2.DistanceBetween(vp)) && side->GetRadial() == NULL)
            return P3C_REFUSE;
        if (ii + 1 >= V.GetSize() && ii == 0)
            return P3C_REFUSE; // m_vVertices[lIndex - 1] with lIndex == 0
        return P3C_NOT_RUN;    // SmFilletEdge::MakeFilletEdge
    }
    return 0.0;
}

double p3_check_tangent(int64_t kind, SmFilletCorner* c, Model& M)
{
    if (kind == 2)
        return P3C_NOT_RUN; // SmFilletNx1ClosedCorner::AdjustTangentCorner
    if (kind != 7)
        return 0.0;
    SmTArray<SmEdgeuse*>* s = c->*P3CornerPeek::solver();
    SmTArray<SmEdgeuse*>* all = c->*P3CornerPeek::all();
    if (s == NULL || s->GetSize() != 2 || all == NULL || all->GetSize() != 4)
        return 0.0;
    SmPoint3d pt;
    SmVector3d d1, d2;
    // upstream ignores both statuses: a failed evaluation leaves the vector at its
    // default SM_UNDEF_DOUBLE members, which IsParallelTo then reads (well defined)
    (*s)[0]->NormalizedEvaluate(0.0, FALSE, pt, &d1);
    (*s)[1]->NormalizedEvaluate(0.0, FALSE, pt, &d2);
    if (!d1.IsParallelTo(d2, 2.0))
        return 0.0;
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    double tol = c->*P3CornerPeek::tol();
    for (ULONG ii = 0; ii < V.GetSize(); ++ii)
    {
        SmFilletVertex* v = V[ii];
        if (v->GetFilletVertexType() != SM_FV_RAIL_X_EDGEUSE)
            continue;
        SmTArray<SmFilletGeom*> gs;
        v->GetFilletGeoms(gs);
        if (gs.GetSize() != 2)
            continue;
        for (ULONG g = 0; g < 2; ++g)
            for (ULONG r = 0; r < 2; ++r)
                if (gs[g]->GetRail(r) == NULL)
                    return P3C_REFUSE;
        for (ULONG g = 0; g < 2; ++g)
            for (ULONG r = 0; r < 2; ++r)
                if (gs[g]->GetRail(r)->GetCurve() == NULL)
                    return 0.0;
        SmFilletVertexuse* vu1 = v->GetVUAtRailEnd(gs[0]);
        SmFilletVertexuse* vu2 = v->GetVUAtRailEnd(gs[1], vu1);
        if (vu1 == NULL || vu2 == NULL)
            return P3C_REFUSE;
        SmTsectPnt& t1 = vu1->GetTsectPnt();
        SmTsectPnt& t2 = vu2->GetTsectPnt();
        for (int k = 0; k < 2; ++k)
            if (t1.m_apUserPointer[k] == NULL || t2.m_apUserPointer[k] == NULL)
                return P3C_REFUSE;
        SmPoint3d a = p3_eval_oob((SmSurface*)t1.m_apUserPointer[0], t1.UVPos(0));
        SmPoint3d b = p3_eval_oob((SmSurface*)t2.m_apUserPointer[0], t2.UVPos(0));
        if (a.DistanceBetween(b) > tol)
            return P3C_NOT_RUN; // GetRailIndexOfVertex -> GetRailIndexOfVertexuse
        a = p3_eval_oob((SmSurface*)t1.m_apUserPointer[1], t1.UVPos(1));
        b = p3_eval_oob((SmSurface*)t2.m_apUserPointer[1], t2.UVPos(1));
        if (a.DistanceBetween(b) > tol)
            return P3C_NOT_RUN;
    }
    return 0.0;
}

void p3_put_corner(std::vector<double>& o, SmFilletCorner* c, Model& M)
{
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    o.push_back((double)V.GetSize());
    for (ULONG i = 0; i < V.GetSize(); ++i)
        o.push_back((double)p3_index_of_vertex(M, V[i]));
    o.push_back((double)E.GetSize());
    for (ULONG i = 0; i < E.GetSize(); ++i)
        o.push_back(M.edge_index(E[i]));
    o.push_back(flg(c->*P3CornerPeek::bevel()));
    o.push_back(flg(c->*P3CornerPeek::blending()));
    o.push_back(flg(c->*P3CornerPeek::chamfer()));
    o.push_back(flg(c->*P3CornerPeek::degenerate()));
    o.push_back(flg(c->*P3CornerPeek::constrad()));
    o.push_back(flg(c->*P3CornerPeek::adjusted()));
    o.push_back(flg(c->*P3CornerPeek::ext()));
    o.push_back(c->*P3CornerPeek::setback());
    o.push_back((double)(c->*P3CornerPeek::surfs()).GetSize());
}

int32_t p3_run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    P3PlaneGuard planes; // outlives M, whose intersection points point at the planes
    Model M(ctx);
    if (!M.build(rd))
        return SC_ERR_ARG;

    int64_t kind = rd.idx();
    int64_t vEdge = rd.idx();
    int64_t vSub = rd.idx();
    SmTArray<SmEdgeuse*>* solverEUs = NULL;
    SmTArray<SmEdgeuse*>* allEUs = NULL;
    bool ok = p3_read_eus(rd, ctx, M, solverEUs) && p3_read_eus(rd, ctx, M, allEUs);
    std::vector<SmFilletVertex*> verts;
    std::vector<SmFilletEdge*> edges;
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv && !rd.bad && ok; ++i)
    {
        SmFilletVertex* v = M.vertex(rd.idx());
        ok = v != NULL;
        verts.push_back(v);
    }
    int64_t ne = rd.idx();
    for (int64_t i = 0; i < ne && !rd.bad && ok; ++i)
    {
        SmFilletEdge* e = M.edge(rd.idx());
        ok = e != NULL;
        edges.push_back(e);
    }
    bool bevel = rd.flag(), blending = rd.flag(), chamfer = rd.flag(), degen = rd.flag();
    bool ext = rd.flag(), adjusted = rd.flag();
    double setback = rd.num(), tol = rd.num(), tan = rd.num();
    int64_t nt = rd.idx();
    for (int64_t i = 0; i < nt && !rd.bad && ok; ++i)
    {
        int64_t e = rd.idx(), which = rd.idx();
        double t = rd.num();
        SmPoint2d uv0 = rd.uv(), uv1 = rd.uv();
        int64_t s0 = rd.idx(), s1 = rd.idx();
        SmFilletVertexuse* vu = p3_vu(M, e, which);
        if (vu == NULL)
        {
            ok = false;
            break;
        }
        SmTsectPnt& tp = vu->GetTsectPnt();
        tp.m_dCurveParameter = t;
        tp.UVPos(0) = uv0;
        tp.UVPos(1) = uv1;
        tp.m_apUserPointer[0] = p3_user_srf(M, s0);
        tp.m_apUserPointer[1] = p3_user_srf(M, s1);
    }
    int64_t no = rd.idx();
    for (int64_t i = 0; i < no && !rd.bad && ok; ++i)
    {
        SmFilletEdge* e = M.edge(rd.idx());
        int64_t f = rd.idx();
        if (e == NULL || f < 0 || (size_t)f >= M.targetFaces.size())
        {
            ok = false;
            break;
        }
        e->SetOriginalFace(M.targetFaces[(size_t)f]);
    }
    // pseudo edges whose curve becomes a real SmLine through its end points
    int64_t nl = rd.idx();
    for (int64_t i = 0; i < nl && !rd.bad && ok; ++i)
    {
        SmFilletEdge* e = M.edge(rd.idx());
        SmBSplineCurve* bc = e ? SM_CAST_PTR(SmBSplineCurve, e->GetCurve()) : NULL;
        if (bc == NULL)
        {
            ok = false;
            break;
        }
        SmPoint3d a, b;
        bc->GetEnds(a, b);
        SmLine* l = new (M.ctx) SmLine(a, b, 3, &M.ctx);
        l->GetOrCreateGwNurbPointer();
        e->SetCurve(l, TRUE);
    }
    // pseudo edges whose curve becomes a real SmCircle (SmEllipse::JoinWith in 3x2)
    int64_t na = rd.idx();
    for (int64_t i = 0; i < na && !rd.bad && ok; ++i)
    {
        SmFilletEdge* e = M.edge(rd.idx());
        double cx = rd.num(), cy = rd.num(), cz = rd.num(), r = rd.num(), d0 = rd.num(), d1 = rd.num();
        if (e == NULL)
        {
            ok = false;
            break;
        }
        SmCircle* c = new (M.ctx) SmCircle(SmPoint3d(cx, cy, cz), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                                           SmExtent1d(d0, d1), r, 3, &M.ctx);
        c->GetOrCreateGwNurbPointer();
        e->SetCurve(c, TRUE);
    }
    SmVertex* vertex = NULL;
    if (vEdge >= 0)
    {
        if ((size_t)vEdge >= M.targetEdges.size())
            ok = false;
        else
        {
            SmVertex* vs = NULL;
            SmVertex* ve = NULL;
            M.targetEdges[(size_t)vEdge]->GetVertices(vs, ve);
            vertex = vSub == 0 ? vs : ve;
        }
    }
    if (!ok || rd.bad || kind < 0 || kind > 10)
    {
        delete solverEUs;
        delete allEUs;
        return SC_ERR_ARG;
    }

    // The executive's pseudo B-rep is the Model's while the corner is constructed.
    P3ExecPeek* exec = new (ctx) P3ExecPeek(ctx);
    SmFilletBrep* own = exec->*P3ExecPeek::pseudo();
    exec->*P3ExecPeek::pseudo() = M.pseudo;
    SmFilletCorner* c = p3_make_corner(kind, ctx, vertex, solverEUs, allEUs, exec, tol, tan);
    exec->*P3ExecPeek::pseudo() = own;
    for (SmFilletVertex* v : verts)
        (c->*P3CornerPeek::verts()).Add(v);
    for (SmFilletEdge* e : edges)
        (c->*P3CornerPeek::edges()).Add(e);
    c->*P3CornerPeek::bevel() = bevel ? TRUE : FALSE;
    c->*P3CornerPeek::blending() = blending ? TRUE : FALSE;
    c->*P3CornerPeek::chamfer() = chamfer ? TRUE : FALSE;
    c->*P3CornerPeek::degenerate() = degen ? TRUE : FALSE;
    c->*P3CornerPeek::ext() = ext ? TRUE : FALSE;
    c->*P3CornerPeek::adjusted() = adjusted ? TRUE : FALSE;
    c->*P3CornerPeek::setback() = setback;

    double pre = 0.0;
    switch (op)
    {
    case P3_CALC_GEOM: pre = p3_check_calc(kind, c, M); break;
    case P3_PATCH: pre = p3_check_patch(kind, c, M); break;
    case P3_ADJ_TOPO: pre = p3_check_topo(kind, c, M); break;
    default: pre = p3_check_tangent(kind, c, M); break;
    }
    if (pre != 0.0)
    {
        o.push_back(pre);
    }
    else
    {
        SmStatus r = SM_SUCCESS;
        switch (op)
        {
        case P3_CALC_GEOM: r = c->CalcCornerGeom(); break;
        case P3_PATCH: r = c->CreateCornerPatch(); break;
        case P3_ADJ_TOPO: r = c->AdjustTopology(); break;
        default: r = c->AdjustTangentCorner(); break;
        }
        o.push_back(st(r));
        p3_put_corner(o, c, M);
        M.put(o);
    }
    // the Model owns the fillet vertices; the corner owns its edge use arrays
    (c->*P3CornerPeek::verts()).ReSet();
    delete c;
    delete exec;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_corner_p3_corner(int32_t op, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    if (op < P3_CALC_GEOM || op > P3_ADJ_TANGENT)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    std::vector<double> o;
    int32_t rc = p3_run(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
