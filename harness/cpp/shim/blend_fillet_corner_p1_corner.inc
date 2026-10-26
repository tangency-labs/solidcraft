// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp, after blend_fillet_corner_p3_static.inc: the
// difftest shim of unit `blend.fillet_corner_p1`, part 2 (the SmFilletCorner methods). Every op
// builds a corner of a given derived class on blend_fillet_geom_p1.cpp's pseudo-B-rep Model, as
// blend_fillet_corner_p3_static.inc does (its p3_make_corner / p3_read_eus / P3CornerPeek /
// p3_put_corner are reused), so this shim lives in that translation unit. Part 1 (the file-static
// helpers, on a real SmBrep) is blend_fillet_corner_p1.cpp.
//
// One entry point, sc_blend_fillet_corner_p1_corner(op, in, n_in, out, cap, out_len). Layout in
// crates/solidcore-ffi/src/units/blend_fillet_corner_p1.rs (`corner`), which validates every
// input. Output: `-2` (refused: upstream would dereference null or free an object twice), `-3`
// (not run: the Rust port of the routine is a stub, the case is deferred), else `0` and the op's
// record.

#include <string>
#include <utility>

namespace
{
enum : int32_t
{
    C1_PEEK = 0,
    C1_SOLVERS = 1,
    C1_EXT_SURFACE = 2,
    C1_REINIT = 3,
    C1_SQUEEZE = 4,
    C1_RAIL_RAIL = 5,
    C1_VERT_GEOM = 6,
    C1_EDGE_GEOM = 7,
    C1_FIX_DEGENERACY = 8,
    C1_END_PATCH = 9,
    C1_MAKE_FACE = 10,
    C1_INSERT = 11,
    C1_CONST_RAD = 12,
};

const double C1_REFUSE = -2.0;
const double C1_NOT_RUN = -3.0;

struct C1ExecPeek : SmFilletExecutive
{
    C1ExecPeek(const SmContext& c) : SmFilletExecutive(c) {}
    static SmFilletBrep* SmFilletExecutive::*pseudo() { return &C1ExecPeek::m_pPseudoBrep; }
    static SmMapPtrToPtr<SmEdgeuse, SmFilletSolver> SmFilletExecutive::*solvers()
    {
        return &C1ExecPeek::m_vEdgeuseSolverMap;
    }
    static SmTArray<SmEdge*> SmFilletExecutive::*topo() { return &C1ExecPeek::m_vTopoEdges; }
    static SmBrep* SmFilletExecutive::*fillet() { return &C1ExecPeek::m_pFilletBrep; }
};

// The solvers registered in the executive's edge-use-to-solver map (owned here).
struct C1Solvers
{
    std::vector<P4Solver*> v;
    ~C1Solvers()
    {
        for (size_t i = 0; i < v.size(); ++i)
            delete v[i];
    }
};

// A corner of the given derived class on the Model: `kind v_edge v_sub [solver eus] [all eus]
// n verts.. n edges.. bevel blending chamfer degenerate ext adjusted setback tol tan` (the first
// part of blend_fillet_corner_p3_static.inc's corner layout, without intersection points and
// original faces).
struct C1Corner
{
    Model& M;
    C1ExecPeek* exec;
    SmFilletCorner* c;
    int64_t kind;
    SmVertex* vertex;
    bool ok;

    C1Corner(Model& m, Rd& rd)
        : M(m), exec(NULL), c(NULL), kind(-1), vertex(NULL), ok(true)
    {
        const SmContext& ctx = M.ctx;
        kind = rd.idx();
        int64_t vEdge = rd.idx();
        int64_t vSub = rd.idx();
        SmTArray<SmEdgeuse*>* solverEUs = NULL;
        SmTArray<SmEdgeuse*>* allEUs = NULL;
        ok = p3_read_eus(rd, ctx, M, solverEUs) && p3_read_eus(rd, ctx, M, allEUs);
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
        // original faces: `n (edge face)..` (SetOriginalFace of a pseudo edge)
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
        if (vEdge <= -2)
        {
            // vertex vSub of target face -(vEdge + 2), GetVertices order
            int64_t f = -(vEdge + 2);
            SmTArray<SmVertex*> fv;
            if ((size_t)f < M.targetFaces.size())
                M.targetFaces[(size_t)f]->GetVertices(fv);
            if (vSub < 0 || (ULONG)vSub >= fv.GetSize())
                ok = false;
            else
                vertex = fv[(ULONG)vSub];
        }
        else if (vEdge >= 0)
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
            ok = false;
            return;
        }
        // The executive's pseudo B-rep is the Model's while the corner is constructed.
        exec = new (ctx) C1ExecPeek(ctx);
        SmFilletBrep* own = exec->*C1ExecPeek::pseudo();
        exec->*C1ExecPeek::pseudo() = M.pseudo;
        c = p3_make_corner(kind, ctx, vertex, solverEUs, allEUs, exec, tol, tan);
        exec->*C1ExecPeek::pseudo() = own;
        for (size_t i = 0; i < verts.size(); ++i)
            (c->*P3CornerPeek::verts()).Add(verts[i]);
        for (size_t i = 0; i < edges.size(); ++i)
            (c->*P3CornerPeek::edges()).Add(edges[i]);
        c->*P3CornerPeek::bevel() = bevel ? TRUE : FALSE;
        c->*P3CornerPeek::blending() = blending ? TRUE : FALSE;
        c->*P3CornerPeek::chamfer() = chamfer ? TRUE : FALSE;
        c->*P3CornerPeek::degenerate() = degen ? TRUE : FALSE;
        c->*P3CornerPeek::ext() = ext ? TRUE : FALSE;
        c->*P3CornerPeek::adjusted() = adjusted ? TRUE : FALSE;
        c->*P3CornerPeek::setback() = setback;
    }

