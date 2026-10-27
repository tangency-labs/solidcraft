// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.edgeuse_p3`: the SmEdgeuse members owned by the unit
// (source/SMLib/src/SmEdgeuse.cpp) on real SmBrep objects: CreateUVTrimCurve,
// GetOrCreateUVTrimCurve, EvaluateBinormal and NormalizedEvaluate.
//
// One entry point, sc_topology_edgeuse_p3_eval(op, path, in, n_in, out, cap, out_len).
// Every op starts with the B-rep block of unit topology.edgeuse_p1 (built and exported by
// the shared sc_edgeuse_graph.h; the export itself is topology.edgeuse_p1's Export op).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_edgeuse_p3.rs, which
// validates every input. Outputs start with `0`, or `-1 status` when the B-rep could not
// be built. `-2` is "upstream would dereference null".
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmObject.h>
#include <SmSurface.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <vector>

#include "sc_edgeuse_graph.h"
#include "sc_trim_curve_record.h"

namespace
{
enum Op3 : int32_t
{
    OP_CREATE = 1,
    OP_BINORMAL = 2,
    OP_NORMALIZED = 3,
    OP_GET_OR_CREATE = 4,
    OP_EXPORT_GAPS = 5,
};

const double ST_NULL_DEREF = -2.0;

struct GapPeek : SmEdgeuse
{
    static SmEdgeFaceGap SmEdgeuse::*cached() { return &GapPeek::m_sMaxEdgeFaceGap3d; }
};

// GetMate/GetRadial/GetUVTrimCurve need the edge's primary edgeuse (loop edgeuses).
bool has_prim(SmEdgeuse* p)
{
    return p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
}

// Every pointer a loop edgeuse's UV trim curve creation or binormal dereferences: the
// edge with its curve, the faceuse, face and surface, the vertexuse, the primary edgeuse
// with its mate and both their vertexuses (SetUVTrimCurve), the edge's two vertices.
bool loop_safe(SmEdgeuse* p)
{
    if (p == nullptr || !p->IsLoopEdgeuse() || !p->GetEdge() || !p->GetEdge()->GetCurve() || !p->GetFaceuse() ||
        !p->GetFace() || !p->GetFace()->GetSurface() || !p->GetVertexuse() || !has_prim(p))
        return false;
    SmEdgeuse* prim = p->GetEdge()->GetPrimaryEdgeuse();
    SmEdgeuse* pm = prim->GetMate();
    if (pm == nullptr || prim->GetVertexuse() == nullptr || pm->GetVertexuse() == nullptr)
        return false;
    SmEdgeuse* mate = p->GetMate();
    if (mate && mate->GetVertexuse() == nullptr)
        return false;
    return p->GetEdge()->GetStartVertex() != nullptr && p->GetEdge()->GetEndVertex() != nullptr;
}

void put_edge_state(std::vector<double>& o, SmEdgeuse* p)
{
    SmEdge* e = p->GetEdge();
    SmExtent1d ivl = e->GetInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    o.push_back((double)e->GetTolerance());
}

// The tail every op reports: the edge use's and its mate's state, the edge state, then
// the full records of the edge use's and its mate's UV trim curves.
void put_states(std::vector<double>& o, Graph& g, SmEdgeuse* p, SmEdgeuse* mate)
{
    put_eu_state(o, g, p);
    put_eu_state(o, g, mate);
    put_edge_state(o, p);
    sc_trim_curve::put_trim_curve(o, p->*EuPeek::uv());
    sc_trim_curve::put_trim_curve(o, mate ? mate->*EuPeek::uv() : nullptr);
}

int32_t run_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return SC_OK;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    if (op == OP_EXPORT_GAPS)
    {
        o.push_back((double)g.edgeuses.size());
        for (SmEdgeuse* eu : g.edgeuses)
        {
            const SmEdgeFaceGap& gap = eu->*GapPeek::cached();
            o.push_back((double)g.find(6, gap.GetEdgeuse()));
            o.push_back(gap.GetLength());
            o.push_back(gap.GetEdgeT());
            o.push_back(gap.GetFaceUV().x);
            o.push_back(gap.GetFaceUV().y);
        }
        return SC_OK;
    }
    const int64_t selected = rd.idx();
    // A file fixture can change after the validating export. Refuse a stale
    // selector instead of indexing beyond the freshly rebuilt graph. Ordinary
    // malformed selectors are already rejected by the Rust wrapper.
    if (selected < 0 || (uint64_t)selected >= g.edgeuses.size())
        return SC_ERR_ARG;
    SmEdgeuse* p = g.edgeuses[(size_t)selected];
    switch (op)
    {
    case OP_CREATE:
    {
        SmBoolean want_dev = rd.bflag();
        bool has_fail = rd.flag();
        // CreateUVTrimCurve starts with GetBrep() and NER(GetFaceuse()); a wire edgeuse
        // returns SM_ERR_NULL_POINTER before anything else is touched.
        if (p->GetFaceuse() == nullptr)
        {
            SmBSplineCurve* c = nullptr;
            double d = -1.0;
            SmStatus s = p->CreateUVTrimCurve(d, c, NULL, NULL);
            o.push_back(st(s));
            delete c;
            break;
        }
        if (!loop_safe(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* mate = p->GetMate();
        SmDropCurveFail fail;
        double dist = 0.0, dev = 0.0;
        SmBSplineCurve* c = nullptr;
        SmStatus s = p->CreateUVTrimCurve(dist, c, want_dev ? &dev : NULL, has_fail ? &fail : NULL);
        o.push_back(st(s));
        o.push_back(c ? dist : -1.0);
        o.push_back(want_dev && c ? dev : -1.0);
        sc_trim_curve::put_trim_curve(o, c);
        o.push_back(has_fail ? (double)fail.m_eCreatePathType : -1.0);
        o.push_back(has_fail && fail.m_pBrep ? 1.0 : 0.0);
        put_states(o, g, p, mate);
        if (c)
        {
            c->SetOwner(NULL);
            delete c;
        }
        break;
    }
    case OP_GET_OR_CREATE:
    {
        SmBoolean ok_make = rd.bflag();
        SmBoolean want_dist = rd.bflag();
        SmBoolean want_dev = rd.bflag();
        // GetUVTrimCurve (GetMate) at the top
        if (!has_prim(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (p->GetUVTrimCurve() == nullptr && ok_make && p->GetFaceuse() != nullptr && !loop_safe(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (p->GetUVTrimCurve() != nullptr && want_dist && p->IsMaxEdgeFaceGapInit() && !p->IsLoopEdgeuse())
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* mate = p->GetMate();
        SmCurve* uv = nullptr;
        double dist = 0.0, dev = 0.0;
        SmStatus s = p->GetOrCreateUVTrimCurve(uv, want_dist ? &dist : NULL, want_dev ? &dev : NULL, ok_make);
        o.push_back(st(s));
        o.push_back(want_dist ? dist : -2.0);
        o.push_back(want_dev ? dev : -2.0);
        o.push_back(uv ? 1.0 : 0.0);
        put_states(o, g, p, mate);
        break;
    }
    case OP_BINORMAL:
    {
        double frac = rd.num();
        SmBoolean use3d = rd.bflag();
        bool want_uv_vec = rd.flag();
        if (p->GetEdge() == nullptr || p->GetEdge()->GetCurve() == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (p->IsLoopEdgeuse() && !loop_safe(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* mate = has_prim(p) ? p->GetMate() : nullptr;
        double t = p->GetEdge()->GetInterval().Evaluate(frac);
        SmPoint3d pnt(0, 0, 0);
        SmVector3d bin(0, 0, 0), tan(0, 0, 0), nrm(0, 0, 0);
        SmVector2d uv(0, 0), uvvec(0, 0);
        SmStatus s = p->EvaluateBinormal(t, use3d, pnt, bin, &tan, &nrm, &uv, want_uv_vec ? &uvvec : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, pnt);
            put_vec(o, bin);
            put_vec(o, tan);
            put_vec(o, nrm);
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back(uvvec.x);
            o.push_back(uvvec.y);
        }
        put_states(o, g, p, mate);
        break;
    }
    case OP_NORMALIZED:
    {
        double t = rd.num();
        SmBoolean space = rd.bflag();
        bool want_deriv = rd.flag();
        SmBoolean ok_make = rd.bflag();
        if (p->GetEdge() == nullptr || p->GetEdge()->GetCurve() == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (space && !has_prim(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (space && p->GetUVTrimCurve() == nullptr && ok_make && p->GetFaceuse() != nullptr && !loop_safe(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (space && p->GetUVTrimCurve() == nullptr && (!ok_make || p->GetFaceuse() == nullptr) &&
            (p->GetFace() == nullptr || p->GetFace()->GetSurface() == nullptr))
        {
            // the 3D fallback dereferences the face's surface
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* mate = has_prim(p) ? p->GetMate() : nullptr;
        SmPoint3d pnt(0, 0, 0);
        SmVector3d d1(0, 0, 0);
        SmStatus s = p->NormalizedEvaluate(t, space, pnt, want_deriv ? &d1 : NULL, ok_make);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, pnt);
            put_vec(o, d1);
        }
        put_states(o, g, p, mate);
        break;
    }
    default:
        break;
    }
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_topology_edgeuse_p3_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                              double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    const int32_t status = run_op(op, rd, path, o);
    if (status != SC_OK)
        return status;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
