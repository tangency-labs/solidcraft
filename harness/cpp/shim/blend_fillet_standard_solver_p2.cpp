// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_standard_solver_p2`.
// Wraps source/SMLib/src/SmFilletStandardSolver.cpp: SmConstantRadiusFS::LoadInitialValues
// (on an SmConstantRadiusFS or an SmSurfaceSurfaceFS), SmConstantRadiusAssistedFS
// GuessPointOnPlaneSolve (with the file-local SmGuessSphereSolveENFO::Evaluate) and
// PointOnPlaneSolve, the SmSurfaceSurfaceFS constructor and CalcFilletGeom, and the IsKindOf of
// SmConstantRadiusAssistedFS, SmSurfaceSurfaceFS and SmVariableRadiusFS.
//
// One entry point, sc_blend_fillet_standard_solver_p2_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p2.rs,
// which validates every input so upstream only touches valid memory. `out` receives the op
// status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
//
// SmConstantRadiusAssistedFS's only constructor takes an edge use: the solver is built on the
// first edge of an SmApiCreateBox box, then a derived class replaces its offset surfaces
// (offset surfaces of the given base surfaces), orientations, radii and extension factor and
// clears its edge uses (as the blend.fillet_custom_solver shim does).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiPrimitives.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtentNd.h>
#include <SmFilletExecutive.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmLine.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p2.rs).
enum : int32_t
{
    OP_CR_INIT = 0,
    OP_GUESS = 1,
    OP_PLANE = 2,
    OP_SS = 3,
    OP_TYPES = 4,
};

// CalcFilletGeom was not run (it would reach the solver's NULL executive).
const double NOT_RUN = -3.0;

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmPoint3d pt3()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmPoint3d(x, y, z);
    }
    SmPoint2d pt2()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
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

void put2(std::vector<double>& o, const SmPoint2d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
}

void put3(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// A surface `kind ...`: 0 B-spline `n m p q r s cpts u v`; 1 plane `o x y sx sy u0 v0 u1 v1`
// (SmPlane(o, x, y, (sx, sy), domain, ctx)).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.pt3();
        SmVector3d x = rd.pt3();
        SmVector3d y = rd.pt3();
        double sx = rd.num();
        double sy = rd.num();
        double u0 = rd.num();
        double v0 = rd.num();
        double u1 = rd.num();
        double v1 = rd.num();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), SmExtent2d(u0, v0, u1, v1), &ctx);
    }
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = rd.num();
            sur->net->Pw[i][j].y = rd.num();
            sur->net->Pw[i][j].z = rd.num();
            sur->net->Pw[i][j].w = rd.num();
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = rd.num();
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = rd.num();
    return new (ctx) SmBSplineSurface(sur, FALSE, NULL);
}

// IsKindOf of `nk` codes.
void put_kinds(std::vector<double>& o, const SmObject* obj, Rd& rd)
{
    int64_t nk = rd.idx();
    for (int64_t i = 0; i < nk; i++)
        o.push_back(flg(obj->IsKindOf((SM_TYPE)rd.idx()) != FALSE));
}

// ---- solvers -------------------------------------------------------------------------------

struct Cr : SmConstantRadiusFS
{
    Cr(const SmContext& ctx, double r0, double r1, const SmSurface& a, const SmSurface& b, bool o0, bool o1)
        : SmConstantRadiusFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, r0, r1, a, b, o0 ? TRUE : FALSE,
                             o1 ? TRUE : FALSE)
    {
    }
};

struct Ss : SmSurfaceSurfaceFS
{
    Ss(const SmContext& ctx, double r0, double r1, const SmSurface& a, const SmSurface& b, bool o0, bool o1,
       const SmPoint2d& g0, const SmPoint2d& g1)
        : SmSurfaceSurfaceFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, r0, r1, a, b, o0 ? TRUE : FALSE,
                             o1 ? TRUE : FALSE, g0, g1)
    {
    }
    SmPoint2d guess(int i) const { return m_vUVGuesses[i]; }
    SmBoundaryTrimmingType trim() const { return m_eTrimType; }
};

