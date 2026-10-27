// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.face_p3`: the SmFace members owned by the unit
// (source/SMLib/src/SmFace.cpp) on real SmBrep objects.
//
// One entry point, sc_topology_face_p3_eval(op, path, in, n_in, args, n_args, out, cap,
// out_len). `in` is a B-rep block of sc_topology_fixture.h (validated by the FFI wrapper,
// crates/solidcore-ffi/src/units/topology_face_p3.rs, which documents every layout);
// `args` are the op's parameters, with indices into the export's traversal order. Op 0
// exports each face's surface type and each edge's curve type (the graph itself is the
// `topology.vertexuse` export of the same block); the other ops output `-1 status` when
// the B-rep could not be built, else `0` and the op's results.
#include "sc_topology_fixture.h"
#include "sc_trim_curve_record.h"

#include <SmCompositeCurve.h>

namespace
{
enum P3Op : int32_t
{
    P3_TYPES = 0,
    P3_LAMINA = 1,
    P3_SET_TOL = 2,
    P3_NOTIFY = 3,
    P3_LOOP_ORIENT = 4,
    P3_DEGENERATE = 5,
    P3_COMPOSITES = 6,
    P3_UV_TRIM = 7,
    P3_INTERSECT = 8,
};

// Tolerances of every face, edge and vertex, in traversal order.
void put_tols(std::vector<double>& o, Graph& g)
{
    for (SmFace* p : g.faces)
        o.push_back((double)(p->*FPeek::tol()));
    for (SmEdge* p : g.edges)
        o.push_back((double)p->GetTolerance());
    for (SmVertex* p : g.vertices)
        o.push_back((double)p->GetTolerance());
}

// The state Notify clears: per edge use, UV trim curve present and cached gaps
// initialised; per vertex use, cached gaps initialised.
void put_eu_state(std::vector<double>& o, Graph& g)
{
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.push_back(p->*EuPeek::uv() ? 1.0 : 0.0);
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.push_back(flg((p->*VuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*VuPeek::edge_gap()).IsInit()));
    }
}

// A curve's natural interval and its points at 0, 1/2 and 1 of it (status, point).
void put_curve_samples(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int k = 0; k <= 2; k++)
    {
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus s = c->EvaluatePoint(ivl.Evaluate(k / 2.0), p);
        o.push_back(st(s));
        put_vec(o, p);
    }
}

const SmNotifyOperation NOTIFY_OPS[] = {SM_NO_PRE_EDIT, SM_NO_POST_EDIT, SM_NO_CHANGE_GEOMETRY,
                                        SM_NO_ADD_TO_BREP, SM_NO_RM_FROM_BREP};

// Whether the face has a surface and every edge of its loops a curve: the geometric ops
// dereference them without checks (GetSurface()->..., pEdge->GetCurve()->...), and only
// the target face(s) and their loops' edges.
bool face_geometry_complete(SmFace* f)
{
    if (f->*FPeek::srf() == nullptr)
        return false;
    SmTArray<SmEdge*> edges;
    f->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); i++)
        if (edges[i] == nullptr || edges[i]->GetCurve() == nullptr)
            return false;
    return true;
}

// ComputeLoopOrientation(pLoop, .., bDo3D) on face f reads pLoop's first loop use (m_pLU)
// and returns at once for a vertex loop. An edge loop's edge uses evaluate their edges'
// curves (NormalizedEvaluate, GetOrCreateUVTrimCurve) and the 3D walk copies f's
// surface; creating a missing UV trim curve (bDo3D FALSE) builds the trimmed surface cache
// of the loop's own face, whose CreateUVTrimCurves reads that face's surface and every
// edge curve of it. The loop may lie on another face than f.
bool loop_geometry_complete(SmFace* f, SmLoop* l, bool do_3d)
{
    SmLoopuse *lu1 = nullptr, *lu2 = nullptr;
    l->GetLoopuses(lu1, lu2);
    if (lu1 == nullptr)
        return false;
    if (lu1->IsVertexLoopuse())
        return true;
    if (f->*FPeek::srf() == nullptr)
        return false;
    SmTArray<SmEdgeuse*> eus;
    lu1->GetEdgeuses(eus);
    for (ULONG i = 0; i < eus.GetSize(); i++)
        if (eus[i] == nullptr || eus[i]->GetEdge() == nullptr || eus[i]->GetEdge()->GetCurve() == nullptr)
            return false;
    if (do_3d)
        return true;
    SmFaceuse* fu = lu1->GetFaceuse();
    SmFace* lf = fu ? fu->*FuPeek::face() : nullptr;
    return lf != nullptr && face_geometry_complete(lf);
}

