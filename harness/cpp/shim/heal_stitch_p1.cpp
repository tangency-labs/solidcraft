// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.stitch_p1`: the SmStitch / SmStitchCallback members owned
// by the unit (source/SMLib/src/SmStitch.cpp), its file-static helpers sm_CheckInteriorEdge
// and sm_TestAndSplitEdge, and the stitch entry points of source/SM_API/src/SmApiBrep.cpp.
//
// SmStitch.cpp is compiled into this shim with its classes renamed (sc_st_Stitch,
// sc_st_Callback), as harness/cpp/shim/boolean_merge_p1.cpp does for SmMerge.cpp, so the
// static helpers can be called; no algorithm is reimplemented. The SM_API entry points
// are the library's.
//
// One entry point, sc_heal_stitch_p1_eval(op, path, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/heal_stitch_p1.rs, which validates
// every input. Every op starts with a B-rep block: `0` + the B-rep block of
// sc_edgeuse_graph.h (file, wire edge, face, wire network), `1 n (surface u0 v0 u1 v1)*n`
// one face per surface in one SmBrep (unstitched sheets), or `2 P[3] l w h`
// SmApiCreateBox. The export (OP_EXPORT) has the layout of unit topology.brep_p1's
// Export (its extras, then the graph export shared with topology.edgeuse_p1).
#include "sc_shim.h"

// The renames must be active when SmStitch.cpp includes StdAfx.h (SmSmlibAll.h declares
// the classes), so nothing upstream is included before them. SmSmlibAll.h also brings in
// SmTopologySweep.h, whose SmSweepStitchCallback is thereby rebased on sc_st_Callback in
// this translation unit; the shim never uses that class.
#define SmStitch sc_st_Stitch
#define SmStitchCallback sc_st_Callback
#include "../../../upstream/source/SMLib/src/SmStitch.cpp"
#undef SmStitch
#undef SmStitchCallback

#include "sc_edgeuse_graph.h"
#include "sc_stitch_digest.h"

#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmContext.h>
#include <SmRegion.h>
#include <SmShell.h>

