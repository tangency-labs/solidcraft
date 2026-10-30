// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_constructor`: runs a constructor script (a sequence
// of SmBrepConstructor calls over a table of curves and surfaces, layout in
// crates/step-import/src/script.rs) on upstream SmBrepConstructor (source/SMLib/src/
// SmBrepData.cpp) and reports each call's status and the B-rep it built. The Rust side is
// crates/difftest/src/constructor.rs; the binding (crates/solidcore-ffi/src/units/
// topology_brep_constructor.rs) accepts only scripts step_import::script::decode accepts.
//
// sc_topology_brep_constructor_run(in, n_in, mode, accuracy, out, cap, out_len). The script
// runs in a forked child (alarm 120 s) that streams blocks `tag n value*n` to the parent:
//   1  geometry   status of each curve then each surface construction (0 = made)
//   2  op         op code, status (0 success, else the SmStatus; for a call returning a
//                 pointer 1 = NULL; -3 = skipped: its curve or surface was not made)
//   3  crash      (added by the parent when the child dies) the index of the op that was
//                 running, n_ops when the crash was after the ops
//   4  topology   (mode 0) the integer dump of the B-rep (layout below)
//   5  values     (mode 0) the float dump of the B-rep
//   6  state      brep made, IsManifold()
//   101..108      (mode 1) the --step-corpus stages after construction: counts, manifold,
//                 area, volume (at `accuracy`), heal, healed_counts, healed_manifold,
//                 healed_volume, as in step_import.cpp
// Op 20 (not a constructor call) is the importer's SmSurface::AdjustSTEPUVDomain on a table
// surface before SetFaceSurface takes it.
// Ops stop after a successful EndBrep (the caller's B-rep is kept for the dump).
//
// Dump (indices are first-seen traversal order per kind, -1 for NULL): regions of
// GetRegions: n, then per region idx, IsVoid, its GetShells: n, per shell idx,
// IsFaceuseShell, (faceuse shells) its list: n, per faceuse idx, orientation, face, mate,
// its loopuse list: n, per loopuse idx, orientation, type (0 unknown, 1 edgeuse, 2
// vertexuse), loop, mate, then (edge loop) the CCW walk from GetEUorVU: n (-9 when it does
// not close within the bound), per edgeuse idx, orientation, edge, vertex of its vertexuse,
// mate, radial, cw, has UV trim curve; (vertex loop) its vertex. Then GetFaces, GetEdges,
// GetVertices (n, idx*n), per edge its list (n, idx*n), has curve, evaluated (1 when the
// curve was evaluated at the interval ends, then the two statuses), per vertex its use
// count, per face has surface. Values: per edge interval min, max, tolerance (and the two
// evaluated points), per vertex point, tolerance, per face UV domain, tolerance, then the
// B-rep tolerance.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiHeal.h>
#include <SmApiQueries.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCylinder.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmEllipse.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSphere.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurfOfRevolution.h>
#include <SmTorus.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <cmath>
#include <cstring>
#include <map>
#include <vector>

#ifndef _WIN32
#ifdef __linux__
#include <sys/prctl.h>
#endif
#include <sys/wait.h>
#include <unistd.h>
#endif

