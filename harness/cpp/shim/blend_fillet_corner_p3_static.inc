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
// SmFilletCorner::CalcCornerGeom, the virtual CreateCornerPatch, AdjustTopology,
// AdjustTangentCorner, or (unit blend.fillet_corner_p2) UpdateSplitFace (two more inputs
// `orig split`: target faces) and MakeAnalyticCornerSurface (its SmBoolean is the status).
// Layout in crates/solidcore-ffi/src/units/blend_fillet_corner_p3.rs.
//
// Output: `-2` (refused: upstream would dereference null, read past an array or free the
// Model's objects), `-3` (not run: upstream would reach a routine whose Rust port is a stub,
// the case is deferred), else the op's status (`0` = SM_SUCCESS), the corner's members, the
// Model's export (with the fillet vertices the op made appended, and the corner itself
// exported as corner id P3C_THIS_CORNER) and the corner's surfaces. The pre-checks repeat
// upstream's own code up to the point where it would call into what the harness cannot
// provide, as blend_fillet_geom_p4_static.inc does; where upstream's outcome depends on a
// surface fit or blend, they run it (on copies) to know.

// The file-static surface fits of SmFilletCorner.cpp, defined in blend_fillet_corner_p2_static.inc
// (the translation unit that compiles that file).
SmStatus sc_p2_create_planar_surface(const SmContext& ctx, const SmTArray<SmPoint3d>& pts, double tol,
                                     SmSurface*& out);
SmStatus sc_p2_create_srf_of_revolution(const SmContext& ctx, const SmTArray<SmBSplineCurve*>& curves,
                                        ULONG major, double tol, SmSurface*& out);

