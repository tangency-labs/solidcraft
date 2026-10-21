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
    // the closed Nx1 and Nx2 overrides belong to blend.fillet_corner_p2 (Rust stubs)
    if (C.kind == 2 || C.kind == 7)
    {
        o.push_back(C1_NOT_RUN);
        return;
    }
    P4Solver foreign(ctx, 1e-3, SM_EFF_ZERO_RAD, 1e-2);
    const SmFilletSolver* query = q == ns ? (const SmFilletSolver*)&foreign : (const SmFilletSolver*)S.v[(size_t)q];
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
// SmFilletVertex::CalcCornerVertGeom / SmFilletEdge::CalcCornerEdgeGeom per vertex / edge,
// whose Rust ports are stubs (blend.fillet_geom_p3): with any vertex / edge the case is not
// run. Output `0 status` corner Model.
void c1_vert_geom(C1Corner& C, std::vector<double>& o)
{
    if ((C.c->*P3CornerPeek::verts()).GetSize() > 0)
    {
        o.push_back(C1_NOT_RUN);
        return;
    }
    SmStatus r = C.c->CalcCornerVertGeom();
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_corner(o, C.c, C.M);
    C.M.put(o);
}

void c1_edge_geom(C1Corner& C, std::vector<double>& o)
{
    if ((C.c->*P3CornerPeek::edges()).GetSize() > 0)
    {
        o.push_back(C1_NOT_RUN);
        return;
    }
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

// -> `status` corner Model: CreateEndPatch; its MakeFaceBrep is a Rust stub, so a corner
// whose first fillet-side-face edge has an original face is not run.
void c1_end_patch(C1Corner& C, std::vector<double>& o)
{
    SmFilletCorner* c = C.c;
    SmTArray<SmFilletEdge*>& E = c->*P3CornerPeek::edges();
    for (ULONG i = 0; i < E.GetSize(); ++i)
    {
        if (E[i]->GetFilletEdgeType() != SM_FE_FILLET_X_SIDE_FACE)
            continue;
        SmFace* f = E[i]->GetOriginalFace();
        if (f == NULL)
            break; // NER: a status
        SmSurface* s = c->GetExtendedSurface(f);
        if (s == NULL)
            s = f->GetSurface();
        if (s == NULL)
        {
            o.push_back(C1_REFUSE); // CopyAndAddAnalytics on NULL
            return;
        }
        o.push_back(C1_NOT_RUN); // MakeFaceBrep
        return;
    }
    SmStatus r = c->CreateEndPatch();
    o.push_back(0.0);
    o.push_back(st(r));
    c1_put_corner(o, c, C.M);
    C.M.put(o);
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
    default: return SC_ERR_ARG;
    }
    return rd.bad ? SC_ERR_ARG : SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_corner_p1_corner(int32_t op, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    if (op < C1_PEEK || op > C1_END_PATCH)
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
