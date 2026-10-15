// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.poly`.
// Builds an SmPolyBrep from a flat description (SmPolyBrep, SmPolyRegion, SmPolyShell,
// SmPolyFace, SmPolyVertex constructors, StartPolyEdgeLoop, AddPolyEdge without end vertex,
// FinishPolyEdgeLoop, radial rings set directly, SmPolyVertAuxData, SmCPolyFace) and runs
// one operation of the unit (source/SMLib/src/SmPoly.cpp): CalculateBoundingBox (brep,
// face, composite face, edge, vertex), GetNormal, ComputeSphereBound, ContainsPoint,
// PointInPolygon, IsDegenerate, Transform, the setters and destructors, IsKindOf; then
// dumps the topology and the stored values.
//
// One entry point, sc_query_poly_eval(in, n_in, out, cap, out_len). The input layout is
// documented in crates/solidcore-ffi/src/units/query_poly.rs, which validates every index
// so upstream only touches valid memory. Output: sections `key kind len values...`.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmContext.h>
#include <SmCoreTypes.h>
#include <SmExtent3d.h>
#include <SmMapPtrToPtr.h>
#include <SmMemBlockMgr.h>
#include <SmOwningTopology.h>
#include <SmPseudoBox.h>
#include <SmSAGObject.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTopoTypes.h>
#include <SmTypes.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

// The poly classes are final; read their protected members (and set the radial rings,
// the vertex list and the back pointer flag) by opening the access of this header only.
#define protected public
#define private public
#include <SmPoly.h>
#undef private
#undef protected

#include <cstdint>
#include <vector>

namespace
{
enum : int32_t
{
    OP_DUMP = 0,
    OP_BBOX = 1,
    OP_NORMAL = 2,
    OP_SPHERE = 3,
    OP_CONTAINS = 4,
    OP_DEGEN = 5,
    OP_TRANSFORM = 6,
    OP_MUTATE = 7,
    OP_KINDS = 8,
};

// Section keys (keep in sync with crates/difftest/src/units/query_poly.rs).
enum : int32_t
{
    K_STATUS = 0,
    K_REGIONS = 1,
    K_SHELLS = 2,
    K_FACES = 3,
    K_FACES_COMMON = 4,
    K_FIRST_FACE = 5,
    K_FACE_TOPO = 6,
    K_FACE_GEOM = 7,
    K_LOOP_TOPO = 8,
    K_EDGE_TOPO = 9,
    K_EDGE_GEOM = 10,
    K_VERTEX_TOPO = 11,
    K_VERTEX_GEOM = 12,
    K_AUX_TOPO = 13,
    K_AUX_GEOM = 14,
    K_BREP_GEOM = 15,
    K_CPOLY = 16,
    K_BREP_EDGES = 17,
    K_BREP_TOPO = 18,
    K_BBOX = 20,
    K_BBOX_POINTS = 21,
    K_OBJ_BOXES = 22,
    K_NORMALS = 23,
    K_SPHERES = 25,
    K_RESULTS = 26,
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        SmVector3d v;
        v.x = x;
        v.y = y;
        v.z = z;
        return v;
    }
};

struct Out
{
    std::vector<double> o;
    void sec(int32_t key, int32_t kind, const std::vector<double>& v)
    {
        o.push_back((double)key);
        o.push_back((double)kind);
        o.push_back((double)v.size());
        o.insert(o.end(), v.begin(), v.end());
    }
    void floats(int32_t key, const std::vector<double>& v) { sec(key, 0, v); }
    void ints(int32_t key, const std::vector<double>& v) { sec(key, 1, v); }
};

