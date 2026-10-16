// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.edgeuse_p2`: the SmEdgeuse members owned by the unit
// (source/SMLib/src/SmEdgeuse.cpp) on real SmBrep objects: IsTangentSector,
// IsConvexRadialSector, EvaluateBinormalStepOff, RebuildUVTrimCurve,
// CheckAndSetUVTrimCurve and Notify.
//
// One entry point, sc_topology_edgeuse_p2_eval(op, path, in, n_in, out, cap, out_len).
// Every op starts with the B-rep block of unit topology.edgeuse_p1 (built and exported by
// the shared sc_edgeuse_graph.h; the export itself is topology.edgeuse_p1's Export op).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_edgeuse_p2.rs, which
// validates every input. Outputs start with `0`, or `-1 status` when the B-rep could not
// be built. `-2` is "upstream would dereference null": the op (through EvaluateBinormal,
// topology.edgeuse_p3) would dereference a null pointer, so the shim does not call it.
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
#include <SmVertexuse.h>

#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
enum Op2 : int32_t
{
    OP_TANGENT = 1,
    OP_CONVEX = 2,
    OP_STEPOFF = 3,
    OP_REBUILD = 4,
    OP_CHECK_SET = 5,
    OP_NOTIFY = 6,
};

const double ST_NULL_DEREF = -2.0;

// GetMate/GetRadial/GetUVTrimCurve need the edge's primary edgeuse (loop edgeuses).
bool has_prim(SmEdgeuse* p)
{
    return p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
}

// A loop edgeuse whose binormal upstream evaluates without dereferencing null: an edge
// with a curve, a face use, a face with a surface, a vertexuse.
bool binormal_safe(SmEdgeuse* p)
{
    return p != nullptr && p->IsLoopEdgeuse() && p->GetEdge() && p->GetEdge()->GetCurve() && p->GetFaceuse() &&
           p->GetFace() && p->GetFace()->GetSurface() && p->GetVertexuse() && has_prim(p);
}

// EvaluateBinormal returns SM_ERR_INVALID_INPUT for an edge use that is not in a loop before
// touching anything; otherwise the loop edge use must be binormal_safe.
bool eval_safe(SmEdgeuse* p) { return p != nullptr && (!p->IsLoopEdgeuse() || binormal_safe(p)); }

// Both members of a radial sector are safe.
bool sector_safe(SmEdgeuse* p) { return eval_safe(p) && eval_safe(p->GetRadial()); }

void put_gap_flags(std::vector<double>& o, SmEdgeuse* p)
{
    o.push_back(p ? flg((p->*EuPeek::face_gap()).IsInit()) : -1.0);
}

