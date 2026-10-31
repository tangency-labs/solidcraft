// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p7`: the SmBrep members owned by the unit
// (source/SMLib/src/SmBrep.cpp): ReadFromFile, MakeFace, MakeFaceWithCurves,
// GlueEdgesGeneral, CopyFaces, MergeSelfIntersectionCurves.
//
// One entry point, sc_topology_brep_p7_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_brep_p7.rs, which
// validates every input. Curve and surface blocks are those of sc_topology_fixture.h
// (read_curve, read_surface); B-rep ops take their arguments, then that header's B-rep
// block (unit topology.vertexuse). Output slots: `-2` "upstream would dereference null" (not
// called), `-3` "not run": a call the shim keeps away from upstream because its input is
// not one upstream handles safely and the Rust side defers that path anyway (see each
// op).
#include "sc_topology_fixture.h"

#include <SmLine.h>

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <unistd.h>

namespace
{
enum : int32_t
{
    OP_READ = 1,
    OP_MAKE_FACE_WITH_CURVES = 2,
    OP_MAKE_FACE = 3,
    OP_GLUE = 4,
    OP_COPY = 5,
    OP_MERGE_SELF = 6,
};

const double P7_NULL_DEREF = -2.0;
const double P7_NOT_RUN = -3.0;

void p7_put_counts(std::vector<double>& o, SmBrep* b)
{
    Graph g;
    walk(b, g);
    o.push_back((double)g.regions.size());
    o.push_back((double)g.shells.size());
    o.push_back((double)g.faces.size());
    o.push_back((double)g.loops.size());
    o.push_back((double)g.edges.size());
    o.push_back((double)g.vertices.size());
}

// The B-rep's edge tolerances, then its vertex tolerances, each sorted ascending (a fresh
// walk: the op may have deleted edges and vertices).
void p7_put_tolerances(std::vector<double>& o, SmBrep* b)
{
    Graph g;
    walk(b, g);
    std::vector<double> te, tv;
    for (SmEdge* e : g.edges)
        te.push_back((double)e->GetTolerance());
    for (SmVertex* v : g.vertices)
        tv.push_back((double)v->GetTolerance());
    std::sort(te.begin(), te.end());
    std::sort(tv.begin(), tv.end());
    o.insert(o.end(), te.begin(), te.end());
    o.insert(o.end(), tv.begin(), tv.end());
}

// `n byte*n file_type rebuild`: the bytes go to a temporary file read by
// SmBrep::ReadFromFile into a new B-rep. Output `status [counts]`.
void p7_read(Rd& rd, std::vector<double>& o)
{
    int64_t n = rd.idx();
    std::vector<char> bytes;
    for (int64_t i = 0; i < n; i++)
        bytes.push_back((char)(unsigned char)rd.idx());
    SmFileType ft = rd.idx() == 0 ? SM_ASCII : SM_BINARY;
    SmBoolean rebuild = rd.bflag();
    char name[] = "/tmp/sc_brep_p7_XXXXXX";
    int fd = mkstemp(name);
    if (fd < 0)
    {
        o.push_back((double)SC_ERR_ARG);
        return;
    }
    if (!bytes.empty() && write(fd, bytes.data(), bytes.size()) != (ssize_t)bytes.size())
    {
        close(fd);
        unlink(name);
        o.push_back((double)SC_ERR_ARG);
        return;
    }
    close(fd);
    const SmContext& ctx = the_context();
    Built built;
    built.brep = new (ctx) SmBrep();
    SmStatus s = built.brep->ReadFromFile(ctx, name, ft, rebuild);
    unlink(name);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
        p7_put_counts(o, built.brep);
}

// `surface_block uv(4) orient region_null crv_on_surf(0 null,1 FALSE,2 TRUE) nloops
// counts* has3d has_uv ncurves (curve_block orient)* npts pt(3)*` (the 3D or UV curve
// set; when both, 3D blocks then UV blocks). Output `status [new_face new_shell
// new_region counts]`.
void p7_make_face_with_curves(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* srf = read_surface(rd, ctx);
    SmPoint2d a = rd.uv(), b2 = rd.uv();
    SmOrientType orient = (SmOrientType)rd.idx();
    bool region_null = rd.flag();
    int64_t cos = rd.idx();
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
            c3.Add(read_curve(rd, ctx));
        if (hasuv)
            cuv.Add(read_curve(rd, ctx));
        orients.Add((SmOrientType)rd.idx());
    }
    int64_t np = rd.idx();
    SmTArray<SmPoint3d> pts;
    for (int64_t i = 0; i < np; i++)
        pts.Add(rd.vec());
    // pbOptCrvOnSurf TRUE without UV curves: once MakeFace succeeds upstream loops over the
    // null UV curve array (SmBrep.cpp:16600) and crashes; with 3D curves given it gets
    // that far. Its own input checks (SmBrep.cpp:15722-15741: no loops, a degenerate
    // surface) return first and run. Still over-wide for inputs upstream rejects later,
    // before MakeFace. The runner rejects the same inputs (UB-165) before calling here.
    bool input_error = loops.GetSize() == 0 || srf->IsDegenerateCurve() || srf->IsDegeneratePoint();
    if (cos == 2 && !hasuv && has3d && !input_error)
    {
        o.push_back(P7_NULL_DEREF);
        return;
    }
    Built built;
    built.brep = new (ctx) SmBrep();
    SmRegion* region = region_null ? NULL : built.brep->GetInfiniteRegion();
    SmBoolean flag = cos == 2 ? TRUE : FALSE;
    SmRegion* nr = NULL;
    SmShell* ns = NULL;
    SmFace* nf = NULL;
    SmStatus s = built.brep->MakeFaceWithCurves(region, loops, has3d ? &c3 : NULL, hasuv ? &cuv : NULL, orients,
                                                pts, srf, SmExtent2d(a, b2), orient, nr, ns, nf,
                                                cos == 0 ? NULL : &flag);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back(flg(nf != NULL));
        o.push_back(flg(ns != NULL));
        o.push_back(flg(nr != NULL));
        p7_put_counts(o, built.brep);
    }
}