namespace
{
struct Rd
{
    const double* p;
    int64_t n;
    int64_t i = 0;
    bool ok = true;
    double num()
    {
        if (i >= n)
        {
            ok = false;
            return 0.0;
        }
        return p[i++];
    }
    int64_t idx()
    {
        double x = num();
        if (!(x >= 0.0 && x < 1.0e15))
        {
            ok = false;
            return 0;
        }
        return (int64_t)x;
    }
    // -1 or an index (-2 also for region references)
    int64_t ref()
    {
        double x = num();
        if (!(x >= -2.0 && x < 1.0e15))
        {
            ok = false;
            return -1;
        }
        return (int64_t)x;
    }
    SmPoint3d pt()
    {
        double x = num(), y = num(), z = num();
        return SmPoint3d(x, y, z);
    }
};

struct Frame
{
    SmPoint3d o;
    SmVector3d x, y, z;
};

Frame frame(Rd& r)
{
    Frame f;
    f.o = r.pt();
    f.x = r.pt();
    f.y = r.pt();
    f.z = r.pt();
    return f;
}

// Geometry blocks of step_import::flat (as in step_import.cpp, plus 2D curves).
struct CurveData
{
    int kind = -1;
    SmPoint3d o;
    SmVector3d d;
    double mag = 0.0;
    Frame f;
    double r1 = 0.0, r2 = 0.0;
    long degree = 0;
    SmTArray<SmPoint3d> pts;
    bool rational = false;
    SmTArray<double> weights;
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
};

void read_curve(Rd& r, CurveData& c)
{
    c.kind = (int)r.idx();
    switch (c.kind)
    {
    case 0:
        c.o = r.pt();
        c.d = r.pt();
        c.mag = r.num();
        break;
    case 1:
        c.f = frame(r);
        c.r1 = r.num();
        break;
    case 2:
        c.f = frame(r);
        c.r1 = r.num();
        c.r2 = r.num();
        break;
    case 3:
    {
        c.degree = (long)r.idx();
        int64_t n = r.idx();
        c.rational = r.idx() != 0;
        int64_t nk = r.idx();
        if (!r.ok || n > r.n || nk > r.n)
        {
            r.ok = false;
            return;
        }
        for (int64_t j = 0; j < n && r.ok; ++j)
            c.pts.Add(r.pt());
        if (c.rational)
            for (int64_t j = 0; j < n && r.ok; ++j)
                c.weights.Add(r.num());
        for (int64_t j = 0; j < nk && r.ok; ++j)
            c.knots.Add(r.num());
        for (int64_t j = 0; j < nk && r.ok; ++j)
            c.mults.Add((ULONG)r.idx());
        break;
    }
    default:
        r.ok = false;
    }
}

// The curve of dimension `dim` bounded to [lo, hi] (bounded) or as given.
SmStatus make_curve(const SmContext& ctx, CurveData& c, int dim, bool bounded, double lo, double hi, SmCurve*& out)
{
    out = NULL;
    SmExtent1d ivl(lo, hi);
    if (dim == 2)
        for (ULONG j = 0; j < c.pts.GetSize(); ++j)
            c.pts[j].z = 0.0;
    switch (c.kind)
    {
    case 0:
        if (bounded)
        {
            SmPoint3d o = c.o;
            SmVector3d d = c.d;
            if (dim == 2)
            {
                o.z = 0.0;
                d.z = 0.0;
            }
            out = new (ctx) SmLine(o, d, ivl, c.mag, (ULONG)dim, &ctx);
            return SM_SUCCESS;
        }
        else
        {
            SmLine* l = NULL;
            SmStatus s = SmLine::CreateCanonical(ctx, c.o, c.d, l);
            out = l;
            return s;
        }
    case 1:
    {
        SmCircle* e = NULL;
        SmStatus s = SmCircle::CreateCanonical(ctx, SmAxis2Placement(c.f.o, c.f.x, c.f.y), c.r1, e,
                                               bounded ? &ivl : NULL);
        out = e;
        return s;
    }
    case 2:
    {
        SmEllipse* e = NULL;
        SmStatus s = SmEllipse::CreateCanonical(ctx, SmAxis2Placement(c.f.o, c.f.x, c.f.y), c.r1, c.r2, e,
                                                bounded ? &ivl : NULL);
        out = e;
        return s;
    }
    case 3:
    {
        // Trim only to a strict sub-interval of the knot range.
        double k0 = c.knots[0], k1 = c.knots[c.knots.GetSize() - 1];
        bool trim = bounded && !(lo == k0 && hi == k1);
        SmBSplineCurve* b = NULL;
        SmStatus s = SmBSplineCurve::CreateCanonical(ctx, (ULONG)dim, (ULONG)c.degree, c.pts, SM_CF_UNSPECIFIED,
                                                     c.mults, c.knots, SM_KT_UNSPECIFIED,
                                                     c.rational ? &c.weights : NULL, trim ? &ivl : NULL, b);
        out = b;
        return s;
    }
    default:
        return SM_ERR;
    }
}

SmStatus make_surface(const SmContext& ctx, Rd& r, SmSurface*& out)
{
    out = NULL;
    int kind = (int)r.idx();
    switch (kind)
    {
    case 0:
    {
        Frame f = frame(r);
        if (!r.ok)
            return SM_ERR;
        SmPlane* p = NULL;
        SmStatus s = SmPlane::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), p);
        out = p;
        return s;
    }
    case 1:
    {
        Frame f = frame(r);
        double rad = r.num();
        if (!r.ok)
            return SM_ERR;
        SmCylinder* p = NULL;
        SmStatus s = SmCylinder::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), rad, p);
        out = p;
        return s;
    }
    case 2:
    {
        Frame f = frame(r);
        double rad = r.num(), semi = r.num();
        if (!r.ok)
            return SM_ERR;
        SmCone* p = NULL;
        SmStatus s = SmCone::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), rad, semi, p);
        out = p;
        return s;
    }
    case 3:
    {
        Frame f = frame(r);
        double rad = r.num();
        if (!r.ok)
            return SM_ERR;
        SmSphere* p = NULL;
        SmStatus s = SmSphere::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), rad, p);
        out = p;
        return s;
    }
    case 4:
    {
        Frame f = frame(r);
        double maj = r.num(), min = r.num();
        if (!r.ok)
            return SM_ERR;
        SmTorus* p = NULL;
        SmStatus s = SmTorus::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), maj, min, p);
        out = p;
        return s;
    }
    case 5:
    case 6:
    {
        CurveData c;
        read_curve(r, c);
        SmPoint3d a = kind == 5 ? r.pt() : SmPoint3d(0.0, 0.0, 0.0);
        SmVector3d d = r.pt();
        double mag = kind == 6 ? r.num() : 0.0;
        if (!r.ok)
            return SM_ERR;
        SmCurve* swept = NULL;
        SmStatus s = make_curve(ctx, c, 3, false, 0.0, 0.0, swept);
        if (s != SM_SUCCESS)
            return s;
        SmBSplineCurve* b = (SmBSplineCurve*)swept;
        if (kind == 5)
        {
            SmSurfOfRevolution* p = NULL;
            s = SmSurfOfRevolution::CreateCanonical(ctx, b, a, d, p);
            out = p;
        }
        else
        {
            SmSurfOfExtrusion* p = NULL;
            s = SmSurfOfExtrusion::CreateCanonical(ctx, b, SmVector3d(d.x * mag, d.y * mag, d.z * mag), p);
            out = p;
        }
        return s;
    }
    case 7:
    {
        long ud = (long)r.idx(), vd = (long)r.idx();
        int64_t nu = r.idx(), nv = r.idx();
        bool rational = r.idx() != 0;
        int64_t nuk = r.idx(), nvk = r.idx();
        if (!r.ok || nu > r.n || nv > r.n || nu * nv > r.n || nuk > r.n || nvk > r.n)
            return SM_ERR;
        SmTArray<SmPoint3d> pts;
        SmTArray<double> w, uk, vk;
        SmTArray<ULONG> um, vm;
        for (int64_t j = 0; j < nu * nv && r.ok; ++j)
            pts.Add(r.pt());
        if (rational)
            for (int64_t j = 0; j < nu * nv && r.ok; ++j)
                w.Add(r.num());
        for (int64_t j = 0; j < nuk && r.ok; ++j)
            uk.Add(r.num());
        for (int64_t j = 0; j < nuk && r.ok; ++j)
            um.Add((ULONG)r.idx());
        for (int64_t j = 0; j < nvk && r.ok; ++j)
            vk.Add(r.num());
        for (int64_t j = 0; j < nvk && r.ok; ++j)
            vm.Add((ULONG)r.idx());
        if (!r.ok)
            return SM_ERR;
        SmBSplineSurface* p = NULL;
        SmStatus s = SmBSplineSurface::CreateCanonical(ctx, (ULONG)ud, (ULONG)vd, pts, SM_SF_UNSPECIFIED, um, vm, uk,
                                                       vk, SM_KT_UNSPECIFIED, rational ? &w : NULL, NULL, p);
        out = p;
        return s;
    }
    default:
        r.ok = false;
        return SM_ERR;
    }
}

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