namespace
{
// "upstream would dereference null / free twice": the shim did not call it
const double ST_NULL_DEREF = -2.0;

const int64_t TY_FACE = 16002;
const int64_t TY_EDGE = 16016;
const int64_t TY_VERTEX = 16021;

enum Op
{
    OP_EXPORT = 0,
    OP_SELECT = 1,
    OP_INTERIOR = 2,
    OP_SPLIT = 3,
    OP_VPAIRS = 4,
    OP_EDGES = 5,
    OP_SIMPLE = 6,
    OP_UNIFY = 7,
    OP_API = 8,
};

// ---- the topology.brep_p1 export extras (copied from topology_brep_p1.cpp) ----------------

struct BrPeek : SmBrep
{
    static SmTopology* SmBrep::*list() { return &BrPeek::m_pList; }
    typedef double (SmBrep::*GetD)(SmBoolean) const;
    static GetD get_est() { return &BrPeek::GetThisModelSizeEstimate; }
    static GetD get_ratio() { return &BrPeek::GetThisLargeSmallSizeRatio; }
};
double brep_est(const SmBrep* b, SmBoolean chase) { return (b->*BrPeek::get_est())(chase); }
double brep_ratio(const SmBrep* b, SmBoolean chase) { return (b->*BrPeek::get_ratio())(chase); }

template <class T> void put_found(std::vector<double>& o, Graph& g, int kind, const SmTArray<T*>& arr)
{
    std::vector<int64_t> v;
    for (ULONG i = 0; i < arr.GetSize(); i++)
        v.push_back(g.find(kind, arr[i]));
    put_list(o, v);
}

// `inf_region editing_enabled size_estimate ratio head_is_infinite`, `n_v` and each
// vertex's vertex use list, then the B-rep's face, edge and vertex lists.
void put_extras(SmBrep* brep, Graph& g, std::vector<double>& o)
{
    o.push_back((double)g.find(0, brep->GetInfiniteRegion()));
    o.push_back(flg(brep->m_bEditingEnabled));
    o.push_back(brep_est(brep, FALSE));
    o.push_back(brep_ratio(brep, FALSE));
    o.push_back(brep->*BrPeek::list() == (SmTopology*)brep->GetInfiniteRegion() ? 1.0 : 0.0);
    o.push_back((double)g.vertices.size());
    for (SmVertex* v : g.vertices)
    {
        std::vector<int64_t> vus;
        for (SmTopology* t : list_of(v->GetList()))
            vus.push_back(g.find(8, t));
        put_list(o, vus);
    }
    SmTArray<SmFace*> fs;
    brep->GetFaces(fs);
    put_found(o, g, 3, fs);
    SmTArray<SmEdge*> es;
    brep->GetEdges(es);
    put_found(o, g, 7, es);
    SmTArray<SmVertex*> vs;
    brep->GetVertices(vs);
    put_found(o, g, 9, vs);
}

// Registers every member of wire shells and every vertex use of a vertex.
void walk_more(SmBrep* brep, Graph& g)
{
    for (;;)
    {
        size_t before = g.shells.size() + g.edgeuses.size() + g.vertexuses.size() + g.edges.size() + g.vertices.size();
        for (size_t s = 0; s < g.shells.size(); s++)
        {
            SmShell* sh = g.shells[s];
            SM_TYPE ty = sh->*ShPeek::type();
            if (ty != SmFaceuse_TYPE)
                for (SmTopology* t : list_of(sh->GetList()))
                    g.typed(ty, t);
        }
        for (size_t v = 0; v < g.vertices.size(); v++)
            for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                g.vertexuse((SmVertexuse*)t);
        walk(brep, g);
        size_t after = g.shells.size() + g.edgeuses.size() + g.vertexuses.size() + g.edges.size() + g.vertices.size();
        if (after == before)
            break;
    }
}

// ---- B-rep blocks ------------------------------------------------------------------------

SmBrep* build_block(Rd& rd, const char* path, SmStatus& status)
{
    int64_t mode = rd.idx();
    if (mode == 0)
        return build(rd, path, status);
    if (mode == 2)
    {
        SmVector3d p = rd.vec();
        double l = rd.num(), w = rd.num(), h = rd.num();
        SmBrep* box = NULL;
        status = SmApiCreateBox(p, l, w, h, box);
        if (status != SM_SUCCESS || box == NULL)
        {
            if (status == SM_SUCCESS)
                status = SM_ERR;
            delete box;
            return nullptr;
        }
        return box;
    }
    const SmContext& ctx = the_context();
    int64_t n = rd.idx();
    SmBrep* brep = new (ctx) SmBrep();
    status = SM_SUCCESS;
    for (int64_t i = 0; i < n; i++)
    {
        SmSurface* s = read_surface(rd, ctx);
        SmPoint2d a = rd.uv(), b = rd.uv();
        if (status != SM_SUCCESS)
        {
            delete s;
            continue;
        }
        SmFace* pFace = nullptr;
        status = brep->CreateFaceFromSurface(s, SmExtent2d(a, b), pFace);
    }
    if (status != SM_SUCCESS)
    {
        delete brep;
        return nullptr;
    }
    return brep;
}

// ---- helpers -----------------------------------------------------------------------------

SmTopology* topo(Graph& g, int64_t ty, int64_t i)
{
    if (i < 0)
        return nullptr;
    size_t k = (size_t)i;
    if (ty == TY_VERTEX)
        return k < g.vertices.size() ? (SmTopology*)g.vertices[k] : nullptr;
    if (ty == TY_EDGE)
        return k < g.edges.size() ? (SmTopology*)g.edges[k] : nullptr;
    if (ty == TY_FACE)
        return k < g.faces.size() ? (SmTopology*)g.faces[k] : nullptr;
    return nullptr;
}

void read_vertices(Rd& rd, Graph& g, SmTArray<SmVertex*>& out)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
        out.Add(g.vertices[(size_t)rd.idx()]);
}

void read_faces(Rd& rd, Graph& g, SmTArray<SmFace*>& out)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
        out.Add(g.faces[(size_t)rd.idx()]);
}

template <class T> void put_indices(std::vector<double>& o, Graph& g, int kind, const SmTArray<T*>& arr)
{
    o.push_back((double)arr.GetSize());
    for (ULONG i = 0; i < arr.GetSize(); i++)
        o.push_back(arr[i] == NULL ? -1.0 : (double)g.find(kind, arr[i]));
}

// The B-rep after a mutating op: face, edge and vertex counts (GetFaces / GetEdges /
// GetVertices), each vertex's point and tolerance in list order, then the topology of
// sc_stitch_digest.h.
void put_digest(SmBrep* brep, std::vector<double>& o)
{
    SmTArray<SmFace*> fs;
    brep->GetFaces(fs);
    SmTArray<SmEdge*> es;
    brep->GetEdges(es);
    SmTArray<SmVertex*> vs;
    brep->GetVertices(vs);
    o.push_back((double)fs.GetSize());
    o.push_back((double)es.GetSize());
    o.push_back((double)vs.GetSize());
    for (ULONG i = 0; i < vs.GetSize(); i++)
    {
        put_vec(o, vs[i]->GetPoint());
        o.push_back((double)vs[i]->GetTolerance());
    }
    put_topology(brep, o);
}