namespace
{
enum : int32_t
{
    P3_CALC_GEOM = 0,
    P3_PATCH = 1,
    P3_ADJ_TOPO = 2,
    P3_ADJ_TANGENT = 3,
    P3_UPDATE_SPLIT = 4, // SmFilletCorner::UpdateSplitFace (unit blend.fillet_corner_p2)
    P3_ANALYTIC = 5,     // SmFilletCorner::MakeAnalyticCornerSurface (unit blend.fillet_corner_p2)
    P3_BIG_RAD = 6,      // SmFilletExecutive::TestAndConvertBigRadFillet
};

const double P3C_REFUSE = -2.0;
const double P3C_NOT_RUN = -3.0;
// The corner id of the corner under test (`this`) in the export (model corners are 0..3).
const int64_t P3C_THIS_CORNER = 1000000;

struct P3ExecPeek : SmFilletExecutive
{
    P3ExecPeek(const SmContext& c) : SmFilletExecutive(c) {}
    static SmFilletBrep* SmFilletExecutive::*pseudo() { return &P3ExecPeek::m_pPseudoBrep; }
    static SmBrep* SmFilletExecutive::*fillet() { return &P3ExecPeek::m_pFilletBrep; }
    static SmMapPtrToPtr<SmVertex, SmObject> SmFilletExecutive::*vcmap()
    {
        return &P3ExecPeek::m_vVertexCornerMap;
    }
};

struct P3BigRadSolver : P4Solver
{
    P3BigRadSolver(const SmContext& ctx, double tol, double ang, double tang)
        : P4Solver(ctx, tol, ang, tang)
    {
    }
    void set_rails(SmEdgeuse* eu0, SmEdgeuse* eu1, SmOffsetSurface* s0, SmOffsetSurface* s1)
    {
        m_pEdgeuses[0] = eu0;
        m_pEdgeuses[1] = eu1;
        m_pSurfaces[0] = s0;
        m_pSurfaces[1] = s1;
    }
    SmTArray<SmFilletGeom*>& geoms_vec() { return m_vFilletGeoms; }
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
    static double SmFilletCorner::*tantol() { return &P3CornerPeek::m_dTangencyTolRadians; }
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

// What SmFilletCorner::MakeAnalyticCornerSurface (blend.fillet_corner_p2) does on the corner's
// edges E, without changing anything (P3_ANALYTIC runs it, the default corner patch calls it
// first): P3A_REFUSE where it dereferences NULL, P3A_TRUE / P3A_FALSE for its return (its SER /
// NER exits return a non-zero status, which its callers read as TRUE: P3A_TRUE_STATUS, TRUE
// without the surface added to m_vSurfaces; a failed SmFilletCorner::MakeConstantRadiusFillet
// makes it FALSE, p3_const_rad).
// The surface fits are run (the file-static sm_CreatePlanarSurface / sm_CreateSrfOfRevolution
// through blend_fillet_corner_p2_static.inc, CreateSphereFromArcs, the chamfer edges'
// sm_SrfSrfIntersection) and their results deleted. The model's fillet geometries have no
// solver unless the case gives it one (p3_run): a geometry without a solver that the function
// asks for its solver is refused; circular or blend curve generators of two equal fillets
// are refused where the constant radius solver is built. `ov` overrides one edge's 3D
// curve (the 3x2 patch joins one before).
enum P3Analytic
{
    P3A_REFUSE,
    P3A_TRUE,        // a surface was made and added to m_vSurfaces
    P3A_TRUE_STATUS, // a SER / NER exit (read as TRUE): no surface added
    P3A_FALSE,
};

struct P3Override
{
    SmFilletEdge* edge = NULL;
    SmBSplineCurve* curve = NULL;   // the edge's 3D curve
    SmBSplineCurve* mate_uv = NULL; // its primary edge use's mate's UV trim curve
};

SmBSplineCurve* p3_curve_of(SmFilletEdge* e, const P3Override& ov)
{
    return e == ov.edge ? ov.curve : SM_CAST_PTR(SmBSplineCurve, e->GetCurve());
}

// What SmFilletCorner::MakeConstantRadiusFillet (blend.fillet_corner_p1) does on the corner's
// edges E up to where it reads its fillet geometries' solvers: P3A_REFUSE where it dereferences
// NULL or reads m_vEdges[9999], P3A_FALSE for a status (MakeAnalyticCornerSurface then returns
// FALSE). A corner that gets past both projections is refused at the solvers' generators
// when a geometry has none, and past equal circular / blend curve generators (the constant
// radius solver is built there).
P3Analytic p3_const_rad(SmFilletCorner* c, const std::vector<SmFilletEdge*>& E, Model& M, const P3Override& ov)
{
    ULONG n = (ULONG)E.size();
    if (n != 3)
        return P3A_FALSE;
    ULONG found = 9999;
    double minDist = SM_BIG_DOUBLE;
    for (ULONG ii = 0; ii < n; ++ii)
    {
        SmFilletEdge* e = E[ii];
        if (e->GetFilletEdgeType() != SM_FE_CROSS_SECTION)
            return P3A_FALSE;
        SmVertex* v = e->GetVertex();
        SmVertex* ov2 = v != NULL ? e->GetOtherVertex(v) : NULL;
        if (v == NULL || ov2 == NULL)
            return P3A_REFUSE; // GetPoint()
        double d = v->GetPoint().DistanceBetween(ov2->GetPoint());
        if (d < minDist)
        {
            minDist = d;
            found = ii;
        }
    }
    if (found >= n)
        return P3A_REFUSE; // m_vEdges[9999]
    SmFilletEdge* base = E[found];
    SmBSplineCurve* arc = p3_curve_of(base, ov);
    if (arc == NULL)
        return P3A_REFUSE; // pBaseArc->IsArc()
    double tol = c->*P3CornerPeek::tol();
    SmAxis2Placement frame;
    double rad = 0.0, a0 = 0.0, a1 = 0.0;
    if (!arc->IsArc(5, tol, frame, rad, a0, a1))
        return P3A_FALSE;
    SmFilletEdge* e0 = E[(found + n - 1) % n];
    SmFilletEdge* e1 = E[(found + 1) % n];
    SmSurface* srf[2] = {NULL, NULL};
    SmFilletGeom* fgs[2] = {NULL, NULL};
    SmFilletEdge* es[2] = {e0, e1};
    for (int i = 0; i < 2; ++i)
    {
        SmFilletEdgeuse* prim = (SmFilletEdgeuse*)es[i]->GetPrimaryEdgeuse();
        if (prim == NULL)
            return P3A_REFUSE; // GetFilletGeom()
        SmFilletGeom* fg = prim->GetFilletGeom();
        if (fg == NULL || fg->GetFilletSurface() == NULL)
            return P3A_FALSE; // NER
        srf[i] = fg->GetFilletSurface();
        fgs[i] = fg;
    }
    SmPoint3d opp = e0->GetVertex()->GetPoint();
    double t = 0.0;
    SmVector3d z = frame.GetZAxis();
    if (smgu_LineClosestPoint(frame.GetOriginRef(), z, opp, t) != SM_SUCCESS)
        return P3A_FALSE;
    SmLine* line = NULL;
    SmStatus ls = SmLine::CreateLineSegment(M.ctx, 3, frame.GetOriginRef() + z * t, frame.GetOriginRef(), line);
    delete line;
    if (ls != SM_SUCCESS)
        return P3A_FALSE;
    SmVertex* v = base->GetVertex();
    SmVertex* pts[2] = {v, base->GetOtherVertex(v)};
    for (int i = 0; i < 2; ++i)
    {
        SmSolutionArray sols;
        SmExtent2d dom = srf[i]->GetNaturalUVDomain();
        if (srf[i]->GlobalPointSolve(dom, SM_SO_INTERSECT, pts[i]->GetPoint(), tol, NULL, SM_SR_ALL, sols) !=
                SM_SUCCESS ||
            sols.GetSize() != 1)
            return P3A_FALSE;
    }
    // the generators of the two fillets' solvers (a NULL solver or generator is dereferenced)
    SmFilletSurfaceGenerator* fsg[2] = {NULL, NULL};
    for (int i = 0; i < 2; ++i)
    {
        if (fgs[i]->GetFilletSolver() == NULL)
            return P3A_REFUSE;
        fsg[i] = fgs[i]->GetFilletSolver()->GetFilletSurfaceGenerator();
        if (fsg[i] == NULL)
            return P3A_REFUSE;
    }
    SmFilletSurfaceGeneratorType eType = fsg[0]->GetFilletSurfaceGeneratorType();
    if (eType != fsg[1]->GetFilletSurfaceGeneratorType())
        return P3A_FALSE; // SM_ERR
    if (eType == SM_FSG_BLEND_CURVE)
    {
        double s0 = ((SmBlendCurveCrossSectionFSG*)fsg[0])->GetBlendScale();
        double s1 = ((SmBlendCurveCrossSectionFSG*)fsg[1])->GetBlendScale();
        if (smos_Fabs(s0 - s1) > SM_EFF_ZERO)
            return P3A_FALSE; // SM_ERR (unable to convert)
    }
    if (eType == SM_FSG_LINEAR)
        return P3A_FALSE; // SM_ERR
    // SM_FSG_UNKNOWN: pNewFS stays NULL and is dereferenced. Circular or blend curve: the
    // constant radius solver is built and the fillet recomputed (not pre-checked).
    return P3A_REFUSE;
}

// `nulled` (optional) receives the planar corner's edge whose curve upstream deleted
// (`SetCurve(NULL, TRUE)`) before the status exit it then takes (P3A_TRUE_STATUS).
P3Analytic p3_analytic(SmFilletCorner* c, const std::vector<SmFilletEdge*>& E, Model& M,
                       const P3Override& ov = P3Override(), SmFilletEdge** nulled = NULL)
{
    ULONG n = (ULONG)E.size();
    if (n < 3 || n > 4)
        return P3A_FALSE;
    for (ULONG i = 0; i < n; ++i)
    {
        SmFilletEdgeuse* prim = (SmFilletEdgeuse*)E[i]->GetPrimaryEdgeuse();
        if (prim == NULL)
            return P3A_REFUSE;
        SmFilletGeom* fg = prim->GetFilletGeom();
        if (fg == NULL)
            continue;
        if (fg->GetFilletSolver() == NULL || fg->GetFilletSolver()->GetFilletSurfaceGenerator() == NULL)
            return P3A_REFUSE;
        if (fg->GetFilletSolver()->GetFilletSurfaceGenerator()->GetFilletSurfaceGeneratorType() ==
            SM_FSG_BLEND_CURVE)
            return P3A_FALSE;
    }
    double tol = c->*P3CornerPeek::tol();
    bool chamfer = c->*P3CornerPeek::chamfer();
    bool revo = false;
    ULONG major = 0;
    SmTArray<SmPoint3d> pts;
    SmTArray<SmBSplineCurve*> curves;
    for (ULONG i = 0; i < n; ++i)
    {
        SmFilletEdge* e = E[i];
        if (chamfer)
        {
            if (e->GetStartVertex() == NULL)
                return P3A_REFUSE;
            pts.Add(e->GetStartVertex()->GetPoint());
        }
        else if (e->GetFilletEdgeType() == SM_FE_RAIL_RAIL_INTERPOLATION)
        {
            if (e->GetStatus() == SM_FE_CUBIC_RAIL_RAIL_INTERPOLATE)
                return P3A_FALSE;
            revo = true;
            major = i;
        }
        else
        {
            SmFilletEdgeuse* prim = (SmFilletEdgeuse*)e->GetPrimaryEdgeuse();
            SmFilletGeom* fg = prim->GetFilletGeom();
            if (fg == NULL)
            {
                if (prim->GetMate() == NULL)
                    return P3A_REFUSE;
                fg = ((SmFilletEdgeuse*)prim->GetMate())->GetFilletGeom();
            }
            if (fg != NULL && fg->GetFilletSolver() == NULL)
                return P3A_REFUSE; // GetSolverType()
        }
        SmBSplineCurve* b = p3_curve_of(e, ov);
        if (b == NULL)
            return P3A_TRUE_STATUS; // SER(SM_ERR)
        curves.Add(b);
    }
    if (chamfer)
    {
        SmSurface* plane = NULL;
        if (sc_p2_create_planar_surface(M.ctx, pts, tol, plane) != SM_SUCCESS)
        {
            delete plane;
            return P3A_FALSE;
        }
        P3Analytic r = P3A_TRUE;
        for (ULONG i = 0; i < n && r == P3A_TRUE; ++i)
        {
            SmFilletEdge* e = E[i];
            SmPoint3d lp;
            SmVector3d lv;
            if (curves[i]->IsLine(10, tol, lp, lv))
                continue;
            // (the edge's curve is deleted here upstream, SetCurve(NULL, TRUE); the
            // intersection does not read it, but the status exits below leave it NULL)
            SmSurface* fs = NULL;
            double atol = 0.0, ang = 0.0;
            if (e->GetFilletEdgeType() == SM_FE_RAIL_RAIL_INTERPOLATION)
            {
                if (e->GetOriginalFace() == NULL)
                {
                    r = P3A_TRUE_STATUS; // NER
                    if (nulled)
                        *nulled = e;
                    break;
                }
                fs = e->GetOriginalFace()->GetSurface();
                atol = e->GetOriginalFace()->GetTolerance();
                ang = 30.0 * SM_PI / 180.0;
            }
            else
            {
                // pPrimEU->GetFilletGeom() (no mate fallback), its solver's tolerances and
                // its fillet surface: NULL geometry or solver is dereferenced
                SmFilletEdgeuse* pe = (SmFilletEdgeuse*)e->GetPrimaryEdgeuse();
                SmFilletGeom* pfg = pe != NULL ? pe->GetFilletGeom() : NULL;
                if (pfg == NULL || pfg->GetFilletSolver() == NULL)
                {
                    r = P3A_REFUSE;
                    break;
                }
                fs = pfg->GetFilletSurface();
                atol = pfg->GetFilletSolver()->GetThisApproxTol3d();
                ang = pfg->GetFilletSolver()->GetThisAngTolRad();
            }
            if (fs == NULL)
            {
                r = P3A_TRUE_STATUS; // NER
                if (nulled)
                    *nulled = e;
                break;
            }
            SmVertex* sv = e->GetStartVertex();
            SmVertex* ev = e->GetOtherVertex(sv);
            if (ev == NULL)
            {
                r = P3A_REFUSE;
                break;
            }
            SmVector3d dir = ev->GetPoint() - sv->GetPoint();
            SmBSplineCurve *c3 = NULL, *u1 = NULL, *u2 = NULL;
            SmStatus is = sm_SrfSrfIntersection(M.ctx, sv, ev, &dir, NULL, fs, plane, atol, ang, c3, u1, u2,
                                                FALSE, NULL);
            delete c3;
            delete u1;
            delete u2;
            if (is != SM_SUCCESS)
            {
                r = P3A_TRUE_STATUS; // SER
                if (nulled)
                    *nulled = e;
                break;
            }
            if (c3 == NULL)
            {
                r = P3A_REFUSE; // pNew3DCurve->SetOwner()
                break;
            }
        }
        delete plane;
        return r;
    }
    if (revo)
    {
        SmSurface* rs = NULL;
        SmStatus rst = sc_p2_create_srf_of_revolution(M.ctx, curves, major, tol, rs);
        delete rs;
        if (rst == SM_SUCCESS)
            return P3A_TRUE;
        // MakeVariableRadiusFillet: a four-sided corner reaches its first cross section's
        // geometry's solver (dereferenced), unless it returns a status before
        if (n != 4)
            return P3A_FALSE;
        ULONG found = 0;
        while (E[found]->GetFilletEdgeType() != SM_FE_RAIL_RAIL_INTERPOLATION)
            ++found;
        SmFilletEdge* opp = E[(found + 2) % n];
        SmVertex* ov0 = opp->GetVertex();
        if (ov0 == NULL || opp->GetOtherVertex(ov0) == NULL)
            return P3A_REFUSE; // GetPoint()
        SmFilletEdge* cross = E[(found + n - 1) % n];
        if (cross->GetFilletEdgeType() != SM_FE_CROSS_SECTION)
            return P3A_FALSE; // SER(SM_ERR)
        if (((SmFilletEdgeuse*)cross->GetPrimaryEdgeuse())->GetFilletGeom() == NULL)
            return P3A_FALSE; // NER
        return P3A_REFUSE;    // GetFilletSolver()->GetFilletSurfaceGenerator()
    }
    SmTArray<SmCurve*> ccw;
    SmTArray<SmOrientType> orients;
    SmBSplineSurface* sphere = NULL;
    SmStatus sst = SmBSplineSurface::CreateSphereFromArcs(M.ctx, curves, tol, ccw, orients, sphere);
    delete sphere;
    if (sst == SM_SUCCESS)
        return P3A_TRUE;
    // else MakeConstantRadiusFillet: its status makes the function return FALSE
    return p3_const_rad(c, E, M, ov);
}

// SmFilletCorner::MakeFaceBrep after `added` more surfaces on the corner whose m_vEdges are
// `E` (curves overridden by `ov`): with one surface it copies every edge's curve and reads
// the end tangents of the second and third edges and the third edge's vertex: P3C_REFUSE
// where that dereferences NULL or reads past m_vEdges, else 0.0 (it runs).
double p3_check_make_face(SmFilletCorner* c, const std::vector<SmFilletEdge*>& E, const P3Override& ov,
                          ULONG added)
{
    if ((c->*P3CornerPeek::surfs()).GetSize() + added != 1)
        return 0.0;
    if (E.size() < 3)
        return P3C_REFUSE;
    for (SmFilletEdge* e : E)
        if ((e == ov.edge ? (SmCurve*)ov.curve : e->GetCurve()) == NULL)
            return P3C_REFUSE;
    if (E[2]->GetVertex() == NULL)
        return P3C_REFUSE;
    return 0.0;
}

// The default corner patch (SmFilletCorner::CreateCornerPatch) over corner edges E and
// vertices V, through its Coons patch when MakeAnalyticCornerSurface returns FALSE: -2 where
// it dereferences NULL or reads past m_vVertices (or SmFilletCorner::MakeFaceBrep would,
// p3_check_make_face), else 0 (the op runs). CreateCornerBlend runs on the patch's arrays, as
// upstream assembles them, and its blends are deleted.
double p3_check_default_patch(SmFilletCorner* c, const std::vector<SmFilletEdge*>& E,
                              const std::vector<SmFilletVertex*>& V, Model& M,
                              const P3Override& ov = P3Override())
{
    SmFilletEdge* nulled = NULL;
    switch (p3_analytic(c, E, M, ov, &nulled))
    {
    case P3A_REFUSE: return P3C_REFUSE;
    case P3A_TRUE: return p3_check_make_face(c, E, ov, 1);
    // a status exit adds no surface: MakeFaceBrep sees m_vSurfaces as it was (with one
    // surface it copies every edge's curve, the deleted one included)
    case P3A_TRUE_STATUS:
        if (nulled != NULL)
            return (c->*P3CornerPeek::surfs()).GetSize() == 1 ? P3C_REFUSE : 0.0;
        return p3_check_make_face(c, E, ov, 0);
    case P3A_FALSE: break;
    }
    ULONG n = (ULONG)E.size();
    bool ext = c->*P3CornerPeek::ext();
    ULONG init = 0;
    SmFilletVertex* deg = NULL;
    if (n == 3)
    {
        SmFilletCornerType t = c->GetCornerType();
        if (t == SM_FCR_N_x_2 || t == SM_FCR_3_x_2_MIXED || t == SM_FCR_N_x_1)
            for (SmFilletVertex* v : V)
            {
                switch (v->GetFilletVertexType())
                {
                case SM_FV_RAIL_X_EDGEUSE:
                    if (deg == NULL)
                        deg = v;
                    break;
                case SM_FV_FILLET_X_FILLET: deg = v; break;
                case SM_FV_ON_VERTEX:
                    if (t == SM_FCR_N_x_1)
                        deg = v;
                    break;
                default: break;
                }
            }
        if (deg == NULL)
        {
            if (V.size() < 3)
                return P3C_REFUSE; // m_vVertices[ii]
            double d[4];
            for (ULONG ii = 0; ii < 3; ++ii)
                d[ii] = V[ii]->GetPoint().DistanceBetween(V[(ii + 1) % 3]->GetPoint());
            d[3] = d[0];
            ULONG k = 0;
            double minDiff = smos_Fabs(d[2] - d[0]);
            for (ULONG ii = 1; ii < 3; ++ii)
            {
                double diff = smos_Fabs(d[ii - 1] - d[ii]);
                if (diff < minDiff)
                {
                    minDiff = diff;
                    k = ii;
                }
            }
            deg = V[k];
        }
        for (ULONG ii = 0; ii < 3; ++ii)
            if (E[ii]->GetVertex() == deg)
            {
                init = ii;
                break;
            }
    }
    else if (n == 4)
    {
        for (ULONG ii = 0; ii < n; ++ii)
            if (E[ii]->GetFilletEdgeType() == SM_FE_RAIL_RAIL_INTERPOLATION)
            {
                init = (ii + 1) % n;
                break;
            }
    }
    SmTArray<SmBSplineCurve*> uv;
    SmTArray<SmCurve*> c3d;
    SmTArray<SmOrientType> orients;
    SmTArray<SmSurface*> surfs;
    for (ULONG ii = 0; ii < n; ++ii)
    {
        SmFilletEdge* fe = E[(init + ii) % n];
        SmFilletEdgeuse* prim = (SmFilletEdgeuse*)fe->GetPrimaryEdgeuse();
        if (prim == NULL)
            return P3C_REFUSE;
        SmBSplineCurve* ps = NULL;
        SmSurface* base = NULL;
        if (ext)
        {
            if (fe->GetOriginalFace())
            {
                SmEdgeuse* mate = prim->GetMate();
                if (mate == NULL)
                    return P3C_REFUSE;
                ps = fe == ov.edge ? ov.mate_uv : mate->GetUVTrimCurve();
                base = fe->GetOriginalFace()->GetSurface();
            }
            else
            {
                ps = prim->GetUVTrimCurve();
                SmFilletGeom* fg = prim->GetFilletGeom();
                if (fg == NULL)
                    return 0.0; // NER
                base = fg->GetFilletSurface();
            }
        }
        SmCurve* curve = fe == ov.edge ? ov.curve : fe->GetCurve();
        // A NULL curve is not refused: CreateCornerBlend returns SM_ERR below four curves,
        // else sm_CreateNonRationalCurve casts null-safely and returns SM_ERR_NULL_POINTER.
        uv.Add(ps);
        c3d.Add(curve);
        orients.Add(SM_OT_SAME);
        surfs.Add(base);
    }
    SmBSplineCurve* pc = NULL;
    SmBSplineCurve* upc = NULL;
    if (deg)
    {
        SmPoint3d dp = deg->GetPoint();
        if (SmBSplineCurve::CreatePointCurve(M.ctx, dp, pc) != SM_SUCCESS)
            return 0.0;
        SmSurface* base = NULL;
        SmFace* face = SM_CAST_PTR(SmFace, deg->GetPointClassObject());
        if (face && ext)
        {
            base = face->GetSurface();
            SmPoint3d pt(deg->GetOriginalUV());
            if (SmBSplineCurve::CreatePointCurve(M.ctx, pt, upc) != SM_SUCCESS)
            {
                delete pc;
                return 0.0;
            }
            upc->ConvertTo2D();
        }
        uv.Add(upc);
        c3d.Add(pc);
        orients.Add(SM_OT_SAME);
        surfs.Add(base);
    }
    SmTArray<SmSurface*> blends;
    SmStatus bst = SmBSplineSurface::CreateCornerBlend(M.ctx, c3d, orients, uv, surfs, c->*P3CornerPeek::tol(),
                                                      c->*P3CornerPeek::tantol(), blends, ext ? 0 : 1);
    ULONG nblends = blends.GetSize();
    for (ULONG i = 0; i < blends.GetSize(); ++i)
        delete blends[i];
    delete pc;
    delete upc;
    return bst == SM_SUCCESS ? p3_check_make_face(c, E, ov, nblends) : 0.0;
}

// m_vVertices as a vector, without `drop` and at most `count` of them.
std::vector<SmFilletVertex*> p3_verts(SmFilletCorner* c, ULONG count = ~0UL, SmFilletVertex* drop = NULL)
{
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    std::vector<SmFilletVertex*> out;
    for (ULONG i = 0; i < V.GetSize() && out.size() < count; ++i)
        if (V[i] != drop)
            out.push_back(V[i]);
    return out;
}

// The corner's first `count` edges (all of them by default).
std::vector<SmFilletEdge*> p3_edges(SmFilletCorner* c, ULONG count = ~0UL)
{
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    std::vector<SmFilletEdge*> out;
    for (ULONG i = 0; i < E.GetSize() && i < count; ++i)
        out.push_back(E[i]);
    return out;
}

// SmFilletCorner::CreateEndPatch on m_vEdges `E`: without a fillet-side-face edge it returns
// SM_SUCCESS without a surface (-1: m_vSurfaces[0] then reads an empty array), without an
// original face a status (0), a face without surface is dereferenced (P3C_REFUSE), else a copy
// of the surface is added and MakeFaceBrep runs (1, or P3C_REFUSE where p3_check_make_face
// refuses it).
// The fillet vertex the 3x2 patch the current op runs deletes (set by p3_check_patch).
SmFilletVertex* p3_deleted_vertex = NULL;

double p3_check_end_patch(SmFilletCorner* c, const std::vector<SmFilletEdge*>& E)
{
    for (SmFilletEdge* e : E)
    {
        if (e->GetFilletEdgeType() != SM_FE_FILLET_X_SIDE_FACE)
            continue;
        SmFace* f = e->GetOriginalFace();
        if (f == NULL)
            return 0.0;
        SmSurface* s = c->GetExtendedSurface(f);
        if (s == NULL)
            s = f->GetSurface();
        if (s == NULL)
            return P3C_REFUSE;
        return p3_check_make_face(c, E, P3Override(), 1) == 0.0 ? 1.0 : P3C_REFUSE;
    }
    return -1.0;
}

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
        // upstream deletes pFEOnEdge but removes only one m_vEdges entry: a repeated
        // on-edge edge is used after its free
        ULONG uses = 0;
        for (ULONG i = 0; i < 4; ++i)
            uses += E[i] == onEdge ? 1 : 0;
        if (uses > 1)
            return P3C_REFUSE;
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
        delete k2;
        if (js != SM_SUCCESS)
        {
            delete k1;
            return 0.0;
        }
        ULONG idx = 0;
        if (V.FindElement(cornerV, idx) == FALSE)
        {
            delete k1;
            return 0.0; // SER(SM_ERR) before any deletion
        }
        // From here upstream has deleted pFEOnEdge (~SmTopology unlinks it from the pseudo
        // B-rep's edge list) and pCornerV (its uses moved to the surviving vertex, its mates
        // cleared) before any later return, exactly as on success: the op runs, and the
        // Model drops the corner vertex from its own list so neither the export nor ~Model
        // touches it (p3_deleted_vertex, as for the success path below).
        if (onExt->GetOriginalFace() == NULL)
        {
            delete k1;
            p3_deleted_vertex = cornerV;
            return 0.0; // NER(pOrigFace) after the deletions
        }
        // pSurface->DropCurve(.., *pCurve1 (the joined curve), ..) dereferences the original
        // face's surface; the mate of the primary edge use is dereferenced after it. Run the
        // DropCurve here (a pure function of the surface and the joined copy; the model's
        // faces are unit bilinear patches, so UB-182 / HANG-38 do not arise): when it (or the
        // one-curve check) fails, upstream returns its status after the deletions (run, as
        // a missing original face above); else the default patch over the rest follows.
        SmSurface* srf = onExt->GetOriginalFace()->GetSurface();
        if (srf == NULL)
        {
            delete k1;
            return P3C_REFUSE;
        }
        double maxDist = 0, deviation = 0;
        SmTArray<SmBSplineCurve*> uv(M.ctx);
        SmStatus ds = srf->DropCurve(M.ctx, srf->GetNaturalUVDomain(), *k1, k1->GetNaturalInterval(),
                                     c->*P3CornerPeek::tol(), maxDist, deviation, uv);
        bool one = uv.GetSize() == 1;
        SmEdgeuse* primary = onExt->GetPrimaryEdgeuse();
        double pre = 0.0;
        if (ds != SM_SUCCESS || !one)
            p3_deleted_vertex = cornerV; // SER(DropCurve) / SER(SM_ERR) after the deletions
        else if (primary == NULL || primary->GetMate() == NULL)
            pre = P3C_REFUSE;
        else
        {
            // the default patch over the remaining edges and vertices (the on-edge edge and
            // the corner vertex are deleted; the extended edge keeps its type and now holds
            // the joined curve, its mate the dropped UV curve)
            std::vector<SmFilletEdge*> rest;
            for (ULONG i = 0; i < 4; ++i)
                if (E[i] != onEdge)
                    rest.push_back(E[i]);
            P3Override ov;
            ov.edge = onExt;
            ov.curve = SM_CAST_PTR(SmBSplineCurve, k1);
            ov.mate_uv = uv[0];
            pre = p3_check_default_patch(c, rest, p3_verts(c, ~0UL, cornerV), M, ov);
            // the op runs and deletes the corner vertex (upstream `delete`s it): the Model
            // must not export it
            if (pre == 0.0)
                p3_deleted_vertex = cornerV;
        }
        for (ULONG i = 0; i < uv.GetSize(); ++i)
            delete uv[i];
        delete k1;
        return pre;
    }
    case 3:
    case 5: // Nx1, open
    {
        if (E.GetSize() != 3)
            return 0.0; // SM_DBG_WARN only
        if (!(c->*P3CornerPeek::ext()))
            return p3_check_default_patch(c, p3_edges(c), p3_verts(c), M); // the default patch
        // CreateEndPatch (blend.fillet_corner_p1): the first fillet-side-face edge's
        // original face's extended surface or surface is copied, then MakeFaceBrep
        return p3_check_end_patch(c, p3_edges(c)) == P3C_REFUSE ? P3C_REFUSE : 0.0;
    }
    case 7: // Nx2
        if (E.GetSize() > 3 && E.GetSize() != 6 && V.GetSize() == 0)
            return P3C_REFUSE; // unsigned index wraps around
        if (E.GetSize() == 6)
        {
            // CreateEndPatch over the last three edges, then m_vSurfaces[0]
            std::vector<SmFilletEdge*> last;
            for (ULONG i = 3; i < 6; ++i)
                last.push_back(E[i]);
            double end = p3_check_end_patch(c, last);
            if (end == 0.0)
                return 0.0; // SER: a status
            if (end != 1.0)
                return P3C_REFUSE;
            if (V.GetSize() == 0)
                return P3C_REFUSE; // unsigned index wraps around
            // the default patch over the first three edges, on an empty m_vSurfaces
            if ((c->*P3CornerPeek::surfs()).GetSize() != 0)
                return P3C_REFUSE; // (not built by the generators)
        }
        // the default patch over the first three edges
        return p3_check_default_patch(c, p3_edges(c, 3), p3_verts(c, 3), M);
    default:
        return p3_check_default_patch(c, p3_edges(c), p3_verts(c), M); // the default patch
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

// SmFilletNx2Corner::AdjustConcaveCornerTopology up to its first SmFilletEdge::MakeFilletEdge
// (blend.fillet_geom_p5, a stub on the Rust side): -2 where it dereferences null or casts a
// classification object that is not an edge use, 0 when it returns before (no such vertex:
// NER; rails meeting within tolerance), else -3.
double p3_check_concave(SmFilletCorner* c)
{
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    SmFilletVertex* adjust = NULL;
    SmTArray<SmFilletGeom*> gs;
    for (ULONG ii = 0; ii < V.GetSize(); ++ii)
    {
        if (V[ii]->GetFilletVertexType() != SM_FV_RAIL_X_EDGEUSE)
            continue;
        V[ii]->GetFilletGeoms(gs);
        if (gs.GetSize() != 2)
            continue;
        adjust = V[ii];
        break;
    }
    if (adjust == NULL)
        return 0.0; // NER
    SmEdgeuse* side = SM_CAST_PTR(SmEdgeuse, adjust->GetPointClassObject());
    if (side == NULL)
        return P3C_REFUSE;
    SmFilletVertexuse* vu1 = adjust->GetVUAtRailEnd(gs[0]);
    SmFilletVertexuse* vu2 = vu1 ? adjust->GetVUAtRailEnd(gs[1], vu1) : NULL;
    if (vu1 == NULL || vu2 == NULL || side->GetEdge() == NULL || side->GetEdge()->GetCurve() == NULL)
        return P3C_REFUSE;
    SmPoint3d p1, p2;
    side->GetEdge()->GetCurve()->EvaluatePoint(vu1->GetTsectPnt().m_dCurveParameter, p1);
    side->GetEdge()->GetCurve()->EvaluatePoint(vu2->GetTsectPnt().m_dCurveParameter, p2);
    if (p1.DistanceBetween(p2) < c->*P3CornerPeek::tol())
        return 0.0;
    // coincident rail points with a zero tolerance: dGapDist == 0, so
    // AdjustConcaveCornerTopology (SmFilletCorner.cpp:7185) extends the side surface by
    // 10 * 0 and CreateExtendedSurface halves 0 forever (HANG-9, addendum 2026-10-26;
    // stack confirmed at runtime)
    if (p1.DistanceBetween(p2) == 0.0)
        return P3C_REFUSE;
    const SmVertex* cv = c->GetFilletedVertex();
    if (cv == NULL)
        return P3C_REFUSE;
    SmPoint3d vp = cv->GetPoint();
    bool near1 = p1.DistanceBetween(vp) < p2.DistanceBetween(vp);
    if (!near1 && side->GetRadial() == NULL)
        return P3C_REFUSE;
    // the side face's surface is extended (pSideFace->GetSurface()->CreateExtendedSurface)
    SmFace* sideFace = near1 ? side->GetFace() : side->GetRadial()->GetFace();
    if (sideFace == NULL || sideFace->GetSurface() == NULL)
        return P3C_REFUSE;
    // the mates loop over pAdjustVert's remaining vertex uses (vu1 and vu2 moved away; the
    // new edges' uses are not cross sections): GetEdgeuse()->GetEdge(), the mate of an edge
    // use without geometry, and the other vertex's GetMate(0) when the geometry is one of
    // the two
    SmTArray<SmVertexuse*> vus;
    adjust->GetVertexuses(vus);
    for (ULONG ii = 0; ii < vus.GetSize(); ++ii)
    {
        if (vus[ii] == vu1 || vus[ii] == vu2)
            continue;
        SmFilletEdgeuse* eu = (SmFilletEdgeuse*)vus[ii]->GetEdgeuse();
        if (eu == NULL || eu->GetEdge() == NULL)
            return P3C_REFUSE;
        SmFilletEdge* e = (SmFilletEdge*)eu->GetEdge();
        if (e->GetFilletEdgeType() != SM_FE_CROSS_SECTION)
            continue;
        SmFilletGeom* fg = eu->GetFilletGeom();
        if (fg == NULL)
        {
            if (eu->GetMate() == NULL)
                return P3C_REFUSE;
            fg = ((SmFilletEdgeuse*)eu->GetMate())->GetFilletGeom();
            if (fg == NULL)
                return 0.0; // NER(pFG)
        }
        if ((fg == gs[0] || fg == gs[1]) && e->GetOtherVertex(adjust) == NULL)
            return P3C_REFUSE;
    }
    return 0.0;
}

// SmFilletCorner::UpdateSplitFace: a fillet edge whose original face is the split face calls
// SmFilletEdge::ShouldSwapFaces (blend.fillet_geom_p6, ported), after the fillet vertices are
// updated. It dereferences the edge's curve (with an interval or a classification) or its
// corner's vertex: refused (-2) where those are NULL, as blend.fillet_geom_p6's own guard. A
// vertex's point classification object of another kind than a face is compared as a pointer
// only (well defined).
double p3_check_split(SmFilletCorner* c, SmFace* orig)
{
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    for (ULONG ii = 0; ii < E.GetSize(); ++ii)
    {
        SmFilletEdge* e = E[ii];
        if (e->GetOriginalFace() != orig)
            continue;
        SmExtent1d ivl = e->GetInterval();
        if (!(ivl.IsInit() || ivl.HasNegativeLength()) || e->GetCurveClass() != NULL)
        {
            if (e->GetCurve() == NULL)
                return P3C_REFUSE;
        }
        else if (e->GetFilletCorner() != NULL && e->GetFilletCorner()->GetFilletedVertex() == NULL)
            return P3C_REFUSE;
    }
    return 0.0;
}

// The edge use of a fillet vertex use, its edge's type and the fillet geometry on it or its
// mate, as SmFilletConvexNxNCorner::AdjustTopology reads them; false where it would
// dereference NULL.
bool p3_cross_section_geom(SmVertexuse* vu, bool& cross, SmFilletGeom*& fg)
{
    SmFilletEdgeuse* eu = (SmFilletEdgeuse*)vu->GetEdgeuse();
    if (eu == NULL || eu->GetEdge() == NULL)
        return false;
    cross = ((SmFilletEdge*)eu->GetEdge())->GetFilletEdgeType() == SM_FE_CROSS_SECTION;
    fg = NULL;
    if (!cross)
        return true;
    fg = eu->GetFilletGeom();
    if (fg == NULL)
    {
        if (eu->GetMate() == NULL)
            return false;
        fg = ((SmFilletEdgeuse*)eu->GetMate())->GetFilletGeom();
    }
    return true;
}

// SmFilletConvexNxNCorner::AdjustTopology (blend.fillet_corner_p2): -2 where it dereferences
// NULL or writes past sVec (a third cross section at a vertex), else 0. The candidate loop
// dereferences the fillet solver of every cross section's geometry (FindIndexOfRailOnFace,
// GetEdgeuse(0)): with the model's solverless geometries a cross section at a RAIL_X_RAIL
// vertex on a face is refused; the loop's exits before that (more than three vertices, a
// vertex of another type: SER, a vertex not on a face, a geometry missing on both edge uses:
// NER) run.
double p3_check_convex_nxn(SmFilletCorner* c)
{
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    ULONG total = V.GetSize();
    if (total > 3)
        return 0.0;
    for (ULONG ii = 0; ii < total; ++ii)
    {
        SmFilletVertex* cur = V[ii];
        if (cur->GetFilletVertexType() != SM_FV_RAIL_X_RAIL)
            return 0.0; // SER(SM_ERR)
        if (SM_CAST_PTR(SmFace, cur->GetPointClassObject()) == NULL)
            return 0.0; // NER(pOrigFace)
        SmTArray<SmVertexuse*> vus;
        cur->GetVertexuses(vus);
        ULONG count = 0;
        for (ULONG jj = 0; jj < vus.GetSize(); ++jj)
        {
            bool cross = false;
            SmFilletGeom* fg = NULL;
            if (!p3_cross_section_geom(vus[jj], cross, fg))
                return P3C_REFUSE;
            if (!cross)
                continue;
            if (fg == NULL)
                return 0.0; // NER(pFilletGeom)
            if (fg->GetFilletSolver() == NULL)
                return P3C_REFUSE; // pFilSolver->FindIndexOfRailOnFace
            if (++count > 2)
                return P3C_REFUSE; // sVec[2]
            return P3C_REFUSE; // (no model geometry has a solver)
        }
        // no cross section: lCount stays 0 and sVec / sPnt keep their SM_UNDEF_DOUBLE
        // members (equal lengths, no distance): not a candidate
    }
    return 0.0;
}

// Pre-checks SmFilletExecutive::TestAndConvertBigRadFillet(fg) on the standalone corner Model
// (where vertices hold either NULL or opaque integer corner tags 1..=4, and geometries have no
// solver): P3C_REFUSE where it would dereference NULL, an opaque corner tag or a NULL solver,
// 1.0 where NER returns SM_ERR_NULL_POINTER cleanly (stopping AdjustTopology via SER), and 0.0
// where it returns SM_SUCCESS with bConvertedToCorner == FALSE.
double p3_check_big_rad_on_model(SmFilletGeom* fg, SmFilletCorner* c)
{
    if (fg == NULL)
        return P3C_REFUSE;
    SmFilletCorner* sc = NULL;
    SmFilletCorner* ec = NULL;
    for (ULONG r = 0; r < 2; ++r)
    {
        SmFilletEdge* rail = fg->GetRail(r);
        if (rail == NULL)
            return P3C_REFUSE;
        SmEdgeuse* prim = rail->GetPrimaryEdgeuse();
        if (prim == NULL || prim->GetMate() == NULL)
            return P3C_REFUSE;
        if (prim->GetVertexuse() == NULL || prim->GetMate()->GetVertexuse() == NULL)
            return P3C_REFUSE;
        SmFilletVertex* sv = (SmFilletVertex*)prim->GetVertexuse()->GetVertex();
        SmFilletVertex* ev = (SmFilletVertex*)prim->GetMate()->GetVertexuse()->GetVertex();
        if (sv == NULL || ev == NULL)
            return P3C_REFUSE;
        if (r == 0)
        {
            sc = sv->GetFilletCorner();
            if (sc == NULL)
                return 1.0; // NER(pStartCorner)
            if (sc != c)
                return P3C_REFUSE; // opaque integer corner tag in Model
            ec = ev->GetFilletCorner();
            if (ec == NULL)
                return 1.0; // NER(pEndCorner)
            if (ec != c)
                return P3C_REFUSE; // opaque integer corner tag in Model
            if (sc->*P3CornerPeek::adjusted() || ec->*P3CornerPeek::adjusted())
                return 0.0;
        }
        if (sc->GetCornerType() != SM_FCR_N_x_2 || ec->GetCornerType() != SM_FCR_N_x_2
            || sv->IsProcessed() == FALSE || ev->IsProcessed() == FALSE)
        {
            return 0.0;
        }
    }
    if (fg->GetFilletSolver() == NULL)
        return P3C_REFUSE;
    return P3C_REFUSE;
}

double p3_check_topo(int64_t kind, SmFilletCorner* c, Model& M)
{
    if (kind == 9 || kind == 10)
        return p3_check_convex_nxn(c); // SmFilletConvexNxNCorner::AdjustTopology
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
        return p3_check_concave(c); // AdjustConcaveCornerTopology (blend.fillet_corner_p2)
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
        {
            double br1 = p3_check_big_rad_on_model(gs[0], c);
            if (br1 == P3C_REFUSE)
                return P3C_REFUSE;
            if (br1 > 0.0)
                return 0.0;
            double br2 = p3_check_big_rad_on_model(gs[1], c);
            if (br2 == P3C_REFUSE)
                return P3C_REFUSE;
            if (br2 > 0.0)
                return 0.0;
            continue;
        }
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
        // after the split, m_vEdges[lIndex - 1] (lIndex: ii + 1, or ii when that is past the
        // vertices) is removed and deleted: past the array it is undefined
        ULONG removed = ii + 1 < V.GetSize() ? ii : ii - 1;
        if (removed >= E.GetSize())
            return P3C_REFUSE;
        // the loop goes on at ii + 2 with the split done: only the first split is checked
        // here, a second RAIL_X_EDGEUSE vertex with two geometries is not run
        for (ULONG jj = ii + 1; jj < V.GetSize(); ++jj)
        {
            SmTArray<SmFilletGeom*> gs2;
            V[jj]->GetFilletGeoms(gs2);
            if (V[jj]->GetFilletVertexType() == SM_FV_RAIL_X_EDGEUSE && gs2.GetSize() == 2)
                return P3C_NOT_RUN;
        }
        return 0.0;
    }
    return 0.0;
}

