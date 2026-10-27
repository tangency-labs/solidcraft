// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p8`: the SmBrep face builders owned by the unit
// (source/SMLib/src/SmBrep.cpp): CreateFaceInRegionFromSurface, CreateFaceFromSurface,
// CreateFacesFromSurface, CreatePlanarFaceWith3DCurves, CreatePlanarFacesWith3DCurves,
// CreateFacesWithCurves and MakeFacesWithCurves, each on a new SmBrep (`new SmBrep()`,
// its void infinite region).
//
// One entry point, sc_topology_brep_p8_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/topology_brep_p8.rs, which validates every
// input. Surface blocks are `-1` (null) or sc_topology_fixture.h's read_surface block
// (SmPlane, SmBSplineSurface); curve blocks are `0` + read_curve's block (an
// SmBSplineCurve), `1 P0[3] P1[3]` (an SmLine(P0, P1, 3)) or `2` + read_curve's block
// (the SmBSplineCurve with the given knots written back over its own NURB).
//
// Output: `status`, after a success the op's results (objects by their index in a walk of
// the B-rep after the op, -1 null), then the B-rep after the op, also after a failure: the topology graph
// of the topology.brep_p3 shim (`put_counts`) and the geometry of every vertex, edge, face
// and edge use in walk order (`put_geometry`).
#include "sc_topology_fixture.h"

#include <SmLine.h>

namespace
{
enum : int32_t
{
    OP_IN_REGION = 1,
    OP_FROM_SURFACE = 2,
    OP_FACES_FROM_SURFACE = 3,
    OP_PLANAR_FACE = 4,
    OP_PLANAR_FACES = 5,
    OP_FACES_WITH_CURVES = 6,
    OP_MAKE_FACES = 7,
};

struct P8RegPeek : SmRegion
{
    static SmBoolean SmRegion::*void_flag() { return &P8RegPeek::m_bIsVoidFlag; }
};

struct P8ShellPeek : SmShell
{
    static SM_TYPE SmShell::*shell_type() { return &P8ShellPeek::m_tShellType; }
};

SmSurface* p8_surface(Rd& rd, const SmContext& ctx)
{
    if ((int64_t)rd.in[rd.at] == -1)
    {
        rd.at++;
        return nullptr;
    }
    return read_surface(rd, ctx);
}

SmCurve* p8_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        return new (ctx) SmLine(a, b, 3, &ctx);
    }
    if (kind == 2)
    {
        // read_curve's block, then the given knots written back over the fixed-up NURB
        int64_t start = rd.at;
        SmBSplineCurve* c = read_curve(rd, ctx);
        gw_INDEX m = (gw_INDEX)rd.in[start + 3];
        gw_CURVE* own = c->GetGwNurbPointer();
        for (gw_INDEX i = 0; i <= m; i++)
            own->knt->U[i] = rd.in[rd.at - (int64_t)(m + 1) + (int64_t)i];
        return c;
    }
    return read_curve(rd, ctx);
}