// `kind`: 0 a null surface, 1 a square wire loop (side `s`) in the infinite region on a
// plane `z = 0` (`s orient region_null`). Output `status [new_face new_shell new_region
// counts]`.
void p7_make_face(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t kind = rd.idx();
    double side = rd.num();
    SmOrientType orient = (SmOrientType)rd.idx();
    bool region_null = rd.flag();
    Built built;
    built.brep = new (ctx) SmBrep();
    SmBrep* b = built.brep;
    SmTArray<ULONG> loops;
    SmTArray<SmEdgeuse*> eus;
    SmTArray<SmVertex*> lvs;
    SmRegion* nr = NULL;
    SmShell* ns = NULL;
    SmFace* nf = NULL;
    if (kind == 0)
    {
        o.push_back(st(b->MakeFace(region_null ? NULL : b->GetInfiniteRegion(), loops, eus, lvs, NULL,
                                   SmExtent2d(SmPoint2d(0, 0), SmPoint2d(1, 1)), orient, nr, ns, nf)));
        return;
    }
    // the square's wire edges
    SmPoint3d p[4] = {SmPoint3d(0, 0, 0), SmPoint3d(side, 0, 0), SmPoint3d(side, side, 0), SmPoint3d(0, side, 0)};
    SmShell* sh = NULL;
    SmVertex* v0 = NULL;
    SmStatus s = b->MakeShellVertex(b->GetInfiniteRegion(), p[0], sh, v0);
    SmVertex* vs[4] = {v0, NULL, NULL, NULL};
    SmEdge* es[4] = {NULL, NULL, NULL, NULL};
    for (int i = 0; i < 3 && s == SM_SUCCESS; i++)
    {
        SmLine* l = NULL;
        SmLine::CreateLineSegment(ctx, 3, p[i], p[i + 1], l);
        if (l == NULL)
            break; // (a side too small for a line: not a face this op can build)
        s = b->MakeWireEdgeVertex(b->GetInfiniteRegion(), vs[i], l, l->GetNaturalInterval(), SM_OT_SAME, p[i + 1],
                                  es[i], vs[i + 1]);
    }
    bool lines_ok = es[0] && es[1] && es[2];
    if (s == SM_SUCCESS && lines_ok)
    {
        SmLine* l = NULL;
        SmLine::CreateLineSegment(ctx, 3, p[3], p[0], l);
        if (l == NULL)
        {
            o.push_back(P7_NOT_RUN);
            return;
        }
        s = b->MakeWireEdge(b->GetInfiniteRegion(), vs[3], vs[0], l, l->GetNaturalInterval(), SM_OT_SAME, es[3]);
    }
    if (!lines_ok)
    {
        // CreateLineSegment failed (the side's square underflows), or MakeShellVertex /
        // MakeWireEdgeVertex failed before the third side (not reached with the sides
        // used): the shim cannot build the loop; the Rust side reports `-3` for the first
        // and defers every square-loop case at MakeShellVertex
        o.push_back(P7_NOT_RUN);
        return;
    }
    if (s != SM_SUCCESS)
    {
        o.push_back((double)SC_ERR_ARG);
        return;
    }
    for (int i = 0; i < 4; i++)
    {
        SmTArray<SmEdgeuse*> all;
        es[i]->GetEdgeuses(all);
        for (ULONG j = 0; j < all.GetSize(); j++)
            if (all[j]->GetOrientation() == SM_OT_SAME)
            {
                eus.Add(all[j]);
                break;
            }
    }
    loops.Add(4);
    SmSurface* pl = new (ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(0, 0, 1), &ctx);
    s = b->MakeFace(region_null ? NULL : b->GetInfiniteRegion(), loops, eus, lvs, pl, pl->GetNaturalUVDomain(), orient,
                    nr, ns, nf);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back(flg(nf != NULL));
        o.push_back(flg(ns != NULL));
        o.push_back(flg(nr != NULL));
        p7_put_counts(o, b);
    }
}