void put(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void put_box(std::vector<double>& o, const SmExtent3d& b)
{
    put(o, b.GetMin());
    put(o, b.GetMax());
}

template <typename T> double ix(const std::vector<T*>& v, const void* p)
{
    if (p == NULL)
        return -1.0;
    for (size_t i = 0; i < v.size(); ++i)
        if ((const void*)v[i] == p)
            return (double)i;
    return -2.0;
}

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

void* fake(int64_t label)
{
    return label <= 0 ? NULL : (void*)(uintptr_t)(label << 4);
}

int64_t unfake(const void* p)
{
    return p == NULL ? 0 : (int64_t)((uintptr_t)p >> 4);
}

struct Model
{
    SmPolyBrep* brep = NULL;
    std::vector<SmPolyRegion*> regions;
    std::vector<SmPolyShell*> shells;
    std::vector<SmPolyFace*> faces; // plain faces, then composite faces
    std::vector<bool> face_alive;
    std::vector<SmPolyLoop*> loops;
    std::vector<SmPolyEdge*> edges;
    std::vector<bool> edge_alive;
    std::vector<SmPolyVertex*> vertices;
    std::vector<SmPolyVertAuxData*> aux;
};

void build(Rd& rd, const SmContext& ctx, Model& m)
{
    double tol = rd.num();
    m.brep = new (ctx) SmPolyBrep(tol);
    SmPolyBrep* b = m.brep;

    int64_t nr = rd.idx();
    for (int64_t r = 0; r < nr; ++r)
        m.regions.push_back(new (b) SmPolyRegion(b));
    // shells after all regions (same creation order as the Rust side)
    std::vector<int64_t> per_region;
    rd.at -= 0;
    for (int64_t r = 0; r < nr; ++r)
    {
        int64_t ns = rd.idx();
        for (int64_t s = 0; s < ns; ++s)
            m.shells.push_back(new (b) SmPolyShell(m.regions[r]));
    }

    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; ++i)
    {
        SmVector3d p = rd.vec();
        double t = rd.num();
        SmPolyVertex* v = new (b) SmPolyVertex(p, t);
        b->m_pPolyVertexListHead->PostInsert(v);
        m.vertices.push_back(v);
    }

    int64_t nf = rd.idx();
    for (int64_t i = 0; i < nf; ++i)
    {
        int64_t shell = rd.idx();
        double ftol = rd.num();
        bool has_n = rd.flag();
        SmVector3d n = rd.vec();
        SmPolyFace* f = new (b) SmPolyFace(ftol, NULL, NULL, m.shells[shell], has_n ? &n : NULL);
        m.faces.push_back(f);
        m.face_alive.push_back(true);
        int64_t nl = rd.idx();
        for (int64_t l = 0; l < nl; ++l)
        {
            int64_t mode = rd.idx();
            double etol = rd.num();
            int64_t k = rd.idx();
            std::vector<SmPolyVertex*> vs;
            for (int64_t j = 0; j < k; ++j)
                vs.push_back(m.vertices[rd.idx()]);
            SmPolyLoop* lp = NULL;
            f->StartPolyEdgeLoop(lp);
            m.loops.push_back(lp);
            int64_t ne = mode == 2 ? k - 1 : k;
            for (int64_t j = 0; j < ne; ++j)
            {
                SmPolyVertex* v0 = vs[j];
                SmPolyVertex* v1 = vs[(j + 1) % k];
                SmPolyEdge* e = NULL;
                lp->AddPolyEdge(etol, v0->GetPoint(), v1->GetPoint(), v0, NULL, b, e);
                m.edges.push_back(e);
                m.edge_alive.push_back(true);
            }
            if (mode == 1)
                lp->FinishPolyEdgeLoop(b);
            else if (mode == 2)
                lp->SetLastEndPolyVertex(vs[k - 1]);
        }
    }

    int64_t ng = rd.idx();
    for (int64_t g = 0; g < ng; ++g)
    {
        int64_t k = rd.idx();
        std::vector<SmPolyEdge*> es;
        for (int64_t j = 0; j < k; ++j)
            es.push_back(m.edges[rd.idx()]);
        for (int64_t j = 0; j < k; ++j)
        {
            es[j]->m_pNextRadialE = es[(j + 1) % k];
            es[j]->m_pLastRadialE = es[(j + k - 1) % k];
        }
    }

    int64_t na = rd.idx();
    for (int64_t i = 0; i < na; ++i)
    {
        SmPolyVertAuxData* a = new (ctx) SmPolyVertAuxData();
        a->m_vNormal = rd.vec();
        double u = rd.num();
        double v = rd.num();
        a->m_vUV.x = u;
        a->m_vUV.y = v;
        m.aux.push_back(a);
    }
    int64_t nea = rd.idx();
    for (int64_t i = 0; i < nea; ++i)
    {
        int64_t e = rd.idx();
        int64_t a = rd.idx();
        m.edges[e]->SetAuxData(m.aux[a]);
    }
    int64_t nvn = rd.idx();
    for (int64_t i = 0; i < nvn; ++i)
    {
        int64_t v = rd.idx();
        m.vertices[v]->AddFaceNormal(rd.vec());
    }
    int64_t nc = rd.idx();
    for (int64_t i = 0; i < nc; ++i)
    {
        int64_t k = rd.idx();
        SmCPolyFace* c = NULL;
        for (int64_t j = 0; j < k; ++j)
        {
            SmPolyFace* f = m.faces[rd.idx()];
            if (j == 0)
                c = new (b) SmCPolyFace(f);
            else
                c->AddPolyFace(f);
        }
        m.faces.push_back(c);
        m.face_alive.push_back(true);
    }
    int64_t nof = rd.idx();
    for (int64_t i = 0; i < nof; ++i)
    {
        int64_t f = rd.idx();
        int64_t label = rd.idx();
        m.faces[f]->SetOriginalFace((SmFace*)fake(label), FALSE);
    }
    int64_t noe = rd.idx();
    for (int64_t i = 0; i < noe; ++i)
    {
        int64_t e = rd.idx();
        int64_t label = rd.idx();
        m.edges[e]->m_pOriginalEdgeuse = (SmEdgeuse*)fake(label);
    }
    b->m_bOKBackPtrs = rd.flag() ? TRUE : FALSE;
}

