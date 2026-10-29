// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.poly_copy`.
// Builds an SmPolyBrep (regions, shells, faces, loops added with AddPolyEdge and closed,
// left open or given a last end vertex, radial rings set directly, aux data, attributes
// on the poly B-rep, as query_poly.cpp does), copies it with SmApiPolyBrepCopy
// (source/SM_API/src/SmApiPolygons.cpp: the SmPolyBrep copy constructor, FromPolyBrep,
// MakeTopologyFromData) and dumps the copy and the source faces' cached normals.
//
// One entry point, sc_query_poly_copy_eval(in, n_in, out, cap, out_len). Block:
//   brep_tol nr (ns)[nr]
//   nv (x y z tol)[nv]
//   nf (shell ftol has_n n[3] nl (mode etol k v[k])[nl])[nf]   mode 0 open, 1 closed,
//                                                              2 last end vertex
//   ng (k e[k])[ng]
//   na (n[3] u v)[na]  nea (e a)[nea]
//   nat (id value behavior)[nat]
// crates/solidcore-ffi/src/units/query_poly_copy.rs validates every index.
// Output: `status` then, on success:
//   tol, regions: nr (ns (nf (ftol n[3] maxdev nl (orient closed ne)[nl])[nf])[ns])[nr],
//   edges (traversal order): (tol vertex next_radial last_radial next_at_v prev_at_v aux)[],
//   vertices: nv (x y z tol edge_list)[nv], ok_back_ptrs, n_cpoly,
//   attributes: nat (id behavior source_index)[nat],
//   source faces: nf (n[3] maxdev)[nf]
// Indices: position in the copy's own lists; -1 = NULL, -2 = not in the list.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmAttribute.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCoreTypes.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmMapPtrToPtr.h>
#include <SmMemBlockMgr.h>
#include <SmNurbsSrf.h>
#include <SmOwningTopology.h>
#include <SmPlane.h>
#include <SmPseudoBox.h>
#include <SmSAGObject.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTopoTypes.h>
#include <SmTypes.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

// Set the radial rings and read the stored members by opening the access of this header
// only (as query_poly.cpp).
#define protected public
#define private public
#include <SmPoly.h>
#undef private
#undef protected

#include <SmApiPolygons.h>

#include <cstdint>
#include <vector>