bool geometry_complete(int32_t op, Graph& g, const double* a)
{
    if (op == P3_LOOP_ORIENT)
        return loop_geometry_complete(g.faces.at((size_t)a[0]), g.loops.at((size_t)a[1]), a[3] != 0.0);
    if (!face_geometry_complete(g.faces.at((size_t)a[0])))
        return false;
    return op != P3_INTERSECT || face_geometry_complete(g.faces.at((size_t)a[1]));
}

void run(int32_t op, Graph& g, const double* a, std::vector<double>& o)
{
    o.push_back(0.0);
    if (op >= P3_LOOP_ORIENT && !geometry_complete(op, g, a))
    {
        o.push_back(-2.0);
        return;
    }
    switch (op)
    {
    case P3_LAMINA:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        int which = (int)a[1];
        o.push_back(flg(which == 0 ? f->IsLamina() : which == 1 ? f->HasLaminaEdge() : f->HasConsecutiveLaminaEdges()));
        break;
    }
    case P3_SET_TOL:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        f->SetTolerance(a[1], a[2] != 0.0 ? TRUE : FALSE, a[3] != 0.0 ? TRUE : FALSE);
        put_tols(o, g);
        break;
    }
    case P3_NOTIFY:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        f->Notify(NOTIFY_OPS[(size_t)a[1]], nullptr, nullptr, nullptr);
        o.push_back((double)(f->*FPeek::rect()));
        put_eu_state(o, g);
        break;
    }
    case P3_LOOP_ORIENT:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        SmLoop* l = g.loops.at((size_t)a[1]);
        SmOrientType orient = SM_OT_UNKNOWN;
        SmExtent2d bounds;
        SmStatus s = f->ComputeLoopOrientation(l, orient, a[2] != 0.0 ? &bounds : nullptr,
                                               a[3] != 0.0 ? TRUE : FALSE);
        o.push_back(st(s));
        o.push_back((double)orient);
        if (a[2] != 0.0)
        {
            o.push_back(bounds.GetMin().x);
            o.push_back(bounds.GetMin().y);
            o.push_back(bounds.GetMax().x);
            o.push_back(bounds.GetMax().y);
        }
        // the loop's edge uses' UV trim curves may have been created
        put_eu_state(o, g);
        break;
    }
    case P3_DEGENERATE:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        SmZoneTol3d tol = a[3];
        o.push_back(flg(f->IsDegenerate(a[1] != 0.0 ? TRUE : FALSE, a[2] != 0.0 ? &tol : nullptr)));
        break;
    }
    case P3_COMPOSITES:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        SmTArray<SmCompositeCurve*> comps;
        SmStatus s = f->CreateComposites(the_context(), comps);
        o.push_back(st(s));
        o.push_back((double)comps.GetSize());
        for (ULONG i = 0; i < comps.GetSize(); i++)
        {
            SmCompositeCurve* c = comps[i];
            o.push_back((double)c->GetNumSegments());
            SmExtent1d ivl = c->GetNaturalInterval();
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
            o.push_back(flg(c->IsClosed(ivl)));
            for (ULONG k = 0; k < c->GetNumSegments(); k++)
            {
                const SmCompositeCurveSegment* seg = c->GetCurveSegment(k);
                o.push_back((double)seg->m_eTransition);
                o.push_back(flg(seg->m_bSameSense));
                o.push_back(seg->m_dGapStart);
                o.push_back(seg->m_dGapEnd);
                put_curve_samples(o, seg->m_pParentCurve);
            }
            delete c;
        }
        break;
    }
    case P3_UV_TRIM:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        double g1 = 0, g2 = 0, g3 = 0, g4 = 0, g5 = 0, g6 = 0;
        // a[2] != 0: hand the face's own UV trim curves (copies, with their edge use's
        // orientation) to CreateUVTrimCurves after clearing them, so that
        // sm_FindBestUVCurve / CheckAndSetUVTrimCurve match them to the edge uses again.
        SmTArray<SmBSplineCurve*> uvs;
        SmTArray<SmOrientType> ors;
        const bool opt = a[2] != 0.0;
        if (opt)
        {
            std::vector<SmEdgeuse*> fes;
            for (SmEdgeuse* eu : g.edgeuses)
                if (eu->IsLoopEdgeuse() && eu->GetFace() == f)
                    fes.push_back(eu);
            for (SmEdgeuse* eu : fes)
                if (SmBSplineCurve* c = eu->*EuPeek::uv())
                {
                    uvs.Add(new (the_context()) SmBSplineCurve(*c));
                    ors.Add(eu->GetOrientation());
                }
            for (SmEdgeuse* eu : fes)
                eu->DeleteUVTrimCurve();
        }
        SmStatus s = f->CreateUVTrimCurves(a[1] != 0.0 ? TRUE : FALSE, opt ? &uvs : nullptr, opt ? &ors : nullptr,
                                           g1, g2, g3, g4, g5, g6);
        o.push_back(st(s));
        for (double x : {g1, g2, g3, g4, g5, g6})
            o.push_back(x);
        put_tols(o, g);
        put_eu_state(o, g);
        for (SmEdgeuse* eu : g.edgeuses)
            sc_trim_curve::put_trim_curve(o, eu->*EuPeek::uv());
        break;
    }
    case P3_INTERSECT:
    {
        SmFace* f = g.faces.at((size_t)a[0]);
        SmFace* h = g.faces.at((size_t)a[1]);
        // a[4]: -1 = no coincidence flag (run CoincidenceCheck), 0 = FALSE, 1 = TRUE
        SmBoolean flag = a[4] > 0.5 ? TRUE : FALSE;
        SmTArray<SmCurve*> curves;
        SmTArray<SmPoint3d> points;
        SmStatus s = f->FaceIntersect(the_context(), h, a[2], a[3], curves, nullptr, nullptr,
                                      a[5] != 0.0 ? &points : nullptr, nullptr, nullptr, nullptr, nullptr,
                                      nullptr, a[4] >= 0.0 ? &flag : nullptr);
        o.push_back(st(s));
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            put_curve_samples(o, curves[i]);
            delete curves[i];
        }
        o.push_back((double)points.GetSize());
        for (ULONG i = 0; i < points.GetSize(); i++)
            put_vec(o, points[i]);
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_face_p3_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           const double* args, int64_t n_args, double* out, int64_t cap,
                                           int64_t* out_len)
{
    if (!path || !in || n_in < 1 || !out_len || op < P3_TYPES || op > P3_INTERSECT || n_args < 0 ||
        (op != P3_TYPES && n_args < 6) || (n_args > 0 && !args))
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    SmStatus status;
    Built built;
    built.brep = build(rd, path, status);
    std::vector<double> o;
    if (!built.brep)
    {
        o.push_back(-1);
        o.push_back(st(status));
    }
    else
    {
        Graph g;
        walk(built.brep, g);
        if (op == P3_TYPES)
        {
            // `0 n_f type(surface)* n_e type(curve)*`, -1 for none
            o.push_back(0.0);
            o.push_back((double)g.faces.size());
            for (SmFace* p : g.faces)
            {
                SmSurface* s = p->*FPeek::srf();
                o.push_back(s ? (double)s->GetType() : -1.0);
            }
            o.push_back((double)g.edges.size());
            for (SmEdge* p : g.edges)
                o.push_back(p->GetCurve() ? (double)p->GetCurve()->GetType() : -1.0);
        }
        else
            run(op, g, args, o);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
