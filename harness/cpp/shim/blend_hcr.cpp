// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.hcr`: the SmHCR members (source/SMLib/src/SmHCR.cpp,
// source/SMLib/inc/SmHCR.h) on real SmBrep objects.
//
// One entry point, sc_blend_hcr_eval(op, path, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/blend_hcr.rs, which validates every input.
// The input is the B-rep block of unit topology.vertexuse (sc_topology_fixture.h:
// file, wire edge, face from surface, wire network), then
//   `0 | 1 curve t0 t1`                    an extra wire edge (CreateWireEdgeFromCurve)
//   `view_mode V[3] | O[3] Z[3] Xref[3]`  0: SmHCR(.., crViewVector, ..); 1: SmHCR(..,
//                                         placement SetSTEPCanonical(O, Z, Xref), ..)
//   `approx_tol ang_tol seams smooth`
//   (ops other than 8) `n_sil sil[n_sil] n_back back[n_back]` scene edge indices seeded as silhouette edges
//                                         (the silhouette branch of ComputeSilhouettes'
//                                         edge loop, replicated below) and back-facing
//   op arguments (see below).
// The output is `status record_len record[..] n_e curve_type[n_e] scene_box(6)
// scene_export`, the scene being
// GetSceneBrep() right after construction (the export of sc_topology_fixture.h) and
// scene_box its SmBrep::CalculateBoundingBox. Private members are read through
// explicitly instantiated member pointers. Ops whose upstream path reads an
// uninitialised or out-of-range value (UB-170: more silhouette vertices than edges in a
// curve's box; an edge without curve) output status -2 instead of calling it.
#include "sc_topology_fixture.h"
#include <SmAxis2Placement.h>
#include <SmCurveClass.h>
#include <SmHCR.h>
#include <SmMapPtrToPtr.h>
#include <SmPointClass.h>
#include <SmTree.h>
#include <algorithm>
#include <memory>