struct Cra : SmConstantRadiusAssistedFS
{
    Cra(const SmContext& ctx, double r0, double r1, SmEdgeuse* eu)
        : SmConstantRadiusAssistedFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, r0, r1, eu)
    {
    }
    void setup(SmOffsetSurface* s0, SmOffsetSurface* s1, double r0, double r1, double o0, double o1, double ext)
    {
        delete m_pSurfaces[0];
        delete m_pSurfaces[1];
        m_pSurfaces[0] = s0;
        m_pSurfaces[1] = s1;
        m_dFilletRadii[0] = r0;
        m_dFilletRadii[1] = r1;
        m_dOrientations[0] = o0;
        m_dOrientations[1] = o1;
        m_pEdgeuses[0] = NULL;
        m_pEdgeuses[1] = NULL;
        m_dSurfaceExtensionFactor = ext;
    }
    // The plane solves are protected.
    SmStatus solve(bool guess, const SmPoint3d& orig, const SmVector3d& normal, const SmExtent2d& d1,
                   const SmExtent2d& d2, const SmVector2d& uv1, const SmVector2d& uv2, SmBoolean& found,
                   SmTsectPnt& t)
    {
        return guess ? GuessPointOnPlaneSolve(orig, normal, d1, d2, uv1, uv2, found, t)
                     : PointOnPlaneSolve(orig, normal, d1, d2, uv1, uv2, found, t);
    }
};

// The solver block `cls same surfaceA [surfaceB] r0 r1 o0 o1 ext`: `cls` 0 SmConstantRadiusFS,
// 1 SmSurfaceSurfaceFS (guesses (0.5, 0.5)), 2 SmConstantRadiusAssistedFS (built on a box edge,
// rail i offsetting its base by r_i o_i, o_i = +-1); the extension factor is set on all.
struct Built
{
    SmBrep* brep = NULL;
    SmSurface* a = NULL;
    SmSurface* b = NULL;
    SmSurface* copy = NULL;
    SmFilletSolver* fs = NULL;
    SmOffsetSurface* extra_a = NULL;
    SmOffsetSurface* extra_copy = NULL;
    ~Built()
    {
        delete extra_a;
        delete extra_copy;
        delete fs;
        delete copy;
        delete a;
        if (b != a)
            delete b;
        delete brep;
    }
    bool build(Rd& rd, const SmContext& ctx)
    {
        int64_t cls = rd.idx();
        bool same = rd.flag();
        a = read_surface(rd, ctx);
        b = same ? a : read_surface(rd, ctx);
        double r0 = rd.num();
        double r1 = rd.num();
        bool o0 = rd.flag();
        bool o1 = rd.flag();
        double ext = rd.num();
        if (cls == 0)
            fs = new (ctx) Cr(ctx, r0, r1, *a, *b, o0, o1);
        else if (cls == 1)
            fs = new (ctx) Ss(ctx, r0, r1, *a, *b, o0, o1, SmPoint2d(0.5, 0.5), SmPoint2d(0.5, 0.5));
        else
        {
            if (SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), 1.0, 1.0, 1.0, brep) != SM_SUCCESS || brep == NULL)
                return false;
            SmTArray<SmEdge*> edges;
            brep->GetEdges(edges);
            if (edges.GetSize() == 0)
                return false;
            SmEdgeuse* eu = edges[0]->GetPrimaryEdgeuse();
            if (eu == NULL)
                return false;
            Cra* c = new (ctx) Cra(ctx, r0, r1, eu);
            fs = c;
            double d0 = o0 ? 1.0 : -1.0;
            double d1 = o1 ? 1.0 : -1.0;
            SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(r0 * d0, *a);
            SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(r1 * d1, *b);
            s0->SetAllowExtension(TRUE);
            s1->SetAllowExtension(TRUE);
            c->setup(s0, s1, r0, r1, d0, d1, ext);
        }
        fs->SetExtensionFactor(ext);
        if (a->Copy(ctx, copy) != SM_SUCCESS || copy == NULL)
            return false;
        extra_a = new (ctx) SmOffsetSurface(r0, *a);
        extra_copy = new (ctx) SmOffsetSurface(r0, *copy);
        return true;
    }
    // Selector 0 NULL, 1 GetSurface(0), 2 GetSurface(1), 3 offset of base A, 4 offset of a copy.
    SmSurface* select(int64_t sel)
    {
        switch (sel)
        {
        case 1: return fs->GetSurface(0);
        case 2: return fs->GetSurface(1);
        case 3: return extra_a;
        case 4: return extra_copy;
        default: return NULL;
        }
    }
};

// Per rail offset after the op: `offset umin vmin umax vmax has_extension` (the natural
// UV domain and GetExtendedBaseSurface() != NULL show the extensions the op left).
void put_offsets(std::vector<double>& o, SmFilletSolver* fs)
{
    for (int i = 0; i < 2; i++)
    {
        SmOffsetSurface* s = fs->GetSurface(i);
        o.push_back(s->GetOffsetDistance());
        SmExtent2d d = s->GetNaturalUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back(s->GetExtendedBaseSurface() != NULL ? 1.0 : 0.0);
    }
}