void dump(Model& m, Out& out)
{
    SmPolyBrep* b = m.brep;
    std::vector<double> o;

    SmTArray<SmPolyRegion*> regs;
    b->GetPolyRegions(regs);
    for (ULONG i = 0; i < regs.GetSize(); ++i)
    {
        SmTArray<SmPolyShell*> sh;
        regs[i]->GetPolyShells(sh);
        o.push_back(ix(m.regions, regs[i]));
        o.push_back((double)sh.GetSize());
        for (ULONG j = 0; j < sh.GetSize(); ++j)
            o.push_back(ix(m.shells, sh[j]));
    }
    out.ints(K_REGIONS, o);

    o.clear();
    SmTArray<SmPolyShell*> shells;
    b->GetPolyShells(shells);
    for (ULONG i = 0; i < shells.GetSize(); ++i)
    {
        SmTArray<SmPolyFace*> fs;
        shells[i]->GetPolyFaces(fs);
        o.push_back(ix(m.shells, shells[i]));
        o.push_back(ix(m.regions, shells[i]->GetPolyRegion()));
        o.push_back((double)fs.GetSize());
        for (ULONG j = 0; j < fs.GetSize(); ++j)
            o.push_back(ix(m.faces, fs[j]));
    }
    out.ints(K_SHELLS, o);

    o.clear();
    SmTArray<SmPolyFace*> faces;
    b->GetPolyFaces(faces);
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        o.push_back(ix(m.faces, faces[i]));
    out.ints(K_FACES, o);

    o.clear();
    for (int64_t label = 1; label <= 3; ++label)
    {
        SmTArray<SmPolyFace*> fs;
        b->GetPolyFaces(fs, (SmFace*)fake(label));
        o.push_back((double)fs.GetSize());
        for (ULONG j = 0; j < fs.GetSize(); ++j)
            o.push_back(ix(m.faces, fs[j]));
    }
    out.ints(K_FACES_COMMON, o);

    o.clear();
    o.push_back(ix(m.faces, b->GetFirstPolyFace()));
    out.ints(K_FIRST_FACE, o);

    std::vector<double> g;
    o.clear();
    for (size_t i = 0; i < m.faces.size(); ++i)
    {
        if (!m.face_alive[i])
            continue;
        SmPolyFace* f = m.faces[i];
        SmTArray<SmPolyLoop*> ls;
        f->GetPolyLoops(ls);
        SmTArray<SmPolyEdge*> es;
        f->GetPolyEdges(es);
        o.push_back((double)i);
        o.push_back(ix(m.faces, f->m_pCPolyFace));
        o.push_back(f->m_pCPolyFace == f ? -1.0 : ix(m.shells, f->GetPolyShell()));
        o.push_back(f->GetOKBackPtrs() ? 1.0 : 0.0);
        o.push_back((double)unfake(f->GetOriginalFace()));
        o.push_back(ix(m.loops, f->GetOuterPolyLoop()));
        o.push_back((double)ls.GetSize());
        for (ULONG j = 0; j < ls.GetSize(); ++j)
            o.push_back(ix(m.loops, ls[j]));
        o.push_back((double)es.GetSize());
        for (ULONG j = 0; j < es.GetSize(); ++j)
            o.push_back(ix(m.edges, es[j]));
        if (f->m_pCPolyFace == f)
        {
            SmTArray<SmPolyFace*> mem;
            f->GetPolyFacesOfComposite(mem);
            o.push_back((double)mem.GetSize());
            for (ULONG j = 0; j < mem.GetSize(); ++j)
                o.push_back(ix(m.faces, mem[j]));
        }
        g.push_back(f->GetTolerance());
        put(g, f->m_vNormal);
        g.push_back(f->GetMaxDeviation());
        put(g, f->m_vSphereCenter);
        g.push_back(f->m_dSphereRadius);
    }
    out.ints(K_FACE_TOPO, o);
    out.floats(K_FACE_GEOM, g);

    o.clear();
    for (size_t i = 0; i < m.loops.size(); ++i)
    {
        SmPolyLoop* l = m.loops[i];
        SmTArray<SmPolyEdge*> es;
        l->GetPolyEdges(es);
        o.push_back((double)i);
        o.push_back(ix(m.faces, l->GetPolyFace()));
        o.push_back(l->IsClosed() ? 1.0 : 0.0);
        o.push_back((double)l->GetOrientation());
        o.push_back(ix(m.edges, l->GetFirstPolyEdge()));
        o.push_back((double)es.GetSize());
        for (ULONG j = 0; j < es.GetSize(); ++j)
            o.push_back(ix(m.edges, es[j]));
    }
    out.ints(K_LOOP_TOPO, o);

    o.clear();
    g.clear();
    for (size_t i = 0; i < m.edges.size(); ++i)
    {
        if (!m.edge_alive[i])
            continue;
        SmPolyEdge* e = m.edges[i];
        SmTArray<SmPolyEdge*> rads;
        e->GetAllRadials(rads);
        SmPolyVertex* ev = e->GetEndPolyVertex();
        o.push_back((double)i);
        o.push_back(ix(m.loops, e->GetPolyLoop()));
        o.push_back(ix(m.faces, e->GetPolyFace()));
        o.push_back(ix(m.vertices, e->GetStartPolyVertex()));
        o.push_back(ix(m.vertices, ev));
        o.push_back(ix(m.edges, e->GetCWPolyEdge()));
        o.push_back(ix(m.edges, e->GetCCWPolyEdge()));
        o.push_back(ix(m.edges, e->GetRadial()));
        o.push_back((double)rads.GetSize());
        for (ULONG j = 0; j < rads.GetSize(); ++j)
            o.push_back(ix(m.edges, rads[j]));
        o.push_back(ix(m.aux, e->GetAuxData()));
        o.push_back(e->Is2dOrZero() ? 1.0 : 0.0);
        o.push_back(e->GetOKBackPtrs() ? 1.0 : 0.0);
        o.push_back((double)unfake(e->GetOriginalEdgeuse()));
        g.push_back(e->GetTolerance());
        put(g, e->GetStartPoint());
        if (ev)
            put(g, e->GetEndPoint());
    }
    out.ints(K_EDGE_TOPO, o);
    out.floats(K_EDGE_GEOM, g);

    o.clear();
    g.clear();
    SmTArray<SmPolyVertex*> verts;
    b->GetPolyVertices(verts);
    for (ULONG i = 0; i < verts.GetSize(); ++i)
    {
        SmPolyVertex* v = verts[i];
        SmTArray<SmPolyEdge*> se, pe;
        SmTArray<SmPolyFace*> pf;
        v->GetStartingPolyEdges(se);
        v->GetPolyEdges(pe);
        v->GetPolyFaces(pf);
        o.push_back(ix(m.vertices, v));
        o.push_back((double)se.GetSize());
        for (ULONG j = 0; j < se.GetSize(); ++j)
            o.push_back(ix(m.edges, se[j]));
        o.push_back((double)pe.GetSize());
        for (ULONG j = 0; j < pe.GetSize(); ++j)
            o.push_back(ix(m.edges, pe[j]));
        o.push_back((double)pf.GetSize());
        for (ULONG j = 0; j < pf.GetSize(); ++j)
            o.push_back(ix(m.faces, pf[j]));
        o.push_back(v->Is2dOrZero() ? 1.0 : 0.0);
        put(g, v->GetPoint());
        g.push_back(v->GetTolerance());
        SmTArray<SmVector3d>& ns = v->GetNormalsRef();
        g.push_back((double)ns.GetSize());
        for (ULONG j = 0; j < ns.GetSize(); ++j)
            put(g, ns[j]);
    }
    out.ints(K_VERTEX_TOPO, o);
    out.floats(K_VERTEX_GEOM, g);

    o.clear();
    g.clear();
    SmTArray<SmPolyVertAuxData*> al;
    b->GetAuxDataList(al);
    std::vector<SmPolyVertAuxData*> seen;
    for (ULONG i = 0; i < al.GetSize(); ++i)
    {
        o.push_back(ix(m.aux, al[i]));
        bool dup = false;
        for (size_t j = 0; j < seen.size(); ++j)
            dup = dup || seen[j] == al[i];
        if (!dup)
            seen.push_back(al[i]);
    }
    o.push_back(-3.0);
    for (size_t i = 0; i < seen.size(); ++i)
    {
        o.push_back(ix(m.aux, seen[i]));
        o.push_back((double)seen[i]->GetNumUsers());
        put(g, seen[i]->m_vNormal);
        g.push_back(seen[i]->m_vUV.x);
        g.push_back(seen[i]->m_vUV.y);
    }
    out.ints(K_AUX_TOPO, o);
    out.floats(K_AUX_GEOM, g);

    g.clear();
    g.push_back(b->GetTolerance());
    SmTArray<SmPoint3d> pts;
    b->GetPolyPoints(pts);
    for (ULONG i = 0; i < pts.GetSize(); ++i)
        put(g, pts[i]);
    out.floats(K_BREP_GEOM, g);

    o.clear();
    SmTArray<SmCPolyFace*> cps;
    b->GetCPolyFaces(cps);
    for (ULONG i = 0; i < cps.GetSize(); ++i)
        o.push_back(ix(m.faces, cps[i]));
    out.ints(K_CPOLY, o);

    o.clear();
    SmTArray<SmPolyEdge*> bes;
    b->GetPolyEdges(bes);
    for (ULONG i = 0; i < bes.GetSize(); ++i)
        o.push_back(ix(m.edges, bes[i]));
    out.ints(K_BREP_EDGES, o);

    o.clear();
    o.push_back(b->GetOKBackPtrs() ? 1.0 : 0.0);
    o.push_back((double)verts.GetSize());
    out.ints(K_BREP_TOPO, o);
}