namespace
{
template <class Tag, typename Tag::type M> struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};
#define SC_HCR_MEMBER(name, T)                                                                     \
    struct Hcr_##name                                                                              \
    {                                                                                              \
        typedef T SmHCR::*type;                                                                    \
        friend type get(Hcr_##name);                                                               \
    };                                                                                             \
    template struct Rob<Hcr_##name, &SmHCR::name>;
SC_HCR_MEMBER(m_vViewVector, SmVector3d)
SC_HCR_MEMBER(m_dRayOffsetTolerance, double)
SC_HCR_MEMBER(m_vViewTransform, SmAxis2Placement)
SC_HCR_MEMBER(m_vInvViewTransform, SmAxis2Placement)
SC_HCR_MEMBER(m_vProjBBox, SmExtent3d)
SC_HCR_MEMBER(m_pSilVertexTree, SmTree*)
SC_HCR_MEMBER(m_pSilEdgeTree, SmTree*)
typedef SmMapPtrToPtr<SmVertex, SmVertex>* VMapPtr;
typedef SmMapPtrToPtr<SmEdge, SmEdge>* EMapPtr;
SC_HCR_MEMBER(m_pSilVertices, VMapPtr)
SC_HCR_MEMBER(m_pSilEdges, EMapPtr)
SC_HCR_MEMBER(m_pBackFacingEdges, EMapPtr)
#undef SC_HCR_MEMBER

void put_axis(std::vector<double>& o, const SmAxis2Placement& a)
{
    put_vec(o, a.GetOrigin());
    put_vec(o, a.GetXAxis());
    put_vec(o, a.GetYAxis());
    put_vec(o, a.GetZAxis());
}

void put_box(std::vector<double>& o, const SmExtent3d& b)
{
    put_vec(o, b.GetMin());
    put_vec(o, b.GetMax());
}

// The silhouette branch of SmHCR::ComputeSilhouettes' edge loop (SmHCR.cpp:1378-1402).
void seed_silhouette(SmHCR& h, SmEdge* pEdge)
{
    SmVertex* pV1 = pEdge->GetVertex();
    SmVertex* pV2 = pEdge->GetOtherVertex(pV1);
    SmExtent3d sEBBox;
    SE(pEdge->GetCurve()->CalculateBoundingBox(pEdge->GetInterval(), &sEBBox));
    (h.*get(Hcr_m_pSilEdges()))->Insert(pEdge, pEdge);
    SE((h.*get(Hcr_m_pSilEdgeTree()))->AddToSpatialTree(sEBBox, pEdge));
    if (pV1 != NULL)
    {
        SmExtent3d sVBBox1(pV1->GetPoint());
        (h.*get(Hcr_m_pSilVertices()))->Insert(pV1, pV1);
        SE((h.*get(Hcr_m_pSilVertexTree()))->AddToSpatialTree(sVBBox1, pV1));
    }
    if (pV2 != NULL)
    {
        SmExtent3d sVBBox2(pV2->GetPoint());
        (h.*get(Hcr_m_pSilVertices()))->Insert(pV2, pV2);
        SE((h.*get(Hcr_m_pSilVertexTree()))->AddToSpatialTree(sVBBox2, pV2));
    }
}

// UB-170: IntersectCurveWithSilhouettes reads its edge objects up to the count of
// vertex objects in the curve's box (SmHCR.cpp:1972-1977, 1991, 2120-2121).
bool icws_undefined(SmHCR& h, const SmCurveClassification& cc)
{
    SmExtent3d sEBBox;
    if (cc.GetCurve()->CalculateBoundingBox(cc.GetInterval(), &sEBBox) != SM_SUCCESS)
        return false; // returns before the loops
    SmPoint3d sCorner = sEBBox.GetMax();
    sCorner.z = (h.*get(Hcr_m_vProjBBox())).GetMax().z;
    sEBBox.AddPoint3d(sCorner);
    sEBBox.ExpandAbsolute(SmTol::GetXSectTol3d(&cc, &cc));
    SmTArray<SmObject*> v, e;
    if ((h.*get(Hcr_m_pSilVertexTree()))->GetObjectsInBox(sEBBox, v) != SM_SUCCESS)
        return false;
    if ((h.*get(Hcr_m_pSilEdgeTree()))->GetObjectsInBox(sEBBox, e) != SM_SUCCESS)
        return false;
    return v.GetSize() > e.GetSize();
}

int64_t obj_kind(const SmObject* p)
{
    if (p == nullptr)
        return 0;
    if (p->IsKindOf(SmVertex_TYPE))
        return 1;
    if (p->IsKindOf(SmEdge_TYPE))
        return 2;
    if (p->IsKindOf(SmRegion_TYPE))
        return 3;
    if (p->IsKindOf(SmCurve_TYPE))
        return 4;
    return 5;
}

void put_pc(std::vector<double>& o, Graph& g, const SmPointClassification& pc)
{
    o.push_back((double)pc.GetPointClass());
    const SmObject* p = pc.GetObject();
    int64_t k = obj_kind(p);
    o.push_back((double)k);
    o.push_back(k == 1   ? (double)g.find(9, (const SmVertex*)p)
                : k == 2 ? (double)g.find(7, (const SmEdge*)p)
                : k == 3 ? (double)g.find(0, (const SmRegion*)p)
                         : -1.0);
    o.push_back(pc.GetPreSnapParam());
    o.push_back(pc.GetTParam());
}

void put_cc(std::vector<double>& o, Graph& g, SmCurveClassification& cc)
{
    o.push_back((double)cc.GetSize());
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        SmCurveInterval& c = cc[i];
        o.push_back(c.m_vInterval.GetMin());
        o.push_back(c.m_vInterval.GetMax());
        put_pc(o, g, c.m_vStart);
        put_pc(o, g, c.m_vMid);
        put_pc(o, g, c.m_vEnd);
    }
}

// A curve piece: natural interval and 5 points.
void put_piece(std::vector<double>& o, SmCurve* c)
{
    SmExtent1d n = c->GetNaturalInterval();
    o.push_back(n.GetMin());
    o.push_back(n.GetMax());
    for (int i = 0; i < 5; i++)
    {
        SmPoint3d p(0, 0, 0);
        SmStatus s = c->EvaluatePoint(n.Evaluate(i / 4.0), p);
        o.push_back(st(s));
        put_vec(o, p);
    }
}

void put_pieces(std::vector<double>& o, SmHCR& h)
{
    SmTArray<SmCurve*> v, iv;
    h.GetVisibleCurves(v);
    h.GetInvisibleCurves(iv);
    o.push_back((double)v.GetSize());
    for (ULONG i = 0; i < v.GetSize(); i++)
        put_piece(o, v[i]);
    o.push_back((double)iv.GetSize());
    for (ULONG i = 0; i < iv.GetSize(); i++)
        put_piece(o, iv[i]);
}