SmOrientType orient(int64_t x)
{
    return x == 1 ? SM_OT_SAME : x == 2 ? SM_OT_OPPOSITE : SM_OT_UNKNOWN;
}

struct Op
{
    int code = -1;
    double a[8] = {0, 0, 0, 0, 0, 0, 0, 0};
};

// Reads the script header, geometry and ops (validated again by the Rust binding);
// surfaces are made as they are read, curves later.
bool read_script(const SmContext& ctx, Rd& r, std::vector<CurveData>& curves, std::vector<int>& dims,
                 std::vector<double>& bounds, std::vector<SmSurface*>& surfaces, std::vector<double>& surface_status,
                 std::vector<Op>& ops)
{
    if (r.num() != 31.0 || r.num() != 1.0)
        return false;
    int64_t nc = r.idx(), ns = r.idx(), no = r.idx();
    if (!r.ok || nc > r.n || ns > r.n || no > r.n)
        return false;
    curves.resize((size_t)nc);
    for (int64_t i = 0; i < nc && r.ok; ++i)
    {
        dims.push_back((int)r.idx());
        double b = r.num(), lo = r.num(), hi = r.num();
        bounds.push_back(b);
        bounds.push_back(lo);
        bounds.push_back(hi);
        read_curve(r, curves[(size_t)i]);
    }
    for (int64_t i = 0; i < ns && r.ok; ++i)
    {
        SmSurface* sf = NULL;
        SmStatus s = make_surface(ctx, r, sf);
        if (s != SM_SUCCESS)
            sf = NULL;
        surfaces.push_back(sf);
        surface_status.push_back(sf ? 0.0 : (s == SM_SUCCESS ? (double)SM_ERR : st(s)));
    }
    for (int64_t i = 0; i < no && r.ok; ++i)
    {
        Op op;
        op.code = (int)r.idx();
        int nargs = 0;
        switch (op.code)
        {
        case 2:
            nargs = 2;
            break;
        case 5:
        case 6:
        case 9:
            nargs = 1;
            break;
        case 8:
            nargs = 6;
            break;
        case 11:
        case 12:
        case 17:
            nargs = 3;
            break;
        case 13:
            nargs = 4;
            break;
        case 14:
            nargs = 2;
            break;
        case 15:
            nargs = 8;
            break;
        case 20:
            nargs = 5;
            break;
        default:
            nargs = 0;
        }
        for (int j = 0; j < nargs; ++j)
            op.a[j] = r.num();
        ops.push_back(op);
    }
    return r.ok && r.i == r.n;
}