    ~C1Corner()
    {
        // the Model owns the fillet vertices; the corner owns its edge use arrays; the
        // executive owns the extended surfaces (its destructor deletes them)
        if (c != NULL)
        {
            (c->*P3CornerPeek::verts()).ReSet();
            delete c;
        }
        delete exec;
    }

    // The corner's vertex as `(target edge, 0 start / 1 end)`, `(-1, -1)` for none.
    void put_vertex(std::vector<double>& o, const SmVertex* v) const
    {
        for (size_t i = 0; i < M.targetEdges.size(); ++i)
        {
            SmVertex* vs = NULL;
            SmVertex* ve = NULL;
            M.targetEdges[i]->GetVertices(vs, ve);
            if (v != NULL && vs == v)
            {
                o.push_back((double)i);
                o.push_back(0.0);
                return;
            }
            if (v != NULL && ve == v)
            {
                o.push_back((double)i);
                o.push_back(1.0);
                return;
            }
        }
        o.push_back(-1.0);
        o.push_back(-1.0);
    }
};

// `set_bevel set_constrad set_chamfer add_vertex nk kinds..` -> the data access methods.
void c1_peek(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    int64_t setBevel = rd.idx(), setConst = rd.idx(), setChamfer = rd.idx(), addVertex = rd.idx();
    int64_t setG1 = rd.idx(), setStatus = rd.idx();
    bool prefill = rd.flag();
    int64_t nk = rd.idx();
    std::vector<int64_t> kinds;
    for (int64_t i = 0; i < nk && !rd.bad; ++i)
        kinds.push_back(rd.idx());
    if (rd.bad)
        return;
    if (setBevel >= 0)
        c->SetBevel(setBevel != 0 ? TRUE : FALSE);
    if (setConst >= 0)
        c->SetCalcAsConstRad(setConst != 0 ? TRUE : FALSE);
    if (setChamfer >= 0 && (C.kind == 9 || C.kind == 10))
        ((SmFilletConvexNxNCorner*)c)->SetChamfer(setChamfer != 0 ? TRUE : FALSE);
    if (addVertex >= 0)
    {
        SmFilletVertex* v = C.M.vertex(addVertex);
        if (v == NULL)
        {
            o.push_back(C1_REFUSE);
            return;
        }
        c->AddFilletVertex(v);
    }
    // m_bG1Case is a public member of the two derived classes that have it
    if (setG1 >= 0 && C.kind == 2)
        ((SmFilletNx1ClosedCorner*)c)->m_bG1Case = setG1 != 0 ? TRUE : FALSE;
    if (setG1 >= 0 && C.kind == 7)
        ((SmFilletNx2Corner*)c)->m_bG1Case = setG1 != 0 ? TRUE : FALSE;
    if (setStatus >= 0)
        c->SetStatus((SmFilStatus)setStatus);
    o.push_back(0.0);
    o.push_back(c->GetThisApproxTol3d());
    SmTArray<SmFilletEdge*> E;
    if (prefill && C.M.edge(0) != NULL)
        E.Add(C.M.edge(0));
    c->GetFilletEdges(E);
    o.push_back((double)E.GetSize());
    for (ULONG i = 0; i < E.GetSize(); ++i)
        o.push_back(C.M.edge_index(E[i]));
    SmTArray<SmFilletVertex*> V;
    if (prefill && C.M.vertex(0) != NULL)
        V.Add(C.M.vertex(0));
    c->GetFilletVertices(V);
    o.push_back((double)V.GetSize());
    for (ULONG i = 0; i < V.GetSize(); ++i)
        o.push_back((double)p3_index_of_vertex(C.M, V[i]));
    C.put_vertex(o, c->GetFilletedVertex());
    o.push_back(c->GetSetBackDist());
    o.push_back((double)c->GetStatus());
    o.push_back((double)c->GetType());
    o.push_back(std::basic_string<TCHAR>(c->GetTypeString()) == std::basic_string<TCHAR>(_T("SmFilletCorner_TYPE")) ? 1.0
                                                                                                                 : 0.0);
    for (size_t i = 0; i < kinds.size(); ++i)
        o.push_back(flg(c->IsKindOf((SM_TYPE)kinds[i])));
    o.push_back(flg(c->IsBevelCorner()));
    o.push_back(flg(c->IsBlendingCorner()));
    o.push_back(flg(c->IsDegenerateCorner()));
    o.push_back(flg(c->IsExtendedSurfacePatch()));
    o.push_back(flg(c->IsCalcAsConstRad()));
    o.push_back(flg(c->IsTangentCorner()));
}