// DoSimpleFaceStitch's validation, read-only: a VERBATIM copy of SmStitch.cpp:625-780 (commit
// c9c5979) with every `return (SM_ERR)` turned into `return false`; true when the call would
// reach RemoveFaces, with the vertex pairs it found. `dDistToCurve` starts at 0 here, where
// upstream reads it uninitialised after a failed CurveMaxDistanceBetween (`SE`).
bool simple_face_validates(SmBrep* pBrepToStitch, const SmTArray<SmFace*>& cpFacesToKeep,
                           const SmTArray<SmFace*>& cpFacesToDelete, double m_dStitchTol3d,
                           SmTArray<SmVertex*>& sKVertices, SmTArray<SmVertex*>& sDVertices)
{
    ULONG nFPairs = cpFacesToKeep.GetSize();
    if (nFPairs == 0 || nFPairs != cpFacesToDelete.GetSize())
        return false;
    SmBrep* pBrep0 = (cpFacesToKeep[0]->GetBrep());
    if (!pBrep0 || pBrep0 != pBrepToStitch)
        return false;
    SmBrep* pBrep1 = (cpFacesToDelete[0]->GetBrep());
    if (!pBrep1 || pBrep1 != pBrepToStitch)
        return false;
    ULONG nVerts, nEdges;
    for (ULONG iPair = 0; iPair < nFPairs; iPair++)
    {
        SmFace* pFaceToKeep = cpFacesToKeep[iPair];
        SmFace* pFaceToDelete = cpFacesToDelete[iPair];
        SmTArray<SmVertex*> sKVerts;
        pFaceToKeep->GetVertices(sKVerts);
        SmTArray<SmVertex*> sDVerts;
        pFaceToDelete->GetVertices(sDVerts);
        nVerts = sKVerts.GetSize();
        if (nVerts != sDVerts.GetSize())
            return false;
        double Vtol = m_dStitchTol3d;
        ULONG nFound = 0;
        ULONG iK;
        for (iK = 0; iK < nVerts; iK++)
        {
            SmVertex* pKVertex = sKVerts[iK];
            SmPoint3d pKVpt = pKVertex->GetPoint();
            for (ULONG iD = 0; iD < nVerts; iD++)
            {
                SmVertex* pDVertex = sDVerts[iD];
                SmPoint3d pDVpt = pDVertex->GetPoint();
                double Vdist = pKVpt.DistanceBetween(pDVpt);
                if (Vdist < Vtol)
                {
                    sKVertices.Add(pKVertex);
                    sDVertices.Add(pDVertex);
                    nFound++;
                    iD = nVerts;
                }
            }
        }
        if (nFound != nVerts)
            return false;
        SmTArray<SmEdge*> sKEds;
        pFaceToKeep->GetEdges(sKEds);
        SmTArray<SmEdge*> sDEds;
        pFaceToDelete->GetEdges(sDEds);
        nEdges = sKEds.GetSize();
        if (nEdges != sDEds.GetSize() || nEdges != nVerts)
            return false;
        nFound = 0;
        for (iK = 0; iK < nEdges; iK++)
        {
            SmEdge* pKEdge = sKEds[iK];
            SmVertex* pKV0 = pKEdge->GetVertex();
            SmVertex* pKV1 = pKEdge->GetOtherVertex(pKV0);
            ULONG lIndex;
            if (!sKVertices.FindElement(pKV0, lIndex))
                return false;
            SmVertex* pDKV0 = sDVertices[lIndex];
            if (!sKVertices.FindElement(pKV1, lIndex))
                return false;
            SmVertex* pDKV1 = sDVertices[lIndex];
            SmCurve* pKCurve = pKEdge->GetCurve();
            for (ULONG iD = 0; iD < nEdges; iD++)
            {
                SmEdge* pDEdge = sDEds[iD];
                SmCurve* pDCurve = pDEdge->GetCurve();
                SmVertex* pDV0 = pDEdge->GetVertex();
                SmVertex* pDV1 = pDEdge->GetOtherVertex(pDV0);
                if ((pDKV0 == pDV0 && pDKV1 == pDV1) || (pDKV0 == pDV1 && pDKV1 == pDV0))
                {
                    nFound++;
                    double dKStart, dKEnd, dDStart, dDEnd;
                    double dDistToCurve = 0.0;
                    dKStart = pKEdge->GetInterval().GetMin();
                    dKEnd = pKEdge->GetInterval().GetMax();
                    dDStart = pDEdge->GetInterval().GetMin();
                    dDEnd = pDEdge->GetInterval().GetMax();
                    SmPoint3d sKpt0, sKpt1, sDpt0, sDpt1;
                    if (pKCurve->EvaluatePoint(dKStart, sKpt0) != SM_SUCCESS) return false;
                    if (pKCurve->EvaluatePoint(dKEnd, sKpt1) != SM_SUCCESS) return false;
                    if (pDCurve->EvaluatePoint(dDStart, sDpt0) != SM_SUCCESS) return false;
                    if (pDCurve->EvaluatePoint(dDEnd, sDpt1) != SM_SUCCESS) return false;
                    if (sKpt0.DistanceBetween(sDpt0) < Vtol && sKpt1.DistanceBetween(sDpt1) < Vtol)
                    {
                    }
                    else if (sKpt0.DistanceBetween(sDpt1) < Vtol && sKpt1.DistanceBetween(sDpt0) < Vtol)
                    {
                        double dTemp = dDStart;
                        dDStart = dDEnd;
                        dDEnd = dTemp;
                    }
                    else
                        return false;
                    double dTol = smos_Max(m_dStitchTol3d, (double)(pKEdge->GetTolerance() + pDEdge->GetTolerance()));
                    pKCurve->CurveMaxDistanceBetween(pKEdge->GetInterval(), *pDCurve, dDStart, dDEnd, 10, &dTol,
                                                     dDistToCurve);
                    if (dDistToCurve > dTol)
                        return false;
                    iD = nEdges;
                }
            }
        }
        if (nFound != nEdges)
            return false;
    }
    return true;
}

