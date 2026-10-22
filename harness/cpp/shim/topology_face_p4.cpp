// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.face_p4`: the SmFace members owned by the unit
// (source/SMLib/src/SmFace.cpp, source/SMLib/inc/SmFace.h) on real SmBrep objects.
//
// One entry point, sc_topology_face_p4_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_face_p4.rs, which
// validates every input. The input is a face selector (index into the traversal-order
// face list), the op's fixed-size arguments, then the B-rep block of unit
// topology.vertexuse (built by the shared sc_topology_fixture.h), whose graph that unit's
// Export op exports for the Rust import. As the topology.face_p1 shim, the faces and
// edges get the plain B-spline copies of analytic geometry (what the Rust import
// rebuilds), keeping the owners, before the op.
//
// Output: `0` (or `-1 status` when the B-rep build failed), then the op record. A slot of
// `-2` marks a call the shim skips because upstream would dereference null.
#include "sc_topology_fixture.h"

#include <SmCurveClass.h>
#include <SmFaceProps.h>
#include <SmSolutionArray.h>

namespace
{
enum : int32_t
{
    FP4_OP_BREP = 1,
    FP4_OP_EDGES = 2,
    FP4_OP_VERTICES = 3,
    FP4_OP_UP_LOOPUSES = 4,
    FP4_OP_UP_VERTEXUSES = 5,
    FP4_OP_UP_EDGEUSES = 6,
    FP4_OP_CONNECTED = 7,
    FP4_OP_LINE = 8,
    FP4_OP_SECTOR = 9,
    FP4_OP_SEAM = 10,
};

const double FP4_NULL_DEREF = -2.0;

struct Fp4EdgePeek : SmEdge
{
    static SmCurve* SmEdge::*curve() { return &Fp4EdgePeek::m_pCurve; }
};

// The Rust import rebuilds NURBs as plain B-splines (and lines, planes) and the analytic
// surfaces the export sends exactly (sc_analytic_export.h) as their own kind: give every
// other face and edge of the C++ B-rep the same geometry (as heal_gap.cpp's normalise).
// The analytic originals are leaked (they may still be referenced elsewhere).
void fp4_normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*FPeek::srf();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        // exported exactly: the Rust side runs the same analytic surface
        std::vector<double> head;
        if (sc_analytic_export::put_exact_head(head, s, the_context()))
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb = new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        bool owned = s->GetOwner() == f;
        f->*FPeek::srf() = nb;
        if (owned)
        {
            s->SetOwner(NULL);
            nb->SetOwner(f);
        }
    }
    for (SmEdge* e : g.edges)
    {
        SmCurve* c = e->*Fp4EdgePeek::curve();
        if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE) || c->GetType() == SmBSplineCurve_TYPE ||
            c->GetType() == SmLine_TYPE)
            continue;
        SmBSplineCurve* b = (SmBSplineCurve*)c;
        gw_CURVE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineCurve* nc = new (the_context()) SmBSplineCurve(sm_AllocateAndCopyNurbCurve(nurb), b->GetDim(), FALSE);
        bool owned = c->GetOwner() == e;
        e->*Fp4EdgePeek::curve() = nc;
        if (owned)
        {
            c->SetOwner(NULL);
            nc->SetOwner(e);
        }
    }
}

// The number of op arguments after the face selector.
int64_t fp4_nargs(int32_t op)
{
    switch (op)
    {
    case FP4_OP_EDGES:
    case FP4_OP_VERTICES:
    case FP4_OP_UP_LOOPUSES:
    case FP4_OP_UP_VERTEXUSES:
    case FP4_OP_UP_EDGEUSES:
    case FP4_OP_CONNECTED: return 2;
    case FP4_OP_LINE: return 12;
    case FP4_OP_SECTOR: return 9;
    case FP4_OP_SEAM: return 3;
    default: return 0;
    }
}

template <class T> void fp4_put_ids(std::vector<double>& o, Graph& g, int kind, SmTArray<T*>& v)
{
    o.push_back((double)v.GetSize());
    for (ULONG i = 0; i < v.GetSize(); i++)
        o.push_back((double)g.find(kind, v[i]));
}

void fp4_put_class(std::vector<double>& o, const SmCurveClassification& c)
{
    o.push_back(flg(c.GetTgtLoopsOnly()));
    o.push_back((double)c.GetSize());
    for (ULONG i = 0; i < c.GetSize(); i++)
    {
        o.push_back((double)c[i].m_vStart.GetPointClass());
        o.push_back((double)c[i].m_vMid.GetPointClass());
        o.push_back((double)c[i].m_vEnd.GetPointClass());
    }
}