template <class T> T* at_ref(const std::vector<T*>& v, double x)
{
    int64_t i = (int64_t)x;
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : NULL;
}

// ---- output -------------------------------------------------------------------------------

struct Sink
{
#ifndef _WIN32
    int fd = -1;
#endif
    std::vector<double>* direct = NULL;
    void block(int tag, const std::vector<double>& v)
    {
        std::vector<double> b;
        b.push_back((double)tag);
        b.push_back((double)v.size());
        b.insert(b.end(), v.begin(), v.end());
        if (direct)
        {
            direct->insert(direct->end(), b.begin(), b.end());
            return;
        }
#ifndef _WIN32
        const char* c = (const char*)b.data();
        size_t n = b.size() * sizeof(double);
        while (n > 0)
        {
            ssize_t w = write(fd, c, n);
            if (w <= 0)
                return;
            c += w;
            n -= (size_t)w;
        }
#endif
    }
};

// ---- dump ---------------------------------------------------------------------------------

struct Ix
{
    std::map<const void*, double> m;
    std::vector<const void*> order;
    double operator()(const void* p)
    {
        if (p == NULL)
            return -1.0;
        auto it = m.find(p);
        if (it != m.end())
            return it->second;
        double i = (double)order.size();
        m[p] = i;
        order.push_back(p);
        return i;
    }
};

