// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `primitive.api_primitives`: the SM_API primitive entry points of
// source/SM_API/src/SmApiPrimitives.cpp, called exactly as a caller of the API would.
//
// One entry point, sc_primitive_api_primitives_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/primitive_api_primitives.rs,
// which validates every input so upstream only touches valid memory. Every read is bounds
// checked here as well (a malformed program is SC_ERR_ARG). `out` receives the API status
// (0 = SM_SUCCESS, else the SmStatus) and, on success, the result record.
#include "sc_shim.h"

#include "StdAfx.h"
#include "support/primitive_record.h"
#include <SmApiCurves.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCylinder.h>
#include <SmExtent2d.h>
#include <SmPlane.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTorus.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <stdexcept>
#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/primitive_api_primitives.rs).
enum : int32_t
{
    OP_PLANE_SURFACE = 1,
    OP_CYLINDER_SURFACE = 2,
    OP_CONE_SURFACE = 3,
    OP_TORUS_SURFACE = 4,
    OP_PLANE_BREP = 10,
    OP_PLANAR_CIRCLE = 11,
    OP_BOX = 12,
    OP_SPHERE = 13,
    OP_CYLINDER = 14,
    OP_CONE = 15,
    OP_TORUS = 16,
    OP_PYRAMID = 17,
    OP_CYLINDRICAL_BOX = 18,
    OP_PARTIAL_SPHERE = 19,
    OP_SPHERE_NO_POLE = 20,
    OP_PARTIAL_CONE = 21,
    OP_CONE_ELLIPTIC_ENDS = 22,
    OP_PARTIAL_CYLINDER = 23,
    OP_PARTIAL_TORUS = 24,
    OP_SWUNG = 25,
};

// Samples per STEP direction of the surface evaluation grid.
const int GRID = 5;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void put_dom(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

void put_pos(std::vector<double>& o, const SmAxis2Placement& p)
{
    put_vec(o, p.GetOrigin());
    put_vec(o, p.GetXAxis());
    put_vec(o, p.GetYAxis());
    put_vec(o, p.GetZAxis());
}

void put_surface_nurb(std::vector<double>& o, const gw_SURFACE* sur)
{
    o.push_back(sur ? 1.0 : 0.0);
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX s = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= s; i++)
        o.push_back(sur->knv->U[i]);
}

bool finite_dom(const SmExtent2d& d)
{
    return std::isfinite(d.GetMin().x) && std::isfinite(d.GetMin().y) && std::isfinite(d.GetMax().x) &&
           std::isfinite(d.GetMax().y);
}

// The grid parameter `i` of `GRID` over [lo, hi].
double grid_at(double lo, double hi, int i)
{
    return lo + (hi - lo) * ((double)i / (double)(GRID - 1));
}

// The evaluation grid over the STEP domain (only for a finite domain): per sample the status
// and, on success, the point. `step` selects EvaluateSTEPPoint, else EvaluatePoint.
void put_grid(std::vector<double>& o, const SmSurface* s, const SmExtent2d& d, bool step)
{
    bool fin = finite_dom(d);
    o.push_back(flg(fin));
    if (!fin)
        return;
    for (int i = 0; i < GRID; i++)
        for (int j = 0; j < GRID; j++)
        {
            SmPoint2d uv(grid_at(d.GetMin().x, d.GetMax().x, i), grid_at(d.GetMin().y, d.GetMax().y, j));
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus stt = step ? s->EvaluateSTEPPoint(uv, p) : s->EvaluatePoint(uv, p);
            o.push_back(st(stt));
            if (stt == SM_SUCCESS)
                put_vec(o, p);
        }
}

void put_plane(std::vector<double>& o, SmPlane* a)
{
    o.push_back((double)a->GetType());
    put_pos(o, a->GetPosition());
    SmExtent2d d = a->GetSTEPUVDomain();
    put_dom(o, d);
    SmVector2d sc = a->GetUVScale();
    o.push_back(sc.x);
    o.push_back(sc.y);
    put_surface_nurb(o, a->GetOrCreateGwNurbPointer());
    put_grid(o, a, d, false);
}