// Whether SmFilletGeom::GetRailVertexuse(r, e) can run for every rail end of g without
// dereferencing NULL (the rail, its primary edge use, the mates it walks to).
bool p3_rail_vus_ok(SmFilletGeom* g)
{
    for (ULONG r = 0; r < 2; ++r)
    {
        SmFilletEdge* rail = g->GetRail(r);
        if (rail == NULL)
            return false;
        SmEdgeuse* eu = rail->GetPrimaryEdgeuse();
        if (eu == NULL)
            return false;
        if (eu->GetOrientation() == SM_OT_OPPOSITE)
            eu = eu->GetMate();
        if (eu == NULL || eu->GetMate() == NULL)
            return false;
        if (eu->GetVertexuse() == NULL || eu->GetMate()->GetVertexuse() == NULL)
            return false;
    }
    return true;
}

// SmFilletNx1ClosedCorner::AdjustTangentCorner (blend.fillet_corner_p2) up to its split:
// -2 where it reads m_vVertices[0] / [1] or the geometries of the first vertex past their
// arrays, dereferences a NULL fillet surface, rail vertex use or (the split) fillet solver;
// else 0 (it returns before the split, or SER(SM_ERR)s on a vertex type or a negative
// intersection curve parameter). The model's geometries have no solver, so every case that
// reaches the split is refused.
double p3_check_nx1_closed_tangent(SmFilletCorner* c)
{
    SmTArray<SmEdgeuse*>* s = c->*P3CornerPeek::solver();
    SmTArray<SmEdgeuse*>* all = c->*P3CornerPeek::all();
    if (s == NULL || s->GetSize() != 1 || all == NULL || all->GetSize() != 3)
        return 0.0;
    SmPoint3d pt;
    SmVector3d d1, d2;
    // upstream ignores both statuses (as in the Nx2 check below)
    (*s)[0]->NormalizedEvaluate(0.0, FALSE, pt, &d1);
    (*s)[0]->NormalizedEvaluate(1.0, FALSE, pt, &d2);
    if (!d1.IsParallelTo(d2, 2.0))
        return 0.0;
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    if (V.GetSize() == 0)
        return P3C_REFUSE; // m_vVertices[0]
    SmTArray<SmFilletGeom*> gs;
    V[0]->GetFilletGeoms(gs);
    if (gs.GetSize() == 0 || gs[0]->GetFilletSurface() == NULL)
        return P3C_REFUSE; // sFilletGeoms[0], pFilletSrf
    SmFilletGeom* fg = gs[0];
    SmSurface* srf = fg->GetFilletSurface();
    SmExtent2d dom = srf->GetNaturalUVDomain();
    SmPoint3d p00, p10, p01, p11;
    srf->EvaluatePoint(dom.Evaluate(0, 0), p00);
    srf->EvaluatePoint(dom.Evaluate(1, 0), p10);
    srf->EvaluatePoint(dom.Evaluate(0, 1), p01);
    srf->EvaluatePoint(dom.Evaluate(1, 1), p11);
    double tol = c->*P3CornerPeek::tol();
    if (p00.DistanceBetween(p10) < tol && p01.DistanceBetween(p11) < tol)
        return 0.0;
    SmVector3d su, sv;
    srf->Evaluate1stDerivatives(dom.Evaluate(0, 0), TRUE, TRUE, p00, su, sv);
    double t = 0.0;
    smgu_LineClosestPoint(p00, su, p10, t);
    if (!(t > 0.0))
        return 0.0;
    SmFilletVertex* fv = V[0];
    if (fv->GetFilletVertexType() != SM_FV_RAIL_X_EDGEUSE)
    {
        if (V.GetSize() < 2)
            return P3C_REFUSE; // m_vVertices[1]
        if (V[1]->GetFilletVertexType() != SM_FV_RAIL_X_EDGEUSE)
            return 0.0; // SER(SM_ERR)
    }
    if (!p3_rail_vus_ok(fg))
        return P3C_REFUSE; // GetRailVertexuse(r, e)->GetTsectPnt()
    for (ULONG r = 0; r < 2; ++r)
        for (ULONG e = 0; e < 2; ++e)
            if (fg->GetRailVertexuse(r, e)->GetTsectPnt().m_dCurveParameter < -SM_EFF_ZERO)
                return 0.0; // SER(SM_ERR)
    if (fg->GetFilletSolver() == NULL)
        return P3C_REFUSE; // GetFilletSolver()->SplitFilletGeomAtSideEdge
    return P3C_NOT_RUN;    // (no model geometry has a solver)
}

