// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.stitch_p2`: SmStitch::DoStitching, StitchIntoSolid,
// StitchIntoShell, the one-list StitchVertexPairs and SplitEdgesWithVertices
// (source/SMLib/src/SmStitch.cpp). All five are public members of the library's SmStitch.
//
// One entry point, sc_heal_stitch_p2_eval(op, path, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/heal_stitch_p2.rs, which validates
// every input. Every op starts with the B-rep block of unit heal.stitch_p1 (`0` + the
// B-rep block of sc_edgeuse_graph.h, `1 n (surface u0 v0 u1 v1)*n` unstitched sheets, or
// `2 P[3] l w h` SmApiCreateBox); indices refer to the export (OP_EXPORT, the layout of unit
// topology.brep_p1's Export: its extras, then the graph shared with topology.edgeuse_p1).
// The build, walk and export are copied from heal_stitch_p1.cpp.
#include "sc_shim.h"
#include "sc_edgeuse_graph.h"

#include <SmApiPrimitives.h>
#include <SmContext.h>
#include <SmExtent3d.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmStitch.h>

#include <cmath>

namespace
{
// "upstream would dereference null / free twice": the shim did not call it
const double ST_NULL_DEREF = -2.0;
// "upstream would never return": the shim did not call it
const double ST_NO_RETURN = -3.0;

enum Op
{
    OP_EXPORT = 0,
    OP_DO = 1,
    OP_SOLID = 2,
    OP_SHELL = 3,
    OP_VLIST = 4,
    OP_SPLIT = 5,
};

// ---- the B-rep of heal.stitch_p1 and its export (copied from heal_stitch_p1.cpp) ---------

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

// The extras of unit topology.brep_p1's Export: `inf_region editing_enabled
// size_estimate ratio head_is_infinite`, `n_v` and each vertex's vertex use list, then the
// B-rep's face, edge and vertex lists.
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

// `n idx*n` vertex indices, `-1` null.
void read_vertices(Rd& rd, Graph& g, SmTArray<SmVertex*>& out)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t k = rd.idx();
        out.Add(k < 0 ? (SmVertex*)NULL : g.vertices[(size_t)k]);
    }
}

// `n idx*n` edge indices, `-1` null.
void read_edges(Rd& rd, Graph& g, SmTArray<SmEdge*>& out)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t k = rd.idx();
        out.Add(k < 0 ? (SmEdge*)NULL : g.edges[(size_t)k]);
    }
}

// `n idx*n`: traversal indices of the original B-rep, `-1` null, `-3` an entity the op
// created.
template <class T> void put_indices(std::vector<double>& o, Graph& g, int kind, const SmTArray<T*>& arr)
{
    o.push_back((double)arr.GetSize());
    for (ULONG i = 0; i < arr.GetSize(); i++)
        o.push_back(arr[i] == NULL ? -1.0 : (double)g.find(kind, arr[i]));
}

// The B-rep after a mutating op: face, edge and vertex counts (GetFaces / GetEdges /
// GetVertices), then each vertex's point and tolerance in list order.
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
}