namespace
{
struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
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

void put(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
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

SmPolyBrep* build(Rd& rd, const SmContext& ctx, std::vector<SmAttribute*>& attrs)
{
    double tol = rd.num();
    SmPolyBrep* b = new (ctx) SmPolyBrep(tol);

    std::vector<SmPolyRegion*> regions;
    int64_t nr = rd.idx();
    for (int64_t r = 0; r < nr; ++r)
        regions.push_back(new (b) SmPolyRegion(b));
    std::vector<SmPolyShell*> shells;
    for (int64_t r = 0; r < nr; ++r)
    {
        int64_t ns = rd.idx();
        for (int64_t s = 0; s < ns; ++s)
            shells.push_back(new (b) SmPolyShell(regions[r]));
    }

    std::vector<SmPolyVertex*> vertices;
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; ++i)
    {
        SmVector3d p = rd.vec();
        double t = rd.num();
        SmPolyVertex* v = new (b) SmPolyVertex(p, t);
        b->m_pPolyVertexListHead->PostInsert(v);
        vertices.push_back(v);
    }

    std::vector<SmPolyEdge*> edges;
    int64_t nf = rd.idx();
    for (int64_t i = 0; i < nf; ++i)
    {
        int64_t shell = rd.idx();
        double ftol = rd.num();
        bool has_n = rd.num() != 0.0;
        SmVector3d n = rd.vec();
        SmPolyFace* f = new (b) SmPolyFace(ftol, NULL, NULL, shells[shell], has_n ? &n : NULL);
        int64_t nl = rd.idx();
        for (int64_t l = 0; l < nl; ++l)
        {
            int64_t mode = rd.idx();
            double etol = rd.num();
            int64_t k = rd.idx();
            std::vector<SmPolyVertex*> vs;
            for (int64_t j = 0; j < k; ++j)
                vs.push_back(vertices[rd.idx()]);
            SmPolyLoop* lp = NULL;
            f->StartPolyEdgeLoop(lp);
            int64_t ne = mode == 2 ? k - 1 : k;
            for (int64_t j = 0; j < ne; ++j)
            {
                SmPolyVertex* v0 = vs[j];
                SmPolyVertex* v1 = vs[(j + 1) % k];
                SmPolyEdge* e = NULL;
                lp->AddPolyEdge(etol, v0->GetPoint(), v1->GetPoint(), v0, NULL, b, e);
                edges.push_back(e);
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
            es.push_back(edges[rd.idx()]);
        for (int64_t j = 0; j < k; ++j)
        {
            es[j]->m_pNextRadialE = es[(j + 1) % k];
            es[j]->m_pLastRadialE = es[(j + k - 1) % k];
        }
    }

    std::vector<SmPolyVertAuxData*> aux;
    int64_t na = rd.idx();
    for (int64_t i = 0; i < na; ++i)
    {
        SmPolyVertAuxData* a = new (ctx) SmPolyVertAuxData();
        a->m_vNormal = rd.vec();
        double u = rd.num();
        double v = rd.num();
        a->m_vUV.x = u;
        a->m_vUV.y = v;
        aux.push_back(a);
    }
    int64_t nea = rd.idx();
    for (int64_t i = 0; i < nea; ++i)
    {
        int64_t e = rd.idx();
        int64_t a = rd.idx();
        edges[e]->SetAuxData(aux[a]);
    }

    int64_t nat = rd.idx();
    for (int64_t i = 0; i < nat; ++i)
    {
        ULONG id = (ULONG)rd.idx();
        long value = (long)rd.idx();
        SmAttributeBehaviorType beh = (SmAttributeBehaviorType)rd.idx();
        SmAttribute* a = new (ctx) SmLongAttribute(id, value, beh);
        attrs.push_back(a);
        b->AddAttribute(a);
    }
    return b;
}

void dump(SmPolyBrep* b, const std::vector<SmAttribute*>& src_attrs, std::vector<double>& o)
{
    o.push_back(b->GetTolerance());

    std::vector<SmPolyEdge*> edges;
    SmTArray<SmPolyRegion*> regs;
    b->GetPolyRegions(regs);
    o.push_back((double)regs.GetSize());
    for (ULONG r = 0; r < regs.GetSize(); ++r)
    {
        SmTArray<SmPolyShell*> sh;
        regs[r]->GetPolyShells(sh);
        o.push_back((double)sh.GetSize());
        for (ULONG s = 0; s < sh.GetSize(); ++s)
        {
            SmTArray<SmPolyFace*> fs;
            sh[s]->GetPolyFaces(fs);
            o.push_back((double)fs.GetSize());
            for (ULONG f = 0; f < fs.GetSize(); ++f)
            {
                SmPolyFace* face = fs[f];
                o.push_back(face->GetTolerance());
                put(o, face->m_vNormal);
                o.push_back(face->m_dMaxDeviation);
                SmTArray<SmPolyLoop*> ls;
                face->GetPolyLoops(ls);
                o.push_back((double)ls.GetSize());
                for (ULONG l = 0; l < ls.GetSize(); ++l)
                {
                    SmTArray<SmPolyEdge*> es;
                    ls[l]->GetPolyEdges(es);
                    o.push_back((double)ls[l]->GetOrientation());
                    o.push_back(ls[l]->m_bClosed ? 1.0 : 0.0);
                    o.push_back((double)es.GetSize());
                    for (ULONG e = 0; e < es.GetSize(); ++e)
                        edges.push_back(es[e]);
                }
            }
        }
    }

    SmTArray<SmPolyVertex*> vl;
    b->GetPolyVertices(vl);
    std::vector<SmPolyVertex*> verts;
    for (ULONG i = 0; i < vl.GetSize(); ++i)
        verts.push_back(vl[i]);

    o.push_back((double)edges.size());
    for (SmPolyEdge* e : edges)
    {
        o.push_back(e->GetTolerance());
        o.push_back(ix(verts, e->m_pPolyVertex));
        o.push_back(ix(edges, e->m_pNextRadialE));
        o.push_back(ix(edges, e->m_pLastRadialE));
        o.push_back(ix(edges, e->m_pNextEdgeAtV));
        o.push_back(ix(edges, e->m_pPrevEdgeAtV));
        o.push_back(e->m_pAuxData != NULL ? 1.0 : 0.0);
    }

    o.push_back((double)verts.size());
    for (SmPolyVertex* v : verts)
    {
        put(o, v->GetPoint());
        o.push_back(v->GetTolerance());
        o.push_back(ix(edges, v->m_pPolyEdgeList));
    }

    o.push_back(b->m_bOKBackPtrs ? 1.0 : 0.0);
    SmTArray<SmCPolyFace*> cp;
    b->GetCPolyFaces(cp);
    o.push_back((double)cp.GetSize());

    SmTArray<SmAttribute*> at;
    b->GetAttributes(at);
    o.push_back((double)at.GetSize());
    for (ULONG i = 0; i < at.GetSize(); ++i)
    {
        o.push_back((double)at[i]->GetAttributeID());
        o.push_back((double)at[i]->GetBehavior());
        o.push_back(ix(src_attrs, at[i]));
    }
}
} // namespace

SC_EXPORT int32_t sc_query_poly_copy_eval(const double* input, int64_t n, double* out, int64_t cap,
                                          int64_t* out_len)
{
    SC_TRY
    (void)n;
    SmContext ctx;
    Rd rd{input, 0};
    std::vector<SmAttribute*> attrs;
    SmPolyBrep* src = build(rd, ctx, attrs);

    SmPolyBrep* copy = NULL;
    long s = SmApiPolyBrepCopy(src, copy);
    std::vector<double> o;
    o.push_back(s == SM_SUCCESS ? 0.0 : (double)s);
    if (s == SM_SUCCESS && copy != NULL)
    {
        dump(copy, attrs, o);
        SmTArray<SmPolyFace*> fs;
        src->GetPolyFaces(fs);
        o.push_back((double)fs.GetSize());
        for (ULONG i = 0; i < fs.GetSize(); ++i)
        {
            put(o, fs[i]->m_vNormal);
            o.push_back(fs[i]->m_dMaxDeviation);
        }
        delete copy;
    }
    delete src;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