// `ns (edge which approx tang).. q` -> `tangent n solvers..`: the solvers registered for the
// given target edge uses, then GetTangentFilletSolver(solver q; q == ns: a solver that is not
// registered) and GetFilletSolvers, each solver as its registration index (-1 NULL).
void c1_solvers(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    const SmContext& ctx = C.M.ctx;
    C1Solvers S;
    int64_t ns = rd.idx();
    for (int64_t i = 0; i < ns && !rd.bad; ++i)
    {
        int64_t e = rd.idx(), which = rd.idx();
        double approx = rd.num(), tang = rd.num();
        if (e < 0 || (size_t)e >= C.M.targetEdges.size())
        {
            rd.bad = true;
            break;
        }
        SmEdgeuse* eu = C.M.targetEdges[(size_t)e]->GetPrimaryEdgeuse();
        if (eu != NULL && which == 1)
            eu = eu->GetMate();
        if (eu == NULL)
        {
            rd.bad = true;
            break;
        }
        P4Solver* s = new (ctx) P4Solver(ctx, approx, SM_EFF_ZERO_RAD, tang);
        S.v.push_back(s);
        (C.exec->*C1ExecPeek::solvers()).Insert(eu, s);
    }
    int64_t q = rd.idx();
    if (rd.bad || q < 0 || q > ns)
    {
        rd.bad = true;
        return;
    }
    // the 1x1 / 2x2 overrides and GetFilletSolvers dereference m_pSolverEUs
    if (c->*P3CornerPeek::solver() == NULL)
    {
        o.push_back(C1_REFUSE);
        return;
    }
    P4Solver foreign(ctx, 1e-3, SM_EFF_ZERO_RAD, 1e-2);
    const SmFilletSolver* query = q == ns ? (const SmFilletSolver*)&foreign : (const SmFilletSolver*)S.v[(size_t)q];
    // The closed Nx1 and Nx2 overrides (unit blend.fillet_corner_p2) run sm_TestG1Continuity
    // on the filleted edges, which evaluates both edges' curves (dereferenced) once the
    // corner vertex and the edges are set: refuse a curveless edge there. The closed Nx1
    // override tests only when its one filleted edge use's solver is the queried one, the
    // Nx2 override when it has exactly two filleted edge uses.
    if (C.kind == 2 || C.kind == 7)
    {
        SmTArray<SmEdgeuse*>& s = *(c->*P3CornerPeek::solver());
        const SmVertex* v = c->GetFilletedVertex();
        auto curveless = [](SmEdgeuse* eu) {
            return eu->GetEdge() != NULL && eu->GetEdge()->GetCurve() == NULL;
        };
        bool tests = false;
        bool bad = false;
        if (C.kind == 2 && s.GetSize() == 1)
        {
            tests = C.exec->GetFilletSolverOfEdgeuse(s[0]) == query;
            bad = curveless(s[0]);
        }
        else if (C.kind == 7 && s.GetSize() == 2)
        {
            tests = s[0]->GetEdge() != NULL && s[1]->GetEdge() != NULL;
            bad = curveless(s[0]) || curveless(s[1]);
        }
        if (tests && v != NULL && bad)
        {
            o.push_back(C1_REFUSE);
            return;
        }
    }
    SmFilletSolver* t = c->GetTangentFilletSolver(query);
    o.push_back(0.0);
    o.push_back((double)index_of(S.v, (P4Solver*)t));
    SmTArray<SmFilletSolver*> all;
    c->GetFilletSolvers(all);
    o.push_back((double)all.GetSize());
    for (ULONG i = 0; i < all.GetSize(); ++i)
        o.push_back((double)index_of(S.v, (P4Solver*)all[i]));
}