void run_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
    switch (op)
    {
    case OP_TANGENT:
    {
        double tol = rd.num();
        SmBoolean coin = rd.bflag();
        SmBoolean ok_make = rd.bflag();
        SmEdge* e = p->GetEdge();
        if (e == nullptr || e->IsLamina())
            o.push_back(flg(p->IsTangentSector(tol, coin, ok_make)));
        else if (!has_prim(p))
            o.push_back(ST_NULL_DEREF);
        else if (!sector_safe(p))
            o.push_back(ST_NULL_DEREF);
        else
            o.push_back(flg(p->IsTangentSector(tol, coin, ok_make)));
        break;
    }
    case OP_CONVEX:
    {
        ULONG n = (ULONG)rd.idx();
        SmBoolean step = rd.bflag();
        double tol = rd.num();
        if (!has_prim(p) || p->GetEdge() == nullptr)
            o.push_back(ST_NULL_DEREF);
        else if (n > 0 && p->IsLoopEdgeuse() && !sector_safe(p))
            o.push_back(ST_NULL_DEREF);
        else
            o.push_back(flg(p->IsConvexRadialSector(n, step, tol)));
        break;
    }
    case OP_STEPOFF:
    {
        double frac = rd.num();
        double dist = rd.num();
        bool has_c = rd.flag();
        SmPoint3d c = rd.vec();
        bool has_n = rd.flag();
        SmVector3d n = rd.vec();
        SmBoolean ok_make = rd.bflag();
        if (p->GetEdge() == nullptr || !eval_safe(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        double t = p->GetEdge()->GetInterval().Evaluate(frac);
        SmPoint3d pnt(0, 0, 0);
        SmVector3d bin(0, 0, 0), tan(0, 0, 0), nrm(0, 0, 0);
        SmVector2d uv(0, 0);
        SmStatus s = p->EvaluateBinormalStepOff(t, dist, pnt, bin, &tan, &nrm, has_c ? &c : NULL,
                                                has_n ? &n : NULL, &uv, ok_make);
        o.push_back(st(s));
        // the out-params are compared whatever the status: a failing call leaves what it
        // wrote before failing (zeros from EvaluateBinormal's init, the edge point's
        // values once that succeeded)
        put_vec(o, pnt);
        put_vec(o, bin);
        put_vec(o, tan);
        put_vec(o, nrm);
        o.push_back(uv.x);
        o.push_back(uv.y);
        break;
    }
    case OP_REBUILD:
    {
        // a wire edge use has no face use: CreateUVTrimCurve returns its status first
        if (p->GetFaceuse() != nullptr && !binormal_safe(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(st(p->RebuildUVTrimCurve()));
        put_eu_state(o, g, p);
        break;
    }
    case OP_CHECK_SET:
    {
        const SmContext& ctx = the_context();
        SmBoolean adjust = rd.bflag();
        bool has_fail = rd.flag();
        int64_t fail_status = rd.idx();
        SmBSplineCurve* nc = read_curve(rd, ctx);
        SmDropCurveFail fail;
        fail.m_sFailStatus = (SmStatus)fail_status;
        // GetUVTrimCurve (GetMate) at the top; GetFace()->GetSurface() and
        // GetEdge()->GetCurve() are dereferenced on every path.
        if (!has_prim(p) || p->GetFace() == nullptr || p->GetEdge() == nullptr)
        {
            delete nc;
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* mate = p->GetMate();
        // SetUVTrimCurve dereferences both vertexuses; SmCrvOnSurf the surface; the
        // distance measurement the edge curve.
        if (p->GetVertexuse() == nullptr || (mate && mate->GetVertexuse() == nullptr) ||
            p->GetFace()->GetSurface() == nullptr || p->GetEdge()->GetCurve() == nullptr)
        {
            delete nc;
            o.push_back(ST_NULL_DEREF);
            break;
        }
        double dmax = -1.0;
        SmBoolean replaced = 7;
        // nc is attached or deleted by CheckAndSetUVTrimCurve (see the Rust module docs)
        SmStatus s = p->CheckAndSetUVTrimCurve(nc, adjust, dmax, replaced, has_fail ? &fail : NULL);
        o.push_back(st(s));
        o.push_back(dmax);
        o.push_back(flg(replaced));
        put_eu_state(o, g, p);
        put_eu_state(o, g, mate);
        o.push_back((double)p->GetEdge()->GetTolerance());
        o.push_back(has_fail && fail.m_pBrep ? 1.0 : 0.0);
        break;
    }
    case OP_NOTIFY:
    {
        SmNotifyOperation e = (SmNotifyOperation)rd.idx();
        bool split = e == SM_NO_SPLIT_IN_BREP || e == SM_NO_CHANGE_GEOMETRY || e == SM_NO_SPLIT;
        SmEdgeuse* mate = has_prim(p) ? p->GetMate() : nullptr;
        if (split)
        {
            // SetUVTrimCurve(NULL) dereferences GetMate() and, with a curve to remove,
            // both vertexuses; then the mate is dereferenced
            if (!has_prim(p))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            bool cur = p->GetUVTrimCurve() != nullptr;
            if ((cur && (p->GetVertexuse() == nullptr || (mate && mate->GetVertexuse() == nullptr))) ||
                mate == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        p->Notify(e, NULL, NULL, NULL);
        o.push_back(0.0);
        put_eu_state(o, g, p);
        put_eu_state(o, g, mate);
        put_gap_flags(o, p);
        put_gap_flags(o, mate);
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_edgeuse_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                              double* out, int64_t cap, int64_t* out_len)
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