void put_cone(std::vector<double>& o, SmCone* c)
{
    o.push_back((double)c->GetType());
    put_pos(o, c->GetPosition());
    SmExtent2d d = c->GetSTEPUVDomain();
    put_dom(o, d);
    o.push_back(c->GetBotRadius());
    o.push_back(c->GetTopRadius());
    o.push_back(flg(c->GetInsideOut()));
    o.push_back(flg(c->GetSwapUV()));
    put_surface_nurb(o, c->GetGwNurbPointer());
    put_grid(o, c, d, true);
}

void put_torus(std::vector<double>& o, SmTorus* t)
{
    o.push_back((double)t->GetType());
    put_pos(o, t->GetPosition());
    SmExtent2d d = t->GetSTEPUVDomain();
    put_dom(o, d);
    o.push_back(t->GetMajorRadius());
    o.push_back(t->GetMinorRadius());
    o.push_back(flg(t->GetInsideOut()));
    o.push_back(flg(t->GetSwapUV()));
    put_surface_nurb(o, t->GetGwNurbPointer());
    put_grid(o, t, d, true);
}

// The status, then on success `has` and the B-rep's face, edge and vertex counts. `b` is
// taken by reference: call sites pass the out-parameter of the call that is their status
// argument, and a by-value `b` could be read before that call writes it (unspecified
// argument evaluation order).
void put_brep(std::vector<double>& o, SmApiStatus s, SmBrep*& b, bool rays=false)
{
    o.push_back(st(s));
    if (s != SM_SUCCESS)
    {
        delete b;
        return;
    }
    o.push_back(flg(b != NULL));
    if (b)
    {
        o.push_back((double)b->GetNumFaces());
        o.push_back((double)b->GetNumEdges());
        o.push_back((double)b->GetNumVertices());
        primitive_record::append(o, b);
        if(rays) primitive_record::pyramid_rays(o, b);
    }
    delete b;
}

// A curve list: `k`, then `k` arcs `C[3] r a0 a1` (SmApiCreateArc). A failing constructor is
// a harness error.
void read_arcs(Rd& rd, SmTArray<SmCurve*>& out)
{
    int64_t k = rd.idx(0, 16);
    for (int64_t i = 0; i < k; i++)
    {
        SmVector3d c = rd.vec();
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        SmBSplineCurve* arc = NULL;
        if (SmApiCreateArc(c, r, a0, a1, arc) != SM_SUCCESS || arc == NULL)
        {
            delete arc;
            throw Bad();
        }
        out.Add(arc);
    }
}

void free_curves(SmTArray<SmCurve*>& cs)
{
    for (ULONG i = 0; i < cs.GetSize(); i++)
        delete cs[i];
    cs.ReSet();
}