// `na (face s).. nq faces..` -> per query `umax vmax` (-1 -1 NULL): AddExtendedOriginalSurface
// of a plane with natural domain [0, s] x [0, s] per add, then GetExtendedSurface per query.
void c1_ext_surface(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    const SmContext& ctx = C.M.ctx;
    int64_t na = rd.idx();
    for (int64_t i = 0; i < na && !rd.bad; ++i)
    {
        int64_t f = rd.idx();
        double s = rd.num();
        if (f < 0 || (size_t)f >= C.M.targetFaces.size())
        {
            rd.bad = true;
            return;
        }
        SmPlane* pl = new (ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0), SmVector2d(1, 1),
                                        SmExtent2d(0, 0, s, s), &ctx);
        c->AddExtendedOriginalSurface(C.M.targetFaces[(size_t)f], pl);
    }
    int64_t nq = rd.idx();
    std::vector<int64_t> queries;
    for (int64_t i = 0; i < nq && !rd.bad; ++i)
    {
        int64_t f = rd.idx();
        if (f < 0 || (size_t)f >= C.M.targetFaces.size())
        {
            rd.bad = true;
            return;
        }
        queries.push_back(f);
    }
    if (rd.bad)
        return;
    o.push_back(0.0);
    for (size_t i = 0; i < queries.size(); ++i)
    {
        SmSurface* s = c->GetExtendedSurface(C.M.targetFaces[(size_t)queries[i]]);
        if (s == NULL)
        {
            o.push_back(-1.0);
            o.push_back(-1.0);
        }
        else
        {
            SmExtent2d d = s->GetNaturalUVDomain();
            o.push_back(d.GetMax().x);
            o.push_back(d.GetMax().y);
        }
    }
}

// -> the Model after ReInitializeCornerEdges.
void c1_reinit(C1Corner& C, std::vector<double>& o)
{
    C.c->ReInitializeCornerEdges();
    o.push_back(0.0);
    C.M.put(o);
}

