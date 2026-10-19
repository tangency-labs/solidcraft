// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.face_p1`.
//
// sc_query_face_p1_curve calls the real SmApiEvaluateCurve entry point
// (source/SM_API/src/SmApiCurves.cpp) on a null curve, an SmLine or an SmHermiteCurve.
//
// sc_query_face_p1_eval builds a real SmBrep with the shared topology fixture
// (sc_topology_fixture::apply: unit topology.edgeuse_p1's B-rep block and traversal
// order, which the Rust side imports from that unit's export) and runs one op on it:
//   0 FACE_FU   each face's m_pFU (protected primary face use) as a face-use index, -1
//               for none (the edge-use export does not carry it).
//   1 BOX       SmFace / SmShell / SmRegion::CalculateBoundingBox, with an explicit
//               bTight or the default argument, into a seeded box.
//   2 POINT3D   SmFace::Point3DClassify of a 3D point (optional UV guess, target loops).
//   3 EDGE_ON   SmFace::EdgeCurveOnClassify of a B-spline line segment (2D or 3D)
//               crossing a selected edge (or a NULL edge).
// Selectors are taken modulo the entity count; an op on an empty kind outputs -2. The
// trimmed surface cache a file read can leave on a face surface is deleted before the
// face ops (as the query.face_p2 shim does): the port has no object caches.
#include "sc_shim.h"
#include "support/topology_fixture.h"

#include "StdAfx.h"
#include <SmApiCurves.h>
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmPointClass.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVertex.h>

#include <memory>
#include <vector>

SC_EXPORT int32_t sc_query_face_p1_curve(int32_t kind, const double* data, double parameter,
                                         int32_t mask, double* out, int32_t* operation_status)
{
    if (!data || !out || !operation_status || kind < 0 || kind > 2 || mask < 0 || mask > 7)
        return SC_ERR_ARG;
    SC_TRY
    SmContext context;
    auto vec = [&](int i) { return SmVector3d(data[i], data[i + 1], data[i + 2]); };
    std::unique_ptr<SmCurve> curve;
    if (kind == 1)
        curve.reset(new (context) SmLine(vec(0), vec(6), 3, &context));
    if (kind == 2)
        curve.reset(new (context) SmHermiteCurve(vec(0), vec(3), vec(6), vec(9), 3));
    SmVector3d point(11, 13, 17), d1(19, 23, 29), d2(31, 37, 41);
    SmVector3d* p = (mask & 1) ? &point : nullptr;
    SmVector3d* q = (mask & 2) ? &d1 : nullptr;
    SmVector3d* r = (mask & 4) ? &d2 : nullptr;
    *operation_status = SmApiEvaluateCurve(curve.get(), parameter, p, q, r);
    const SmVector3d values[3] = {point, d1, d2};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j)
            out[i * 3 + j] = values[i][j];
    return 0;
    SC_CATCH
}