void run(Rd& rd, std::vector<double>& o)
{
    int32_t op = (int32_t)rd.idx(1, 25);
    SmBrep* b = NULL;
    switch (op)
    {
    case OP_PLANE_SURFACE:
    {
        SmVector3d org = rd.vec();
        double l = rd.num();
        double w = rd.num();
        SmPlane* p = NULL;
        SmApiStatus s = SmApiCreatePlane(org, l, w, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(p != NULL));
            if (p)
                put_plane(o, p);
        }
        delete p;
        return;
    }
    case OP_CYLINDER_SURFACE:
    case OP_CONE_SURFACE:
    case OP_TORUS_SURFACE:
    {
        SmVector3d org = rd.vec();
        double a = rd.num();
        double c = rd.num();
        double h = rd.num();
        SmSurface* srf = NULL;
        SmApiStatus s = op == OP_CYLINDER_SURFACE ? SmApiCreateCylinder(org, a, c, srf)
                        : op == OP_CONE_SURFACE   ? SmApiCreateCone(org, a, c, h, srf)
                                                  : SmApiCreateTorus(org, a, c, srf);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(srf != NULL));
            if (srf && op == OP_TORUS_SURFACE)
            {
                if (!srf->IsKindOf(SmTorus_TYPE))
                {
                    delete srf;
                    throw Bad();
                }
                put_torus(o, static_cast<SmTorus*>(srf));
            }
            else if (srf)
            {
                if (!srf->IsKindOf(SmCone_TYPE))
                {
                    delete srf;
                    throw Bad();
                }
                put_cone(o, static_cast<SmCone*>(srf));
            }
        }
        delete srf;
        return;
    }
    case OP_PLANE_BREP:
    {
        SmVector3d org = rd.vec();
        double x = rd.num();
        double y = rd.num();
        SmApiStatus s = SmApiCreatePlane(org, x, y, b);
        put_brep(o, s, b);
        return;
    }
    case OP_PLANAR_CIRCLE:
    {
        SmVector3d org = rd.vec();
        double r = rd.num();
        put_brep(o, SmApiCreatePlanarCircle(org, r, b), b);
        return;
    }
    case OP_BOX:
    {
        SmVector3d org = rd.vec();
        double l = rd.num();
        double w = rd.num();
        double h = rd.num();
        put_brep(o, SmApiCreateBox(org, l, w, h, b), b);
        return;
    }
    case OP_SPHERE:
    case OP_SPHERE_NO_POLE:
    {
        SmVector3d org = rd.vec();
        double r = rd.num();
        SmApiStatus s = op == OP_SPHERE ? SmApiCreateSphere(org, r, b) : SmApiCreateSphereNoPole(org, r, b);
        put_brep(o, s, b);
        return;
    }
    case OP_CYLINDER:
    {
        SmVector3d org = rd.vec();
        double r = rd.num();
        double h = rd.num();
        put_brep(o, SmApiCreateCylinder(org, r, h, b), b);
        return;
    }
    case OP_CONE:
    {
        SmVector3d org = rd.vec();
        double rb = rd.num();
        double rt = rd.num();
        double h = rd.num();
        put_brep(o, SmApiCreateCone(org, rb, rt, h, b), b);
        return;
    }
    case OP_TORUS:
    {
        SmVector3d org = rd.vec();
        double ra = rd.num();
        double ri = rd.num();
        put_brep(o, SmApiCreateTorus(org, ra, ri, b), b);
        return;
    }
    case OP_PYRAMID:
    {
        SmVector3d org = rd.vec();
        double l = rd.num();
        double h = rd.num();
        put_brep(o, SmApiCreatePyramid(org, l, h, b), b, true);
        return;
    }
    case OP_CYLINDRICAL_BOX:
    {
        SmVector3d org = rd.vec();
        double len = rd.num();
        double ri = rd.num();
        double ro = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        put_brep(o, SmApiCreateCylindricalBox(org, len, ri, ro, a0, a1, b), b);
        return;
    }
    case OP_PARTIAL_SPHERE:
    {
        SmVector3d org = rd.vec();
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        put_brep(o, SmApiCreatePartialSphere(org, r, a0, a1, b), b);
        return;
    }
    case OP_PARTIAL_CONE:
    {
        SmVector3d org = rd.vec();
        double rb = rd.num();
        double rt = rd.num();
        double h = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        put_brep(o, SmApiCreatePartialCone(org, rb, rt, h, a0, a1, b), b);
        return;
    }
    case OP_CONE_ELLIPTIC_ENDS:
    {
        double fr = rd.num();
        double rr = rd.num();
        double h = rd.num();
        double fy = rd.num();
        double ry = rd.num();
        double fx = rd.num();
        double rx = rd.num();
        bool caps = rd.idx(0, 1) != 0;
        put_brep(o, SmApiCreateConeEllipticEnds(fr, rr, h, fy, ry, fx, rx, caps ? TRUE : FALSE, b), b);
        return;
    }
    case OP_PARTIAL_CYLINDER:
    {
        SmVector3d org = rd.vec();
        double r = rd.num();
        double h = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        put_brep(o, SmApiCreatePartialCylinder(org, r, h, a0, a1, b), b);
        return;
    }
    case OP_PARTIAL_TORUS:
    {
        SmVector3d org = rd.vec();
        double ra = rd.num();
        double ri = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        put_brep(o, SmApiCreatePartialTorus(org, ra, ri, a0, a1, b), b);
        return;
    }
    case OP_SWUNG:
    {
        SmTArray<SmCurve*> xy, xz;
        try
        {
            read_arcs(rd, xy);
            read_arcs(rd, xz);
        }
        catch (...)
        {
            free_curves(xy);
            free_curves(xz);
            throw;
        }
        double scale = rd.num();
        SmApiStatus s = SmApiCreateSwungPrimitive(xy, xz, scale, b);
        free_curves(xy);
        free_curves(xz);
        put_brep(o, s, b);
        return;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_primitive_api_primitives_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                  int64_t* out_len)
{
    if (in == NULL || n_in < 1)
        return SC_ERR_ARG;
    SC_TRY
    SmApiGetOrCreateContext();
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        run(rd, o);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    if (rd.at != n_in)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
