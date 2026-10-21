// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_corner_p1`: the file-static helpers of upstream
// source/SMLib/src/SmFilletCorner.cpp (sm_TestG1Continuity, sm_GetFaceByTwoEdgeuses,
// sm_CheckTangentialEdgeuse, sm_CheckFaceComplimentaryCornerConcavity,
// sm_CreateSrfOfRevolution, sm_CreatePlanarSurface, sm_GetFilletRadius, sm_CheckSeamOut,
// sm_SetSurfaceClosure) and the static SmFilletNx1Corner::CheckNx1Degeneracy.
//
// Static functions cannot be linked against, so this translation unit compiles
// SmFilletCorner.cpp itself, at global scope (as blend_fillet_geom_p1.cpp does with
// SmFilletGeom.cpp): SMLib is a static archive, so this object supplies every SmFilletCorner
// symbol and the archive's copy is not linked. It is the ONLY shim that may include
// SmFilletCorner.cpp (a second copy gives duplicate symbols); other corner shims reach the
// member functions through the linker. It cannot share blend_fillet_geom_p1.cpp's
// translation unit: sm_CheckSeamOut is `static` in both upstream files.
//
// One entry point, sc_blend_fillet_corner_p1_helper(op, path, in, n_in, out, cap, out_len).
// Layouts in crates/solidcore-ffi/src/units/blend_fillet_corner_p1.rs, which validates every
// input. The topology ops build a real SmBrep from unit topology.edgeuse_p1's B-rep block
// (sc_edgeuse_graph.h; `path` is the .smb file of a file block) and address its entities by
// traversal index; their output starts with `0`, or is `-1 status` when the B-rep could not be
// built. A record of `-2` means "refused": upstream would dereference null.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFilletCorner.h>
#include <SmFilletExecutive.h>
#include <SmPlane.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include "../../../upstream/source/SMLib/src/SmFilletCorner.cpp"

#include <cstdint>
#include <stdexcept>
#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_corner_p1.rs).
enum : int32_t
{
    OP_PLANAR = 0,
    OP_REVOLUTION = 1,
    OP_FILLET_RADIUS = 2,
    OP_SEAM_OUT = 3,
    OP_CLOSURE = 4,
    OP_G1 = 5,
    OP_FACE_BY_TWO = 6,
    OP_TANGENTIAL = 7,
    OP_CONCAVITY = 8,
    OP_NX1_DEGENERACY = 9,
};

const double REFUSED = -2.0;

// A curve whose evaluation upstream would dereference: an edge without a curve.
bool has_curve(const SmEdge* e) { return e == nullptr || e->GetCurve() != nullptr; }

// `n p.. tol` -> `status` [surface block].
void run_planar(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t n = rd.idx();
    SmTArray<SmPoint3d> pts;
    for (int64_t i = 0; i < n; ++i)
        pts.Add(rd.vec());
    double tol = rd.num();
    SmSurface* s = NULL;
    SmStatus rc = sm_CreatePlanarSurface(ctx, pts, tol, s);
    o.push_back(st(rc));
    if (rc == SM_SUCCESS)
        put_surface(o, s);
    delete s;
}

// `n curve.. major tol` -> `status` [surface block].
void run_revolution(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t n = rd.idx();
    SmTArray<SmBSplineCurve*> curves;
    for (int64_t i = 0; i < n; ++i)
        curves.Add(read_curve(rd, ctx));
    ULONG major = (ULONG)rd.idx();
    double tol = rd.num();
    SmSurface* s = NULL;
    SmStatus rc = sm_CreateSrfOfRevolution(ctx, curves, major, tol, s, 0);
    o.push_back(st(rc));
    if (rc == SM_SUCCESS)
        put_surface(o, s);
    delete s;
    for (ULONG i = 0; i < curves.GetSize(); ++i)
        delete curves[i];
}