// `solver do2 rail sel1 s1_index dim` `dim` x `min max`, `np per[np] ng guess[ng]`.
int32_t cr_init(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Built bt;
    if (!bt.build(rd, ctx))
        return SC_ERR_ARG;
    SmConstantRadiusFS* fs = (SmConstantRadiusFS*)bt.fs;
    ULONG do2 = rd.flag() ? 1 : 0;
    ULONG rail = (ULONG)rd.idx();
    SmSurface* s1 = bt.select(rd.idx());
    ULONG s1_index = (ULONG)rd.idx();
    ULONG dim = (ULONG)rd.idx();
    SmExtentNd ivls(dim);
    for (ULONG i = 0; i < dim; i++)
    {
        double lo = rd.num();
        double hi = rd.num();
        ivls[i] = SmExtent1d(lo, hi);
    }
    int64_t np = rd.idx();
    SmTArray<SmBoolean> per;
    for (int64_t i = 0; i < np; i++)
        per.Add(rd.flag() ? TRUE : FALSE);
    int64_t ng = rd.idx();
    SmTArray<double> g;
    for (int64_t i = 0; i < ng; i++)
        g.Add(rd.num());
    if (s1 == NULL)
        return SC_ERR_ARG;
    SmSurface* s2 = NULL;
    ULONG s2_index = 999;
    SmStatus stt = fs->LoadInitialValues(do2, rail, *s1, s1_index, s2, s2_index, ivls, per, g);
    o.push_back(st(stt));
    double s2_rail = -1.0;
    if (s2 != NULL)
        s2_rail = s2 == fs->GetSurface(0) ? 0.0 : s2 == fs->GetSurface(1) ? 1.0 : -2.0;
    o.push_back(s2_rail);
    o.push_back((double)s2_index);
    for (ULONG i = 0; i < dim; i++)
    {
        o.push_back(ivls[i].GetMin());
        o.push_back(ivls[i].GetMax());
    }
    o.push_back((double)per.GetSize());
    for (ULONG i = 0; i < per.GetSize(); i++)
        o.push_back(flg(per[i] != FALSE));
    o.push_back((double)g.GetSize());
    for (ULONG i = 0; i < g.GetSize(); i++)
        o.push_back(g[i]);
    put_offsets(o, fs);
    return SC_OK;
}

// `solver orig[3] normal[3] d1[4] d2[4] uv1[2] uv2[2]` on an SmConstantRadiusAssistedFS (cls 2).
int32_t plane_solve(Rd& rd, const SmContext& ctx, std::vector<double>& o, bool guess)
{
    Built bt;
    if (!bt.build(rd, ctx))
        return SC_ERR_ARG;
    Cra* fs = (Cra*)bt.fs;
    SmPoint3d orig = rd.pt3();
    SmVector3d normal = rd.pt3();
    double a0 = rd.num();
    double a1 = rd.num();
    double a2 = rd.num();
    double a3 = rd.num();
    double b0 = rd.num();
    double b1 = rd.num();
    double b2 = rd.num();
    double b3 = rd.num();
    SmExtent2d d1(a0, a1, a2, a3);
    SmExtent2d d2(b0, b1, b2, b3);
    SmPoint2d uv1 = rd.pt2();
    SmPoint2d uv2 = rd.pt2();
    SmBoolean found = FALSE;
    SmTsectPnt t;
    SmStatus stt = fs->solve(guess, orig, normal, d1, d2, uv1, uv2, found, t);
    o.push_back(st(stt));
    if (stt == SM_SUCCESS)
        o.push_back(flg(found != FALSE));
    if (stt == SM_SUCCESS && found)
    {
        put2(o, t.UVPos(0));
        put2(o, t.UVPos(1));
        if (!guess)
        {
            for (int i = 0; i < 2; i++)
            {
                put3(o, t.SrfPos(i));
                put3(o, t.SrfDu(i));
                put3(o, t.SrfDv(i));
                put3(o, t.SrfNorm(i));
            }
            put3(o, t.CrvPos());
        }
    }
    put_offsets(o, fs);
    return SC_OK;
}