// UB-177: once the validation passes, RemoveFaces frees every vertex only the delete faces
// use and the glue loop then reads those freed vertices; a face repeated in the delete list
// is freed twice; a vertex in more than one entry of sKVertices / sDVertices (both lists
// are built with plain Add across the face pairs: a vertex shared by two keep or two delete
// faces, a repeated keep face, or a vertex shared by the two lists) is freed by the glue of
// its first entry (GlueVertices / SqueezeEdge delete the other vertex) and read by the next.
// Those inputs are refused (-2); everything else runs, so upstream's SM_ERR statuses are
// compared. A repeated vertex whose first entry frees nothing (squeeze skipped, a short
// manifold common edge, the m_bMakingManifoldSolid non-lamina skip, or the loop ending first
// with upstream's defined SM_ERR: dMaxDistance > dTol or a failed SelectTopologyToKeep) is
// refused too: a deliberate over-refusal of a rare input, because whether that entry frees
// it depends on the stateful glue loop itself.
bool simple_face_guard(SmBrep* brep, const SmTArray<SmFace*>& keep, const SmTArray<SmFace*>& del, double tol)
{
    SmTArray<SmVertex*> kv, dv;
    if (!simple_face_validates(brep, keep, del, tol, kv, dv))
        return true;
    for (ULONG i = 0; i < del.GetSize(); i++)
        for (ULONG j = 0; j < i; j++)
            if (del[i] == del[j])
                return false;
    SmTArray<SmVertex*> all;
    all.Append(kv);
    all.Append(dv);
    for (ULONG i = 0; i < all.GetSize(); i++)
        for (ULONG j = 0; j < i; j++)
            if (all[i] == all[j])
                return false;
    for (ULONG i = 0; i < dv.GetSize(); i++)
    {
        SmTArray<SmFace*> fs;
        dv[i]->GetFaces(fs);
        bool other = false;
        ULONG at;
        for (ULONG f = 0; f < fs.GetSize(); f++)
            if (!del.FindElement(fs[f], at))
                other = true;
        if (!other)
            return false;
    }
    return true;
}

// ---- ops ---------------------------------------------------------------------------------