// The topology graph after an op (the topology.brep_p3 shim's put_graph): a fresh walk
// names every object by its index in its kind's list, then every owner / adjacency link is
// written as such an index (-1 null, -3 an object the walk did not reach); layout in the
// FFI wrapper.
void p8_put_graph(std::vector<double>& o, SmBrep* brep, Graph& g)
{
    auto I = [&](int k, auto p) { return (double)g.find(k, p); };
    auto T = [&](SM_TYPE t, const SmTopology* p) -> double {
        switch (t)
        {
        case SmShell_TYPE: return (double)g.find(1, (const SmShell*)p);
        case SmFaceuse_TYPE: return (double)g.find(2, (const SmFaceuse*)p);
        case SmLoopuse_TYPE: return (double)g.find(4, (const SmLoopuse*)p);
        case SmEdgeuse_TYPE: return (double)g.find(6, (const SmEdgeuse*)p);
        case SmVertexuse_TYPE: return (double)g.find(8, (const SmVertexuse*)p);
        default: return p == nullptr ? -1.0 : -4.0;
        }
    };
    for (size_t n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                     g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.push_back((double)n);
    for (SmRegion* r : g.regions)
    {
        std::vector<SmTopology*> ss = list_of(r->GetList());
        o.push_back((double)(r->*P8RegPeek::void_flag()));
        o.push_back(r == brep->GetInfiniteRegion() ? 1.0 : 0.0);
        o.push_back((double)ss.size());
        for (SmTopology* t : ss)
            o.push_back(I(1, (SmShell*)t));
    }
    for (SmShell* sh : g.shells)
    {
        SM_TYPE ty = sh->*P8ShellPeek::shell_type();
        o.push_back((double)ty);
        o.push_back((double)sh->GetSize());
        o.push_back(I(0, (SmRegion*)sh->GetOwner()));
        o.push_back(T(ty, sh->GetList()));
    }
    for (SmFaceuse* fu : g.faceuses)
    {
        std::vector<SmTopology*> lus = list_of(fu->GetList());
        o.push_back(I(3, fu->*FuPeek::face()));
        o.push_back(I(2, fu->*FuPeek::mate()));
        o.push_back(I(1, (SmShell*)fu->GetOwner()));
        o.push_back(I(2, (SmFaceuse*)fu->GetNext()));
        o.push_back(I(2, (SmFaceuse*)fu->GetLast()));
        o.push_back((double)fu->GetOrientation());
        o.push_back((double)lus.size());
        for (SmTopology* t : lus)
            o.push_back(I(4, (SmLoopuse*)t));
    }
    for (SmFace* f : g.faces)
        o.push_back(I(2, f->*FPeek::fu()));
    for (SmLoopuse* lu : g.loopuses)
    {
        o.push_back(I(5, lu->*LuPeek::loop()));
        o.push_back(I(4, lu->*LuPeek::mate()));
        o.push_back(I(2, (SmFaceuse*)lu->GetOwner()));
        o.push_back((double)(lu->*LuPeek::type()));
        o.push_back(T(lu->*LuPeek::type(), lu->*LuPeek::start()));
        o.push_back((double)(lu->*LuPeek::orient()));
    }
    for (SmLoop* l : g.loops)
        o.push_back(I(4, l->GetLoopuse()));
    for (SmEdgeuse* eu : g.edgeuses)
    {
        o.push_back(I(7, (SmEdge*)eu->GetOwner()));
        o.push_back((double)eu->GetEdgeuseType());
        o.push_back(T(eu->GetEdgeuseType(), eu->*EuPeek::sorlu()));
        o.push_back(I(6, eu->GetCWEdgeuse()));
        o.push_back(I(6, eu->GetCCWEdgeuse()));
        o.push_back(I(8, eu->*EuPeek::vu()));
        o.push_back(I(6, (SmEdgeuse*)eu->GetNext()));
        o.push_back(I(6, (SmEdgeuse*)eu->GetLast()));
        o.push_back((double)eu->GetOrientation());
    }
    for (SmEdge* e : g.edges)
    {
        std::vector<SmTopology*> eus = list_of(e->GetList());
        o.push_back((double)eus.size());
        for (SmTopology* t : eus)
            o.push_back(I(6, (SmEdgeuse*)t));
    }
    for (SmVertexuse* vu : g.vertexuses)
    {
        o.push_back(I(9, (SmVertex*)vu->GetOwner()));
        o.push_back((double)vu->GetVertexuseType());
        o.push_back(T(vu->GetVertexuseType(), vu->*VuPeek::owner()));
    }
    for (SmVertex* v : g.vertices)
    {
        std::vector<SmTopology*> vus = list_of(v->GetList());
        o.push_back((double)vus.size());
        for (SmTopology* t : vus)
            o.push_back(I(8, (SmVertexuse*)t));
    }
}

// A curve: `-1` none, else `type t0 t1` and 5 samples `ok x y z` at t0 + i/4 (t1 - t0)
// (`ok` 0 when EvaluatePoint fails, then 0 0 0).
void p8_put_curve(std::vector<double>& o, const SmCurve* c)
{
    if (c == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    SmExtent1d d = c->GetNaturalInterval();
    o.push_back((double)c->GetType());
    o.push_back(d.GetMin());
    o.push_back(d.GetMax());
    for (int i = 0; i <= 4; i++)
    {
        double t = d.GetMin() + (d.GetMax() - d.GetMin()) * ((double)i / 4.0);
        SmPoint3d p(0.0, 0.0, 0.0);
        bool ok = c->EvaluatePoint(t, p) == SM_SUCCESS;
        o.push_back(ok ? 1.0 : 0.0);
        put_vec(o, ok ? p : SmPoint3d(0.0, 0.0, 0.0));
    }
}

// A surface: `-1` none, else `type natural(u0 v0 u1 v1)` and 3x3 samples `ok x y z` at
// fractions 0, 1/2, 1 of `dom` (u outer).
void p8_put_surface(std::vector<double>& o, const SmSurface* s, const SmExtent2d& dom)
{
    if (s == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    SmExtent2d n = s->GetNaturalUVDomain();
    o.push_back((double)s->GetType());
    o.push_back(n.GetMin().x);
    o.push_back(n.GetMin().y);
    o.push_back(n.GetMax().x);
    o.push_back(n.GetMax().y);
    for (int i = 0; i <= 2; i++)
        for (int j = 0; j <= 2; j++)
        {
            SmPoint2d uv(dom.GetMin().x + (dom.GetMax().x - dom.GetMin().x) * ((double)i / 2.0),
                         dom.GetMin().y + (dom.GetMax().y - dom.GetMin().y) * ((double)j / 2.0));
            SmPoint3d p(0.0, 0.0, 0.0);
            bool ok = s->EvaluatePoint(uv, p) == SM_SUCCESS;
            o.push_back(ok ? 1.0 : 0.0);
            put_vec(o, ok ? p : SmPoint3d(0.0, 0.0, 0.0));
        }
}

// Per vertex `x y z tol`, per edge `tol t0 t1 curve`, per face `tol u0 v0 u1 v1 rect
// surface`, per edge use its own UV trim curve (m_pUVTrimCurve), in walk order.
void p8_put_geometry(std::vector<double>& o, Graph& g)
{
    for (SmVertex* v : g.vertices)
    {
        put_vec(o, v->GetPoint());
        o.push_back((double)v->GetTolerance());
    }
    for (SmEdge* e : g.edges)
    {
        SmExtent1d d = e->GetInterval();
        o.push_back((double)e->GetTolerance());
        o.push_back(d.GetMin());
        o.push_back(d.GetMax());
        p8_put_curve(o, e->GetCurve());
    }
    for (SmFace* f : g.faces)
    {
        SmExtent2d d = f->*FPeek::dom();
        o.push_back((double)(f->*FPeek::tol()));
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)(f->*FPeek::rect()));
        p8_put_surface(o, f->*FPeek::srf(), d);
    }
    for (SmEdgeuse* eu : g.edgeuses)
        p8_put_curve(o, eu->*EuPeek::uv());
}

// `faces edges vertices regions`, then per region `void infinite nshells` and per shell
// `type size has_head` (the topology.brep_p3 shim's put_counts), the graph and geometry.
void p8_put_brep(std::vector<double>& o, SmBrep* brep, Graph& g)
{
    SmTArray<SmFace*> fs;
    SmTArray<SmEdge*> es;
    SmTArray<SmVertex*> vs;
    SmTArray<SmRegion*> rs;
    brep->GetFaces(fs);
    brep->GetEdges(es);
    brep->GetVertices(vs);
    brep->GetRegions(rs);
    o.push_back((double)fs.GetSize());
    o.push_back((double)es.GetSize());
    o.push_back((double)vs.GetSize());
    o.push_back((double)rs.GetSize());
    for (ULONG i = 0; i < rs.GetSize(); i++)
    {
        SmRegion* r = rs[i];
        SmTArray<SmShell*> ss;
        r->GetShells(ss);
        o.push_back((double)(r->*P8RegPeek::void_flag()));
        o.push_back(r == brep->GetInfiniteRegion() ? 1.0 : 0.0);
        o.push_back((double)ss.GetSize());
        for (ULONG j = 0; j < ss.GetSize(); j++)
        {
            SmShell* sh = ss[j];
            o.push_back((double)(sh->*P8ShellPeek::shell_type()));
            o.push_back((double)sh->GetSize());
            o.push_back(sh->GetList() != NULL ? 1.0 : 0.0);
        }
    }
    p8_put_graph(o, brep, g);
    p8_put_geometry(o, g);
}

void p8_put_faces(std::vector<double>& o, Graph& g, SmTArray<SmFace*>& faces)
{
    o.push_back((double)faces.GetSize());
    for (ULONG i = 0; i < faces.GetSize(); i++)
        o.push_back((double)g.find(3, faces[i]));
}

void p8_run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Built built;
    built.brep = new (ctx) SmBrep();
    SmBrep* b = built.brep;
    SmStatus s = SM_SUCCESS;
    // (results, written after the walk)
    SmShell* shell = NULL;
    SmRegion* region = NULL;
    SmFace* face = NULL;
    SmTArray<SmFace*> faces;
    switch (op)
    {
    case OP_IN_REGION:
    case OP_FROM_SURFACE:
    {
        SmSurface* srf = p8_surface(rd, ctx);
        bool region_null = op == OP_IN_REGION ? rd.flag() : false;
        SmPoint2d a = rd.uv(), c = rd.uv();
        if (op == OP_IN_REGION)
            s = b->CreateFaceInRegionFromSurface(region_null ? NULL : b->GetInfiniteRegion(), srf,
                                                 SmExtent2d(a, c), shell, face);
        else
            s = b->CreateFaceFromSurface(srf, SmExtent2d(a, c), face);
        break;
    }
    case OP_FACES_FROM_SURFACE:
    {
        SmSurface* srf = p8_surface(rd, ctx);
        SmContinuityType cont = (SmContinuityType)rd.idx();
        SmExtent2d dom = srf ? srf->GetNaturalUVDomain() : SmExtent2d(SmPoint2d(0, 0), SmPoint2d(1, 1));
        s = b->CreateFacesFromSurface(srf, dom, cont, faces);
        break;
    }
    case OP_PLANAR_FACE:
    case OP_PLANAR_FACES:
    {
        bool region_null = rd.flag();
        double tol = rd.num();
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        for (int64_t i = 0; i < n; i++)
            curves.Add(p8_curve(rd, ctx));
        SmRegion* r = region_null ? NULL : b->GetInfiniteRegion();
        if (op == OP_PLANAR_FACE)
            s = b->CreatePlanarFaceWith3DCurves(r, curves, tol, face);
        else
            s = b->CreatePlanarFacesWith3DCurves(r, curves, tol, faces);
        break;
    }
    case OP_FACES_WITH_CURVES:
    {
        SmSurface* srf = p8_surface(rd, ctx);
        bool region_null = rd.flag();
        int64_t nl = rd.idx();
        SmTArray<ULONG> loops;
        for (int64_t i = 0; i < nl; i++)
            loops.Add((ULONG)rd.idx());
        bool has3d = rd.flag();
        int64_t nc = rd.idx();
        SmTArray<SmCurve*> curves;
        SmTArray<SmOrientType> orients;
        for (int64_t i = 0; i < nc; i++)
        {
            curves.Add(p8_curve(rd, ctx));
            orients.Add((SmOrientType)rd.idx());
        }
        s = b->CreateFacesWithCurves(region_null ? NULL : b->GetInfiniteRegion(), loops, has3d ? &curves : NULL,
                                     orients, srf, shell, faces);
        break;
    }
    case OP_MAKE_FACES:
    {
        SmSurface* srf = p8_surface(rd, ctx);
        SmPoint2d a = rd.uv(), c = rd.uv();
        SmOrientType orient = (SmOrientType)rd.idx();
        bool region_null = rd.flag();
        int64_t nl = rd.idx();
        SmTArray<ULONG> loops;
        for (int64_t i = 0; i < nl; i++)
            loops.Add((ULONG)rd.idx());
        bool has3d = rd.flag();
        bool hasuv = rd.flag();
        int64_t nc = rd.idx();
        SmTArray<SmCurve*> c3;
        SmTArray<SmBSplineCurve*> cuv;
        SmTArray<SmOrientType> orients;
        for (int64_t i = 0; i < nc; i++)
        {
            if (has3d)
                c3.Add(p8_curve(rd, ctx));
            if (hasuv)
                cuv.Add(read_curve(rd, ctx));
            orients.Add((SmOrientType)rd.idx());
        }
        int64_t np = rd.idx();
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        s = b->MakeFacesWithCurves(region_null ? NULL : b->GetInfiniteRegion(), loops, has3d ? &c3 : NULL,
                                   hasuv ? &cuv : NULL, orients, pts, srf, SmExtent2d(a, c), orient, region,
                                   shell, faces);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); return;
    }
    Graph g;
    walk(b, g);
    o.push_back(st(s));
    // (the out-parameters only after a success: a failure leaves them as the op stopped)
    if (s == SM_SUCCESS)
        switch (op)
        {
    case OP_IN_REGION:
        o.push_back((double)g.find(1, shell));
        o.push_back((double)g.find(3, face));
        break;
    case OP_FROM_SURFACE:
    case OP_PLANAR_FACE: o.push_back((double)g.find(3, face)); break;
    case OP_FACES_WITH_CURVES:
        o.push_back((double)g.find(1, shell));
        p8_put_faces(o, g, faces);
        break;
    case OP_MAKE_FACES:
        o.push_back((double)g.find(0, region));
        o.push_back((double)g.find(1, shell));
        p8_put_faces(o, g, faces);
        break;
    default: p8_put_faces(o, g, faces); break;
        }
    p8_put_brep(o, b, g);
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_p8_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                           int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    p8_run(op, rd, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