// `p1 n1 p2 n2 orient` -> `radius span`.
void run_fillet_radius(Rd& rd, std::vector<double>& o)
{
    SmPoint3d p1 = rd.vec();
    SmVector3d n1 = rd.vec();
    SmPoint3d p2 = rd.vec();
    SmVector3d n2 = rd.vec();
    double orient = rd.num();
    double radius = 0.0, span = 0.0;
    sm_GetFilletRadius(p1, n1, p2, n2, orient, radius, span);
    o.push_back(radius);
    o.push_back(span);
}

// `surface u v tol closure` -> `ret u v`.
void run_seam_out(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    SmPoint2d uv = rd.uv();
    double tol = rd.num();
    SmSurfParamType closure = (SmSurfParamType)rd.idx();
    SmBoolean ret = sm_CheckSeamOut(s, uv, tol, closure);
    o.push_back(flg(ret));
    o.push_back(uv.x);
    o.push_back(uv.y);
    delete s;
}

// `surface u0 v0 u1 v1` -> `closure`.
void run_closure(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    SmPoint2d a = rd.uv(), b = rd.uv();
    SmSurfParamType closure = sm_SetSurfaceClosure(s, SmExtent2d(a, b));
    o.push_back((double)closure);
    delete s;
}

// Graph entity `i` (`-1` = NULL); an index past the graph throws (SC_ERR_EXCEPTION), which
// the FFI's range checks never let through.
template <class T> T* at(const std::vector<T*>& v, int64_t i) { return i < 0 ? nullptr : v.at((size_t)i); }

// A graph entity the routine dereferences unconditionally (`-1` is rejected by the FFI).
template <class T> T* req(const std::vector<T*>& v, int64_t i)
{
    T* p = at(v, i);
    if (p == nullptr)
        throw std::invalid_argument("required entity");
    return p;
}