void op_bbox(Rd& rd, Model& m, Out& out)
{
    bool want_pseudo = rd.flag();
    int64_t pts_mode = rd.idx();
    SmTArray<SmPoint3d> pts;
    if (pts_mode == 2)
    {
        int64_t n = rd.idx();
        for (int64_t i = 0; i < n; ++i)
            pts.Add(rd.vec());
    }
    bool want_centroid = rd.flag();
    bool has_normal = rd.flag();
    SmVector3d nrm = rd.vec();

    SmExtent3d box;
    SmPseudoBox pb;
    SmPoint3d cen;
    SmStatus s = m.brep->CalculateBoundingBox(box, want_pseudo ? &pb : NULL,
                                              pts_mode != 0 ? &pts : NULL,
                                              want_centroid ? &cen : NULL,
                                              has_normal ? &nrm : NULL);
    std::vector<double> o;
    o.push_back(st(s));
    out.ints(K_STATUS, o);
    std::vector<double> g;
    put_box(g, box);
    if (want_pseudo)
    {
        for (ULONG i = 0; i < 3; ++i)
            put(g, pb.GetBasis(i));
        for (ULONG i = 0; i < 3; ++i)
        {
            g.push_back(pb.GetInterval(i).GetMin());
            g.push_back(pb.GetInterval(i).GetMax());
        }
    }
    if (want_centroid)
        put(g, cen);
    if (has_normal)
        put(g, nrm);
    out.floats(K_BBOX, g);
    g.clear();
    for (ULONG i = 0; i < pts.GetSize(); ++i)
        put(g, pts[i]);
    out.floats(K_BBOX_POINTS, g);

    g.clear();
    for (size_t i = 0; i < m.faces.size(); ++i)
    {
        SmExtent3d fb;
        m.faces[i]->CalculateBoundingBox(fb);
        put_box(g, fb);
    }
    for (size_t i = 0; i < m.edges.size(); ++i)
    {
        SmExtent3d eb;
        m.edges[i]->CalculateBoundingBox(eb);
        put_box(g, eb);
    }
    for (size_t i = 0; i < m.vertices.size(); ++i)
    {
        SmExtent3d vb;
        m.vertices[i]->CalculateBoundingBox(vb);
        put_box(g, vb);
    }
    out.floats(K_OBJ_BOXES, g);
}

