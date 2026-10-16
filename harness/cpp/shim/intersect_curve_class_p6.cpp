// SPDX-License-Identifier: Apache-2.0
// Native oracle for intersect.curve_class_p6; source/SMLib/src/SmCurveClass.cpp.
// Geometry/topology construction is separate from the operation status record.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmBrep.h>
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmPointClass.h>
#include <SmRegion.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmTrimSrfCache.h>
#include <SmTol.h>
#include <SmVertex.h>
#include <nurbs.h>
#include <memory>
#include <vector>

namespace
{
struct Objects
{
    std::unique_ptr<SmBrep> brep;
    std::unique_ptr<SmCurve> curve;
    SmTArray<SmVertex*> vertices;
    SmTArray<SmEdge*> edges;
    SmTArray<SmFace*> faces;
    SmTArray<SmRegion*> regions;

    SmObject* object(int kind) const
    {
        switch (kind)
        {
        case 0: return nullptr;
        case 1: case 6: return curve.get();
        case 2: return faces[0];
        case 3: return edges[0];
        case 4: return vertices[0];
        case 5: return regions[0];
        }
        return nullptr;
    }

    int id(const SmObject* p) const
    {
        if (!p) return 0;
        for (int i = 1; i <= 5; ++i)
            if (object(i) == p) return i;
        return -1;
    }
};

double status_code(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

// MergeInterval assumes its UNKNOWN start can be recovered from its FACE
// interior. If projection fails, it continues and dereferences a null vertex.
// Check exactly the private sm_ComputeFacePointClass projection/classification
// sequence (SmCurveClass.cpp:3937-4044), without changing the input classification
// or topology. Successful native inputs remain available to the real operation.
bool merge_start_can_merge(const SmCurveClassification& cc, ULONG index)
{
    const SmCurveInterval& ivl = cc[index];
    if (ivl.m_vStart.GetPointClass() != SM_PC_UNKNOWN ||
        ivl.m_vMid.GetPointClass() == SM_PC_UNKNOWN ||
        ivl.m_vMid.GetPointClass() == SM_PC_REGION)
        return true;
    // The closed/degenerate branch returns before the unconditional vertex
    // dereference. Do not reject these valid constant/short-curve inputs.
    SmXSectTol3d closed_tol = SmTol::GetXSectTol3d(
        cc.GetPointObjZoneTol3d(index), cc.GetPointObjZoneTol3d(index + 1));
    const SmPoint3d point = cc.GetClassifyCrvLoc(index);
    const SmPoint3d end = cc.GetClassifyCrvLoc(index + 1);
    if (point.DistanceBetween(end) < closed_tol &&
        SmTol::IsDegenerate(cc.GetCurve(), ivl.m_vInterval, &closed_tol))
        return true;
    if (ivl.m_vMid.GetPointClass() != SM_PC_FACE) return false;
    SmFace* face = (SmFace*)ivl.m_vMid.GetObject();
    SmSurface* surface = face->GetSurface();
    if (!surface) return false;
    SmSurfaceCache* cache = smsurf_GetSurfaceCache(surface);
    if (!cache) return false;
    SmCacheCheckOutIn hold(cache);
    SmSolution data[16];
    SmSolutionArray solutions(16, data);
    SmZoneTol3d src = SmTol::GetSrcZoneTol3d(&cc);
    SmXSectTol3d xsect = SmTol::GetXSectTol3d(src, SmTol::GetZoneTol3d(face));
    {
        SmTemporaryChangeValue<SmBoolean> disable(cache->m_bPointTestEnabled, FALSE);
        for (double tol : {(double)src, 10 * (double)xsect, 100 * (double)xsect})
        {
            if (surface->GlobalPointSolve(face->GetUVDomain(), SM_SO_INTERSECT,
                    point, SmZoneTol3d(tol), nullptr, SM_SR_ALL, solutions) != SM_SUCCESS)
                return false;
            if (solutions.GetSize() != 0) break;
        }
    }
    if (solutions.GetSize() == 0) return false;
    double best = SM_BIG_DOUBLE;
    SmPoint2d uv;
    for (ULONG i = 0; i < solutions.GetSize(); ++i)
        if (solutions[i].m_vStart.m_dSolutionValue < best)
        {
            best = solutions[i].m_vStart.m_dSolutionValue;
            uv.Set(solutions[i].m_vStart[0], solutions[i].m_vStart[1]);
        }
    if (best == SM_BIG_DOUBLE) return false;
    SmPointClassification pc = ivl.m_vStart;
    face->PointClassify(uv, src, TRUE, TRUE, pc);
    return pc.GetObject() != nullptr;
}

SmPointClassType pc_type(int kind)
{
    const SmPointClassType types[] = {SM_PC_UNKNOWN, SM_PC_CURVE, SM_PC_FACE,
        SM_PC_EDGE, SM_PC_VERTEX, SM_PC_REGION, SM_PC_NOTHING};
    return types[kind];
}

void read_pc(SmPointClassification& pc, const double* p, const Objects& objs, double tol)
{
    pc.SetClassObject(pc_type((int)p[0]), objs.object((int)p[0]));
    pc.SetGap3d(p[1]);
    pc.SetTParam(p[2], FALSE);
    pc.SetPreSnapParam(p[3]);
    pc.SetSrcZoneTol3d(SmZoneTol3d(tol));
    if (p[4] != 0) pc.SetSSSPoint(SmPoint3d(p[5], p[6], p[7]));
}

void read_ivl(SmCurveInterval& ivl, const double* p, const Objects& objs, double tol)
{
    ivl.SetInterval(SmExtent1d(p[0], p[1]));
    read_pc(ivl.m_vStart, p + 2, objs, tol);
    read_pc(ivl.m_vMid, p + 10, objs, tol);
    read_pc(ivl.m_vEnd, p + 18, objs, tol);
}

void put_pc(std::vector<double>& o, const SmPointClassification& pc, const Objects& objs)
{
    SmPoint2d uv = pc.GetUVParam();
    SmPoint3d uvw = pc.GetUVWParam(), sss = pc.GetSSSPoint();
    o.insert(o.end(), {(double)pc.GetPointClass(), (double)objs.id(pc.GetObject()),
        pc.GetDeviation(), pc.GetTParam(), (double)objs.id(pc.GetFace()),
        uv.x, uv.y, uvw.x, uvw.y, uvw.z, (double)pc.AreParametersSet(),
        pc.GetPreSnapParam(), (double)pc.GetSSSPointFlag(), sss.x, sss.y, sss.z,
        (double)SmTol::GetSrcZoneTol3d(&pc)});
}

void put_cc(std::vector<double>& o, const SmCurveClassification& cc, const Objects& objs)
{
    o.insert(o.end(), {(double)cc.GetSize(), cc.GetInterval().GetMin(),
        cc.GetInterval().GetMax(), SmTol::GetSrcZoneTol3d(&cc)});
    for (ULONG i = 0; i < cc.GetSize(); ++i)
    {
        const SmCurveInterval& ivl = cc[i];
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_pc(o, ivl.m_vStart, objs);
        put_pc(o, ivl.m_vMid, objs);
        put_pc(o, ivl.m_vEnd, objs);
        // Actual evaluation records distinguish geometric kinds and scales.
        for (double t : {ivl.GetMin(), ivl.m_vInterval.Evaluate(0.5), ivl.GetMax()})
        {
            SmPoint3d p;
            SmStatus status = objs.curve->EvaluatePoint(t, p);
            o.push_back(status_code(status));
            if (status == SM_SUCCESS) o.insert(o.end(), {p.x, p.y, p.z});
        }
    }
}
}

SC_EXPORT int32_t sc_intersect_curve_class_p6_run(
    int32_t op, int32_t kind, double scale, double tol, int32_t flags, int64_t index,
    const double* a, int64_t na, const double* b, int64_t nb, const double* input,
    double* out, int64_t cap, int64_t* len)
{
    if (op < 0 || op > 3 || kind < 0 || kind > 4 || flags < 0 || flags > 3 ||
        na < 0 || na > 32 || nb < 0 || nb > 32 || index < 0 ||
        (op == 3 && (index >= na || ((flags & 2) && nb != na))))
        return SC_ERR_ARG;
    SC_TRY
    const SmContext* ctx = SmApiGetOrCreateContext();
    if (!ctx) return SC_ERR_ARG;
    Objects objs;
    SmBrep* box = nullptr;
    SmStatus built = SmApiCreateBox(SmPoint3d(0, 0, 0), 1, 1, 1, box);
    objs.brep.reset(box);
    if (built != SM_SUCCESS || !box) return SC_ERR_ARG;
    box->GetVertices(objs.vertices);
    box->GetEdges(objs.edges);
    box->GetFaces(objs.faces);
    box->GetRegions(objs.regions);
    if (objs.vertices.GetSize() == 0 || objs.edges.GetSize() == 0 ||
        objs.faces.GetSize() == 0 || objs.regions.GetSize() == 0)
        return SC_ERR_ARG;

    if (kind == 0)
        objs.curve.reset(new(ctx) SmLine(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0),
            SmExtent1d(0, 1), scale, 3, ctx));
    else if (kind == 1)
        objs.curve.reset(new(ctx) SmCircle(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0),
            SmVector3d(0, 1, 0), SmExtent1d(0, 1), scale, 3, ctx));
    else if (kind == 2)
        objs.curve.reset(new(ctx) SmEllipse(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0),
            SmVector3d(0, 1, 0), SmExtent1d(0, 1), scale, scale * 0.5, 3, ctx));
    else
    {
        gw_CURVE* c = sm_AllocateNurbCurve(2, 2, 5);
        if (!c) return SC_ERR_ARG;
        for (int i = 0; i < 3; ++i)
        {
            c->pol->Pw[i].x = kind == 4 ? scale : scale * i;
            c->pol->Pw[i].y = kind == 4 ? 0 : scale * (i == 1 ? 1 : 0);
            c->pol->Pw[i].z = 0;
            c->pol->Pw[i].w = 1;
        }
        for (int i = 0; i < 6; ++i) c->knt->U[i] = i < 3 ? 0 : 1;
        objs.curve.reset(new(ctx) SmBSplineCurve(c, 3, FALSE));
    }
    if (!objs.curve) return SC_ERR_ARG;

    SmCurveClassification cc, other;
    cc.SetCurve(objs.curve.get(), FALSE);
    other.SetCurve(objs.curve.get(), FALSE);
    SmTol::SetZoneTol3d(&cc, SmZoneTol3d(tol));
    SmTol::SetZoneTol3d(&other, SmZoneTol3d(tol));
    cc.SetSize((ULONG)na);
    other.SetSize((ULONG)nb);
    for (int64_t i = 0; i < na; ++i) read_ivl(cc[(ULONG)i], a + 26 * i, objs, tol);
    for (int64_t i = 0; i < nb; ++i) read_ivl(other[(ULONG)i], b + 26 * i, objs, tol);
    cc.SetFilletData((flags & 2) ? &other : nullptr, (flags & 2) ? TRUE : FALSE);
    if (op == 3 && !(flags & 1) && !merge_start_can_merge(cc, (ULONG)index))
        return SC_ERR_ARG;
    SmCurveInterval incoming(SmExtent1d(0, 1), ctx, &cc);
    read_ivl(incoming, input, objs, tol);
    SmBoolean changed = FALSE;
    SmStatus status;
    SmTArray<SmFace*> new_faces;
    SmTArray<SmEdge*> new_edges;
    SmTArray<SmVertex*> new_vertices;
    if (op == 0) status = cc.InsertCurveInterval(incoming);
    else if (op == 1) status = cc.Homogenize(other);
    else if (op == 2)
    {
        SmPoint3d p(input[7], input[8], input[9]);
        status = cc.FixEndPointClass((ULONG)index, p, changed);
    }
    else
        status = cc.MergeInterval((ULONG)index, &new_faces, &new_edges, &new_vertices,
            (flags & 1) ? TRUE : FALSE);

    std::vector<double> o{status_code(status)};
    if (status == SM_SUCCESS)
    {
        if (op == 2) o.push_back(changed ? 1 : 0);
        if (op == 3)
        {
            o.push_back((double)new_faces.GetSize());
            for (ULONG i = 0; i < new_faces.GetSize(); ++i) o.push_back(objs.id(new_faces[i]));
            o.push_back((double)new_edges.GetSize());
            for (ULONG i = 0; i < new_edges.GetSize(); ++i) o.push_back(objs.id(new_edges[i]));
            o.push_back((double)new_vertices.GetSize());
            for (ULONG i = 0; i < new_vertices.GetSize(); ++i) o.push_back(objs.id(new_vertices[i]));
        }
        put_cc(o, cc, objs);
        put_cc(o, other, objs);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, len);
    SC_CATCH
}