// `surfaceA surfaceB r0 r1 o0 o1 g0[2] g1[2] calc kinds`.
int32_t ss(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* a = read_surface(rd, ctx);
    SmSurface* b = read_surface(rd, ctx);
    double r0 = rd.num();
    double r1 = rd.num();
    bool o0 = rd.flag();
    bool o1 = rd.flag();
    SmPoint2d g0 = rd.pt2();
    SmPoint2d g1 = rd.pt2();
    bool calc = rd.flag();
    Ss* fs = new (ctx) Ss(ctx, r0, r1, *a, *b, o0, o1, g0, g1);
    double cs = 0.0;
    if (calc)
    {
        // CalcFilletGeom's first steps; past its parallel-normal exit it would step along the
        // fillet and reach the solver's NULL executive, so it runs only for parallel normals.
        SmVector3d n1, n2;
        SmStatus e1 = fs->GetSurface(0)->EvaluateNormal(g0, TRUE, TRUE, n1);
        SmStatus e2 = fs->GetSurface(1)->EvaluateNormal(g1, TRUE, TRUE, n2);
        if (e1 == SM_SUCCESS && e2 == SM_SUCCESS && (n1 * n2).LengthSquared() < SM_EFF_ZERO_SQ)
            cs = st(fs->CalcFilletGeom());
        else
            cs = NOT_RUN;
    }
    o.push_back(cs);
    o.push_back(fs->GetFilletRadius(0));
    o.push_back(fs->GetFilletRadius(1));
    o.push_back(fs->GetOrientation(0));
    o.push_back(fs->GetOrientation(1));
    put_offsets(o, fs);
    put2(o, fs->guess(0));
    put2(o, fs->guess(1));
    o.push_back((double)fs->trim());
    o.push_back((double)fs->GetSolverType());
    o.push_back((double)fs->GetType());
    put_kinds(o, fs, rd);
    delete fs;
    delete a;
    delete b;
    return SC_OK;
}

// `kinds`: IsKindOf of an SmConstantRadiusAssistedFS, an SmSurfaceSurfaceFS and an
// SmVariableRadiusFS (over two planes, an SmLine centre and a constant linear law).
int32_t types(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t nk = rd.idx();
    std::vector<SM_TYPE> ks;
    for (int64_t i = 0; i < nk; i++)
        ks.push_back((SM_TYPE)rd.idx());
    SmBrep* brep = NULL;
    if (SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), 1.0, 1.0, 1.0, brep) != SM_SUCCESS || brep == NULL)
        return SC_ERR_ARG;
    SmTArray<SmEdge*> edges;
    brep->GetEdges(edges);
    if (edges.GetSize() == 0 || edges[0]->GetPrimaryEdgeuse() == NULL)
    {
        delete brep;
        return SC_ERR_ARG;
    }
    SmPlane* pa = new (ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0), SmVector2d(1, 1),
                                    SmExtent2d(0, 0, 1, 1), &ctx);
    SmPlane* pb = new (ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 0, 1), SmVector2d(1, 1),
                                    SmExtent2d(0, 0, 1, 1), &ctx);
    SmLine* center = new (ctx) SmLine(SmPoint3d(0, 0, 0), SmPoint3d(1, 0, 0), 3, &ctx);
    SmFilletLaw* law = new (ctx) SmLinearFilletLaw(0.1, 0.1);
    Cra* cra = new (ctx) Cra(ctx, 0.1, 0.1, edges[0]->GetPrimaryEdgeuse());
    Ss* ssf = new (ctx) Ss(ctx, 0.1, 0.1, *pa, *pb, true, true, SmPoint2d(0.5, 0.5), SmPoint2d(0.5, 0.5));
    SmVariableRadiusFS* vr = new (ctx) SmVariableRadiusFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, pa,
                                                          pb, 1.0, 1.0, center, *law, FALSE);
    const SmObject* objs[3] = {cra, ssf, vr};
    o.push_back(0.0);
    for (int j = 0; j < 3; j++)
    {
        o.push_back((double)objs[j]->GetType());
        for (size_t i = 0; i < ks.size(); i++)
            o.push_back(flg(objs[j]->IsKindOf(ks[i]) != FALSE));
    }
    delete vr;
    delete ssf;
    delete cra;
    delete law;
    delete center;
    delete pa;
    delete pb;
    delete brep;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_standard_solver_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t rc = SC_OK;
    switch (op)
    {
    case OP_CR_INIT: rc = cr_init(rd, ctx, o); break;
    case OP_GUESS: rc = plane_solve(rd, ctx, o, true); break;
    case OP_PLANE: rc = plane_solve(rd, ctx, o, false); break;
    case OP_SS: rc = ss(rd, ctx, o); break;
    case OP_TYPES: rc = types(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