void run_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build_block(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    SmBrep* brep = b.brep;
    o.push_back(0.0);
    Graph g;
    walk(brep, g);
    walk_more(brep, g);
    switch (op)
    {
    case OP_EXPORT:
    {
        put_extras(brep, g, o);
        export_graph(brep, g, o);
        break;
    }
    case OP_SELECT:
    {
        int64_t t1 = rd.idx(), i1 = rd.idx(), t2 = rd.idx(), i2 = rd.idx();
        double dist = rd.num();
        SmTopology* a = topo(g, t1, i1);
        SmTopology* c = topo(g, t2, i2);
        // the edge branch dereferences the surfaces of both edges' faces. The vertex branch
        // reaches the edge branch too (its recursive calls, SmStitch.cpp:157, :222), unguarded:
        // that is safe because no block of this harness builds a face without a surface. The
        // face kinds, the sheets and SmApiCreateBox always attach one, and none of the 15
        // upstream .smb files in topology.edgeuse_p1's FILES has a surface-less face (checked
        // over every face after ReadFromFile; SmallBox.smp and Platform.smp are excluded because
        // ReadFromFile itself crashes on them).
        if (t1 == TY_EDGE && t2 == TY_EDGE)
            for (SmFace* f : g.faces)
                if (f->GetSurface() == NULL)
                {
                    o.push_back(ST_NULL_DEREF);
                    return;
                }
        sc_st_Callback cb;
        double max_dist = 0.0;
        SmBoolean keep = TRUE;
        SmStatus s = cb.SelectTopologyToKeep(a, c, dist, max_dist, keep);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(keep));
            o.push_back(max_dist);
        }
        break;
    }
    case OP_INTERIOR:
    {
        SmEdge* e = g.edges[(size_t)rd.idx()];
        double t = rd.num(), tol = rd.num();
        SmBoolean interior = FALSE;
        SmStatus s = sm_CheckInteriorEdge(e, t, tol, interior);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(flg(interior));
        break;
    }
    case OP_SPLIT:
    {
        SmEdge* e = g.edges[(size_t)rd.idx()];
        SmVector3d p = rd.vec();
        double tol = rd.num();
        sReasonType reason = NO_REASON;
        double dist = 0.0;
        SmVertex* nv = NULL;
        SmEdge* ne = NULL;
        SmStatus s = sm_TestAndSplitEdge(e, p, tol, reason, dist, nv, ne);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)reason);
            o.push_back(dist);
            o.push_back(nv ? 1.0 : 0.0);
            if (nv)
                put_digest(brep, o);
        }
        break;
    }
    case OP_VPAIRS:
    {
        double tol = rd.num();
        bool ignore = rd.flag();
        SmTArray<SmVertex*> l1, l2, proc, del;
        read_vertices(rd, g, l1);
        read_vertices(rd, g, l2);
        sc_st_Callback cb;
        sc_st_Stitch stitch(cb, tol);
        stitch.m_bIgnoreProblems = ignore ? TRUE : FALSE;
        double max_gap = 0.0, min_gap = 0.0;
        SmStatus s = stitch.StitchVertexPairs(l1, l2, proc, del, max_gap, min_gap);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(max_gap);
            o.push_back(min_gap);
            put_indices(o, g, 9, proc);
            put_indices(o, g, 9, del);
            put_indices(o, g, 9, l1);
            put_indices(o, g, 9, l2);
            if (proc.GetSize() > 0)
                put_digest(brep, o);
        }
        break;
    }
    case OP_EDGES:
    {
        double tol = rd.num();
        bool manifold = rd.flag(), fast = rd.flag(), ignore = rd.flag(), nesting = rd.flag();
        SmTArray<SmVertex*> verts, procv;
        read_vertices(rd, g, verts);
        sc_st_Callback cb;
        sc_st_Stitch stitch(cb, tol, nesting ? TRUE : FALSE);
        stitch.m_bMakingManifoldSolid = manifold ? TRUE : FALSE;
        stitch.m_bFastEdgeCompare = fast ? TRUE : FALSE;
        stitch.m_bIgnoreProblems = ignore ? TRUE : FALSE;
        SmTArray<SmEdge*> glued, deleted;
        double max_gap = 0.0, min_gap = 0.0;
        SmStatus s = stitch.StitchEdgesOfVertices(verts, glued, deleted, procv, max_gap, min_gap);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(max_gap);
            o.push_back(min_gap);
            put_indices(o, g, 7, glued);
            put_indices(o, g, 7, deleted);
            if (glued.GetSize() > 0)
                put_digest(brep, o);
        }
        break;
    }
    case OP_SIMPLE:
    {
        bool api = rd.flag();
        double tol = rd.num();
        SmTArray<SmFace*> keep, del;
        read_faces(rd, g, keep);
        read_faces(rd, g, del);
        // the API variant stitches with the B-rep tolerance
        if (!simple_face_guard(brep, keep, del, api ? (double)brep->GetTolerance() : tol))
        {
            o.push_back(ST_NULL_DEREF);
            return;
        }
        double max_v = 0.0, max_e = 0.0;
        ULONG n = 0;
        SmStatus s;
        if (api)
            s = SmApiSimpleFaceStitch(brep, keep, del, n, max_v, max_e);
        else
        {
            sc_st_Callback cb;
            sc_st_Stitch stitch(cb, tol);
            s = stitch.DoSimpleFaceStitch(brep, keep, del, max_v, max_e, n);
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(max_v);
            o.push_back(max_e);
            o.push_back((double)n);
            put_digest(brep, o);
        }
        break;
    }
    case OP_UNIFY:
    {
        bool api = rd.flag();
        double tol = rd.num();
        SmFace* start = g.faces[(size_t)rd.idx()];
        ULONG n = (ULONG)rd.idx();
        SmTArray<SmFace*> flipped;
        SmStatus s;
        if (api)
            s = SmApiUnifyNormals(brep, start, n, flipped);
        else
        {
            sc_st_Callback cb;
            sc_st_Stitch stitch(cb, tol);
            s = stitch.UnifyNormals(brep, start, n, flipped);
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_indices(o, g, 3, flipped);
            for (ULONG i = 0; i < flipped.GetSize(); i++)
            {
                SmExtent2d d = flipped[i]->GetUVDomain();
                o.push_back(d.GetMin().x);
                o.push_back(d.GetMin().y);
                o.push_back(d.GetMax().x);
                o.push_back(d.GetMax().y);
            }
            put_digest(brep, o);
        }
        break;
    }
    case OP_API:
    {
        int64_t which = rd.idx();
        bool null_brep = rd.flag();
        SmBrep* p = null_brep ? NULL : brep;
        switch (which)
        {
        case 0:
        {
            SmStatus s = SmApiStitch(p);
            o.push_back(st(s));
            if (s == SM_SUCCESS && p != NULL)
                put_digest(brep, o);
            break;
        }
        case 1:
        {
            int64_t f = rd.idx() - 1;
            SmFace* face = f < 0 ? NULL : g.faces[(size_t)f];
            SmStatus s = SmApiStitch(p, face);
            o.push_back(st(s));
            if (s == SM_SUCCESS && p != NULL)
                put_digest(brep, o);
            break;
        }
        case 2:
        {
            SmBoolean solid = FALSE;
            ULONG n = 0;
            double mv = 0.0, me = 0.0;
            SmStatus s = SmApiStitchIntoSolid(p, solid, n, mv, me);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(flg(solid));
                o.push_back((double)n);
                o.push_back(mv);
                o.push_back(me);
                put_digest(brep, o);
            }
            break;
        }
        case 3:
        {
            SmBoolean well = rd.bflag();
            double ratio = rd.num();
            ULONG n = 0;
            double mv = 0.0, me = 0.0;
            SmStatus s = SmApiStitchIntoShell(p, well, ratio, n, mv, me);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back((double)n);
                o.push_back(mv);
                o.push_back(me);
                put_digest(brep, o);
            }
            break;
        }
        case 4:
        {
            int64_t f = rd.idx() - 1;
            ULONG n = (ULONG)rd.idx();
            SmFace* face = f < 0 ? NULL : g.faces[(size_t)f];
            SmTArray<SmFace*> flipped;
            SmStatus s = SmApiUnifyNormals(p, face, n, flipped);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                put_indices(o, g, 3, flipped);
                put_digest(brep, o);
            }
            break;
        }
        case 5:
        {
            SmTArray<SmFace*> keep, del;
            ULONG n = 0;
            double mv = 0.0, me = 0.0;
            SmStatus s = SmApiSimpleFaceStitch(p, keep, del, n, mv, me);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                put_digest(brep, o);
            break;
        }
        default:
        {
            double tol = rd.num();
            SmBoolean sq = rd.bflag(), split = rd.bflag(), manifold = rd.bflag(), slivers = rd.bflag();
            ULONG n = 0, lam = 0;
            double mv = 0.0, me = 0.0;
            SmStatus s = SmApiStitchAdvanced(p, tol, sq, split, manifold, slivers, n, lam, mv, me);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back((double)n);
                o.push_back((double)lam);
                o.push_back(mv);
                o.push_back(me);
                put_digest(brep, o);
            }
            break;
        }
        }
        break;
    }
    default: o.push_back(-4.0); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_heal_stitch_p1_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
