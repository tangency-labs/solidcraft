// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_custom_solver`.
// Wraps source/SMLib/src/SmFilletCustomSolver.cpp: SmEiffelTowerCrossSectionFSG::CreateSurface,
// SmEiffelTowerFS::LoadJacobian / LoadInitialValues and SmSecantConstantRadiusFS::LoadJacobian /
// LoadInitialValues.
//
// One entry point, sc_blend_fillet_custom_solver_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/blend_fillet_custom_solver.rs, which validates every
// input so upstream only touches valid memory. `out` receives the op status (0 = SM_SUCCESS, else
// the SmStatus value) and the op's outputs.
//
// The solvers' only constructors take an edge use: they are built on the first edge of an
// SmApiCreateBox box, then a derived class replaces the offset surfaces (offset surfaces of the
// given base surfaces), orientations, radii and extension factor, and clears the edge uses (the
// drop-to-surface branch of LoadInitialValues). The fillet surface generator only reads the
// solver's context and tolerance; its solver is an SmConstantRadiusFS over two given surfaces.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiPrimitives.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtentNd.h>
#include <SmFilletCustomSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmGeomUtility.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_custom_solver.rs).
enum : int32_t
{
    OP_CREATE = 0,
    OP_JAC_EIFFEL = 1,
    OP_JAC_SECANT = 2,
    OP_INIT_EIFFEL = 3,
    OP_INIT_SECANT = 4,
};

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
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface.
SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx)
{
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

// Reads `n p m cpts knots` into SmBSplineCurve(gw_CURVE*, 3, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = rd.num();
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

void put_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(flg(cur != NULL));
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

void put_surface(std::vector<double>& o, const SmBSplineSurface* s)
{
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX ss = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)ss);
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
    for (gw_INDEX i = 0; i <= ss; i++)
        o.push_back(sur->knv->U[i]);
}

// A solver with its protected members exposed for set-up.
template <class FS> struct Shim : FS
{
    Shim(const SmContext& ctx, double tol, double r0, double r1, double ang, SmEdgeuse* eu);

    void setup(SmOffsetSurface* s0, SmOffsetSurface* s1, double r0, double r1, double o0, double o1,
               double ext)
    {
        delete this->m_pSurfaces[0];
        delete this->m_pSurfaces[1];
        this->m_pSurfaces[0] = s0;
        this->m_pSurfaces[1] = s1;
        this->m_dFilletRadii[0] = r0;
        this->m_dFilletRadii[1] = r1;
        this->m_dOrientations[0] = o0;
        this->m_dOrientations[1] = o1;
        this->m_pEdgeuses[0] = NULL;
        this->m_pEdgeuses[1] = NULL;
        this->m_dSurfaceExtensionFactor = ext;
    }
};

template <>
Shim<SmEiffelTowerFS>::Shim(const SmContext& ctx, double tol, double r0, double r1, double ang,
                            SmEdgeuse* eu)
    : SmEiffelTowerFS(ctx, tol, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, r0, r1, ang, eu)
{
}

template <>
Shim<SmSecantConstantRadiusFS>::Shim(const SmContext& ctx, double tol, double r0, double,
                                     double ang, SmEdgeuse* eu)
    : SmSecantConstantRadiusFS(ctx, tol, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, r0, ang, eu)
{
}