void op_normal(Rd& rd, Model& m, Out& out)
{
    int64_t n = rd.idx();
    std::vector<double> o, g;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t f = rd.idx();
        bool force = rd.flag();
        bool upd = rd.flag();
        SmBoolean deg = UNSURE;
        SmVector3d v = m.faces[f]->GetNormal(force ? TRUE : FALSE, upd ? TRUE : FALSE, &deg);
        o.push_back(deg ? 1.0 : 0.0);
        put(g, v);
    }
    out.ints(K_RESULTS, o);
    out.floats(K_NORMALS, g);
}

void op_sphere(Rd& rd, Model& m, Out& out)
{
    int64_t n = rd.idx();
    std::vector<double> o, g;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t f = rd.idx();
        bool force = rd.flag();
        SmPoint3d c;
        double r = 0.0;
        SmStatus s = m.faces[f]->ComputeSphereBound(force ? TRUE : FALSE, c, r);
        o.push_back(st(s));
        put(g, c);
        g.push_back(r);
    }
    out.ints(K_STATUS, o);
    out.floats(K_SPHERES, g);
}

void op_contains(Rd& rd, Model& m, Out& out)
{
    int64_t n = rd.idx();
    std::vector<double> o;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t kind = rd.idx();
        int64_t id = rd.idx();
        SmVector3d p = rd.vec();
        double tol = rd.num();
        bool b3d = rd.flag();
        if (kind == 0)
        {
            SmPolyContainmentType t = SM_PCT_UNKNOWN;
            SmStatus s = m.loops[id]->ContainsPoint(p, t, tol, b3d ? TRUE : FALSE);
            o.push_back(st(s));
            o.push_back(s == SM_SUCCESS ? (double)t : -1.0);
        }
        else
        {
            SmBoolean inside = FALSE;
            SmStatus s = m.faces[id]->PointInPolygon(p, inside, b3d ? TRUE : FALSE);
            o.push_back(st(s));
            o.push_back(s == SM_SUCCESS ? (inside ? 1.0 : 0.0) : -1.0);
        }
    }
    out.ints(K_RESULTS, o);
}