// The topology ops on a built B-rep (graph indices validated by the FFI).
void run_topology(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst = SM_SUCCESS;
    SmBrep* brep = build(rd, path, bst);
    if (brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    Built owner;
    owner.brep = brep;
    Graph g;
    walk(brep, g);
    o.push_back(0.0);

    switch (op)
    {
    case OP_G1:
    {
        // `v e1 e2 tol` -> `flag has_angle angle`
        SmVertex* v = at(g.vertices, rd.idx());
        SmEdge* e1 = at(g.edges, rd.idx());
        SmEdge* e2 = at(g.edges, rd.idx());
        double tol = rd.num();
        if (v != nullptr && e1 != nullptr && e2 != nullptr && (!has_curve(e1) || !has_curve(e2)))
        {
            o.push_back(REFUSED); // pCrv->Evaluate on a NULL curve
            return;
        }
        double ang = SM_UNDEF_DOUBLE;
        SmBoolean flag = sm_TestG1Continuity(v, e1, e2, tol, &ang, 0);
        o.push_back(flg(flag));
        o.push_back(ang == SM_UNDEF_DOUBLE ? 0.0 : 1.0);
        o.push_back(ang == SM_UNDEF_DOUBLE ? 0.0 : ang);
        return;
    }
    case OP_FACE_BY_TWO:
    {
        // `eu1 eu2 eu3` -> `face`
        SmEdgeuse* eu1 = at(g.edgeuses, rd.idx());
        SmEdgeuse* eu2 = at(g.edgeuses, rd.idx());
        SmEdgeuse* eu3 = at(g.edgeuses, rd.idx());
        if (eu1 != nullptr && eu2 != nullptr && (eu1->GetRadial() == nullptr || eu2->GetRadial() == nullptr))
        {
            o.push_back(REFUSED); // GetRadial()->GetFace() on NULL
            return;
        }
        SmFace* f = sm_GetFaceByTwoEdgeuses(eu1, eu2, eu3);
        o.push_back((double)g.find(3, f));
        return;
    }
    case OP_TANGENTIAL:
    {
        // `eu tol` -> `flag`
        SmEdgeuse* eu = req(g.edgeuses, rd.idx());
        double tol = rd.num();
        if (eu->GetEdge() == nullptr || eu->GetRadial() == nullptr)
        {
            o.push_back(REFUSED);
            return;
        }
        o.push_back(flg(sm_CheckTangentialEdgeuse(eu, tol)));
        return;
    }
    case OP_CONCAVITY:
    {
        // `v eu1 eu2 ref` -> `flag`
        SmVertex* v = req(g.vertices, rd.idx());
        SmEdgeuse* eu1 = req(g.edgeuses, rd.idx());
        SmEdgeuse* eu2 = req(g.edgeuses, rd.idx());
        SmEdgeuse* ref = at(g.edgeuses, rd.idx());
        // Every edge use the routine dereferences: the reference (edge, radial), both test
        // edge uses (radial) and whichever of each pair ends up used (vertex use, edge).
        auto usable = [](SmEdgeuse* p) { return p->GetVertexuse() != nullptr && p->GetEdge() != nullptr; };
        bool ok = eu1->GetRadial() != nullptr && eu2->GetRadial() != nullptr && usable(eu1) && usable(eu2) &&
                  usable(eu1->GetRadial()) && usable(eu2->GetRadial()) &&
                  (ref == nullptr || (ref->GetEdge() != nullptr && ref->GetRadial() != nullptr));
        if (!ok)
        {
            o.push_back(REFUSED);
            return;
        }
        o.push_back(flg(sm_CheckFaceComplimentaryCornerConcavity(v, eu1, eu2, ref, 0)));
        return;
    }
    case OP_NX1_DEGENERACY:
    {
        // `v eu` -> `ret`: SmFilletNx1Corner::CheckNx1Degeneracy (its raw int return: the
        // flag, or the status `SER` returns through it)
        SmVertex* v = req(g.vertices, rd.idx());
        SmEdgeuse* eu = req(g.edgeuses, rd.idx());
        // every dereference before the evaluations: the clockwise neighbour, the radial's
        // counter-clockwise neighbour (its mate when they coincide), their vertex uses and
        // edges
        SmEdgeuse* side1 = eu->GetCWEdgeuse();
        SmEdgeuse* side2 = eu->GetRadial() ? eu->GetRadial()->GetCCWEdgeuse() : nullptr;
        if (side1 != nullptr && side1 == side2)
            side2 = side1->GetMate();
        bool ok = side1 != nullptr && side2 != nullptr && side1->GetVertexuse() != nullptr &&
                  side2->GetVertexuse() != nullptr && side1->GetEdge() != nullptr && side2->GetEdge() != nullptr;
        if (!ok)
        {
            o.push_back(REFUSED);
            return;
        }
        o.push_back((double)SmFilletNx1Corner::CheckNx1Degeneracy(v, eu));
        return;
    }
    default: o.push_back(REFUSED); return;
    }
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_corner_p1_helper(int32_t op, const char* path, const double* in, int64_t n_in,
                                                   double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    switch (op)
    {
    case OP_PLANAR: run_planar(rd, o); break;
    case OP_REVOLUTION: run_revolution(rd, o); break;
    case OP_FILLET_RADIUS: run_fillet_radius(rd, o); break;
    case OP_SEAM_OUT: run_seam_out(rd, o); break;
    case OP_CLOSURE: run_closure(rd, o); break;
    case OP_G1:
    case OP_FACE_BY_TWO:
    case OP_TANGENTIAL:
    case OP_CONCAVITY:
    case OP_NX1_DEGENERACY: run_topology(op, rd, path, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// blend.fillet_corner_p2 (#268) adds its entry points for the p2 static helpers of
// SmFilletCorner.cpp (sm_FindConcaveEdgeuses) here, in this translation unit, via
// `#include "blend_fillet_corner_p2_static.inc"`.
#include "blend_fillet_corner_p2_static.inc"