void dump(SmBrep* b, int64_t bound, Sink& out)
{
    std::vector<double> t, v;
    Ix regions, shells, faceuses, faces, loopuses, loops, edgeuses, edges, vertices;
    SmTArray<SmRegion*> rl;
    b->GetRegions(rl);
    t.push_back((double)rl.GetSize());
    for (ULONG ri = 0; ri < rl.GetSize(); ++ri)
    {
        SmRegion* r = rl[ri];
        t.push_back(regions(r));
        t.push_back((double)r->IsVoid());
        SmTArray<SmShell*> sl;
        r->GetShells(sl);
        t.push_back((double)sl.GetSize());
        for (ULONG si = 0; si < sl.GetSize(); ++si)
        {
            SmShell* s = sl[si];
            t.push_back(shells(s));
            bool fus = s->IsFaceuseShell() != 0;
            t.push_back(fus ? 1.0 : 0.0);
            if (!fus)
                continue;
            std::vector<SmFaceuse*> ful;
            SmTopology* head = s->GetList();
            for (SmTopology* x = head; x != NULL && (int64_t)ful.size() < bound;)
            {
                ful.push_back((SmFaceuse*)x);
                x = x->GetNext();
                if (x == head)
                    break;
            }
            t.push_back((double)ful.size());
            for (SmFaceuse* fu : ful)
            {
                t.push_back(faceuses(fu));
                t.push_back((double)fu->GetOrientation());
                t.push_back(faces(fu->GetFace()));
                t.push_back(faceuses(fu->GetMate()));
                std::vector<SmLoopuse*> lul;
                SmTopology* lh = fu->GetList();
                for (SmTopology* x = lh; x != NULL && (int64_t)lul.size() < bound;)
                {
                    lul.push_back((SmLoopuse*)x);
                    x = x->GetNext();
                    if (x == lh)
                        break;
                }
                t.push_back((double)lul.size());
                for (SmLoopuse* lu : lul)
                {
                    t.push_back(loopuses(lu));
                    t.push_back((double)lu->GetOrientation());
                    SM_TYPE ty = lu->GetLoopuseType();
                    int code = ty == SmEdgeuse_TYPE ? 1 : ty == SmVertexuse_TYPE ? 2 : 0;
                    t.push_back((double)code);
                    t.push_back(loops(lu->GetLoop()));
                    t.push_back(loopuses(lu->GetOtherLoopuse()));
                    if (code == 1)
                    {
                        SmEdgeuse* start = (SmEdgeuse*)lu->GetEUorVU();
                        std::vector<SmEdgeuse*> eul;
                        bool closed = false;
                        for (SmEdgeuse* x = start; x != NULL && (int64_t)eul.size() < bound;)
                        {
                            eul.push_back(x);
                            x = x->GetCCWEdgeuse();
                            if (x == start)
                            {
                                closed = true;
                                break;
                            }
                        }
                        if (!closed)
                        {
                            t.push_back(-9.0);
                            continue;
                        }
                        t.push_back((double)eul.size());
                        for (SmEdgeuse* eu : eul)
                        {
                            t.push_back(edgeuses(eu));
                            t.push_back((double)eu->GetOrientation());
                            t.push_back(edges(eu->GetEdge()));
                            SmVertexuse* vu = eu->GetVertexuse();
                            t.push_back(vertices(vu ? vu->GetVertex() : NULL));
                            t.push_back(edgeuses(eu->GetMate()));
                            t.push_back(edgeuses(eu->GetRadial()));
                            t.push_back(edgeuses(eu->GetCWEdgeuse()));
                            t.push_back(eu->GetUVTrimCurvePointer() != NULL ? 1.0 : 0.0);
                        }
                    }
                    else if (code == 2)
                    {
                        SmVertexuse* vu = (SmVertexuse*)lu->GetEUorVU();
                        t.push_back(vertices(vu ? vu->GetVertex() : NULL));
                    }
                }
            }
        }
    }
    SmTArray<SmFace*> fl;
    SmTArray<SmEdge*> el;
    SmTArray<SmVertex*> vl;
    b->GetFaces(fl);
    b->GetEdges(el);
    b->GetVertices(vl);
    t.push_back((double)fl.GetSize());
    for (ULONG i = 0; i < fl.GetSize(); ++i)
        t.push_back(faces(fl[i]));
    t.push_back((double)el.GetSize());
    for (ULONG i = 0; i < el.GetSize(); ++i)
        t.push_back(edges(el[i]));
    t.push_back((double)vl.GetSize());
    for (ULONG i = 0; i < vl.GetSize(); ++i)
        t.push_back(vertices(vl[i]));

    // per edge (edge uses first seen here get indices after the walk's)
    for (size_t i = 0; i < edges.order.size(); ++i)
    {
        SmEdge* e = (SmEdge*)edges.order[i];
        std::vector<SmEdgeuse*> eul;
        SmTopology* h = e->GetList();
        for (SmTopology* x = h; x != NULL && (int64_t)eul.size() < bound;)
        {
            eul.push_back((SmEdgeuse*)x);
            x = x->GetNext();
            if (x == h)
                break;
        }
        t.push_back((double)eul.size());
        for (SmEdgeuse* eu : eul)
            t.push_back(edgeuses(eu));
        SmCurve* c = e->GetCurve();
        SmExtent1d ivl = e->GetInterval();
        t.push_back(c != NULL ? 1.0 : 0.0);
        v.push_back(ivl.GetMin());
        v.push_back(ivl.GetMax());
        v.push_back(e->GetTolerance());
        bool eval = c != NULL && ivl.GetMin() <= ivl.GetMax() && std::fabs(ivl.GetMin()) < 1.0e20 &&
                    std::fabs(ivl.GetMax()) < 1.0e20;
        t.push_back(eval ? 1.0 : 0.0);
        if (eval)
        {
            for (double x : {ivl.GetMin(), ivl.GetMax()})
            {
                SmPoint3d p(0.0, 0.0, 0.0);
                SmStatus s = c->EvaluatePoint(x, p);
                t.push_back(st(s));
                if (s != SM_SUCCESS)
                    p = SmPoint3d(0.0, 0.0, 0.0);
                v.push_back(p.x);
                v.push_back(p.y);
                v.push_back(p.z);
            }
        }
    }
    for (size_t i = 0; i < vertices.order.size(); ++i)
    {
        SmVertex* x = (SmVertex*)vertices.order[i];
        int64_t n = 0;
        SmTopology* h = x->GetList();
        for (SmTopology* y = h; y != NULL && n < bound;)
        {
            ++n;
            y = y->GetNext();
            if (y == h)
                break;
        }
        t.push_back((double)n);
        SmPoint3d p = x->GetPoint();
        v.push_back(p.x);
        v.push_back(p.y);
        v.push_back(p.z);
        v.push_back(x->GetTolerance());
    }
    for (size_t i = 0; i < faces.order.size(); ++i)
    {
        SmFace* f = (SmFace*)faces.order[i];
        t.push_back(f->GetSurface() != NULL ? 1.0 : 0.0);
        SmExtent2d d = f->GetUVDomain();
        v.push_back(d.GetMin().x);
        v.push_back(d.GetMin().y);
        v.push_back(d.GetMax().x);
        v.push_back(d.GetMax().y);
        v.push_back(f->GetTolerance());
    }
    v.push_back(b->GetTolerance());
    out.block(4, t);
    out.block(5, v);
}