void fp4_brep_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    int64_t fi = rd.idx();
    std::vector<double> args;
    for (int64_t i = 0; i < fp4_nargs(op); i++)
        args.push_back(rd.num());
    Rd ad{args.data(), 0};
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
    fp4_normalise(g);
    if (fi < 0 || fi >= (int64_t)g.faces.size())
    {
        o.push_back((double)SC_ERR_ARG);
        return;
    }
    SmFace* f = g.faces[(size_t)fi];
    switch (op)
    {
    case FP4_OP_BREP:
        o.push_back(flg(f->GetBrep() != NULL));
        break;
    case FP4_OP_EDGES:
    case FP4_OP_VERTICES:
    case FP4_OP_UP_LOOPUSES:
    case FP4_OP_UP_VERTEXUSES:
    case FP4_OP_UP_EDGEUSES:
    {
        SmBoolean tgt_only = ad.bflag();
        int64_t li = ad.idx();
        SmTArray<SmLoop*> tgts;
        if (li >= 0 && li < (int64_t)g.loops.size())
            tgts.Add(g.loops[(size_t)li]);
        // GetLoopuses dereferences a target loop's loop use and its mate
        for (ULONG i = 0; i < tgts.GetSize(); i++)
        {
            SmLoopuse* lu = tgts[i]->GetLoopuse();
            if (tgt_only && f->GetUpwardFaceuse() != nullptr && (lu == nullptr || (lu->GetFaceuse() != f->GetUpwardFaceuse() && lu->GetOtherLoopuse() == nullptr)))
            {
                o.push_back(FP4_NULL_DEREF);
                return;
            }
        }
        const SmTArray<SmLoop*>* pt = li >= 0 ? &tgts : NULL;
        if (op == FP4_OP_EDGES)
        {
            SmTArray<SmEdge*> v;
            f->GetEdges(v, NULL, tgt_only, pt);
            fp4_put_ids(o, g, 7, v);
        }
        else if (op == FP4_OP_VERTICES)
        {
            SmTArray<SmVertex*> v;
            f->GetVertices(v, NULL, tgt_only, pt);
            fp4_put_ids(o, g, 9, v);
        }
        else if (op == FP4_OP_UP_LOOPUSES)
        {
            SmTArray<SmLoopuse*> v;
            f->GetUpwardLoopuses(v, tgt_only, pt);
            fp4_put_ids(o, g, 4, v);
        }
        else if (op == FP4_OP_UP_VERTEXUSES)
        {
            SmTArray<SmVertexuse*> v;
            f->GetUpwardVertexuses(v, tgt_only, pt);
            fp4_put_ids(o, g, 8, v);
        }
        else
        {
            SmTArray<SmEdgeuse*> v;
            f->GetUpwardEdgeuses(v, tgt_only, pt);
            fp4_put_ids(o, g, 6, v);
        }
        break;
    }
    case FP4_OP_CONNECTED:
    {
        // the target's export slot (regions, shells, face uses, faces, loop uses, loops,
        // edge uses, edges, vertex uses, vertices) and index; slot -1 is NULL
        static const SM_TYPE SLOT_TYPES[10] = {SmRegion_TYPE,  SmShell_TYPE, SmFaceuse_TYPE,   SmFace_TYPE,
                                               SmLoopuse_TYPE, SmLoop_TYPE,  SmEdgeuse_TYPE,   SmEdge_TYPE,
                                               SmVertexuse_TYPE, SmVertex_TYPE};
        int64_t slot = ad.idx();
        int64_t ti = ad.idx();
        SmTopology* t = (slot < 0 || slot > 9) ? nullptr : target_of(g, SLOT_TYPES[slot], ti);
        if (slot >= 0 && t == nullptr)
        {
            o.push_back((double)SC_ERR_ARG);
            return;
        }
        // the shell / region / loop use branches dereference without checks: the face
        // uses (the second only past a first that does not match) and, for a region, their
        // shells
        SmFaceuse *fu1 = nullptr, *fu2 = nullptr;
        f->GetFaceuses(fu1, fu2);
        if (t != nullptr)
        {
            SM_TYPE tt = t->GetType();
            bool bad = false;
            if (tt == SmShell_TYPE)
                bad = fu1 == nullptr || (fu1->GetShell() != (SmShell*)t && fu2 == nullptr);
            if (tt == SmRegion_TYPE)
                bad = fu1 == nullptr || fu1->GetShell() == nullptr
                      || (fu1->GetShell()->GetRegion() != (SmRegion*)t
                          && (fu2 == nullptr || fu2->GetShell() == nullptr));
            if (tt == SmLoopuse_TYPE)
                bad = ((SmLoopuse*)t)->GetFaceuse() == nullptr;
            if (bad)
            {
                o.push_back(FP4_NULL_DEREF);
                return;
            }
        }
        o.push_back(flg(f->IsConnectedTo(t)));
        break;
    }
    case FP4_OP_LINE:
    {
        // One reader call per statement: the evaluation order of constructor arguments
        // is unspecified (x86-64 g++ reads them right to left).
        double px = ad.num(), py = ad.num(), pz = ad.num();
        double vx = ad.num(), vy = ad.num(), vz = ad.num();
        SmPoint3d p(px, py, pz);
        SmVector3d v(vx, vy, vz);
        bool has_ivl = ad.flag();
        double t0 = ad.num(), t1 = ad.num();
        SmBoolean fire = ad.bflag();
        bool face_tol = ad.flag();
        double tol = ad.num();
        if (f->GetSurface() == nullptr)
        {
            o.push_back(FP4_NULL_DEREF);
            return;
        }
        SmExtent1d ivl(t0, t1);
        SmSolutionArray sols;
        SmStatus r = f->GlobalLineIntersect(f->GetUVDomain(), p, v, has_ivl ? &ivl : NULL, fire,
                                            face_tol ? (double)f->GetTolerance() : tol, sols);
        o.push_back(st(r));
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
        {
            const SmSolution& s = sols[i];
            o.push_back((double)s.m_eSolutionType);
            for (int k = 0; k < 3; k++)
                o.push_back(s.m_vStart[k]);
            for (int k = 0; k < 3; k++)
                o.push_back(s.m_eSolutionType == SM_ST_RANGE_OF_VALUES ? s.m_vEnd[k] : 0.0);
        }
        break;
    }
    case FP4_OP_SECTOR:
    {
        int64_t vi = ad.idx();
        int64_t ei = ad.idx();
        bool use_uv = ad.flag();
        bool at_start = ad.flag();
        SmOrientType orient = ad.flag() ? SM_OT_OPPOSITE : SM_OT_SAME;
        double tol = ad.num();
        SmBoolean validate = ad.bflag();
        SmBoolean uv_ok = ad.bflag();
        bool face_tol = ad.flag();
        if (vi < 0 || vi >= (int64_t)g.vertices.size() || ei < 0 || ei >= (int64_t)g.edges.size())
        {
            o.push_back((double)SC_ERR_ARG);
            return;
        }
        SmVertex* vx = g.vertices[(size_t)vi];
        SmEdge* e = g.edges[(size_t)ei];
        SmCurve* c = e->GetCurve();
        // a face without surface, an edge without curve: upstream dereferences null
        if (c == nullptr || f->GetSurface() == nullptr)
        {
            o.push_back(FP4_NULL_DEREF);
            return;
        }
        // every vertex use on the upward face use with an edge use needs its edge and
        // CW neighbour (upstream dereferences them)
        SmTArray<SmVertexuse*> vus;
        vx->GetVertexuses(vus);
        for (ULONG i = 0; i < vus.GetSize(); i++)
        {
            SmVertexuse* vu = vus[i];
            if (vu->GetFaceuse() != f->GetUpwardFaceuse() || vu->IsLoopVertexuse())
                continue;
            SmEdgeuse* eu = vu->GetEdgeuse();
            if (eu == nullptr || eu->GetEdge() == nullptr || eu->GetCWEdgeuse() == nullptr ||
                eu->GetCWEdgeuse()->GetEdge() == nullptr)
            {
                o.push_back(FP4_NULL_DEREF);
                return;
            }
        }
        SmEdgeuse* peu = e->GetPrimaryEdgeuse();
        const SmCurve* uvc = (use_uv && peu != nullptr) ? peu->GetUVTrimCurve() : nullptr;
        SmExtent1d ivl = e->GetInterval();
        double at = at_start ? ivl.GetMin() : ivl.GetMax();
        double end = at_start ? ivl.GetMax() : ivl.GetMin();
        SmVertexuse* vu = nullptr;
        SmBoolean problem = FALSE;
        SmClassifyProblem cause = SM_CP_NONE;
        SmStatus r = f->FindVertexSector(vx, *c, uvc, at, end, orient, face_tol ? (double)f->GetTolerance() : tol,
                                         validate, vu, problem, cause, uv_ok);
        o.push_back(st(r));
        o.push_back((double)g.find(8, vu));
        o.push_back(flg(problem));
        o.push_back((double)cause);
        break;
    }
    case FP4_OP_SEAM:
    {
        SmBoolean outer = ad.bflag();
        int64_t pu = ad.idx();
        int64_t pv = ad.idx();
        SmBoolean bu = pu == 1 ? TRUE : FALSE, bv = pv == 1 ? TRUE : FALSE;
        SmCurveClassification cu, cv;
        SmSurfParamType crossed = SM_SP_UNKNOWN, missing = SM_SP_UNKNOWN, near = SM_SP_UNKNOWN;
        SmBoolean has = f->HasSeamProblem(cu, cv, crossed, missing, near, outer, pu < 0 ? NULL : &bu,
                                          pv < 0 ? NULL : &bv, NULL);
        o.push_back(flg(has));
        o.push_back((double)crossed);
        o.push_back((double)missing);
        o.push_back((double)near);
        fp4_put_class(o, cu);
        fp4_put_class(o, cv);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_face_p4_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    fp4_brep_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