// The solver input block: `same_base surfaceA [surfaceB] r0 r1 o0 o1 angle ext_factor`, built
// into a solver whose rail 0 offsets base A and rail 1 base B (A again when same_base). `extra`
// receives the two extra offset surfaces of the selectors (on base A, on a copy of A).
template <class FS> struct Solver
{
    SmBrep* brep = NULL;
    SmBSplineSurface* a = NULL;
    SmBSplineSurface* b = NULL;
    SmBSplineSurface* foreign = NULL;
    Shim<FS>* fs = NULL;
    SmOffsetSurface* extra_a = NULL;
    SmOffsetSurface* extra_foreign = NULL;

    ~Solver()
    {
        delete extra_a;
        delete extra_foreign;
        delete fs;
        delete a;
        if (b != a)
            delete b;
        delete foreign;
        delete brep;
    }

    bool build(Rd& rd, const SmContext& ctx)
    {
        bool same = rd.flag();
        a = read_surface(rd, ctx);
        b = same ? a : read_surface(rd, ctx);
        double r0 = rd.num();
        double r1 = rd.num();
        double o0 = rd.num();
        double o1 = rd.num();
        double ang = rd.num();
        double ext = rd.num();
        if (SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), 1.0, 1.0, 1.0, brep) != SM_SUCCESS ||
            brep == NULL)
            return false;
        SmTArray<SmEdge*> edges;
        brep->GetEdges(edges);
        if (edges.GetSize() == 0)
            return false;
        SmEdgeuse* eu = edges[0]->GetPrimaryEdgeuse();
        if (eu == NULL)
            return false;
        fs = new (ctx) Shim<FS>(ctx, 1.0e-6, r0, r1, ang, eu);
        SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(r0 * o0, *a);
        SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(r1 * o1, *b);
        s0->SetAllowExtension(TRUE);
        s1->SetAllowExtension(TRUE);
        fs->setup(s0, s1, r0, r1, o0, o1, ext);
        foreign = (SmBSplineSurface*)NULL;
        SmSurface* copy = NULL;
        if (a->Copy(ctx, copy) != SM_SUCCESS || copy == NULL)
            return false;
        foreign = (SmBSplineSurface*)copy;
        extra_a = new (ctx) SmOffsetSurface(r0 * o0, *a);
        extra_foreign = new (ctx) SmOffsetSurface(r0 * o0, *foreign);
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
        case 4: return extra_foreign;
        default: return NULL;
        }
    }
};