void put_sorted(std::vector<double>& o, Graph& g, SmTArray<SmEdge*>& edges)
{
    std::vector<int64_t> ids;
    for (ULONG i = 0; i < edges.GetSize(); i++)
        ids.push_back(g.find(7, edges[i]));
    std::sort(ids.begin(), ids.end());
    o.push_back((double)ids.size());
    for (int64_t i : ids)
        o.push_back((double)i);
}
} // namespace

// op: 0 construction state, 1 IsSilhouetteEdge(e), 2 IsBackFacingEdge(e),
// 3 IntersectCurveWithSilhouettes(e, fa, fb), 4 ComputeCurveVisibility(e, fa, fb, hints),
// 5 ComputeEdgeVisibility(e, vertex, visible, premark), 6 GetSilhouetteEdges,
// 7 IsPointVisible(P), 8 the scene only.
SC_EXPORT int32_t sc_blend_hcr_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                    double* out, int64_t cap, int64_t* out_len)
{
    if (op < 0 || op > 8 || in == nullptr || n_in <= 0)
        return SC_ERR_ARG;
    SC_TRY
    const SmContext& ctx = the_context();
    Rd rd{in, 0};
    SmStatus build_status;
    Built built;
    built.brep = build(rd, path, build_status);
    if (!built.brep)
        return SC_ERR_ARG;
    if (rd.flag())
    {
        // the extra wire edge
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t0 = rd.num(), t1 = rd.num();
        SmEdge* pExtra = nullptr;
        if (built.brep->CreateWireEdgeFromCurve(c, SmExtent1d(t0, t1), pExtra) != SM_SUCCESS)
            return SC_ERR_ARG;
    }

    int64_t view_mode = rd.idx();
    SmVector3d a = rd.vec();
    SmAxis2Placement placement;
    if (view_mode == 1)
    {
        SmVector3d z = rd.vec();
        SmVector3d x = rd.vec();
        SE(placement.SetSTEPCanonical(a, z, x));
    }
    double approx_tol = rd.num();
    double ang_tol = rd.num();
    SmBoolean seams = rd.bflag();
    SmBoolean smooth = rd.bflag();
    std::unique_ptr<SmHCR> ph(view_mode == 0 ? new SmHCR(ctx, built.brep, a, approx_tol, ang_tol, seams, smooth)
                                             : new SmHCR(ctx, built.brep, placement, approx_tol, ang_tol, seams, smooth));
    SmHCR& h = *ph;
    SmBrep* scene = h.GetSceneBrep();
    Graph g;
    walk(scene, g);
    SmExtent3d scene_box;
    scene->CalculateBoundingBox(scene_box);
    std::vector<double> scene_ex;
    scene_ex.push_back((double)g.edges.size());
    for (SmEdge* e : g.edges)
        scene_ex.push_back(e->GetCurve() ? (double)e->GetCurve()->GetType() : -1.0);
    put_box(scene_ex, scene_box);
    scene_ex.push_back(0.0); // the build status slot of the export layout
    export_graph(scene, g, scene_ex);
    auto edge_at = [&](int64_t i) -> SmEdge* { return (i >= 0 && i < (int64_t)g.edges.size()) ? g.edges[(size_t)i] : nullptr; };

    int64_t n_sil = op == 8 ? 0 : rd.idx();
    for (int64_t i = 0; i < n_sil; i++)
    {
        SmEdge* e = edge_at(rd.idx());
        if (e == nullptr || e->GetCurve() == nullptr)
            return SC_ERR_ARG;
        seed_silhouette(h, e);
    }
    int64_t n_back = op == 8 ? 0 : rd.idx();
    for (int64_t i = 0; i < n_back; i++)
    {
        SmEdge* e = edge_at(rd.idx());
        if (e == nullptr)
            return SC_ERR_ARG;
        (h.*get(Hcr_m_pBackFacingEdges()))->Insert(e, e);
    }

    std::vector<double> record;
    double status = 0.0;
    SmEdge* e_arg = (op >= 1 && op <= 5) ? edge_at(rd.idx()) : nullptr;
    if (op >= 1 && op <= 5 && e_arg == nullptr)
        return SC_ERR_ARG;
    auto edge_arg = [&]() -> SmEdge* { return e_arg; };
    if (op == 8)
    {
    }
    else if (op == 0)
    {
        put_axis(record, h.*get(Hcr_m_vViewTransform()));
        put_axis(record, h.*get(Hcr_m_vInvViewTransform()));
        put_box(record, h.*get(Hcr_m_vProjBBox()));
        record.push_back(h.*get(Hcr_m_dRayOffsetTolerance()));
        put_vec(record, h.*get(Hcr_m_vViewVector()));
        record.push_back(h.GetThisApproxTol3d());
        record.push_back(h.GetThisAngTolRad());
        record.push_back(flg(h.GetDisplaySeams()));
        record.push_back(flg(h.GetDisplaySmoothEdges()));
    }
    else if (op == 1 || op == 2)
    {
        SmEdge* e = edge_arg();
        record.push_back(flg(op == 1 ? h.IsSilhouetteEdge(e) : h.IsBackFacingEdge(e)));
    }
    else if (op == 3 || op == 4)
    {
        SmEdge* e = edge_arg();
        double fa = rd.num(), fb = rd.num();
        ULONG start_hint = (ULONG)rd.idx(), end_hint = (ULONG)rd.idx();
        if (e->GetCurve() == nullptr)
            status = -2.0;
        else
        {
            SmExtent1d eivl = e->GetInterval();
            SmExtent1d ivl(eivl.Evaluate(fa), eivl.Evaluate(fb));
            SmCurveInterval data[64];
            SmCurveClassification cc(e->GetCurve(), ivl, NULL, e->GetTolerance(), 64, data);
            if (icws_undefined(h, cc))
                status = -2.0;
            else
            {
                SmStatus s = op == 3 ? h.IntersectCurveWithSilhouettes(cc)
                                     : h.ComputeCurveVisibility(cc, start_hint, end_hint);
                status = st(s);
                put_cc(record, g, cc);
            }
        }
    }
    else if (op == 5)
    {
        SmEdge* e = edge_arg();
        int64_t vsel = rd.idx();
        SmBoolean visible = rd.bflag();
        bool premark = rd.flag();
        if (vsel >= (int64_t)g.vertices.size())
            return SC_ERR_ARG;
        SmVertex* v = vsel < 0 ? nullptr : g.vertices[(size_t)vsel];
        SmNewMarkAndLock lock(&ctx, SM_MT_ALLMARKS);
        SmMarkType mark = lock.GetMarkType();
        if (premark)
            e->Mark(mark);
        // the path ComputeEdgeVisibility takes to its classification (SmHCR.cpp:778-797)
        bool reaches = !premark && (h.*get(Hcr_m_pBackFacingEdges()))->At(e) == nullptr;
        if (reaches && !h.GetDisplaySeams() && e->IsSeam())
            reaches = h.IsSilhouetteEdge(e);
        if (reaches && !h.GetDisplaySmoothEdges() && e->IsTangentEdge(2.0))
            reaches = h.IsSilhouetteEdge(e);
        if (!premark && e->GetCurve() == nullptr)
            status = -2.0;
        else if (reaches && v != nullptr && (e->GetPrimaryEdgeuse() == nullptr || e->GetPrimaryEdgeuse()->GetMate() == nullptr))
            status = -2.0;
        else
        {
            bool undefined = false;
            if (reaches)
            {
                SmCurveInterval data[64];
                SmCurveClassification cc(e->GetCurve(), e->GetInterval(), NULL, e->GetTolerance(), 64, data);
                undefined = icws_undefined(h, cc);
            }
            if (undefined)
                status = -2.0;
            else
            {
                status = st(h.ComputeEdgeVisibility(e, v, visible, mark));
                put_pieces(record, h);
            }
        }
    }
    else if (op == 6)
    {
        SmTArray<SmEdge*> edges;
        status = st(h.GetSilhouetteEdges(edges));
        put_sorted(record, g, edges);
    }
    else
    {
        SmPoint3d p = rd.vec();
        record.push_back(flg(h.IsPointVisible(p)));
    }
    if (rd.at != n_in)
        return SC_ERR_ARG;

    std::vector<double> result{status, (double)record.size()};
    result.insert(result.end(), record.begin(), record.end());
    result.insert(result.end(), scene_ex.begin(), scene_ex.end());
    return sc_copy_out(result.data(), result.size(), out, cap, out_len);
    SC_CATCH
}