// The corner's members as blend_fillet_corner_p3_static.inc's p3_put_corner, without the NULL
// slots SqueezeEdge leaves in m_vVertices / m_vEdges (the Rust port removes the entries at
// once; CalcCornerEdgeGeom compresses them upstream).
void c1_put_corner(std::vector<double>& o, SmFilletCorner* c, Model& M)
{
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    ULONG nv = 0, ne = 0;
    for (ULONG i = 0; i < V.GetSize(); ++i)
        nv += V[i] != NULL ? 1 : 0;
    for (ULONG i = 0; i < E.GetSize(); ++i)
        ne += E[i] != NULL ? 1 : 0;
    o.push_back((double)nv);
    for (ULONG i = 0; i < V.GetSize(); ++i)
        if (V[i] != NULL)
            o.push_back((double)p3_index_of_vertex(M, V[i]));
    o.push_back((double)ne);
    for (ULONG i = 0; i < E.GetSize(); ++i)
        if (E[i] != NULL)
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

// `e` -> `status` corner Model after SqueezeEdge(e).
void c1_squeeze(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    SmFilletEdge* fe = C.M.edge(rd.idx());
    if (fe == NULL || rd.bad)
    {
        rd.bad = true;
        return;
    }
    // upstream's choice of the vertex to delete
    SmFilletVertex* surviving = (SmFilletVertex*)fe->GetVertex();
    SmFilletVertex* del = surviving ? (SmFilletVertex*)fe->GetOtherVertex(surviving) : NULL;
    if (surviving == NULL || del == NULL)
    {
        o.push_back(C1_REFUSE);
        return;
    }
    if (del->GetStatus() != SM_FV_TO_BE_DELETED)
        std::swap(surviving, del);
    bool runs = del->GetStatus() == SM_FV_TO_BE_DELETED;
    if (runs)
    {
        // Upstream's loop over the deleted vertex's vertex uses, in order (UB-212): a
        // rail's vertex use moves; a non-rail edge missing from m_vEdges returns SM_ERR
        // before anything else is freed; a present non-rail edge is deleted with its
        // vertex uses, so a later vertex use of the same (closed) edge is read after the
        // free. After the loop the vertex is deleted when it is in m_vVertices, and a
        // closed squeezed edge then leaves the vertex uses moved to the "surviving"
        // (deleted) vertex dangling. Only the paths that reach a free are refused.
        SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
        SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
        SmTArray<SmVertexuse*> vus;
        del->GetVertexuses(vus);
        std::vector<SmFilletEdge*> edgeOf;
        for (ULONG i = 0; i < vus.GetSize(); ++i)
        {
            SmEdgeuse* eu = vus[i]->GetEdgeuse();
            edgeOf.push_back(eu ? (SmFilletEdge*)eu->GetEdge() : NULL);
        }
        bool deletes = true; // the loop completes and `delete pDeleteFV` is reached
        std::vector<SmFilletEdge*> freed;
        for (size_t i = 0; i < edgeOf.size(); ++i)
        {
            SmFilletEdge* e = edgeOf[i];
            if (e == NULL || index_of(freed, e) >= 0)
            {
                o.push_back(C1_REFUSE); // a null dereference, or a vertex use already freed
                return;
            }
            if (e->GetFilletEdgeType() == SM_FE_RAIL)
                continue;
            ULONG idx = 0;
            if (!E.FindElement(e, idx))
            {
                deletes = false; // SM_ERR
                break;
            }
            freed.push_back(e);
        }
        ULONG vidx = 0;
        if (deletes && !V.FindElement(del, vidx))
            deletes = false; // SM_ERR
        if (deletes && surviving == del)
        {
            o.push_back(C1_REFUSE);
            return;
        }
        if (deletes)
        {
            std::vector<SmFilletVertex*>& mv = C.M.verts;
            for (size_t i = 0; i < mv.size(); ++i)
                if (mv[i] == del)
                {
                    mv.erase(mv.begin() + (std::ptrdiff_t)i);
                    break;
                }
        }
    }
    SmStatus r = c->SqueezeEdge(fe);
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_corner(o, c, C.M);
    C.M.put(o);
}

// `curve` -> `ret`: SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve (its raw int
// return: the flag, or the status `SER` returns through it).
void c1_rail_rail(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = C.M.ctx;
    SmBSplineCurve* crv = read_curve(rd, ctx);
    if (crv == NULL || rd.bad)
    {
        delete crv;
        rd.bad = true;
        return;
    }
    if (C.kind != 10 || C.vertex == NULL)
    {
        delete crv;
        o.push_back(C1_REFUSE); // m_cpVertex->GetPoint() on NULL
        return;
    }
    SmBoolean ret = ((SmFilletConcaveNxNCorner*)C.c)->TestRailRailInterpolationCurve(crv);
    delete crv;
    o.push_back(0.0);
    o.push_back((double)ret);
}

// The corner geometry steps. `CalcCornerVertGeom` / `CalcCornerEdgeGeom` call
// SmFilletVertex::CalcCornerVertGeom / SmFilletEdge::CalcCornerEdgeGeom per vertex / edge
// (blend.fillet_geom_p3). Output `0 status` corner Model.
void c1_vert_geom(C1Corner& C, std::vector<double>& o)
{
    SmStatus r = C.c->CalcCornerVertGeom();
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_corner(o, C.c, C.M);
    C.M.put(o);
}

void c1_edge_geom(C1Corner& C, std::vector<double>& o)
{
    SmStatus r = C.c->CalcCornerEdgeGeom();
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_corner(o, C.c, C.M);
    C.M.put(o);
}

// `nt topo..` -> `ret ntopo topo..` corner Model: the executive's topology edges (model
// edges) before CheckAndFixDegeneracy, the remaining ones after. The FFI validator
// (`validate_corner`) refuses the deletion sets upstream frees twice or dereferences
// after freeing (repeated corner vertices / edges, one-way mate links to an
// earlier-deleted corner vertex) before this runs; the dry run below covers the null
// dereferences only.
void c1_fix_degeneracy(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    Model& M = C.M;
    SmTArray<SmEdge*>& topo = C.exec->*C1ExecPeek::topo();
    int64_t nt = rd.idx();
    for (int64_t i = 0; i < nt && !rd.bad; ++i)
    {
        SmFilletEdge* e = M.edge(rd.idx());
        if (e == NULL)
        {
            rd.bad = true;
            return;
        }
        topo.Add(e);
    }
    if (rd.bad)
        return;
    if (C.vertex == NULL)
    {
        o.push_back(C1_REFUSE); // m_cpVertex->GetPoint() on NULL
        return;
    }
    SmTArray<SmFilletVertex*>& V = c->*P3CornerPeek::verts();
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    // upstream's own early-outs: an unprocessed or far vertex
    bool degenerate = true;
    SmPoint3d cornerPnt = C.vertex->GetPoint();
    for (ULONG i = 0; i < V.GetSize() && degenerate; ++i)
    {
        if (!V[i]->IsProcessed() || V[i]->GetPoint().DistanceBetween(cornerPnt) > c->*P3CornerPeek::tol())
            degenerate = false;
    }
    if (degenerate)
    {
        if (V.GetSize() == 0)
        {
            o.push_back(C1_REFUSE); // m_vVertices[0] of an empty list
            return;
        }
        // The vertex uses the merged first vertex ends up with: those of every corner
        // vertex except the ones of the (deleted) corner edges, the first vertex's own
        // first. For each, in order, upstream returns a status for a rail without original
        // face or a rail edge use without fillet geometry, dereferences the geometry's
        // (NULL) solver, and then the original vertex uses' (NULL) face uses.
        std::vector<SmFilletVertexuse*> fil;
        for (ULONG i = 0; i < V.GetSize(); ++i)
        {
            SmTArray<SmVertexuse*> vus;
            V[i]->GetVertexuses(vus);
            for (ULONG j = 0; j < vus.GetSize(); ++j)
            {
                SmEdgeuse* eu = vus[j]->GetEdgeuse();
                SmFilletEdge* e = eu ? (SmFilletEdge*)eu->GetEdge() : NULL;
                ULONG idx = 0;
                if (e == NULL)
                {
                    o.push_back(C1_REFUSE);
                    return;
                }
                if (!E.FindElement(e, idx))
                    fil.push_back((SmFilletVertexuse*)vus[j]);
            }
        }
        for (size_t i = 0; i < fil.size(); ++i)
        {
            SmFilletEdgeuse* eu = (SmFilletEdgeuse*)fil[i]->GetEdgeuse();
            SmFilletEdge* rail = (SmFilletEdge*)eu->GetEdge();
            if (rail->GetOriginalFace() == NULL)
                break; // NER: a status
            if (eu->GetFilletGeom() == NULL)
                break; // NER: a status
            // pFG->GetFilletSolver() is NULL on the Model: FindIndexOfRailOnFace on NULL
            o.push_back(C1_REFUSE);
            return;
        }
        // the merged vertices are deleted
        std::vector<SmFilletVertex*>& mv = M.verts;
        for (ULONG i = 1; i < V.GetSize(); ++i)
            for (size_t k = 0; k < mv.size(); ++k)
                if (mv[k] == V[i])
                {
                    mv.erase(mv.begin() + (std::ptrdiff_t)k);
                    break;
                }
    }
    SmBoolean ret = c->CheckAndFixDegeneracy();
    o.push_back(0.0);
    o.push_back((double)ret);
    o.push_back((double)topo.GetSize());
    for (ULONG i = 0; i < topo.GetSize(); ++i)
        o.push_back(M.edge_index(topo[i]));
    topo.ReSet(); // the Model owns the edges
    c1_put_corner(o, c, M);
    M.put(o);
}

// m_pFilletBrep of the corner ops that build faces: a new SmBrep (as
// SmFilletExecutive::CreateFilletBrep makes it). Left alive (leaked): its faces own surfaces
// the corner's m_vSurfaces still points to.
SmBrep* c1_new_fillet_brep(C1Corner& C)
{
    SmBrep* fb = new (C.M.ctx) SmBrep();
    C.exec->*C1ExecPeek::fillet() = fb;
    return fb;
}

// The record of the face-building ops: `0 status` corner Model fillet-B-rep error-info.
void c1_put_face_op(C1Corner& C, SmStatus r, SmBrep* fb, std::vector<double>& o)
{
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_corner(o, C.c, C.M);
    C.M.put(o);
    p3_put_fillet_brep(o, fb);
    p3_put_error_info(o, C.exec->GetFilletErrorInfo(), C.vertex);
    C.exec->*C1ExecPeek::fillet() = NULL;
}

// `ns surface..` (B-spline surfaces): added to m_vSurfaces.
bool c1_read_surfaces(C1Corner& C, Rd& rd)
{
    int64_t ns = rd.idx();
    if (rd.bad || ns < 0 || ns > 16)
    {
        rd.bad = true;
        return false;
    }
    for (int64_t i = 0; i < ns && !rd.bad; ++i)
        (C.c->*P3CornerPeek::surfs()).Add(read_surface(rd, C.M.ctx));
    return !rd.bad;
}

// -> `status` corner Model fillet-B-rep: CreateEndPatch (refused where it or its MakeFaceBrep
// dereferences NULL, p3_check_end_patch).
void c1_end_patch(C1Corner& C, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    if (p3_check_end_patch(c, p3_edges(c)) == P3C_REFUSE)
    {
        o.push_back(C1_REFUSE);
        return;
    }
    SmBrep* fb = c1_new_fillet_brep(C);
    SmStatus r = c->CreateEndPatch();
    c1_put_face_op(C, r, fb, o);
}

// `ns surface..` -> `status` corner Model fillet-B-rep: MakeFaceBrep on those m_vSurfaces
// (refused where it dereferences NULL, p3_check_make_face).
void c1_make_face(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    if (!c1_read_surfaces(C, rd))
        return;
    if (p3_check_make_face(c, p3_edges(c), P3Override(), 0) != 0.0)
    {
        o.push_back(C1_REFUSE);
        return;
    }
    SmBrep* fb = c1_new_fillet_brep(C);
    SmStatus r = c->MakeFaceBrep();
    c1_put_face_op(C, r, fb, o);
}

// m_pIntersectionTable of an SmTopologyIntersector (a private member, reached through an
// explicit instantiation, which ignores access).
struct C1TableTag
{
    typedef SmIntersectionTable* SmTopologyIntersector::*type;
    friend type c1_table(C1TableTag);
};
template <typename Tag, typename Tag::type M> struct C1Rob
{
    friend typename Tag::type c1_table(Tag) { return M; }
};
template struct C1Rob<C1TableTag, &SmTopologyIntersector::m_pIntersectionTable>;

// The executive topology intersector's intersection table: `n`, then per entry the brep
// surface (index of the face in `vf` whose surface it is, -1 for NULL, -2 otherwise), the
// other surface (index in the corner's m_vSurfaces, -1 for NULL, -2 otherwise) and whether
// the entry's results have curve arrays.
void c1_put_ssi_table(std::vector<double>& o, SmTopologyIntersector& ti, SmTArray<SmFace*>& vf,
                      SmFilletCorner* c)
{
    SmIntersectionTable* t = ti.*c1_table(C1TableTag());
    if (t == NULL)
    {
        o.push_back(0.0);
        return;
    }
    ULONG n = t->m_vBrepSurfaces.GetSize();
    o.push_back((double)n);
    const SmTArray<SmSurface*>& cs = c->*P3CornerPeek::surfs();
    for (ULONG ii = 0; ii < n; ++ii)
    {
        SmSurface* b = t->m_vBrepSurfaces[ii];
        double bi = b == NULL ? -1.0 : -2.0;
        for (ULONG f = 0; b != NULL && f < vf.GetSize(); ++f)
            if (vf[f]->GetSurface() == b)
            {
                bi = (double)f;
                break;
            }
        o.push_back(bi);
        SmSurface* s = ii < t->m_vOtherSurfaces.GetSize() ? t->m_vOtherSurfaces[ii] : NULL;
        double si = s == NULL ? -1.0 : -2.0;
        for (ULONG jj = 0; s != NULL && jj < cs.GetSize(); ++jj)
            if (cs[jj] == s)
            {
                si = (double)jj;
                break;
            }
        o.push_back(si);
        o.push_back(ii < t->m_vResults.GetSize() && t->m_vResults[ii].m_p3DCurves != NULL ? 1.0
                                                                                         : 0.0);
    }
}

// `ns surface..` -> `status` table corner Model: InsertIntersectionTopology on those
// m_vSurfaces, with the Model's fillet B-rep as m_pFilletBrep, on the executive's
// constructor-made topology intersector; `table` is its intersection table after the call
// (c1_put_ssi_table). The executive's constructor makes its edge attribute (m_pAttributeE)
// with no users and CreateFilletBrep is not run, so every fillet-B-rep edge is skipped
// before the merge (MergeCurveOnSurfaces, UpdateTopologyChanges); a blending corner
// registers each face at its vertex with each corner surface (RegisterExistingSSI). A
// blending corner without vertex is refused (m_cpVertex->GetFaces).
void c1_insert(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    if (!c1_read_surfaces(C, rd))
        return;
    SmTArray<SmFace*> vf;
    if (C.vertex != NULL)
        C.vertex->GetFaces(vf);
    else if (c->*P3CornerPeek::blending())
    {
        o.push_back(C1_REFUSE);
        return;
    }
    C.exec->*C1ExecPeek::fillet() = C.M.filletBrep;
    SmStatus r = c->InsertIntersectionTopology();
    C.exec->*C1ExecPeek::fillet() = NULL;
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_ssi_table(o, C.exec->GetTopologyIntersector(), vf, c);
    c1_put_corner(o, c, C.M);
    C.M.put(o);
}

// The surface generators of the C1_CONST_RAD solvers (owned here).
struct C1Fsgs
{
    std::vector<SmFilletSurfaceGenerator*> v;
    ~C1Fsgs()
    {
        for (size_t i = 0; i < v.size(); ++i)
            delete v[i];
    }
};

// `ng (kind scale continuity)..` -> `status` [surface] corner Model: MakeConstantRadiusFillet
// with a solver per Model geometry (`kind` -1: none, 0 circular (defaults), 1 blend curve
// (scale, continuity), 2 linear, 3 the base generator, 4 no generator). Refused where it
// dereferences NULL (p3_const_rad's refusals other than the solver's) or reaches
// SmFilletSolver::GetFilletExecutive-dependent code with an unknown generator (pNewFS NULL).
void c1_const_rad(C1Corner& C, Rd& rd, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    Model& M = C.M;
    const SmContext& ctx = M.ctx;
    int64_t ng = rd.idx();
    if (rd.bad || ng != (int64_t)M.geoms.size())
    {
        rd.bad = true;
        return;
    }
    C1Solvers S;
    C1Fsgs F;
    std::vector<int64_t> kinds;
    for (int64_t i = 0; i < ng && !rd.bad; ++i)
    {
        int64_t kind = rd.idx();
        double scale = rd.num();
        int64_t cont = rd.idx();
        if (kind < -1 || kind > 4 || cont < 0 || cont > 2)
        {
            rd.bad = true;
            return;
        }
        kinds.push_back(kind);
        if (kind < 0)
        {
            M.geoms[(size_t)i]->*P4GeomPeek::solver() = NULL;
            continue;
        }
        P4Solver* s = new (ctx) P4Solver(ctx, 1e-3, SM_EFF_ZERO_RAD, 1e-2);
        S.v.push_back(s);
        SmFilletSurfaceGenerator* g = NULL;
        switch (kind)
        {
        case 0: g = new (ctx) SmCircularCrossSectionFSG(); break;
        case 1: g = new (ctx) SmBlendCurveCrossSectionFSG(scale, (ULONG)cont); break;
        case 2: g = new (ctx) SmLinearCrossSectionFSG(); break;
        case 3: g = new (ctx) SmFilletSurfaceGenerator(); break;
        default: break;
        }
        if (g != NULL)
            F.v.push_back(g);
        s->SetFilletSurfaceGenerator(g);
        M.geoms[(size_t)i]->*P4GeomPeek::solver() = s;
    }
    if (rd.bad)
        return;
    // upstream's own code up to the solvers' generators (p3_const_rad, without its final
    // refusal), then the generators: none (refused), types or scales that differ, linear
    // (statuses), the base generator (no solver is made: refused)
    std::vector<SmFilletEdge*> E = p3_edges(c);
    P3Analytic pre = p3_const_rad(c, E, M, P3Override());
    bool run = true;
    if (pre == P3A_REFUSE)
    {
        // p3_const_rad refuses at the solvers' generators only after both projections
        // succeed: then look at them
        run = false;
        ULONG n = (ULONG)E.size();
        bool solverStage = n == 3;
        ULONG found = 9999;
        double minDist = SM_BIG_DOUBLE;
        for (ULONG ii = 0; ii < n && solverStage; ++ii)
        {
            SmVertex* v = E[ii]->GetVertex();
            SmVertex* w = v != NULL ? E[ii]->GetOtherVertex(v) : NULL;
            if (v == NULL || w == NULL)
                solverStage = false;
            else if (v->GetPoint().DistanceBetween(w->GetPoint()) < minDist)
            {
                minDist = v->GetPoint().DistanceBetween(w->GetPoint());
                found = ii;
            }
        }
        if (solverStage && found < n && SM_CAST_PTR(SmBSplineCurve, E[found]->GetCurve()) != NULL &&
            E[(found + n - 1) % n]->GetPrimaryEdgeuse() != NULL && E[(found + 1) % n]->GetPrimaryEdgeuse() != NULL)
        {
            SmFilletGeom* g0 = ((SmFilletEdgeuse*)E[(found + n - 1) % n]->GetPrimaryEdgeuse())->GetFilletGeom();
            SmFilletGeom* g1 = ((SmFilletEdgeuse*)E[(found + 1) % n]->GetPrimaryEdgeuse())->GetFilletGeom();
            SmFilletSolver* s0 = g0 ? g0->GetFilletSolver() : NULL;
            SmFilletSolver* s1 = g1 ? g1->GetFilletSolver() : NULL;
            SmFilletSurfaceGenerator* f0 = s0 ? s0->GetFilletSurfaceGenerator() : NULL;
            SmFilletSurfaceGenerator* f1 = s1 ? s1->GetFilletSurfaceGenerator() : NULL;
            if (f0 != NULL && f1 != NULL)
            {
                SmFilletSurfaceGeneratorType t = f0->GetFilletSurfaceGeneratorType();
                // an unknown type (equal on both) makes no solver: pNewFS->SetFilletExecutive
                run = !(t == SM_FSG_UNKNOWN && f1->GetFilletSurfaceGeneratorType() == SM_FSG_UNKNOWN);
            }
        }
    }
    if (!run)
    {
        o.push_back(C1_REFUSE);
        return;
    }
    SM_FILLETSURF_TYPE* surface = NULL;
    SmStatus r = c->MakeConstantRadiusFillet(surface);
    o.push_back(0.0);
    o.push_back(st(r));
    o.push_back(flg(r == SM_SUCCESS && surface != NULL));
    if (r == SM_SUCCESS && surface != NULL)
        p3_put_surface(o, surface);
    c1_put_corner(o, c, M);
    M.put(o);
    for (size_t i = 0; i < M.geoms.size(); ++i)
        M.geoms[i]->*P4GeomPeek::solver() = NULL;
}

int32_t c1_run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Model M(ctx);
    if (!M.build(rd))
        return SC_ERR_ARG;
    C1Corner C(M, rd);
    if (!C.ok)
        return SC_ERR_ARG;
    switch (op)
    {
    case C1_PEEK: c1_peek(C, rd, o); break;
    case C1_SOLVERS: c1_solvers(C, rd, o); break;
    case C1_EXT_SURFACE: c1_ext_surface(C, rd, o); break;
    case C1_REINIT: c1_reinit(C, o); break;
    case C1_SQUEEZE: c1_squeeze(C, rd, o); break;
    case C1_RAIL_RAIL: c1_rail_rail(C, rd, o); break;
    case C1_VERT_GEOM: c1_vert_geom(C, o); break;
    case C1_EDGE_GEOM: c1_edge_geom(C, o); break;
    case C1_FIX_DEGENERACY: c1_fix_degeneracy(C, rd, o); break;
    case C1_END_PATCH: c1_end_patch(C, o); break;
    case C1_MAKE_FACE: c1_make_face(C, rd, o); break;
    case C1_INSERT: c1_insert(C, rd, o); break;
    case C1_CONST_RAD: c1_const_rad(C, rd, o); break;
    default: return SC_ERR_ARG;
    }
    return rd.bad ? SC_ERR_ARG : SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_corner_p1_corner(int32_t op, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    if (op < C1_PEEK || op > C1_CONST_RAD)
        return SC_ERR_ARG;
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    std::vector<double> o;
    int32_t rc = c1_run(op, rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
