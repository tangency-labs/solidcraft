// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p2`: the SmBrep members of the unit
// (source/SMLib/src/SmBrep.cpp) on real SmBrep objects built by the shared topology
// fixture (sc_topology_fixture.h: an upstream .smb file, a wire edge, a one-face B-rep
// or the my_test_topology wire network).
//
// One entry point, sc_topology_brep_p2_eval(op, path, in, n_in, out, cap, out_len).
// `in` is the fixture's B-rep block, then `editing` (-1 keep, 0/1 set
// m_bEditingEnabled), then the op's arguments; layouts are documented in
// crates/solidcore-ffi/src/units/topology_brep_p2.rs, which validates every input.
// Outputs start with `0` (built) or `-1 status` (the build failed). Entities are named
// by their index in the B-rep's face/edge/vertex lists, then in the fixture's walk (with
// the vertex lists) of the B-rep before the op. Ops that may change the topology (Make,
// Merge, Mate, Replace, RayFire, Notify) output only statuses and counts after the call
// (Make: its topology summary, read through fresh list queries)
// (Stale: whether each face's seeded trimmed-surface cache survived);
// SetTol / Rebuild only read the entities they cannot delete (tolerances, UV curve
// presence), so no pointer captured before a mutation is followed after it.
#include "sc_topology_fixture.h"

#include <SmCacheMgr.h>
#include <SmPointClass.h>
#include <SmSurfaceCache.h>

namespace
{
enum P2Op : int32_t
{
    P2_EXPORT = 0,
    P2_EXTRAS = 1,
    P2_QUERY = 2,
    P2_SET_TOL = 3,
    P2_REBUILD = 4,
    P2_MAKE = 5,
    P2_SECTOR = 6,
    P2_MERGE = 7,
    P2_MATE = 8,
    P2_NOTIFY = 9,
    P2_STALE = 10,
    P2_SWAP = 11,
    P2_REPLACE = 12,
    P2_RAYFIRE = 13,
    P2_MATE_PROBE = 14,
};

// The B-rep's topology after a Make op, read through fresh list queries: per region
// (GetRegions order) its shells (GetShells order) as `kind ref` (kind 0 unknown, 1 vertex
// shell with ref = its vertex, 2 wire shell with ref = its first wire edge, 3 face-use
// shell, ref -1); per vertex (GetVertices order) its vertex-use count; per edge
// (GetEdges order) the start and end vertex (primary edge use and its mate) and the
// shells of both edge uses (index in the flattened shell list, -1 none).
void topology_summary(SmBrep* brep, std::vector<double>& o)
{
    SmTArray<SmRegion*> regions;
    SmTArray<SmVertex*> vertices;
    SmTArray<SmEdge*> edges;
    brep->GetRegions(regions);
    brep->GetVertices(vertices);
    brep->GetEdges(edges);
    auto vidx = [&](SmVertex* v) -> double {
        for (ULONG i = 0; i < vertices.GetSize(); i++)
            if (vertices[i] == v)
                return (double)i;
        return -1.0;
    };
    auto eidx = [&](SmEdge* e) -> double {
        for (ULONG i = 0; i < edges.GetSize(); i++)
            if (edges[i] == e)
                return (double)i;
        return -1.0;
    };
    std::vector<SmShell*> all;
    o.push_back((double)regions.GetSize());
    for (ULONG r = 0; r < regions.GetSize(); r++)
    {
        SmTArray<SmShell*> shells;
        regions[r]->GetShells(shells);
        o.push_back((double)shells.GetSize());
        for (ULONG i = 0; i < shells.GetSize(); i++)
        {
            SmShell* s = shells[i];
            all.push_back(s);
            if (s->IsVertexShell())
            {
                o.push_back(1.0);
                o.push_back(vidx(s->GetVertex()));
            }
            else if (s->IsWireShell())
            {
                o.push_back(2.0);
                o.push_back(eidx(s->GetWireEdge()));
            }
            else
            {
                o.push_back(s->IsFaceuseShell() ? 3.0 : 0.0);
                o.push_back(-1.0);
            }
        }
    }
    auto sidx = [&](SmShell* s) -> double {
        for (size_t i = 0; i < all.size(); i++)
            if (all[i] == s)
                return (double)i;
        return -1.0;
    };
    o.push_back((double)vertices.GetSize());
    for (ULONG i = 0; i < vertices.GetSize(); i++)
    {
        SmTArray<SmVertexuse*> uses;
        vertices[i]->GetVertexuses(uses);
        o.push_back((double)uses.GetSize());
    }
    o.push_back((double)edges.GetSize());
    for (ULONG i = 0; i < edges.GetSize(); i++)
    {
        SmEdgeuse* p = edges[i]->GetPrimaryEdgeuse();
        SmEdgeuse* m = p ? p->GetMate() : nullptr;
        for (SmEdgeuse* eu : {p, m})
            o.push_back(eu && eu->GetVertexuse() ? vidx(eu->GetVertexuse()->GetVertex()) : -1.0);
        for (SmEdgeuse* eu : {p, m})
            o.push_back(eu ? sidx(eu->GetShell()) : -1.0);
    }
}

struct EdgePeek : SmEdge
{
    static SmCurve* SmEdge::*curve() { return &EdgePeek::m_pCurve; }
};

// A plain SmBSplineCurve of `c`'s NURB, or nullptr when `c` already is one (or a line)
// or has no NURB (as the heal.heal_data_p2 shim).
SmBSplineCurve* plain_curve(SmCurve* c)
{
    if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE) || c->GetType() == SmBSplineCurve_TYPE ||
        c->GetType() == SmLine_TYPE)
        return nullptr;
    SmBSplineCurve* b = (SmBSplineCurve*)c;
    gw_CURVE* nurb = b->GetGwNurbPointer();
    if (nurb == nullptr)
        return nullptr;
    return new (the_context()) SmBSplineCurve(sm_AllocateAndCopyNurbCurve(nurb), b->GetDim(), FALSE);
}