// The stitch outputs StitchIntoSolid / StitchIntoShell share.
void put_counts(std::vector<double>& o, ULONG n, double mv, double me) { o.insert(o.end(), {(double)n, mv, me}); }

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
    case OP_DO:
    {
        double tol = rd.num();
        bool nesting = rd.flag(), ignore = rd.flag(), fast = rd.flag(), squeeze = rd.flag();
        bool split = rd.flag(), manifold = rd.flag(), slivers = rd.flag();
        bool want_v = rd.flag(), want_e = rd.flag();
        bool has_cv = rd.flag();
        SmTArray<SmVertex*> cv;
        read_vertices(rd, g, cv);
        bool has_ce = rd.flag();
        SmTArray<SmEdge*> ce;
        read_edges(rd, g, ce);
        SmStitchCallback cb;
        SmStitch stitch(cb, tol, nesting ? TRUE : FALSE);
        stitch.m_bIgnoreProblems = ignore ? TRUE : FALSE;
        stitch.m_bFastEdgeCompare = fast ? TRUE : FALSE;
        stitch.m_bSqueezeSmallEdges = squeeze ? TRUE : FALSE;
        stitch.m_bSplitEdgesWithVertices = split ? TRUE : FALSE;
        stitch.m_bMakingManifoldSolid = manifold ? TRUE : FALSE;
        stitch.m_bRemoveLaminarSlivers = slivers ? TRUE : FALSE;
        ULONG n = 0, lam = 0;
        double mv = 0.0, me = 0.0, minv = 0.0, mine = 0.0;
        SmStatus s = stitch.DoStitching(brep, has_cv ? &cv : NULL, has_ce ? &ce : NULL, n, lam, mv, me,
                                        want_v ? &minv : NULL, want_e ? &mine : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.insert(o.end(), {(double)n, (double)lam, mv, me});
            if (want_v)
                o.push_back(minv);
            if (want_e)
                o.push_back(mine);
            put_digest(brep, o);
        }
        break;
    }
    case OP_SOLID:
    {
        // the doubling from a tenth of the B-rep tolerance never ends from zero or a
        // negative value (HANG-24); a NaN tolerance or diagonal skips the loop and an
        // infinite diagonal ends it once the tolerance doubles to infinity
        SmExtent3d box;
        if (brep->CalculateBoundingBox(box, TRUE) == SM_SUCCESS)
        {
            double dist = box.GetSize().Length();
            double tol = brep->GetTolerance() / 10.0;
            if (tol < dist / 200.0 && !(tol > 0.0))
            {
                o.push_back(ST_NO_RETURN);
                return;
            }
        }
        SmBoolean solid = FALSE;
        ULONG n = 0;
        double mv = 0.0, me = 0.0;
        SmStatus s = SmStitch::StitchIntoSolid(brep, solid, n, mv, me);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(solid));
            put_counts(o, n, mv, me);
            put_digest(brep, o);
        }
        break;
    }
    case OP_SHELL:
    {
        SmBoolean well = rd.bflag();
        double max_tol = rd.num();
        // the doubling from max_tol / 100 never ends when that underflows to zero (HANG-33)
        if (max_tol / 100.0 == 0.0 && 0.0 < max_tol)
        {
            o.push_back(ST_NO_RETURN);
            return;
        }
        ULONG n = 0;
        double mv = 0.0, me = 0.0;
        SmStatus s = SmStitch::StitchIntoShell(brep, well, max_tol, n, mv, me);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_counts(o, n, mv, me);
            put_digest(brep, o);
        }
        break;
    }
    case OP_VLIST:
    {
        double tol = rd.num();
        bool manifold = rd.flag(), squeeze = rd.flag();
        SmTArray<SmVertex*> list, surv, del;
        SmTArray<SmEdge*> dele;
        read_vertices(rd, g, list);
        SmStitchCallback cb;
        SmStitch stitch(cb, tol);
        stitch.m_bMakingManifoldSolid = manifold ? TRUE : FALSE;
        stitch.m_bSqueezeSmallEdges = squeeze ? TRUE : FALSE;
        double max_gap = 0.0, min_gap = 0.0;
        SmStatus s = stitch.StitchVertexPairs(list, surv, del, dele, max_gap, min_gap);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(max_gap);
            o.push_back(min_gap);
            put_indices(o, g, 9, surv);
            put_indices(o, g, 9, del);
            put_indices(o, g, 7, dele);
            put_indices(o, g, 9, list);
            if (del.GetSize() > 0)
                put_digest(brep, o);
        }
        break;
    }
    case OP_SPLIT:
    {
        double tol = rd.num();
        bool manifold = rd.flag(), ignore = rd.flag();
        SmTArray<SmVertex*> verts, proc;
        SmTArray<SmEdge*> edges, split, made;
        read_vertices(rd, g, verts);
        read_edges(rd, g, edges);
        SmStitchCallback cb;
        SmStitch stitch(cb, tol);
        stitch.m_bMakingManifoldSolid = manifold ? TRUE : FALSE;
        stitch.m_bIgnoreProblems = ignore ? TRUE : FALSE;
        double max_gap = 0.0, min_gap = 0.0;
        SmStatus s = stitch.SplitEdgesWithVertices(verts, edges, proc, split, made, max_gap, min_gap);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(max_gap);
            o.push_back(min_gap);
            put_indices(o, g, 9, proc);
            put_indices(o, g, 7, split);
            put_indices(o, g, 7, made);
            put_indices(o, g, 9, verts);
            put_indices(o, g, 7, edges);
            if (proc.GetSize() > 0 || made.GetSize() > 0)
                put_digest(brep, o);
        }
        break;
    }
    default: o.push_back(-4.0); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_heal_stitch_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
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