void op_degen(Rd& rd, Model& m, Out& out)
{
    int64_t n = rd.idx();
    std::vector<double> o;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t e = rd.idx();
        bool has = rd.flag();
        double t = rd.num();
        SmPolyEdge* pe = m.edges[e];
        SmPolyFace* pf = pe->GetPolyFace();
        if (pf && pf->GetOriginalFace() && pe->GetOKBackPtrs())
        {
            // would dereference the labelled (fake) SmFace: the Rust side defers this
            o.push_back(-9.0);
            continue;
        }
        SmXSectTol3d xs(t);
        o.push_back(pe->IsDegenerate(has ? &xs : NULL) ? 1.0 : 0.0);
    }
    out.ints(K_RESULTS, o);
}

void op_transform(Rd& rd, Model& m, Out& out)
{
    SmVector3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    bool has_scale = rd.flag();
    SmVector3d s = rd.vec();
    SmStatus stt = m.brep->Transform(SmAxis2Placement(org, x, y), has_scale ? &s : NULL);
    std::vector<double> o;
    o.push_back(st(stt));
    out.ints(K_STATUS, o);
}

void op_mutate(Rd& rd, Model& m, Out& out)
{
    int64_t n = rd.idx();
    std::vector<double> o;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t cmd = rd.idx();
        int64_t a = rd.idx();
        int64_t b = rd.idx();
        SmVector3d p = rd.vec();
        switch (cmd)
        {
        case 1:
            m.vertices[a]->SetPoint(p);
            break;
        case 2:
            m.vertices[a]->SetTolerance(p.x);
            break;
        case 3:
            delete m.edges[a];
            m.edge_alive[a] = false;
            break;
        case 4:
            delete m.faces[a];
            m.face_alive[a] = false;
            break;
        case 5:
        {
            SmCPolyFace* c = (SmCPolyFace*)m.faces[a];
            o.push_back(st(c->RemovePolyFace(m.faces[b])));
            break;
        }
        case 6:
            m.faces[a]->SetNormalUnInit();
            break;
        case 7:
            m.edges[a]->SetTolerance(p.x);
            break;
        case 8:
            m.faces[a]->SetTolerance(p.x);
            break;
        case 9:
            m.faces[a]->SetOriginalFace((SmFace*)fake(b), FALSE);
            break;
        case 10:
            m.edges[a]->SetAuxData(NULL);
            break;
        default:
            break;
        }
    }
    out.ints(K_STATUS, o);
}