// `e1 e2 orient gap nesting`, B-rep block (edge indices, `-1` null). Two distinct edges,
// the second not a wire, SM_OT_SAME, the same intervals and exactly one curve: `-2`
// (upstream dereferences the null curve in CurveMaxDistanceBetween, SmBrep.cpp:9875-9896;
// two null curves compare equal). Output `status counts edge_tols vertex_tols` (each
// sorted; or `-2`).
void p7_glue(SmBrep* b, Graph& g, Rd& rd, std::vector<double>& o)
{
    int64_t i1 = rd.idx(), i2 = rd.idx();
    SmOrientType orient = (SmOrientType)rd.idx();
    double gap = rd.num();
    SmBoolean nesting = rd.bflag();
    SmEdge* e1 = i1 >= 0 ? g.edges.at((size_t)i1) : NULL;
    SmEdge* e2 = i2 >= 0 ? g.edges.at((size_t)i2) : NULL;
    if (e1 && e2 && e1 != e2 && !e2->IsWire() && orient == SM_OT_SAME)
    {
        SmExtent1d a = e1->GetInterval(), d = e2->GetInterval();
        if (SM_ARE_SAME(a.GetMin(), d.GetMin()) && SM_ARE_SAME(a.GetMax(), d.GetMax())
            && (e1->GetCurve() == NULL) != (e2->GetCurve() == NULL))
        {
            o.push_back(P7_NULL_DEREF);
            return;
        }
    }
    o.push_back(st(b->GlueEdgesGeneral(e1, orient, gap, nesting, e2)));
    p7_put_counts(o, b);
    p7_put_tolerances(o, b);
}