double p3_check_tangent(int64_t kind, SmFilletCorner* c, Model& M)
{
    if (kind == 2)
        return p3_check_nx1_closed_tangent(c); // SmFilletNx1ClosedCorner::AdjustTangentCorner
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
    bool close_enough = true;
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
        {
            close_enough = false;
            break;
        }
        a = p3_eval_oob((SmSurface*)t1.m_apUserPointer[1], t1.UVPos(1));
        b = p3_eval_oob((SmSurface*)t2.m_apUserPointer[1], t2.UVPos(1));
        if (a.DistanceBetween(b) > tol)
        {
            close_enough = false;
            break;
        }
    }
    if (close_enough)
        return 0.0;

    // The split loop, up to SmFilletSolver::SplitFilletGeomAtSideEdge: a vertex's position
    // drops onto each of its two geometries' rails; a drop inside the rail splits the
    // geometry through pOrigGeom->GetFilletSolver(), which the model's geometries do not
    // have (upstream then dereferences NULL). Without a split nothing is removed from the
    // list, so every vertex is visited.
    for (ULONG ii = 0; ii < V.GetSize(); ++ii)
    {
        SmFilletVertex* v = V[ii];
        if (v->GetFilletVertexType() != SM_FV_RAIL_X_EDGEUSE)
            continue;
        SmTArray<SmFilletGeom*> gs;
        v->GetFilletGeoms(gs);
        if (gs.GetSize() != 2)
            continue;
        SmPoint3d pos = v->GetPoint();
        for (ULONG g = 0; g < 2; ++g)
        {
            // GetRailIndexOfVertex walks every rail end unchecked; its status is ignored
            // and a failure leaves lRailIdx uninitialised
            if (!p3_rail_vus_ok(gs[g]))
                return P3C_REFUSE;
            ULONG idx = 0;
            if (gs[g]->GetRailIndexOfVertex(v, idx) != SM_SUCCESS)
                return P3C_REFUSE;
            SmFilletEdge* rail = gs[g]->GetRail(idx);
            if (rail->GetCurve() == NULL)
                return P3C_REFUSE;
            SmExtent1d ivl = rail->GetInterval();
            SmBoolean ok = FALSE;
            double param = 0.0, dist = 0.0;
            // a failed drop leaves bSuccess uninitialised upstream
            if (rail->GetCurve()->DropPoint(ivl, pos, NULL, SM_BIG_DOUBLE, NULL, ok, param,
                                            dist, SM_SO_MINIMIZE) != SM_SUCCESS)
                return P3C_REFUSE;
            if (!ok)
                continue;
            if (ivl.IsValueOnBoundary(param, tol))
                continue;
            if (gs[g]->GetFilletSolver() == NULL)
                return P3C_REFUSE; // GetFilletSolver()->SplitFilletGeomAtSideEdge
            return P3C_NOT_RUN; // SmFilletSolver::SplitFilletGeomAtSideEdge
        }
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

// The corner's surfaces (m_vSurfaces) after the model export: per surface its kind (`1`
// SmPlane, `2` SmSurfOfRevolution, `0` any other) and its B-spline record (put_surface:
// SmPlane and SmSurfOfRevolution are SmBSplineSurfaces).
void p3_put_surface(std::vector<double>& o, SmSurface* s)
{
    double kind = 0.0;
    if (s != NULL && s->IsKindOf(SmPlane_TYPE))
        kind = 1.0;
    else if (s != NULL && s->IsKindOf(SmSurfOfRevolution_TYPE))
        kind = 2.0;
    o.push_back(kind);
    put_surface(o, s);
}

void p3_put_surfaces(std::vector<double>& o, SmFilletCorner* c)
{
    SmTArray<SmSurface*>& S = c->*P3CornerPeek::surfs();
    o.push_back((double)S.GetSize());
    for (ULONG i = 0; i < S.GetSize(); ++i)
        p3_put_surface(o, S[i]);
}

// The executive's fillet B-rep (m_pFilletBrep, where SmFilletCorner::MakeFaceBrep builds the
// corner's faces): `nf` and per face (GetFaces order) its surface (p3_put_surface), the
// orientation of its first face use, `nl` and per loop (GetLoops) `ne` and per edge use
// (GetEdgeuses, CCW) its orientation, its vertex's point, its edge's interval, its edge's
// 3D curve and its UV trim curve (put_curve).
void p3_put_fillet_brep(std::vector<double>& o, SmBrep* b)
{
    SmTArray<SmFace*> faces;
    if (b != NULL)
        b->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    for (ULONG i = 0; i < faces.GetSize(); ++i)
    {
        SmFace* f = faces[i];
        p3_put_surface(o, f->GetSurface());
        SmFaceuse* fu1 = NULL;
        SmFaceuse* fu2 = NULL;
        f->GetFaceuses(fu1, fu2);
        o.push_back(fu1 != NULL ? (double)fu1->GetOrientation() : -1.0);
        SmTArray<SmLoop*> loops;
        f->GetLoops(loops);
        o.push_back((double)loops.GetSize());
        for (ULONG j = 0; j < loops.GetSize(); ++j)
        {
            SmTArray<SmEdgeuse*> eus;
            loops[j]->GetEdgeuses(eus);
            o.push_back((double)eus.GetSize());
            for (ULONG k = 0; k < eus.GetSize(); ++k)
            {
                SmEdgeuse* eu = eus[k];
                o.push_back((double)eu->GetOrientation());
                SmVertexuse* vu = eu->GetVertexuse();
                SmVertex* v = vu != NULL ? vu->GetVertex() : NULL;
                put_pt(o, v != NULL ? v->GetPoint() : SmPoint3d(0.0, 0.0, 0.0));
                SmEdge* e = eu->GetEdge();
                SmExtent1d ivl = e != NULL ? e->GetInterval() : SmExtent1d(0.0, 0.0);
                o.push_back(ivl.GetMin());
                o.push_back(ivl.GetMax());
                put_curve(o, e != NULL ? e->GetCurve() : NULL);
                put_curve(o, eu->GetUVTrimCurvePointer());
            }
        }
    }
}

void p3_put_error_info(std::vector<double>& o, SmFilletErrorInfo* info, const SmVertex* corner_vert)
{
    if (info == NULL)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back((double)info->GetErrorCode());
    o.push_back((double)info->m_eCornerType);
    double vtag = -1.0;
    if (info->m_pFilletedVertex != NULL)
        vtag = info->m_pFilletedVertex == corner_vert ? 1.0 : 0.0;
    o.push_back(vtag);
    size_t nlen = 0;
    while (nlen < SM_TBLOCK_SIZE && info->m_cEntityName[nlen] != 0)
        ++nlen;
    o.push_back((double)nlen);
    for (size_t i = 0; i < nlen; ++i)
        o.push_back((double)(unsigned char)info->m_cEntityName[i]);
    size_t clen = 0;
    while (clen < SM_TBLOCK_SIZE && info->m_cComment[clen] != 0)
        ++clen;
    o.push_back((double)clen);
    for (size_t i = 0; i < clen; ++i)
        o.push_back((double)(unsigned char)info->m_cComment[i]);
}

int32_t p3_big_rad(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Model M(ctx);
    if (!M.build(rd))
        return SC_ERR_ARG;

    int64_t fg_idx = rd.idx();
    int64_t nc = rd.idx();
    if (rd.bad || fg_idx < 0 || (size_t)fg_idx >= M.geoms.size() || nc < 0 || nc > 4)
        return SC_ERR_ARG;

    P3ExecPeek* exec = new (ctx) P3ExecPeek(ctx);
    exec->*P3ExecPeek::pseudo() = M.pseudo;

    std::vector<SmFilletCorner*> corners;
    for (int64_t i = 0; i < nc && !rd.bad; ++i)
    {
        int64_t kind = rd.idx();
        int64_t vEdge = rd.idx();
        int64_t vSub = rd.idx();
        bool adjusted = rd.flag();
        double tol = rd.num();
        double tan = rd.num();
        int64_t ne = rd.idx();
        const SmVertex* vert = NULL;
        if (vEdge >= 0)
        {
            if ((size_t)vEdge >= M.targetEdges.size())
            {
                rd.bad = true;
                break;
            }
            SmVertex* vs = NULL;
            SmVertex* ve = NULL;
            M.targetEdges[(size_t)vEdge]->GetVertices(vs, ve);
            vert = vSub == 0 ? vs : ve;
        }
        SmFilletCorner* c = p3_make_corner(kind, ctx, vert, NULL, NULL, exec, tol, tan);
        if (c == NULL || ne < 0 || ne > 32)
        {
            delete c;
            rd.bad = true;
            break;
        }
        c->*P3CornerPeek::adjusted() = adjusted ? TRUE : FALSE;
        corners.push_back(c);
        for (int64_t k = 0; k < ne && !rd.bad; ++k)
        {
            SmFilletEdge* e = M.edge(rd.idx());
            if (e == NULL)
            {
                rd.bad = true;
                break;
            }
            (c->*P3CornerPeek::edges()).Add(e);
        }
    }

    int64_t nfc = rd.idx();
    if (nfc < 0 || nfc > nc)
        rd.bad = true;
    for (int64_t i = 0; i < nfc && !rd.bad; ++i)
    {
        int64_t ci = rd.idx();
        if (ci < 0 || ci >= nc)
        {
            rd.bad = true;
            break;
        }
        exec->GetFilletCorners().Add(corners[(size_t)ci]);
    }

    int64_t ns = rd.idx();
    if (ns < 0 || ns > 4)
        rd.bad = true;
    std::vector<P3BigRadSolver*> solvers;
    for (int64_t i = 0; i < ns && !rd.bad; ++i)
    {
        double tol = rd.num();
        double ang = rd.num();
        double tang = rd.num();
        bool has_rails = rd.flag();
        P3BigRadSolver* s = new (ctx) P3BigRadSolver(ctx, tol, ang, tang);
        solvers.push_back(s);
        if (has_rails)
        {
            int64_t f0 = rd.idx(), eu0_i = rd.idx();
            double d0 = rd.num();
            int64_t f1 = rd.idx(), eu1_i = rd.idx();
            double d1 = rd.num();
            if (f0 < 0 || (size_t)f0 >= M.targetFaces.size() || f1 < 0 || (size_t)f1 >= M.targetFaces.size()
                || eu0_i < 0 || eu0_i >= 4 || eu1_i < 0 || eu1_i >= 4)
            {
                rd.bad = true;
                break;
            }
            SmTArray<SmLoop*> l0, l1;
            M.targetFaces[(size_t)f0]->GetLoops(l0);
            M.targetFaces[(size_t)f1]->GetLoops(l1);
            SmTArray<SmEdgeuse*> eus0, eus1;
            if (l0.GetSize() > 0)
                l0[0]->GetEdgeuses(eus0);
            if (l1.GetSize() > 0)
                l1[0]->GetEdgeuses(eus1);
            SmBSplineSurface* bs0 = SM_CAST_PTR(SmBSplineSurface, M.targetFaces[(size_t)f0]->GetSurface());
            SmBSplineSurface* bs1 = SM_CAST_PTR(SmBSplineSurface, M.targetFaces[(size_t)f1]->GetSurface());
            if ((ULONG)eu0_i >= eus0.GetSize() || (ULONG)eu1_i >= eus1.GetSize() || bs0 == NULL || bs1 == NULL)
            {
                rd.bad = true;
                break;
            }
            SmOffsetSurface* os0 = new (ctx) SmOffsetSurface(d0, *bs0);
            SmOffsetSurface* os1 = new (ctx) SmOffsetSurface(d1, *bs1);
            os0->SetAllowExtension(TRUE);
            os1->SetAllowExtension(TRUE);
            s->set_rails(eus0[(ULONG)eu0_i], eus1[(ULONG)eu1_i], os0, os1);
        }
        int64_t ngo = rd.idx();
        if (ngo < 0 || ngo > 4)
        {
            rd.bad = true;
            break;
        }
        for (int64_t k = 0; k < ngo && !rd.bad; ++k)
        {
            SmFilletGeom* g = M.geom(rd.idx());
            if (g == NULL)
            {
                rd.bad = true;
                break;
            }
            s->geoms_vec().Add(g);
        }
    }

    for (size_t g = 0; g < M.geoms.size() && !rd.bad; ++g)
    {
        int64_t si = rd.idx();
        if (si >= (int64_t)solvers.size())
        {
            rd.bad = true;
            break;
        }
        M.geoms[g]->*P4GeomPeek::solver() = si < 0 ? NULL : solvers[(size_t)si];
    }

    int64_t nfs = rd.idx();
    if (nfs < 0 || nfs > ns)
        rd.bad = true;
    for (int64_t i = 0; i < nfs && !rd.bad; ++i)
    {
        int64_t si = rd.idx();
        if (si < 0 || si >= ns)
        {
            rd.bad = true;
            break;
        }
        exec->GetFilletSolvers().Add(solvers[(size_t)si]);
    }

    auto cleanup_all = [&](SmFilletCorner* del_c1, SmFilletCorner* del_c2, P3BigRadSolver* del_s,
                           SmFilletCorner* new_c) {
        exec->*P3ExecPeek::pseudo() = NULL;
        exec->GetFilletCorners().ReSet();
        exec->GetFilletSolvers().ReSet();
        if (new_c != NULL)
        {
            (new_c->*P3CornerPeek::verts()).ReSet();
            delete new_c;
        }
        for (size_t i = 0; i < corners.size(); ++i)
        {
            if (corners[i] != del_c1 && corners[i] != del_c2)
            {
                (corners[i]->*P3CornerPeek::verts()).ReSet();
                delete corners[i];
            }
        }
        for (size_t i = 0; i < solvers.size(); ++i)
        {
            if (solvers[i] != del_s)
            {
                solvers[i]->geoms_vec().ReSet();
                delete solvers[i];
            }
        }
        delete exec;
    };

    if (rd.bad)
    {
        cleanup_all(NULL, NULL, NULL, NULL);
        return SC_ERR_ARG;
    }

    // Replace opaque corner tags (1..=nc) on vertices and edges with the real SmFilletCorner*s;
    // refuse any tag >= nc that would be dereferenced.
    bool bad_tag = false;
    for (size_t i = 0; i < M.verts.size(); ++i)
    {
        uintptr_t raw = (uintptr_t)M.verts[i]->GetFilletCorner();
        if (raw > 0)
        {
            if ((int64_t)(raw - 1) < nc)
                M.verts[i]->SetFilletCorner(corners[(size_t)(raw - 1)]);
            else
                bad_tag = true;
        }
    }
    std::vector<SmFilletEdge*> init_es = M.edges();
    for (size_t i = 0; i < init_es.size(); ++i)
    {
        uintptr_t raw = (uintptr_t)init_es[i]->GetFilletCorner();
        if (raw > 0)
        {
            if ((int64_t)(raw - 1) < nc)
                init_es[i]->SetFilletCorner(corners[(size_t)(raw - 1)]);
            else
                bad_tag = true;
        }
    }

    SmFilletGeom* fg = M.geoms[(size_t)fg_idx];
    // Pre-check null dereferences and self-aliasing UAFs before mutating.
    bool refuse = bad_tag;
    SmFilletVertex* sv[2] = {NULL, NULL};
    SmFilletVertex* ev[2] = {NULL, NULL};
    SmFilletEdge* rails[2] = {NULL, NULL};
    SmFilletCorner* sc = NULL;
    SmFilletCorner* ec = NULL;
    bool early_exit = false;
    for (ULONG r = 0; r < 2 && !refuse && !early_exit; ++r)
    {
        rails[r] = fg->GetRail(r);
        if (rails[r] == NULL)
        {
            refuse = true;
            break;
        }
        SmEdgeuse* prim = rails[r]->GetPrimaryEdgeuse();
        if (prim == NULL || prim->GetMate() == NULL || prim->GetVertexuse() == NULL
            || prim->GetMate()->GetVertexuse() == NULL)
        {
            refuse = true;
            break;
        }
        sv[r] = (SmFilletVertex*)prim->GetVertexuse()->GetVertex();
        ev[r] = (SmFilletVertex*)prim->GetMate()->GetVertexuse()->GetVertex();
        if (sv[r] == NULL || ev[r] == NULL)
        {
            refuse = true;
            break;
        }
        if (r == 0)
        {
            sc = sv[0]->GetFilletCorner();
            ec = ev[0]->GetFilletCorner();
            if (sc == NULL || ec == NULL)
            {
                early_exit = true; // NER
                break;
            }
            if (sc->*P3CornerPeek::adjusted() || ec->*P3CornerPeek::adjusted())
            {
                early_exit = true;
                break;
            }
        }
        if (sc->GetCornerType() != SM_FCR_N_x_2 || ec->GetCornerType() != SM_FCR_N_x_2
            || sv[r]->IsProcessed() == FALSE || ev[r]->IsProcessed() == FALSE)
        {
            early_exit = true;
            break;
        }
    }
    P3BigRadSolver* fg_solver = (P3BigRadSolver*)(fg->*P4GeomPeek::solver());
    SmFilletVertex* will_delete_ev = NULL;
    if (!refuse && !early_exit)
    {
        if (fg_solver == NULL)
        {
            refuse = true;
        }
        else
        {
            SmVector3d d0 = ev[0]->GetPoint() - sv[0]->GetPoint();
            SmVector3d d1 = ev[1]->GetPoint() - sv[1]->GetPoint();
            double tol = fg_solver->GetThisApproxTol3d();
            if (d0.Length() > tol && d1.Length() > tol && d0.Dot(d1) <= 0.0)
            {
                ULONG sq = 0;
                if (sv[0]->GetFilletVertexType() != SM_FV_MATE || ev[0]->GetFilletVertexType() != SM_FV_MATE)
                {
                    sq = 1;
                    if (sv[1]->GetFilletVertexType() != SM_FV_MATE || ev[1]->GetFilletVertexType() != SM_FV_MATE)
                        early_exit = true; // SER(SM_ERR)
                }
                if (!early_exit)
                {
                    ULONG ot = 1 - sq;
                    if (rails[0] == rails[1] || sv[sq] == ev[sq] || ev[sq] == sv[ot] || ev[sq] == ev[ot] || sc == ec)
                        refuse = true;
                    else
                        will_delete_ev = ev[sq];
                }
            }
        }
    }

    if (refuse)
    {
        o.push_back(P3C_REFUSE);
        cleanup_all(NULL, NULL, NULL, NULL);
        return SC_OK;
    }

    // Snapshot geoms and vertices owned by fg_solver in case `delete pFS` runs.
    std::vector<SmFilletGeom*> solver_geoms;
    if (fg_solver != NULL)
    {
        SmTArray<SmFilletGeom*>& gv = fg_solver->geoms_vec();
        for (ULONG i = 0; i < gv.GetSize(); ++i)
            solver_geoms.push_back(gv[i]);
    }

    SmBoolean converted = FALSE;
    SmStatus r = exec->TestAndConvertBigRadFillet(fg, converted);

    // Track what TestAndConvertBigRadFillet deleted so Model::~Model and cleanup_all don't
    // touch freed memory.
    if (will_delete_ev != NULL)
    {
        for (size_t k = 0; k < M.verts.size(); ++k)
            if (M.verts[k] == will_delete_ev)
            {
                M.verts.erase(M.verts.begin() + (std::ptrdiff_t)k);
                break;
            }
    }

    SmFilletCorner* del_c1 = NULL;
    SmFilletCorner* del_c2 = NULL;
    P3BigRadSolver* del_s = NULL;
    SmFilletCorner* new_c = NULL;
    if (r == SM_SUCCESS && converted != FALSE)
    {
        del_c1 = sc;
        del_c2 = ec;
        del_s = fg_solver;
        for (size_t g = 0; g < solver_geoms.size(); ++g)
        {
            for (size_t k = 0; k < M.geoms.size(); ++k)
                if (M.geoms[k] == solver_geoms[g])
                {
                    M.geoms.erase(M.geoms.begin() + (std::ptrdiff_t)k);
                    break;
                }
        }
        SmTArray<SmFilletCorner*>& fc = exec->GetFilletCorners();
        for (ULONG i = 0; i < fc.GetSize(); ++i)
            if (index_of(corners, fc[i]) < 0)
                new_c = fc[i];
    }
    else if (r != SM_SUCCESS && sc != NULL && ec != NULL)
    {
        // Note: if failure occurred at m_vFilletSolvers.FindElement (after lines 4456-4457),
        // sc and ec were already deleted! Detect whether sc/ec were removed from m_vFilletCorners
        // AND pOrigVertS/E were non-NULL (lines 4449-4450 passed).
        ULONG dummy = 0;
        bool sc_in = exec->GetFilletCorners().FindElement(sc, dummy) != FALSE;
        bool ec_in = exec->GetFilletCorners().FindElement(ec, dummy) != FALSE;
        if (!sc_in && !ec_in && sc->GetFilletedVertex() != NULL && ec->GetFilletedVertex() != NULL)
        {
            // Check if vertex_corner_map received the insert (lines 4453-4454 ran right before delete)
            if ((exec->*P3ExecPeek::vcmap()).Contains((SmVertex*)sc->GetFilletedVertex()) != FALSE)
            {
                del_c1 = sc;
                del_c2 = ec;
            }
        }
    }

    int64_t new_id = nc;
    auto map_corner_ptr = [&](const SmFilletCorner* p) -> double {
        if (p == NULL)
            return -1.0;
        if (p == new_c && new_c != NULL)
            return (double)new_id;
        int64_t idx = index_of(corners, p);
        return idx >= 0 ? (double)idx : -2.0;
    };

    // Convert real corner pointers on M's vertices and edges back to corner tags for M.put(o).
    for (size_t i = 0; i < M.verts.size(); ++i)
    {
        double cid = map_corner_ptr(M.verts[i]->GetFilletCorner());
        M.verts[i]->SetFilletCorner(cid < 0.0 ? NULL : (SmFilletCorner*)(uintptr_t)((int64_t)cid + 1));
    }
    std::vector<SmFilletEdge*> out_es = M.edges();
    for (size_t i = 0; i < out_es.size(); ++i)
    {
        double cid = map_corner_ptr(out_es[i]->GetFilletCorner());
        out_es[i]->SetFilletCorner(cid < 0.0 ? NULL : (SmFilletCorner*)(uintptr_t)((int64_t)cid + 1));
    }

    o.push_back(st(r));
    o.push_back(flg(converted));
    if (r == SM_SUCCESS)
    {
        SmTArray<SmFilletCorner*>& fc = exec->GetFilletCorners();
        o.push_back((double)fc.GetSize());
        for (ULONG i = 0; i < fc.GetSize(); ++i)
            o.push_back(map_corner_ptr(fc[i]));

        SmTArray<SmFilletSolver*>& fs = exec->GetFilletSolvers();
        o.push_back((double)fs.GetSize());
        for (ULONG i = 0; i < fs.GetSize(); ++i)
        {
            int64_t si = -1;
            for (size_t k = 0; k < solvers.size(); ++k)
                if ((SmFilletSolver*)solvers[k] == fs[i])
                    si = (int64_t)k;
            o.push_back((double)si);
        }

        if (converted != FALSE && new_c != NULL)
        {
            o.push_back((double)new_c->GetCornerType());
            SmTArray<SmFilletVertex*>& nv = new_c->*P3CornerPeek::verts();
            o.push_back((double)nv.GetSize());
            for (ULONG i = 0; i < nv.GetSize(); ++i)
                o.push_back((double)index_of(M.verts, nv[i]));
            SmTArray<SmFilletEdge*>& ne = new_c->*P3CornerPeek::edges();
            o.push_back((double)ne.GetSize());
            for (ULONG i = 0; i < ne.GetSize(); ++i)
                o.push_back(M.edge_index(ne[i]));
            o.push_back(new_c->*P3CornerPeek::tol());
            o.push_back(new_c->*P3CornerPeek::tantol());

            o.push_back((double)(M.targetEdges.size() * 2));
            for (size_t i = 0; i < M.targetEdges.size(); ++i)
            {
                SmVertex* vs = NULL;
                SmVertex* ve = NULL;
                M.targetEdges[i]->GetVertices(vs, ve);
                for (SmVertex* tv : {vs, ve})
                {
                    SmObject* obj = (exec->*P3ExecPeek::vcmap()).At(tv);
                    if (obj != NULL)
                        o.push_back(map_corner_ptr((const SmFilletCorner*)obj));
                    else
                        o.push_back(-1.0);
                }
            }
        }
        M.put(o);
    }

    cleanup_all(del_c1, del_c2, del_s, new_c);
    return SC_OK;
}

// The solvers (and their surface generators) a corner case puts on the model's geometries.
struct P3GeomSolvers
{
    std::vector<P4Solver*> solvers;
    std::vector<SmFilletSurfaceGenerator*> fsgs;
    ~P3GeomSolvers()
    {
        for (size_t i = 0; i < solvers.size(); ++i)
            delete solvers[i];
        for (size_t i = 0; i < fsgs.size(); ++i)
            delete fsgs[i];
    }
};

int32_t p3_run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    P3PlaneGuard planes; // outlives M, whose intersection points point at the planes
    P3GeomSolvers GS;    // outlives M, whose geometries point at the solvers
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
        l->SetOwner(e); // upstream follows SetCurve with SetOwner(edge)
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
        c->SetOwner(e);
    }
    // solvers on the model's geometries: `n (approx tang fsg)..` and, when n > 0, per
    // geometry its solver index (-1 none); a base solver (angle tolerance SM_EFF_ZERO_RAD)
    // carrying the cross-section kind's generator (GS, declared before M).
    {
        int64_t ngs = rd.idx();
        if (ngs < 0 || ngs > 8)
            rd.bad = true;
        for (int64_t i = 0; i < ngs && !rd.bad; ++i)
        {
            double atol = rd.num();
            double tang = rd.num();
            int64_t fsg = rd.idx();
            if (fsg < 0 || fsg > 3)
            {
                rd.bad = true;
                break;
            }
            P4Solver* s = new (ctx) P4Solver(ctx, atol, SM_EFF_ZERO_RAD, tang);
            SmFilletSurfaceGenerator* f = NULL;
            switch (fsg)
            {
            case 1: f = new (ctx) SmLinearCrossSectionFSG(); break;
            case 2: f = new (ctx) SmCircularCrossSectionFSG(); break;
            case 3: f = new (ctx) SmBlendCurveCrossSectionFSG(); break;
            default: f = new (ctx) SmFilletSurfaceGenerator(); break;
            }
            s->SetFilletSurfaceGenerator(f);
            GS.solvers.push_back(s);
            GS.fsgs.push_back(f);
        }
        for (size_t g = 0; g < M.geoms.size() && ngs > 0 && !rd.bad; ++g)
        {
            int64_t si = rd.idx();
            if (si >= (int64_t)GS.solvers.size())
            {
                rd.bad = true;
                break;
            }
            M.geoms[g]->*P4GeomPeek::solver() = si < 0 ? NULL : GS.solvers[(size_t)si];
        }
    }
    SmFace* splitFaces[2] = {NULL, NULL};
    if (op == P3_UPDATE_SPLIT)
        for (int k = 0; k < 2 && ok && !rd.bad; ++k)
        {
            int64_t f = rd.idx();
            if (f < 0 || (size_t)f >= M.targetFaces.size())
                ok = false;
            else
                splitFaces[k] = M.targetFaces[(size_t)f];
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
    // m_pFilletBrep, where MakeFaceBrep builds the corner's faces (`new (ctx) SmBrep()`, as
    // SmFilletExecutive::CreateFilletBrep makes it). Left alive (leaked): its faces may own
    // surfaces the corner's m_vSurfaces still points to.
    SmBrep* filletBrep = new (ctx) SmBrep();
    exec->*P3ExecPeek::fillet() = filletBrep;
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
    p3_deleted_vertex = NULL;
    switch (op)
    {
    case P3_CALC_GEOM: pre = p3_check_calc(kind, c, M); break;
    case P3_PATCH: pre = p3_check_patch(kind, c, M); break;
    case P3_ADJ_TOPO: pre = p3_check_topo(kind, c, M); break;
    case P3_UPDATE_SPLIT: pre = p3_check_split(c, splitFaces[0]); break;
    case P3_ANALYTIC:
        switch (p3_analytic(c, p3_edges(c), M))
        {
        case P3A_REFUSE: pre = P3C_REFUSE; break;
        default: break;
        }
        break;
    default: pre = p3_check_tangent(kind, c, M); break;
    }
    if (pre != 0.0)
    {
        o.push_back(pre);
    }
    else
    {
        if (p3_deleted_vertex != NULL)
            for (size_t k = 0; k < M.verts.size(); ++k)
                if (M.verts[k] == p3_deleted_vertex)
                {
                    M.verts.erase(M.verts.begin() + (std::ptrdiff_t)k);
                    break;
                }
        SmStatus r = SM_SUCCESS;
        switch (op)
        {
        case P3_CALC_GEOM: r = c->CalcCornerGeom(); break;
        case P3_PATCH: r = c->CreateCornerPatch(); break;
        case P3_ADJ_TOPO: r = c->AdjustTopology(); break;
        case P3_UPDATE_SPLIT: r = c->UpdateSplitFace(splitFaces[0], splitFaces[1]); break;
        case P3_ANALYTIC: break;
        default: r = c->AdjustTangentCorner(); break;
        }
        // MakeAnalyticCornerSurface's SmBoolean (its SER / NER statuses read as TRUE) is the
        // op's status: 1 / 0
        if (op == P3_ANALYTIC)
            o.push_back(c->MakeAnalyticCornerSurface() != FALSE ? 1.0 : 0.0);
        else
            o.push_back(st(r));
        // fillet vertices the op made (the topology adjustments' new SmFilletVertex(this)):
        // appended to the model's list in creation order, as the Rust model lists every
        // live vertex, and owned by the model from here on (the corner never deletes its
        // m_vVertices). Each op appends its new vertices to m_vVertices in the order it
        // makes them; the pseudo B-rep's edges are scanned after, for any other.
        {
            SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
            for (ULONG i = 0; i < V.GetSize(); ++i)
                if (index_of(M.verts, V[i]) < 0)
                    M.verts.push_back(V[i]);
            std::vector<SmFilletEdge*> es = M.edges();
            for (SmFilletEdge* e : es)
            {
                SmVertex* vs = NULL;
                SmVertex* ve = NULL;
                e->GetVertices(vs, ve);
                for (SmVertex* v : {vs, ve})
                    if (v != NULL && index_of(M.verts, (SmFilletVertex*)v) < 0)
                        M.verts.push_back((SmFilletVertex*)v);
            }
        }
        // the vertices and edges the op made store the corner itself (`this`): exported as
        // the corner id P3C_THIS_CORNER the Rust side passes as the corner's identity
        for (SmFilletVertex* v : M.verts)
            if (v->GetFilletCorner() == c)
                v->SetFilletCorner((SmFilletCorner*)(uintptr_t)(P3C_THIS_CORNER + 1));
        for (SmFilletEdge* e : M.edges())
            if (e->GetFilletCorner() == c)
                e->SetFilletCorner((SmFilletCorner*)(uintptr_t)(P3C_THIS_CORNER + 1));
        p3_put_corner(o, c, M);
        M.put(o);
        p3_put_surfaces(o, c);
        p3_put_fillet_brep(o, filletBrep);
        p3_put_error_info(o, exec->GetFilletErrorInfo(), vertex);
    }
    exec->*P3ExecPeek::fillet() = NULL;
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
    if (op < P3_CALC_GEOM || op > P3_BIG_RAD)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    // UpdateSplitFace classifies fillet vertices on the target faces (sm_ClassifyFacePoint
    // builds their surface caches): the UB-166 constructed-v-knot guard of read_surface
    // (blend_fillet_geom_p1.cpp) refuses such a surface while the Model is built, before
    // any classification, as for OP_FB_SPLIT_FACE.
    rd.cache_op = op == P3_UPDATE_SPLIT;
    std::vector<double> o;
    int32_t rc = op == P3_BIG_RAD ? p3_big_rad(rd, o) : p3_run(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