void op_kinds(Rd& rd, Model& m, Out& out)
{
    int64_t n = rd.idx();
    std::vector<SM_TYPE> codes;
    for (int64_t i = 0; i < n; ++i)
        codes.push_back((SM_TYPE)rd.idx());
    std::vector<const SmObject*> objs;
    objs.push_back(m.brep);
    if (!m.regions.empty())
        objs.push_back(m.regions[0]);
    if (!m.shells.empty())
        objs.push_back(m.shells[0]);
    for (size_t i = 0; i < m.faces.size(); ++i)
        objs.push_back(m.faces[i]);
    if (!m.loops.empty())
        objs.push_back(m.loops[0]);
    if (!m.edges.empty())
        objs.push_back(m.edges[0]);
    if (!m.vertices.empty())
        objs.push_back(m.vertices[0]);
    std::vector<double> o;
    for (size_t i = 0; i < objs.size(); ++i)
    {
        o.push_back((double)objs[i]->GetType());
        for (size_t j = 0; j < codes.size(); ++j)
            o.push_back(objs[i]->IsKindOf(codes[j]) ? 1.0 : 0.0);
    }
    out.ints(K_RESULTS, o);
}
} // namespace

SC_EXPORT int32_t sc_query_poly_eval(const double* in, int64_t n_in, double* out, int64_t cap,
                                     int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    Out o;
    Rd rd{in, 0};
    int32_t op = (int32_t)rd.idx();
    Model m;
    build(rd, ctx, m);
    switch (op)
    {
    case OP_BBOX:
        op_bbox(rd, m, o);
        break;
    case OP_NORMAL:
        op_normal(rd, m, o);
        break;
    case OP_SPHERE:
        op_sphere(rd, m, o);
        break;
    case OP_CONTAINS:
        op_contains(rd, m, o);
        break;
    case OP_DEGEN:
        op_degen(rd, m, o);
        break;
    case OP_TRANSFORM:
        op_transform(rd, m, o);
        break;
    case OP_MUTATE:
        op_mutate(rd, m, o);
        break;
    case OP_KINDS:
        op_kinds(rd, m, o);
        break;
    default:
        break;
    }
    dump(m, o);
    delete m.brep;
    return sc_copy_out(o.o.data(), (int64_t)o.o.size(), out, cap, out_len);
    SC_CATCH
}