// One edge's used interval, endpoint vertices, curve (type, natural interval and five
// samples) and edge uses (UV trim type, natural interval and five samples), in that
// order. Shared by the merge and copy exports (both read a B-rep after the op).
void p7_put_edge_geometry(std::vector<double>& o, SmEdge* e)
{
    SmExtent1d interval = e->GetInterval();
    o.push_back(interval.GetMin());
    o.push_back(interval.GetMax());
    for (SmVertex* vertex : {e->GetStartVertex(), e->GetEndVertex()})
    {
        o.push_back(vertex ? 1.0 : 0.0);
        if (vertex)
        {
            SmPoint3d point = vertex->GetPoint();
            o.push_back(point.x);
            o.push_back(point.y);
            o.push_back(point.z);
        }
    }
    SmCurve* curve = e->GetCurve();
    o.push_back(curve ? (double)curve->GetType() : -1.0);
    if (curve)
    {
        SmExtent1d natural = curve->GetNaturalInterval();
        o.push_back(natural.GetMin());
        o.push_back(natural.GetMax());
        for (double fraction : {0.0, 0.25, 0.5, 0.75, 1.0})
        {
            SmPoint3d point;
            SmStatus status = curve->EvaluatePoint(interval.Evaluate(fraction), point);
            o.push_back(st(status));
            if (status == SM_SUCCESS)
            {
                o.push_back(point.x);
                o.push_back(point.y);
                o.push_back(point.z);
            }
        }
    }
    std::vector<SmTopology*> uses = list_of(e->GetList());
    o.push_back((double)uses.size());
    for (SmTopology* topology : uses)
    {
        SmEdgeuse* use = (SmEdgeuse*)topology;
        SmBSplineCurve* trim = use->GetUVTrimCurve();
        o.push_back(trim ? (double)trim->GetType() : -1.0);
        if (trim)
        {
            SmExtent1d natural = trim->GetNaturalInterval();
            o.push_back(natural.GetMin());
            o.push_back(natural.GetMax());
            for (double fraction : {0.0, 0.25, 0.5, 0.75, 1.0})
            {
                SmPoint3d point;
                SmStatus status = trim->EvaluatePoint(natural.Evaluate(fraction), point);
                o.push_back(st(status));
                if (status == SM_SUCCESS)
                {
                    o.push_back(point.x);
                    o.push_back(point.y);
                }
            }
        }
    }
}

// A copied destination face's surface: its type, natural UV domain and five UV samples
// (the natural corners and centre) evaluated to 3D. A face without a surface (not made
// by CopyFaces) reports `-1` only.
void p7_put_face_geometry(std::vector<double>& o, SmFace* f)
{
    SmSurface* s = f->GetSurface();
    o.push_back(s ? (double)s->GetType() : -1.0);
    if (s == NULL)
        return;
    SmExtent2d dom = s->GetNaturalUVDomain();
    SmPoint2d lo = dom.GetMin();
    SmPoint2d hi = dom.GetMax();
    o.push_back(lo.x);
    o.push_back(lo.y);
    o.push_back(hi.x);
    o.push_back(hi.y);
    const double samples[5][2] = {{0.0, 0.0}, {1.0, 0.0}, {1.0, 1.0}, {0.0, 1.0}, {0.5, 0.5}};
    for (int i = 0; i < 5; i++)
    {
        SmPoint3d point;
        SmStatus status = s->EvaluatePoint(dom.Evaluate(samples[i][0], samples[i][1]), point);
        o.push_back(st(status));
        if (status == SM_SUCCESS)
        {
            o.push_back(point.x);
            o.push_back(point.y);
            o.push_back(point.z);
        }
    }
}

// `n use_merge repeat`, B-rep block: the first `n` faces (in traversal order) copied
// `repeat` times into one new B-rep. Output `status... nfaces counts_of_destination`
// (one status per copy, the new faces of all copies), then the whole destination graph
// after every copy: `nfaces face* nedges edge*` (each face its surface, each edge as
// `p7_put_edge_geometry`). The new-face list is reset by every CopyFaces call, so the
// destination is walked, not the returned list.
void p7_copy(SmBrep* b, Graph& g, Rd& rd, std::vector<double>& o)
{
    int64_t n = rd.idx();
    SmBoolean merge = rd.bflag();
    int64_t repeat = rd.idx();
    SmTArray<SmFace*> faces;
    for (int64_t i = 0; i < n && i < (int64_t)g.faces.size(); i++)
        faces.Add(g.faces[(size_t)i]);
    Built dest;
    dest.brep = new (the_context()) SmBrep();
    SmTArray<SmFace*> nfs;
    double made = 0;
    for (int64_t r = 0; r < repeat; r++) {
        // (CopyFaces resets the new-face list first)
        o.push_back(st(b->CopyFaces(faces, dest.brep, &nfs, merge)));
        made += (double)nfs.GetSize();
    }
    o.push_back(made);
    p7_put_counts(o, dest.brep);
    Graph dg;
    walk(dest.brep, dg);
    o.push_back((double)dg.faces.size());
    for (SmFace* f : dg.faces)
        p7_put_face_geometry(o, f);
    o.push_back((double)dg.edges.size());
    for (SmEdge* e : dg.edges)
        p7_put_edge_geometry(o, e);
}