void counts(SmBrep* b, std::vector<double>& v)
{
    SmTArray<SmRegion*> regions;
    SmTArray<SmShell*> shells;
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    SmTArray<SmVertex*> vertices;
    b->GetRegions(regions);
    b->GetShells(shells);
    b->GetFaces(faces);
    b->GetEdges(edges);
    b->GetVertices(vertices);
    v.push_back((double)regions.GetSize());
    v.push_back((double)shells.GetSize());
    v.push_back((double)faces.GetSize());
    v.push_back((double)edges.GetSize());
    v.push_back((double)vertices.GetSize());
}

void stages(SmBrep* brep, double accuracy, Sink& out)
{
    {
        std::vector<double> v;
        counts(brep, v);
        out.block(101, v);
    }
    {
        SmBoolean m = FALSE;
        SmStatus s = SmApiBrepIsManifoldSolid(brep, m);
        out.block(102, {st(s), m ? 1.0 : 0.0});
    }
    {
        double a = 0.0;
        SmStatus s = SmApiBrepComputeArea(brep, accuracy, a);
        out.block(103, {st(s), s == SM_SUCCESS ? a : 0.0});
    }
    {
        double vol = 0.0;
        SmStatus s = SmApiBrepComputeVolume(brep, accuracy, vol);
        out.block(104, {st(s), s == SM_SUCCESS ? vol : 0.0});
    }
    {
        SmStatus s = SmApiHealBrep(brep);
        out.block(105, {st(s)});
    }
    {
        std::vector<double> v;
        counts(brep, v);
        out.block(106, v);
    }
    {
        SmBoolean m = FALSE;
        SmStatus s = SmApiBrepIsManifoldSolid(brep, m);
        out.block(107, {st(s), m ? 1.0 : 0.0});
    }
    {
        double vol = 0.0;
        SmStatus s = SmApiBrepComputeVolume(brep, accuracy, vol);
        out.block(108, {st(s), s == SM_SUCCESS ? vol : 0.0});
    }
}

