// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.poly_properties`.
// Builds an SmPolyBrep (one region, one shell, closed loops, radial rings set directly, as
// query_poly.cpp does) and runs one of SmApiPolyBrepIsManifoldSolid,
// SmApiPolyBrepComputeVolume, SmApiPolyBrepComputeMassProperties
// (source/SM_API/src/SmApiPolygons.cpp), SmPolyBrep::ComputeProperties or
// SmPolyFace::ComputeProperties of every face (source/SMLib/src/SmPoly.cpp).
//
// One entry point, sc_query_poly_properties_eval(in, n_in, out, cap, out_len). Block:
//   op which origin[3] density brep_tol
//   nv (x y z tol)[nv]
//   nf (ftol has_n n[3] nl (etol k v[k])[nl])[nf]
//   ng (k e[k])[ng]
// Edges are numbered in creation order; every loop is closed (FinishPolyEdgeLoop).
// crates/solidcore-ffi/src/units/query_poly_properties.rs validates every index.
// Output: `status` then, on success, by op:
//   0 IsManifoldSolid:        flag
//   1 ComputeVolume:          volume
//   2 ComputeMassProperties:  area volume mass centroid[3] moments[3] products[3]
//   3 SmPolyBrep::ComputeProperties(origin, which): area volume bary[3] m0[3] m1[3]
//   4 per face (GetPolyFaces order): status, then on success the op-3 record
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmAttribute.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCoreTypes.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmMapPtrToPtr.h>
#include <SmMemBlockMgr.h>
#include <SmNurbsSrf.h>
#include <SmOwningTopology.h>
#include <SmPlane.h>
#include <SmPseudoBox.h>
#include <SmSAGObject.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTopoTypes.h>
#include <SmTypes.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

// Set the radial rings and the vertex list directly (as query_poly.cpp): open the access
// of this header only.
#define protected public
#define private public
#include <SmPoly.h>
#undef private
#undef protected

#include <SmApiPolygons.h>

#include <cstdint>
#include <vector>

namespace
{
struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        SmVector3d v;
        v.x = x;
        v.y = y;
        v.z = z;
        return v;
    }
};

void put(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

double st(long s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

void put_props(std::vector<double>& o, double area, double volume, const SmVector3d& bary,
               const SmVector3d m[2])
{
    o.push_back(area);
    o.push_back(volume);
    put(o, bary);
    put(o, m[0]);
    put(o, m[1]);
}
} // namespace

SC_EXPORT int32_t sc_query_poly_properties_eval(const double* input, int64_t n, double* out,
                                                int64_t cap, int64_t* out_len)
{
    SC_TRY
    (void)n;
    SmContext ctx;
    Rd rd{input, 0};
    int64_t op = rd.idx();
    SmMassPropertiesType which = (SmMassPropertiesType)rd.idx();
    SmPoint3d origin = rd.vec();
    double density = rd.num();
    double tol = rd.num();

    SmPolyBrep* b = new (ctx) SmPolyBrep(tol);
    SmPolyRegion* region = new (b) SmPolyRegion(b);
    SmPolyShell* shell = new (b) SmPolyShell(region);

    std::vector<SmPolyVertex*> vertices;
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; ++i)
    {
        SmVector3d p = rd.vec();
        double t = rd.num();
        SmPolyVertex* v = new (b) SmPolyVertex(p, t);
        b->m_pPolyVertexListHead->PostInsert(v);
        vertices.push_back(v);
    }

    std::vector<SmPolyEdge*> edges;
    int64_t nf = rd.idx();
    for (int64_t i = 0; i < nf; ++i)
    {
        double ftol = rd.num();
        bool has_n = rd.num() != 0.0;
        SmVector3d nrm = rd.vec();
        SmPolyFace* f = new (b) SmPolyFace(ftol, NULL, NULL, shell, has_n ? &nrm : NULL);
        int64_t nl = rd.idx();
        for (int64_t l = 0; l < nl; ++l)
        {
            double etol = rd.num();
            int64_t k = rd.idx();
            std::vector<SmPolyVertex*> vs;
            for (int64_t j = 0; j < k; ++j)
                vs.push_back(vertices[rd.idx()]);
            SmPolyLoop* lp = NULL;
            f->StartPolyEdgeLoop(lp);
            for (int64_t j = 0; j < k; ++j)
            {
                SmPolyVertex* v0 = vs[j];
                SmPolyVertex* v1 = vs[(j + 1) % k];
                SmPolyEdge* e = NULL;
                lp->AddPolyEdge(etol, v0->GetPoint(), v1->GetPoint(), v0, NULL, b, e);
                edges.push_back(e);
            }
            lp->FinishPolyEdgeLoop(b);
        }
    }

    int64_t ng = rd.idx();
    for (int64_t g = 0; g < ng; ++g)
    {
        int64_t k = rd.idx();
        std::vector<SmPolyEdge*> es;
        for (int64_t j = 0; j < k; ++j)
            es.push_back(edges[rd.idx()]);
        for (int64_t j = 0; j < k; ++j)
        {
            es[j]->m_pNextRadialE = es[(j + 1) % k];
            es[j]->m_pLastRadialE = es[(j + k - 1) % k];
        }
    }

    std::vector<double> o;
    if (op == 0)
    {
        SmBoolean manifold = FALSE;
        long s = SmApiPolyBrepIsManifoldSolid(b, manifold);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(manifold ? 1.0 : 0.0);
    }
    else if (op == 1)
    {
        double volume = 0.0;
        long s = SmApiPolyBrepComputeVolume(b, volume);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(volume);
    }
    else if (op == 2)
    {
        double area = 0.0, volume = 0.0, mass = 0.0;
        SmPoint3d centroid;
        SmVector3d moments, products;
        long s = SmApiPolyBrepComputeMassProperties(b, density, origin, area, volume, mass,
                                                     centroid, moments, products);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(area);
            o.push_back(volume);
            o.push_back(mass);
            put(o, centroid);
            put(o, moments);
            put(o, products);
        }
    }
    else if (op == 3)
    {
        double area = 0.0, volume = 0.0;
        SmPoint3d bary;
        SmVector3d m[2];
        SmStatus s = b->ComputeProperties(origin, area, volume, bary, m, which);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_props(o, area, volume, bary, m);
    }
    else
    {
        SmTArray<SmPolyFace*> faces;
        b->GetPolyFaces(faces);
        o.push_back(0.0);
        o.push_back((double)faces.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i)
        {
            double area = 0.0, volume = 0.0;
            SmPoint3d bary;
            SmVector3d m[2];
            SmStatus s = faces[i]->ComputeProperties(origin, area, volume, bary, m, which);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                put_props(o, area, volume, bary, m);
        }
    }
    delete b;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