namespace
{
enum Op : int32_t
{
    OP_FACE_FU = 0,
    OP_BOX = 1,
    OP_POINT3D = 2,
    OP_EDGE_ON = 3,
};

using sc_topology_fixture::View;

struct Args
{
    const double* in;
    int64_t n;
    int64_t at;
    bool ok = true;
    double num()
    {
        if (at >= n)
        {
            ok = false;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    SmBoolean bflag() { return num() != 0.0 ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// Legal protected-member access; no derived object is made or cast.
struct FacePeek : SmFace
{
    static SmFaceuse* SmFace::*fu() { return &FacePeek::m_pFU; }
};

template <typename T> int64_t index_of(const std::vector<T*>& v, const void* p)
{
    for (size_t i = 0; i < v.size(); i++)
        if ((const void*)v[i] == p)
            return (int64_t)i;
    return -1;
}

template <typename T> T* pick(const std::vector<T*>& v, int64_t sel)
{
    if (v.empty() || sel < 0)
        return nullptr;
    return v[(size_t)(sel % (int64_t)v.size())];
}

void drop_trim_cache(SmFace* f)
{
    SmSurface* srf = f->GetSurface();
    if (srf != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, srf) != nullptr)
        SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF, srf);
}

// `kind index` of a classification object: 1 face, 2 edge, 3 vertex (traversal index),
// `0 -1` for none, `-3 -1` for anything else.
void put_object(std::vector<double>& o, const View& g, const SmObject* p)
{
    if (p == nullptr)
    {
        o.push_back(0.0);
        o.push_back(-1.0);
    }
    else if (p->IsKindOf(SmFace_TYPE))
    {
        o.push_back(1.0);
        o.push_back((double)index_of(g.faces, p));
    }
    else if (p->IsKindOf(SmEdge_TYPE))
    {
        o.push_back(2.0);
        o.push_back((double)index_of(g.edges, p));
    }
    else if (p->IsKindOf(SmVertex_TYPE))
    {
        o.push_back(3.0);
        o.push_back((double)index_of(g.vertices, p));
    }
    else
    {
        o.push_back(-3.0);
        o.push_back(-1.0);
    }
}

// `class kind index gap src_tol presnap tparam u v param_set face_kind face_index`
void put_pc(std::vector<double>& o, const View& g, const SmPointClassification& pc)
{
    o.push_back((double)pc.GetPointClass());
    put_object(o, g, pc.GetObject());
    o.push_back(pc.GetGap3d());
    o.push_back((double)pc.GetTolerance());
    o.push_back(pc.GetPreSnapParam());
    o.push_back(pc.GetTParam());
    SmPoint2d uv = pc.GetUVParam();
    o.push_back(uv.x);
    o.push_back(uv.y);
    o.push_back(flg(pc.AreParametersSet()));
    put_object(o, g, pc.GetFace());
}

int32_t run(int32_t op, Args& a, const View& g, std::vector<double>& o)
{
    const SmContext& ctx = *g.brep.GetContext();
    switch (op)
    {
    case OP_FACE_FU:
        for (SmFace* f : g.faces)
            o.push_back((double)index_of(g.faceuses, f->*FacePeek::fu()));
        return 0;
    case OP_BOX:
    {
        // `target sel tight use_default`: 0 face, 1 shell, 2 region
        int64_t target = a.idx();
        int64_t sel = a.idx();
        SmBoolean tight = a.bflag();
        bool use_default = a.num() != 0.0;
        if (!a.ok || target < 0 || target > 2)
            return SC_ERR_ARG;
        SmExtent3d box;
        box.SetMinMax(SmPoint3d(-17, -19, -23), SmPoint3d(29, 31, 37));
        SmStatus s = SM_SUCCESS;
        if (target == 0)
        {
            SmFace* f = pick(g.faces, sel);
            if (f == nullptr)
            {
                o.push_back(-2.0);
                return 0;
            }
            if (f->GetSurface() == nullptr)
                return SC_ERR_ARG; // dereferenced unchecked upstream
            s = use_default ? f->CalculateBoundingBox(box) : f->CalculateBoundingBox(box, tight);
        }
        else if (target == 1)
        {
            SmShell* sh = pick(g.shells, sel);
            if (sh == nullptr)
            {
                o.push_back(-2.0);
                return 0;
            }
            s = use_default ? sh->CalculateBoundingBox(box) : sh->CalculateBoundingBox(box, tight);
        }
        else
        {
            SmRegion* r = pick(g.regions, sel);
            if (r == nullptr)
            {
                o.push_back(-2.0);
                return 0;
            }
            s = use_default ? r->CalculateBoundingBox(box) : r->CalculateBoundingBox(box, tight);
        }
        o.push_back(st(s));
        SmPoint3d lo = box.GetMin(), hi = box.GetMax();
        o.insert(o.end(), {lo.x, lo.y, lo.z, hi.x, hi.y, hi.z});
        return 0;
    }
    case OP_POINT3D:
    {
        // `face P[3] src_tol do_bnd has_guess gu gv tgt_only has_loops n loop* uv_ok pc_tol`
        SmFace* f = pick(g.faces, a.idx());
        SmPoint3d p = a.vec();
        double src_tol = a.num();
        SmBoolean do_bnd = a.bflag();
        bool has_guess = a.num() != 0.0;
        double gu = a.num(), gv = a.num();
        SmBoolean tgt_only = a.bflag();
        bool has_loops = a.num() != 0.0;
        int64_t nl = a.idx();
        if (!a.ok || nl < 0 || nl > 64)
            return SC_ERR_ARG;
        SmTArray<SmLoop*> loops;
        for (int64_t k = 0; k < nl; k++)
        {
            SmLoop* l = pick(g.loops, a.idx());
            if (l != nullptr)
                loops.Add(l);
        }
        SmBoolean uv_ok = a.bflag();
        double pc_tol = a.num();
        if (!a.ok)
            return SC_ERR_ARG;
        if (f == nullptr)
        {
            o.push_back(-2.0);
            return 0;
        }
        if (f->GetSurface() == nullptr)
            return SC_ERR_ARG;
        drop_trim_cache(f);
        SmPointClassification pc(SmZoneTol3d(pc_tol), &ctx);
        SmPoint2d guess(gu, gv);
        SmStatus s = f->Point3DClassify(p, SmZoneTol3d(src_tol), do_bnd, pc, has_guess ? &guess : nullptr,
                                        tgt_only, has_loops ? &loops : nullptr, uv_ok);
        o.push_back(st(s));
        put_pc(o, g, pc);
        return 0;
    }
    case OP_EDGE_ON:
    {
        // `face edge dim P[3] Q[3] edge_param i0 i1 orient tol uv_ok`; edge < 0 is NULL
        SmFace* f = pick(g.faces, a.idx());
        int64_t esel = a.idx();
        int64_t dim = a.idx();
        SmPoint3d p = a.vec();
        SmPoint3d q = a.vec();
        double edge_param = a.num();
        double i0 = a.num(), i1 = a.num();
        int64_t orient = a.idx();
        double tol = a.num();
        SmBoolean uv_ok = a.bflag();
        if (!a.ok || (dim != 2 && dim != 3) || (orient != 0 && orient != 1))
            return SC_ERR_ARG;
        if (f == nullptr || (esel >= 0 && g.edges.empty()))
        {
            o.push_back(-2.0);
            return 0;
        }
        if (f->GetSurface() == nullptr)
            return SC_ERR_ARG;
        SmEdge* e = esel < 0 ? nullptr : pick(g.edges, esel);
        drop_trim_cache(f);
        SmBSplineCurve* line = nullptr;
        SmStatus cs = SmBSplineCurve::CreateLineSegment(ctx, (ULONG)dim, p, q, line);
        if (cs != SM_SUCCESS || line == nullptr)
        {
            delete line;
            o.push_back(-3.0);
            o.push_back(st(cs));
            return 0;
        }
        SmBoolean inside = TRUE, problem = TRUE;
        SmClassifyProblem why = SM_CP_UNRESOLVED_CLASSIFICATION;
        SmStatus s = f->EdgeCurveOnClassify(e, edge_param, *line, SmExtent1d(i0, i1),
                                            orient == 0 ? SM_OT_SAME : SM_OT_OPPOSITE, tol, inside, problem,
                                            why, uv_ok);
        delete line;
        o.push_back(st(s));
        o.push_back(flg(inside));
        o.push_back(flg(problem));
        o.push_back((double)why);
        return 0;
    }
    default:
        return SC_ERR_ARG;
    }
}
} // namespace

SC_EXPORT int32_t sc_query_face_p1_eval(int32_t op, const char* path, const double* block, int64_t n_block,
                                        const double* args, int64_t n_args, double* out, int64_t cap,
                                        int64_t* out_len)
{
    if (path == nullptr || block == nullptr || n_block < 1 || out_len == nullptr || n_args < 0 ||
        (n_args > 0 && args == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    int32_t rc = sc_topology_fixture::apply(path, block, n_block, [&](const View& g) -> int32_t {
        Args a{args, n_args, 0};
        return run(op, a, g, o);
    });
    if (rc != 0)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