// ---- the script ---------------------------------------------------------------------------

void run_script(const double* in, int64_t n_in, int mode, double accuracy, Sink& out)
{
    SmContext* ctx = SmApiGetOrCreateContext();
    Rd r{in, n_in};
    std::vector<CurveData> cdata;
    std::vector<int> dims;
    std::vector<double> bounds;
    std::vector<SmSurface*> surfaces;
    std::vector<double> surface_status;
    std::vector<Op> ops;
    if (!read_script(*ctx, r, cdata, dims, bounds, surfaces, surface_status, ops))
        return;

    // geometry table: curves then surfaces (made while reading)
    std::vector<SmCurve*> curves(cdata.size(), (SmCurve*)NULL);
    std::vector<double> gs;
    for (size_t i = 0; i < cdata.size(); ++i)
    {
        SmStatus s = make_curve(*ctx, cdata[i], dims[i], bounds[3 * i] != 0.0, bounds[3 * i + 1], bounds[3 * i + 2],
                                curves[i]);
        if (s != SM_SUCCESS)
            curves[i] = NULL;
        gs.push_back(curves[i] ? 0.0 : (s == SM_SUCCESS ? (double)SM_ERR : st(s)));
    }
    gs.insert(gs.end(), surface_status.begin(), surface_status.end());
    out.block(1, gs);

    SmBrepConstructor c;
    SmBrep* brep = NULL;
    std::vector<SmRegion*> regions;
    std::vector<SmEdge*> edges;
    std::vector<SmVertex*> vertices;
    for (const Op& op : ops)
    {
        double status = 0.0;
        const double* a = op.a;
        switch (op.code)
        {
        case 0:
        {
            SmBrep* b = c.StartBrep(*ctx);
            if (b != NULL)
                brep = b;
            status = b ? 0.0 : 1.0;
            break;
        }
        case 1:
            status = st(c.EndBrep());
            break;
        case 2:
        {
            SmRegion* outer = a[1] == -2.0 ? (brep ? brep->GetInfiniteRegion() : NULL) : at_ref(regions, a[1]);
            SmRegion* x = c.StartRegion(a[0] != 0.0, outer);
            regions.push_back(x);
            status = x ? 0.0 : 1.0;
            break;
        }
        case 3:
            status = st(c.EndRegion());
            break;
        case 4:
        {
            SmShell *o = NULL, *i = NULL;
            status = st(c.StartShell(o, i));
            break;
        }
        case 5:
            status = st(c.EndShell(a[0] != 0.0));
            break;
        case 6:
            status = c.StartFace(orient((int64_t)a[0])) ? 0.0 : 1.0;
            break;
        case 7:
            status = st(c.EndFace());
            break;
        case 8:
        {
            SmSurface* s = surfaces[(size_t)a[0]];
            if (s == NULL)
            {
                status = -3.0;
                break;
            }
            SmExtent2d d = a[1] != 0.0 ? s->GetNaturalUVDomain() : SmExtent2d(a[2], a[3], a[4], a[5]);
            status = st(c.SetFaceSurface(s, d));
            break;
        }
        case 9:
            status = c.StartLoop(orient((int64_t)a[0])) ? 0.0 : 1.0;
            break;
        case 10:
            status = st(c.EndLoop());
            break;
        case 11:
            status = c.StartAndEndSingleVertexLoop(SmPoint3d(a[0], a[1], a[2])) ? 0.0 : 1.0;
            break;
        case 12:
        {
            SmVertex* x = c.StartVertexOfLoop(SmPoint3d(a[0], a[1], a[2]));
            vertices.push_back(x);
            status = x ? 0.0 : 1.0;
            break;
        }
        case 13:
        {
            SmEdge* x = c.StartEdge(orient((int64_t)a[0]), at_ref(edges, a[1]), at_ref(vertices, a[2]), at_ref(vertices, a[3]));
            edges.push_back(x);
            status = x ? 0.0 : 1.0;
            break;
        }
        case 14:
        {
            SmCurve* k = NULL;
            if (a[0] >= 0.0)
            {
                k = curves[(size_t)a[0]];
                if (k == NULL)
                {
                    status = -3.0;
                    break;
                }
            }
            status = st(c.SetEdgeCurve(k, a[1] != 0.0));
            break;
        }
        case 15:
        {
            SmVertexNode n[2];
            for (int j = 0; j < 2; ++j)
            {
                const double* x = a + 4 * j;
                int kind = (int)x[0];
                if (kind == 0)
                    n[j].Set(SmPoint3d(x[1], x[2], x[3]));
                else if (kind == 1)
                    n[j].Set(x[1]);
                else if (kind == 2)
                    n[j].Set(SmPoint2d(x[1], x[2]));
            }
            status = st(c.SetLimits(n[0], n[1]));
            break;
        }
        case 16:
            status = st(c.EndEdge());
            break;
        case 17:
        {
            SmVertex* x = c.StartVertex(SmPoint3d(a[0], a[1], a[2]));
            vertices.push_back(x);
            status = x ? 0.0 : 1.0;
            break;
        }
        case 18:
            status = st(c.EndVertex());
            break;
        case 19:
            c.SwapRegions();
            break;
        default:
        {
            // 20: the importer's AdjustSTEPUVDomain on a surface not yet bound to a face
            SmSurface* s = surfaces[(size_t)a[0]];
            if (s == NULL)
            {
                status = -3.0;
                break;
            }
            status = st(s->AdjustSTEPUVDomain(SmExtent2d(a[1], a[2], a[3], a[4])));
            break;
        }
        }
        out.block(2, {(double)op.code, status});
        if (op.code == 1 && status == 0.0)
            break;
    }
    out.block(6, {brep ? 1.0 : 0.0, c.IsManifold() ? 1.0 : 0.0});
    if (brep == NULL)
        return;
    if (mode == 0)
        dump(brep, 4 * (int64_t)ops.size() + 16, out);
    else
        stages(brep, accuracy, out);
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_constructor_run(const double* in, int64_t n_in, int32_t mode, double accuracy,
                                                   double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || out_len == nullptr || n_in < 5 || (mode != 0 && mode != 1) || !std::isfinite(accuracy))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
#ifdef _WIN32
    Sink s;
    s.direct = &o;
    run_script(in, n_in, mode, accuracy, s);
#else
    int fd[2];
    if (pipe(fd) != 0)
        return SC_ERR_ARG;
    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0)
    {
        close(fd[0]);
        close(fd[1]);
        return SC_ERR_ARG;
    }
    if (pid == 0)
    {
        close(fd[0]);
#ifdef __linux__
        // (no core dump of an expected crash)
        prctl(PR_SET_DUMPABLE, 0);
#endif
        alarm(120);
        int code = 1;
        try
        {
            Sink s;
            s.fd = fd[1];
            run_script(in, n_in, mode, accuracy, s);
            code = 0;
        }
        catch (...)
        {
        }
        close(fd[1]);
        _exit(code);
    }
    close(fd[1]);
    std::vector<char> bytes;
    char buf[65536];
    for (;;)
    {
        ssize_t n = read(fd[0], buf, sizeof buf);
        if (n <= 0)
            break;
        bytes.insert(bytes.end(), buf, buf + n);
    }
    close(fd[0]);
    int ws = 0;
    waitpid(pid, &ws, 0);
    size_t nd = bytes.size() / sizeof(double);
    o.resize(nd);
    for (size_t i = 0; i < nd; ++i)
        std::memcpy(&o[i], bytes.data() + i * sizeof(double), sizeof(double));
    bool died = !(WIFEXITED(ws) && WEXITSTATUS(ws) == 0);
    if (died)
    {
        // the op that was running: the number of op blocks received (whole blocks only)
        double nops = 0.0;
        size_t i = 0;
        std::vector<double> whole;
        while (i + 2 <= o.size())
        {
            size_t n = (size_t)o[i + 1];
            if (i + 2 + n > o.size())
                break;
            if (o[i] == 2.0)
                nops += 1.0;
            i += 2 + n;
        }
        o.resize(i);
        o.push_back(3.0);
        o.push_back(1.0);
        o.push_back(nops);
    }
#endif
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