template <class FS> int32_t jacobian(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Solver<FS> sv;
    if (!sv.build(rd, ctx))
        return SC_ERR_ARG;
    ULONG rail = (ULONG)rd.idx();
    ULONG ne = (ULONG)rd.idx();
    ULONG np = (ULONG)rd.idx();
    SmSurface* s1 = sv.select(rd.idx());
    ULONG off1 = (ULONG)rd.idx();
    SmSurface* s2 = sv.select(rd.idx());
    ULONG off2 = (ULONG)rd.idx();
    int64_t nx = rd.idx();
    SmTArray<double> x;
    for (int64_t i = 0; i < nx; i++)
        x.Add(rd.num());
    int64_t nf = rd.idx();
    SmTArray<double> f;
    for (int64_t i = 0; i < nf; i++)
        f.Add(rd.num());
    bool has_j = rd.flag();
    ULONG rows = (ULONG)rd.idx();
    ULONG cols = (ULONG)rd.idx();
    SmMatrix j(has_j ? rows : 1, has_j ? cols : 1);
    if (has_j)
        for (ULONG r = 0; r < rows; r++)
            for (ULONG c = 0; c < cols; c++)
                j[r][c] = 0.0;
    SmBoolean found = TRUE;
    SmStatus stt = sv.fs->LoadJacobian(ne, np, rail, s1, off1, s2, off2, x, f,
                                       has_j ? &j : NULL, found);
    o.push_back(st(stt));
    o.push_back((double)ne);
    o.push_back((double)np);
    o.push_back(flg(found != FALSE));
    for (ULONG i = 0; i < f.GetSize(); i++)
        o.push_back(f[i]);
    if (has_j)
        for (ULONG r = 0; r < rows; r++)
            for (ULONG c = 0; c < cols; c++)
                o.push_back(j[r][c]);
    o.push_back(sv.fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(sv.fs->GetSurface(1)->GetOffsetDistance());
    return SC_OK;
}

template <class FS> int32_t initial(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Solver<FS> sv;
    if (!sv.build(rd, ctx))
        return SC_ERR_ARG;
    ULONG do2 = rd.flag() ? 1 : 0;
    ULONG rail = (ULONG)rd.idx();
    SmSurface* s1 = sv.select(rd.idx());
    ULONG s1_index = (ULONG)rd.idx();
    ULONG dim = (ULONG)rd.idx();
    SmExtentNd ivls(dim);
    for (ULONG i = 0; i < dim; i++)
    {
        double a = rd.num();
        double b = rd.num();
        ivls[i] = SmExtent1d(a, b);
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
    SmStatus stt =
        sv.fs->LoadInitialValues(do2, rail, *s1, s1_index, s2, s2_index, ivls, per, g);
    o.push_back(st(stt));
    double s2_rail = -1.0;
    if (s2 != NULL)
        s2_rail = s2 == sv.fs->GetSurface(0) ? 0.0 : s2 == sv.fs->GetSurface(1) ? 1.0 : -2.0;
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
    return SC_OK;
}

int32_t create(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    double tol = rd.num();
    SmBSplineCurve* r1 = read_curve(rd, ctx);
    SmBSplineCurve* r2 = read_curve(rd, ctx);
    int64_t k = rd.idx();
    std::vector<SmTsectPnt> pts((size_t)k);
    SmTArray<SmTsectPnt*> ptrs;
    for (int64_t i = 0; i < k; i++)
    {
        pts[(size_t)i].SrfPos(0) = rd.pt3();
        pts[(size_t)i].SrfPos(1) = rd.pt3();
    }
    for (int64_t i = 0; i < k; i++)
        ptrs.Add(&pts[(size_t)i]);

    // A solver for its context and tolerance.
    SmBSplineSurface* sa = NULL;
    SmBSplineSurface* sb = NULL;
    SmBSplineSurface::CreateBilinearSurface(ctx, SmPoint3d(0, 0, 0), SmPoint3d(1, 0, 0),
                                            SmPoint3d(0, 1, 0), SmPoint3d(1, 1, 0), sa);
    SmBSplineSurface::CreateBilinearSurface(ctx, SmPoint3d(0, 0, 0), SmPoint3d(0, 0, 1),
                                            SmPoint3d(1, 0, 0), SmPoint3d(1, 0, 1), sb);
    if (sa == NULL || sb == NULL)
    {
        delete sa;
        delete sb;
        delete r1;
        delete r2;
        return SC_ERR_ARG;
    }
    SmConstantRadiusFS* fs = new (ctx) SmConstantRadiusFS(
        ctx, tol, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, 1.0, 1.0, *sa, *sb, TRUE, TRUE);

    SmEiffelTowerCrossSectionFSG fsg;
    SmBSplineSurface* res = NULL;
    SmStatus stt = fsg.CreateSurface(*fs, NULL, r1, r2, ptrs, NULL, NULL, NULL, NULL, res);
    o.push_back(st(stt));
    put_curve(o, r1);
    put_curve(o, r2);
    if (stt == SM_SUCCESS)
    {
        o.push_back(flg(res != NULL));
        if (res)
            put_surface(o, res);
    }
    delete res;
    delete fs;
    delete sa;
    delete sb;
    delete r1;
    delete r2;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_custom_solver_eval(int32_t op, const double* in, int64_t n_in,
                                                     double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t rc = SC_OK;
    switch (op)
    {
    case OP_CREATE: rc = create(rd, ctx, o); break;
    case OP_JAC_EIFFEL: rc = jacobian<SmEiffelTowerFS>(rd, ctx, o); break;
    case OP_JAC_SECANT: rc = jacobian<SmSecantConstantRadiusFS>(rd, ctx, o); break;
    case OP_INIT_EIFFEL: rc = initial<SmEiffelTowerFS>(rd, ctx, o); break;
    case OP_INIT_SECANT: rc = initial<SmSecantConstantRadiusFS>(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