// The self-intersection curves of a merge: 3D curve and the UV curves on both sheets.
// Upstream does not own them (both classifications are built with bIsCurveOwned FALSE,
// SetCurve defaults to FALSE): freed after the B-rep, which is declared after them.
struct SelfCurves
{
    SmTArray<SmCurve*> c3;
    SmTArray<SmBSplineCurve*> u1, u2;
    ~SelfCurves()
    {
        for (ULONG i = 0; i < c3.GetSize(); i++)
            delete c3[i];
        for (ULONG i = 0; i < u1.GetSize(); i++)
            delete u1[i];
        for (ULONG i = 0; i < u2.GetSize(); i++)
            delete u2[i];
    }
};
// Restore the original knots after the ordinary constructor's knot fix-up.
SmBSplineCurve* p7_read_self_curve(Rd& rd, const SmContext& ctx)
{
    bool raw = rd.flag();
    int64_t start = rd.at;
    int64_t n = (int64_t)rd.in[start + 1];
    int64_t m = (int64_t)rd.in[start + 3];
    SmBSplineCurve* curve = read_curve(rd, ctx);
    if (raw)
    {
        gw_CURVE* nurb = curve->GetGwNurbPointer();
        const double* knots = rd.in + start + 4 + 4 * (n + 1);
        for (int64_t i = 0; i <= m; ++i)
            nurb->knt->U[i] = knots[i];
    }
    return curve;
}


// `face tol`, then `ncurves (raw curve3d raw curveuv1 raw curveuv2)*`, before
// the B-rep block. Output `status nedges counts edges*`; each edge includes its
// curve and UV trim samples (`-2` for a face without surface).
void p7_merge_self(SmBrep* b, Graph& g, Rd& rd, const SelfCurves& cs, std::vector<double>& o)
{
    int64_t fi = rd.idx();
    double tol = rd.num();
    SmFace* f = g.faces.at((size_t)fi);
    SmSurface* s = f->GetSurface();
    if (s == NULL)
    {
        o.push_back(P7_NULL_DEREF);
        return;
    }
    SmTArray<SmEdge*> es;
    o.push_back(st(b->MergeSelfIntersectionCurves(*s, tol, cs.c3, cs.u1, cs.u2, es)));
    o.push_back((double)es.GetSize());
    p7_put_counts(o, b);
    for (ULONG i = 0; i < es.GetSize(); ++i)
        p7_put_edge_geometry(o, es[i]);
}

void p7_brep_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    // the op's arguments come first, then the B-rep block
    int64_t n_args = op == OP_GLUE ? 5 : op == OP_COPY ? 3 : 2;
    std::vector<double> args;
    for (int64_t i = 0; i < n_args; i++)
        args.push_back(rd.num());
    Rd ad{args.data(), 0};
    SelfCurves cs;
    if (op == OP_MERGE_SELF)
    {
        const SmContext& ctx = the_context();
        int64_t nc = rd.idx();
        for (int64_t i = 0; i < nc; i++)
        {
            cs.c3.Add(p7_read_self_curve(rd, ctx));
            cs.u1.Add(p7_read_self_curve(rd, ctx));
            cs.u2.Add(p7_read_self_curve(rd, ctx));
        }
    }
    SmStatus status = SM_SUCCESS;
    Built built;
    built.brep = build(rd, path, status);
    if (built.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(built.brep, g);
    switch (op)
    {
    case OP_GLUE: p7_glue(built.brep, g, ad, o); break;
    case OP_COPY: p7_copy(built.brep, g, ad, o); break;
    case OP_MERGE_SELF: p7_merge_self(built.brep, g, ad, cs, o); break;
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_p7_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    switch (op)
    {
    case OP_READ: p7_read(rd, o); break;
    case OP_MAKE_FACE_WITH_CURVES: p7_make_face_with_curves(rd, o); break;
    case OP_MAKE_FACE: p7_make_face(rd, o); break;
    default: p7_brep_op(op, rd, path, o); break;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