// Gives every analytic face and edge a plain B-spline copy owned by it (the originals
// are leaked), so that the Rust import of the export is exact.
void normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*FPeek::srf();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb = new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        if (s->GetOwner() == (SmObject*)f)
            nb->SetOwner(f);
        f->*FPeek::srf() = nb;
    }
    for (SmEdge* e : g.edges)
    {
        SmCurve* c = e->*EdgePeek::curve();
        SmBSplineCurve* nc = plain_curve(c);
        if (nc == nullptr)
            continue;
        if (c->GetOwner() == (SmObject*)e)
            nc->SetOwner(e);
        e->*EdgePeek::curve() = nc;
    }
}

template <class T> T* at(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : nullptr;
}

void put_ids(std::vector<double>& o, Graph& g, int k, const std::vector<const void*>& v)
{
    o.push_back((double)v.size());
    for (const void* p : v)
        o.push_back((double)g.find(k, p));
}

SmObject* object_of(Graph& g, int64_t ty, int64_t i)
{
    if (ty < 0)
        return nullptr;
    return target_of(g, ty, i);
}

void run(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    SmBrep* brep = b.brep;
    int64_t editing = rd.idx();
    if (editing >= 0)
        brep->m_bEditingEnabled = editing ? TRUE : FALSE;
    o.push_back(0.0);
    // register the B-rep's face, edge and vertex lists first: the port's list order is
    // its arena (import) order
    Graph g;
    {
        SmTArray<SmFace*> fs;
        SmTArray<SmEdge*> es;
        SmTArray<SmVertex*> vs;
        brep->GetFaces(fs);
        brep->GetEdges(es);
        brep->GetVertices(vs);
        for (ULONG i = 0; i < fs.GetSize(); i++)
            g.face(fs[i]);
        for (ULONG i = 0; i < es.GetSize(); i++)
            g.edge(es[i]);
        for (ULONG i = 0; i < vs.GetSize(); i++)
            g.vertex(vs[i]);
    }
    walk(brep, g, true);
    normalise(g);
    switch (op)
    {
    case P2_EXPORT: export_graph(brep, g, o); break;
    case P2_EXTRAS:
    {
        o.push_back((double)brep->GetTolerance());
        o.push_back((double)g.find(0, brep->GetInfiniteRegion()));
        o.push_back((double)g.find(0, (SmRegion*)brep->GetList()));
        o.push_back(flg(brep->m_bEditingEnabled));
        for (SmVertex* v : g.vertices)
        {
            std::vector<const void*> l;
            for (SmTopology* t : list_of(v->GetList()))
                l.push_back(t);
            put_ids(o, g, 8, l);
        }
        for (SmShell* s : g.shells)
            o.push_back((double)(s->*ShPeek::type()));
        break;
    }
    case P2_QUERY:
    {
        int64_t which = rd.idx();
        std::vector<const void*> l;
        if (which == 0)
        {
            SmTArray<SmEdgeuse*> eus;
            brep->GetEdgeuses(eus);
            for (ULONG i = 0; i < eus.GetSize(); i++)
                l.push_back(eus[i]);
            put_ids(o, g, 6, l);
        }
        else if (which == 1)
        {
            SmTArray<SmVertex*> vs;
            brep->GetShellVertices(vs);
            for (ULONG i = 0; i < vs.GetSize(); i++)
                l.push_back(vs[i]);
            put_ids(o, g, 9, l);
        }
        else if (which == 2)
            o.push_back(flg(brep->IsManifoldSolid()));
        else if (which == 3)
            o.push_back(flg(brep->IsSheet()));
        else if (which == 4)
            o.push_back(flg(brep->IsOrientedSheet()));
        else
        {
            double size = 0.0;
            SmEdge* pE = nullptr;
            brep->GetMinEdgeBBoxSize(size, pE);
            o.push_back(size);
            o.push_back((double)g.find(7, pE));
        }
        break;
    }
    case P2_SET_TOL:
    {
        double tol = rd.num();
        SmBoolean only_larger = rd.bflag();
        SmBoolean cascade = rd.bflag();
        brep->SetTolerance(SmZoneTol3d(tol), only_larger, cascade);
        o.push_back((double)brep->GetTolerance());
        for (SmFace* f : g.faces)
            o.push_back((double)(f->*FPeek::tol()));
        for (SmEdge* e : g.edges)
            o.push_back((double)e->GetTolerance());
        for (SmVertex* v : g.vertices)
            o.push_back((double)v->GetTolerance());
        break;
    }
    case P2_REBUILD:
    {
        o.push_back(st(brep->RebuildUVTrimCurves()));
        for (SmEdgeuse* p : g.edgeuses)
            o.push_back(p->*EuPeek::uv() ? 1.0 : 0.0);
        for (SmEdge* e : g.edges)
            o.push_back((double)e->GetTolerance());
        break;
    }
    case P2_MAKE:
    {
        int64_t which = rd.idx();
        SmRegion* r = at(g.regions, rd.idx());
        SmVertex* v1 = at(g.vertices, rd.idx());
        SmVertex* v2 = at(g.vertices, rd.idx());
        SmStatus s = SM_ERR;
        if (which == 0)
        {
            SmShell* pS = nullptr;
            SmVertex* pV = nullptr;
            s = brep->MakeShellVertexTopology(r, pS, pV);
        }
        else if (which == 1)
        {
            SmEdge* pE = nullptr;
            SmVertex* pV = nullptr;
            s = brep->MakeEdgeVertexTopology(r, v1, pE, pV);
        }
        else
        {
            SmEdge* pE = nullptr;
            s = brep->MakeEdgeTopology(r, v1, v2, pE);
        }
        o.push_back(st(s));
        topology_summary(brep, o);
        break;
    }
    case P2_SECTOR:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t0 = rd.num(), t1 = rd.num();
        SmOrientType orient = (SmOrientType)rd.idx();
        SmPointClassificationType cls = (SmPointClassificationType)rd.idx();
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        double t = rd.num();
        SmPoint2d uv = rd.uv();
        SmPointClassification pc;
        pc.SetClassObject(cls, object_of(g, ty, i));
        pc.SetTParam(t);
        pc.SetUVParam(uv);
        SmRegion* r = nullptr;
        o.push_back(st(brep->LocalCurveSector(*c, SmExtent1d(t0, t1), orient, pc, r)));
        o.push_back((double)g.find(0, r));
        delete c;
        break;
    }
    case P2_MERGE:
    {
        SmFace* f = at(g.faces, rd.idx());
        double tol = rd.num();
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        std::vector<SmBSplineCurve*> own;
        for (int64_t i = 0; i < n; i++)
        {
            own.push_back(read_curve(rd, ctx));
            curves.Add(own.back());
        }
        SmTArray<SmFace*> nf;
        SmTArray<SmEdge*> ne;
        SmStatus s = brep->MergeCurvesOnSurface(*f->GetSurface(), tol, curves, NULL, nf, ne);
        o.push_back(st(s));
        o.push_back((double)nf.GetSize());
        o.push_back((double)ne.GetSize());
        for (SmBSplineCurve* c : own)
            delete c;
        break;
    }
    case P2_MATE:
    {
        SmEdge* e = at(g.edges, rd.idx());
        SmSurface* s = read_surface(rd, ctx);
        double f1 = rd.num(), f2 = rd.num();
        o.push_back(st(brep->MateLaminaEdgeToSurface(e, *s, f1, f2)));
        delete s;
        break;
    }
    case P2_MATE_PROBE:
    {
        // The Mate guard's probe (no B-rep change): MateLaminaEdgeToSurface's dMaxDist,
        // computed as SmBrep.cpp:22200-22229 does (extended copy of the mating surface,
        // 21 GlobalPointSolve distances). Outputs `status dMaxDist`. A distance whose
        // square overflows makes dMaxDist infinite, and CreateExtendedSurface then never
        // stops halving (plan/upstream-issues.md HANG-9). The early returns of
        // SmBrep.cpp:22166-22195 are replayed first (without the PRE_EDIT notify, which
        // returns nothing): when Mate would stop there, the status is SM_ERR and the
        // guard lets the Mate run, so its early status is still compared.
        SmEdge* e = at(g.edges, rd.idx());
        SmSurface* s = read_surface(rd, ctx);
        double f1 = rd.num();
        rd.num();
        SmStatus ps = SM_SUCCESS;
        if (brep->m_bEditingEnabled == FALSE || !e->IsLamina())
            ps = SM_ERR;
        SmEdgeuse* pEU = ps == SM_SUCCESS ? e->GetPrimaryEdgeuse() : NULL;
        SmFace* pFace = pEU ? pEU->GetFace() : NULL;
        if (pFace == NULL)
            ps = SM_ERR;
        for (int k = 0; k < 2 && ps == SM_SUCCESS; k++)
        {
            SmEdgeuse* pN = k == 0 ? pEU->GetCCWEdgeuse() : pEU->GetCWEdgeuse();
            SmEdge* pNE = pN ? pN->GetEdge() : NULL;
            if (pNE == NULL || (!pNE->IsLamina() && !pNE->IsSeam(*pFace->GetSurface())))
                ps = SM_ERR;
        }
        SmSurface* pExtendedMating = NULL;
        if (ps == SM_SUCCESS)
            ps = s->Copy(ctx, pExtendedMating);
        SmObjDelete sCleanExtended(pExtendedMating);
        double dMaxDist = 0;
        SmCurve* pECurve = e->GetCurve();
        if (ps == SM_SUCCESS && pECurve == NULL)
            ps = SM_ERR_NULL_POINTER;
        if (ps == SM_SUCCESS)
        {
            SmPlane* pPlane = SM_CAST_PTR(SmPlane, pExtendedMating);
            if (pPlane)
            {
                SmExtent2d sDomain = pPlane->GetSTEPUVDomain();
                sDomain.ExpandAbsolute(sDomain.GetSize().Length() * f1);
                pPlane->AdjustSTEPUVDomain(sDomain);
            }
            SmExtent1d sIvl = e->GetInterval();
            SmSolution sSData[8];
            SmSolutionArray sSolutions(8, sSData);
            for (ULONG j = 0; j <= 20 && ps == SM_SUCCESS; j++)
            {
                SmPoint3d sEdgePnt;
                ps = pECurve->EvaluatePoint(sIvl.Evaluate(j / 20.0), sEdgePnt);
                if (ps != SM_SUCCESS)
                    break;
                ps = pExtendedMating->GlobalPointSolve(pExtendedMating->GetNaturalUVDomain(), SM_SO_MINIMIZE,
                                                       sEdgePnt, SM_EFF_ZERO_SQRT, NULL, SM_SR_ALL, sSolutions);
                if (ps != SM_SUCCESS || sSolutions.GetSize() < 1)
                    break;
                if (sSolutions[0].m_vStart.m_dSolutionValue > dMaxDist)
                    dMaxDist = sSolutions[0].m_vStart.m_dSolutionValue;
            }
        }
        delete s;
        o.push_back(st(ps));
        o.push_back(dMaxDist);
        break;
    }
    case P2_NOTIFY:
    {
        SmNotifyOperation nop = (SmNotifyOperation)rd.idx();
        SmObject* d[3];
        for (int k = 0; k < 3; k++)
        {
            int64_t ty = rd.idx();
            int64_t i = rd.idx();
            d[k] = ty == SmBrep_TYPE ? (SmObject*)brep : object_of(g, ty, i);
        }
        brep->Notify(nop, d[0], d[1], d[2]);
        o.push_back(0.0);
        break;
    }
    case P2_STALE:
    {
        // seed SM_OC_TRIMSRF caches: face i gets one when bit (i % 8) of `create` is
        // set, marked m_bFaceWasModified when that bit of `modified` is set
        int64_t create = rd.idx(), modified = rd.idx();
        // drop the caches reading or validating the B-rep built (the Rust B-rep has none)
        for (SmFace* f : g.faces)
            if (f->GetSurface() != nullptr)
                SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF, f->GetSurface());
        for (size_t i = 0; i < g.faces.size(); i++)
        {
            SmSurface* s = g.faces[i]->GetSurface();
            if (s == nullptr || ((create >> (i % 8)) & 1) == 0)
                continue;
            SmSurfaceCache* c =
                (SmSurfaceCache*)SmCacheMgr::GetOrCreateObjectCache(SM_OC_TRIMSRF, s);
            if (c != nullptr)
                c->m_bFaceWasModified = ((modified >> (i % 8)) & 1) ? TRUE : FALSE;
        }
        o.push_back(st(brep->DeleteStaleCaches()));
        for (SmFace* f : g.faces)
        {
            SmSurface* s = f->GetSurface();
            o.push_back(s != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, s) != nullptr);
        }
        break;
    }
    case P2_SWAP: o.push_back(st(brep->SwapUV())); break;
    case P2_REPLACE:
    {
        SmFace* f = at(g.faces, rd.idx());
        SmSurface* s = read_surface(rd, ctx);
        int64_t keep = rd.idx();
        SmBoolean create = rd.bflag();
        SmBoolean add = keep == 0 ? FALSE : keep == 1 ? TRUE : UNSURE;
        SmTArray<SmFace*> mod;
        SmSurface* old = f->GetSurface();
        SmStatus r = brep->ReplaceSurface(f, s, add, &mod, create);
        if (add == UNSURE)
            delete old;
        o.push_back(st(r));
        o.push_back((double)mod.GetSize());
        break;
    }
    case P2_RAYFIRE:
    {
        SmVector3d p = rd.vec();
        SmVector3d v = rd.vec();
        double tol = rd.num();
        bool lim = rd.flag();
        double limit = rd.num();
        SmRegion* r = nullptr;
        SmPointClassification pc;
        SmStatus s = brep->RayFire(p, v, SmZoneTol3d(tol), r, pc, lim ? &limit : NULL);
        o.push_back(st(s));
        o.push_back((double)g.find(0, r));
        o.push_back((double)pc.GetPointClass());
        break;
    }
    default: o.push_back(-9.0); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 1 || out_len == nullptr || path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
