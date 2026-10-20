// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_solver_p1`.
// Wraps source/SMLib/src/SmFilletSolver.cpp: the SmFilletSolver base (constructor, getters,
// offset-surface bookkeeping, SetupOffsetExtension, FinishConstruction, ComputeSurfaceValues,
// ComputeUVVectors, SplitFilletGeom), SmFilletSphereSolveENFO::Evaluate and
// SmRailUVCurveIntersectENFO::Evaluate.
//
// One entry point, sc_blend_fillet_solver_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/blend_fillet_solver_p1.rs, which validates every
// input. SmFilletSolver is abstract: the shim derives a solver that only names itself, so every
// virtual runs its base version. Members upstream leaves uninitialised (edge uses, executive,
// orientations, saved offsets, extension flags) are set by the derived constructor.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurveClass.h>
#include <SmFilletGeom.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFilletSolver.h>
#include <SmLine.h>
#include <SmMatrix.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <vector>

#include <SmFilletStandardSolver.h>
#include <SmTol.h>

#include <cstddef>

#include "sc_edgeuse_graph.h"

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_solver_p1.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_EXTENSION = 1,
    OP_SPHERE = 2,
    OP_RAIL_UV = 3,
    OP_SURF_VALUES = 4,
    OP_UV_VECTORS = 5,
    OP_SPLIT = 6,
    OP_FINISH = 7,
    OP_FSG = 8,
    OP_MISC = 9,
};

struct ShimFS : SmFilletSolver
{
    ShimFS(const SmContext& ctx, double tol, double ang, double tang)
        : SmFilletSolver(ctx, tol, ang, tang)
    {
        m_pEdgeuses[0] = m_pEdgeuses[1] = NULL;
        m_pExecutive = NULL;
        m_dOrientations[0] = m_dOrientations[1] = 1.0;
        m_dSavedOffsets[0] = m_dSavedOffsets[1] = 0.0;
        m_bExtendBefore = m_bExtendAfter = FALSE;
    }
    virtual SmFilletSolverType GetSolverType() { return SM_FS_UNKNOWN; }
    virtual void GetName(TCHAR* pc, size_t n)
    {
        if (n > 0)
            pc[0] = 0;
    }
    // A deterministic LoadJacobian (mirrored by the runner's ShimSolver) when `load`:
    // row rlNumEquations is x[rlSurf1Offset] - 0.5 x[0] + lRailIndex + 0.25 rlSurf2Offset
    // (the incoming value) with its two partials (written only inside rF and the
    // Jacobian), rlSurf2Offset = rlNumParameters, then rlNumEquations += 1,
    // rlNumParameters += 2, rbFoundAnswer = `found`. With `setup`, SetupOffsetValues
    // succeeds without changing anything (the base fails).
    bool load = false;
    bool found = false;
    bool setup = false;
    virtual SmStatus SetupOffsetValues(const SmTsectPnt& t, double* d = NULL)
    {
        if (!setup)
            return SmFilletSolver::SetupOffsetValues(t, d);
        return SM_SUCCESS;
    }
    virtual SmStatus LoadJacobian(ULONG& ne, ULONG& np, ULONG rail, SmSurface*& s1, ULONG& o1, SmSurface*& s2,
                                  ULONG& o2, const SmTArray<double>& x, SmTArray<double>& f, SmMatrix* j,
                                  SmBoolean& rbFound)
    {
        if (!load)
            return SmFilletSolver::LoadJacobian(ne, np, rail, s1, o1, s2, o2, x, f, j, rbFound);
        if (ne < f.GetSize() && o1 < x.GetSize() && x.GetSize() > 0)
            f[ne] = x[o1] - 0.5 * x[0] + (double)rail + 0.25 * (double)o2;
        if (j && ne < j->GetNumRows())
        {
            if (o1 < j->GetNumColumns())
                (*j)[ne][o1] = 1.0;
            if (j->GetNumColumns() > 0)
                (*j)[ne][0] = -0.5;
        }
        o2 = np;
        ne += 1;
        np += 2;
        rbFound = found ? TRUE : FALSE;
        return SM_SUCCESS;
    }
    double saved(int i) const { return m_dSavedOffsets[i]; }
    void set_orientation(int i, double o) { m_dOrientations[i] = o; }
    void set_edgeuses(SmEdgeuse* a, SmEdgeuse* b)
    {
        m_pEdgeuses[0] = a;
        m_pEdgeuses[1] = b;
    }
    SmStatus finish() { return FinishConstruction(); }
};

// A solver laid out as SmConstantDistanceFS (the base, then the distance): the
// SmFindClippedRadiusEFO constructor casts its solver to SmConstantDistanceFS and reads
// GetDistance() (static shim). Every other virtual is the base version.
struct ShimDistFS : SmFilletSolver
{
    double m_dDistance;
    ShimDistFS(const SmContext& ctx, double tol, double dist) : SmFilletSolver(ctx, tol, 0.1, 0.01), m_dDistance(dist)
    {
        m_pEdgeuses[0] = m_pEdgeuses[1] = NULL;
        m_pExecutive = NULL;
        m_dOrientations[0] = m_dOrientations[1] = 1.0;
        m_dSavedOffsets[0] = m_dSavedOffsets[1] = 0.0;
        m_bExtendBefore = m_bExtendAfter = FALSE;
    }
    virtual SmFilletSolverType GetSolverType() { return SM_FS_UNKNOWN; }
    virtual void GetName(TCHAR* pc, size_t n)
    {
        if (n > 0)
            pc[0] = 0;
    }
    void set_orientation(int i, double o) { m_dOrientations[i] = o; }
};

struct CDPeek : SmConstantDistanceFS
{
    static constexpr size_t distance_offset() { return offsetof(CDPeek, m_dDistance); }
};
static_assert(CDPeek::distance_offset() == offsetof(ShimDistFS, m_dDistance),
              "ShimDistFS must lay out its distance as SmConstantDistanceFS does");

// Peeks at the protected FSG members.
struct CircPeek : SmCircularCrossSectionFSG
{
    CircPeek() {}
    CircPeek(SmBoolean a, double t, SmBoolean m, SmBoolean c) : SmCircularCrossSectionFSG(a, t, m, c) {}
    double mirror() const { return m_bMirror ? 1.0 : 0.0; }
    double complement() const { return m_bComplement ? 1.0 : 0.0; }
};

void put_pt(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// `nx x[nx] nf f[nf] has_j rows cols` -> arrays and an optional zero-filled matrix.
struct Arrays
{
    SmTArray<double> x;
    SmTArray<double> f;
    SmMatrix* j = NULL;
    ~Arrays() { delete j; }
    void read(Rd& rd)
    {
        int64_t nx = rd.idx();
        for (int64_t i = 0; i < nx; i++)
            x.Add(rd.num());
        int64_t nf = rd.idx();
        for (int64_t i = 0; i < nf; i++)
            f.Add(rd.num());
        bool has_j = rd.flag();
        ULONG rows = (ULONG)rd.idx();
        ULONG cols = (ULONG)rd.idx();
        if (has_j)
        {
            j = new SmMatrix(rows, cols);
            for (ULONG r = 0; r < rows; r++)
                for (ULONG c = 0; c < cols; c++)
                    (*j)[r][c] = 0.0;
        }
    }
    void put(std::vector<double>& o)
    {
        for (ULONG i = 0; i < f.GetSize(); i++)
            o.push_back(f[i]);
        if (j)
            for (ULONG r = 0; r < j->GetNumRows(); r++)
                for (ULONG c = 0; c < j->GetNumColumns(); c++)
                    o.push_back((*j)[r][c]);
    }
};

// Offset surfaces made by the state script, keyed by creation order; `alive` false once
// SetOffsetsData deleted one.
struct Pool
{
    std::vector<SmOffsetSurface*> made;
    std::vector<bool> alive;
    double key(const SmOffsetSurface* s) const
    {
        // newest live entry first: a deleted surface's address may be reused
        for (size_t i = made.size(); i > 0; i--)
            if (alive[i - 1] && made[i - 1] == s)
                return (double)(i - 1);
        return -1.0;
    }
    SmOffsetSurface* add(SmOffsetSurface* s)
    {
        made.push_back(s);
        alive.push_back(true);
        return s;
    }
};

void put_state(std::vector<double>& o, ShimFS& fs, const Pool& pool)
{
    for (ULONG i = 0; i < 2; i++)
    {
        SmOffsetSurface* s = fs.GetSurface(i);
        o.push_back(flg(s != NULL));
        o.push_back(s ? pool.key(s) : -1.0);
        o.push_back(s ? s->GetOffsetDistance() : 0.0);
        o.push_back(fs.GetOrientation(i));
        o.push_back(fs.saved((int)i));
    }
    o.push_back(fs.GetExtensionFactor());
    o.push_back(flg(fs.GetReverseTrim()));
    o.push_back((double)fs.GetStatus());
}

int32_t run_state(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    double tol = rd.num();
    double ang = rd.num();
    double tang = rd.num();
    SmSurface* base[2] = {NULL, NULL};
    base[0] = read_surface(rd, ctx);
    base[1] = read_surface(rd, ctx);
    ShimFS* fs = new (ctx) ShimFS(ctx, tol, ang, tang);
    Pool pool;
    for (ULONG i = 0; i < 2; i++)
    {
        bool has = rd.flag();
        double off = rd.num();
        if (has)
        {
            SmOffsetSurface* s = pool.add(new (ctx) SmOffsetSurface(off, *base[i]));
            fs->SetSurface(i, s);
        }
    }
    fs->set_orientation(0, rd.num());
    fs->set_orientation(1, rd.num());

    o.push_back(fs->GetThisApproxTol3d());
    o.push_back(fs->GetThisAngTolRad());
    o.push_back(fs->GetTangencyTolerance());
    o.push_back(fs->GetConversionTol());
    o.push_back(fs->GetExtensionFactor());
    o.push_back(flg(fs->GetReverseTrim()));
    o.push_back((double)fs->GetStatus());
    o.push_back((double)fs->GetJacobianSize());
    o.push_back(flg(fs->CanMakeAnalytic()));
    o.push_back(flg(fs->OffsetRadiiCanChange()));
    SmTArray<SmFilletGeom*> geoms;
    fs->GetFilletGeoms(geoms);
    o.push_back((double)geoms.GetSize());
    o.push_back(flg(fs->GetFirstFilletGeom() == fs->GetLastFilletGeom()));
    o.push_back((double)fs->GetType());
    o.push_back(flg(fs->IsKindOf(SmFilletSolver_TYPE)));
    o.push_back(flg(fs->IsKindOf(SmObject_TYPE)));
    o.push_back(flg(fs->IsKindOf(SmFace_TYPE)));
    o.push_back(flg(fs->GetEdgeuse(0) == NULL));
    o.push_back(flg(fs->GetVertex(0) == NULL));
    o.push_back(flg(fs->GetFilletSurfaceGenerator() == NULL));
    o.push_back(flg(fs->GetFilletGeomForSurfaces() == NULL));
    o.push_back(flg(fs->GetFilletExecutive() == NULL));

    SmFilletSolverOffsetsData saved;
    int64_t k = rd.idx();
    for (int64_t c = 0; c < k; c++)
    {
        int64_t cmd = rd.idx();
        double a = rd.num();
        double b = rd.num();
        switch (cmd)
        {
        case 0: fs->SetSurfacesToZeroOffset(); break;
        case 1: fs->ReloadSurfaceOffsets(); break;
        case 2: fs->GetOffsetsData(saved); break;
        case 3:
        {
            // SetOffsetsData deletes the current surface when it differs from the saved one
            for (ULONG i = 0; i < 2; i++)
            {
                SmOffsetSurface* cur = fs->GetSurface(i);
                if (cur != saved.m_pSurfaces[i] && cur != NULL)
                {
                    double key = pool.key(cur);
                    if (key >= 0.0)
                        pool.alive[(size_t)key] = false;
                }
            }
            fs->SetOffsetsData(saved);
            break;
        }
        case 4:
        {
            ULONG rail = (ULONG)a;
            fs->SetSurface(rail, pool.add(new (ctx) SmOffsetSurface(b, *base[rail])));
            break;
        }
        case 5: fs->SetExtensionFactor(a); break;
        case 6: fs->SetReverseTrim(a != 0.0 ? TRUE : FALSE); break;
        case 7: fs->SetStatus((SmFilStatus)(int)a); break;
        default: break;
        }
        put_state(o, *fs, pool);
    }

    // the solver deletes its two surfaces; the shim deletes every other live one
    for (size_t i = 0; i < pool.made.size(); i++)
        if (pool.alive[i] && pool.made[i] != fs->GetSurface(0) && pool.made[i] != fs->GetSurface(1))
            delete pool.made[i];
    delete fs;
    delete base[0];
    delete base[1];
    return 0;
}

int32_t run_extension(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    double ext = rd.num();
    SmSurface* s = read_surface(rd, ctx);
    double u0 = rd.num();
    double v0 = rd.num();
    double u1 = rd.num();
    double v1 = rd.num();
    SmExtent2d dom(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
    fs.SetExtensionFactor(ext);
    fs.SetupOffsetExtension(s, dom);
    o.push_back(0.0);
    o.push_back(dom.GetMin().x);
    o.push_back(dom.GetMin().y);
    o.push_back(dom.GetMax().x);
    o.push_back(dom.GetMax().y);
    delete s;
    return 0;
}

// `surf has0 off0 has1 off1`: offset surfaces of one base installed into `fs`; returns the
// base (deleted by the caller after the solver, which deletes the offset surfaces).
SmSurface* install_surfaces(Rd& rd, const SmContext& ctx, SmFilletSolver& fs)
{
    SmSurface* base = read_surface(rd, ctx);
    for (ULONG i = 0; i < 2; i++)
    {
        bool has = rd.flag();
        double off = rd.num();
        if (has)
            fs.SetSurface(i, new (ctx) SmOffsetSurface(off, *base));
    }
    return base;
}

int32_t run_sphere(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmPoint3d c = rd.vec();
    double r = rd.num();
    bool adjusts = rd.flag();
    bool load = rd.flag();
    bool found_flag = rd.flag();
    SmSurface* base = NULL;
    {
        ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
        fs.load = load;
        fs.found = found_flag;
        base = install_surfaces(rd, ctx, fs);
        Arrays a;
        a.read(rd);
        SmFilletSphereSolveENFO e(c, r, fs);
        e.SetPlaneAdjusts(adjusts ? TRUE : FALSE);
        SmBoolean found = FALSE;
        SmStatus s = e.Evaluate(a.x, a.f, a.j, found);
        o.push_back(st(s));
        o.push_back(flg(found));
        a.put(o);
    }
    delete base;
    return 0;
}

int32_t run_rail_uv(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    ULONG rail = (ULONG)rd.idx();
    bool load = rd.flag();
    bool found = rd.flag();
    // an SmLine UV curve (virtual Evaluate is analytic) replaces the B-spline when given
    SmLine* line = NULL;
    if (rd.flag())
    {
        SmPoint3d ls = rd.vec();
        SmPoint3d le = rd.vec();
        ULONG ldim = (ULONG)rd.idx();
        line = new (ctx) SmLine(ls, le, ldim, &ctx);
    }
    SmBSplineCurve* crv = read_curve(rd, ctx);
    if (line != NULL)
    {
        delete crv;
        crv = line;
    }
    double t0 = rd.num();
    double t1 = rd.num();
    Arrays a;
    a.read(rd);
    ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
    fs.load = load;
    fs.found = found;
    SmPlane plane(SmPoint3d(0.0, 0.0, 0.0), SmVector3d(0.0, 0.0, 1.0), &ctx);
    {
        SmRailUVCurveIntersectENFO e(*crv, SmExtent1d(t0, t1), plane, rail, fs);
        SmBoolean found = FALSE;
        SmStatus s = e.Evaluate(a.x, a.f, a.j, found);
        o.push_back(st(s));
        o.push_back(flg(found));
        a.put(o);
    }
    delete crv;
    return 0;
}

int32_t run_surf_values(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    bool setup = rd.flag();
    SmSurface* base = NULL;
    {
        ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
        fs.setup = setup;
        base = install_surfaces(rd, ctx, fs);
        SmPoint2d uv[2];
        uv[0] = rd.uv();
        uv[1] = rd.uv();
        SmTsectPnt t;
        SmStatus s = fs.ComputeSurfaceValues(uv, t);
        o.push_back(st(s));
        // on success: per surface position, du, dv, unit normal and UV; then the curve
        // position and the tangent plane angle
        if (s == SM_SUCCESS)
        {
            for (ULONG i = 0; i < 2; i++)
            {
                put_pt(o, t.SrfPos(i));
                put_pt(o, t.SrfDu(i));
                put_pt(o, t.SrfDv(i));
                put_pt(o, t.SrfNorm(i));
                o.push_back(t.UVPos(i).x);
                o.push_back(t.UVPos(i).y);
            }
            put_pt(o, t.CrvPos());
            o.push_back(t.m_dTangentPlaneAngleRad);
        }
    }
    delete base;
    return 0;
}

// `kind a b c d`: 0 SmFilletSurfaceGenerator, 1 SmLinearCrossSectionFSG, 2/3
// SmCircularCrossSectionFSG (defaults / `a b c d`), 4/5 SmBlendCurveCrossSectionFSG
// (defaults / scale `a`, continuity `b`) -> `type approx tol% mirror complement scale
// continuity create` (`create`: the base CreateSurface status, -1 for the derived kinds).
int32_t run_fsg(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t kind = rd.idx();
    double a = rd.num(), b = rd.num(), c = rd.num(), d = rd.num();
    double v[6] = {0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    // the linear kind: `n_rails` (0 or 2) then the rails its CreateSurface is given
    SmBSplineCurve* rail1 = NULL;
    SmBSplineCurve* rail2 = NULL;
    if (kind == 1 && rd.idx() == 2)
    {
        rail1 = read_curve(rd, ctx);
        rail2 = read_curve(rd, ctx);
    }
    SmFilletSurfaceGenerator* g = NULL;
    switch (kind)
    {
    case 0: g = new (ctx) SmFilletSurfaceGenerator(); break;
    case 1: g = new (ctx) SmLinearCrossSectionFSG(); break;
    case 2:
    case 3:
    {
        CircPeek* p = kind == 2 ? new (ctx) CircPeek()
                                : new (ctx) CircPeek(a != 0.0 ? TRUE : FALSE, b, c != 0.0 ? TRUE : FALSE,
                                                     d != 0.0 ? TRUE : FALSE);
        v[0] = flg(p->GetApproximateArcs());
        v[1] = p->GetTolPercentRadius();
        v[2] = p->mirror();
        v[3] = p->complement();
        g = p;
        break;
    }
    default:
    {
        SmBlendCurveCrossSectionFSG* p =
            kind == 4 ? new (ctx) SmBlendCurveCrossSectionFSG() : new (ctx) SmBlendCurveCrossSectionFSG(a, (ULONG)b);
        v[4] = p->GetBlendScale();
        v[5] = (double)p->GetContinuity();
        g = p;
        break;
    }
    }
    o.push_back((double)g->GetFilletSurfaceGeneratorType());
    for (double x : v)
        o.push_back(x);
    if (kind == 0)
    {
        ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
        SmTArray<SmTsectPnt*> pts;
        SmBSplineSurface* out = NULL;
        o.push_back(st(g->CreateSurface(fs, NULL, NULL, NULL, pts, NULL, NULL, NULL, NULL, out)));
    }
    else if (rail1 != NULL)
    {
        // SmLinearCrossSectionFSG::CreateSurface: the ruled surface of the two rails
        // (the solver, centre line, points and UV curves are not used): `status` then
        // `has_nurb [n m p q r s cpts knots_u knots_v]`
        ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
        SmTArray<SmTsectPnt*> pts;
        SmBSplineSurface* out = NULL;
        SmStatus s = g->CreateSurface(fs, NULL, rail1, rail2, pts, NULL, NULL, NULL, NULL, out);
        o.push_back(st(s));
        const gw_SURFACE* sur = (s == SM_SUCCESS && out != NULL) ? out->GetGwNurbPointer() : NULL;
        o.push_back(flg(sur != NULL));
        if (sur != NULL)
        {
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
        delete out;
    }
    else
        o.push_back(-1.0);
    delete rail1;
    delete rail2;
    delete g;
    return 0;
}

// `which ...`: 0 `has ang` -> SmTol::GetAngTolRad(solver or NULL); 1 `do2 rail idx2` -> the
// base LoadInitialValues: `status do2 idx2 surface2_null n_periodic n_guess`.
int32_t run_misc(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t which = rd.idx();
    if (which == 0)
    {
        bool has = rd.flag();
        double ang = rd.num();
        ShimFS fs(ctx, 1.0e-6, ang, 0.01);
        o.push_back(SmTol::GetAngTolRad(has ? (const SmFilletSolver*)&fs : (const SmFilletSolver*)NULL));
        return 0;
    }
    ULONG do2 = (ULONG)rd.idx();
    ULONG rail = (ULONG)rd.idx();
    ULONG idx2 = (ULONG)rd.idx();
    ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
    SmPlane plane(SmPoint3d(0.0, 0.0, 0.0), SmVector3d(0.0, 0.0, 1.0), &ctx);
    SmSurface* s2 = NULL;
    SmExtentNd ext(3);
    SmTArray<SmBoolean> per;
    SmTArray<double> guess;
    SmStatus s = fs.LoadInitialValues(do2, rail, plane, 0, s2, idx2, ext, per, guess);
    o.push_back(st(s));
    o.push_back((double)do2);
    o.push_back((double)idx2);
    o.push_back(flg(s2 == NULL));
    o.push_back((double)per.GetSize());
    o.push_back((double)guess.GetSize());
    return 0;
}

int32_t run_uv_vectors(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
    SmTsectPnt t;
    for (ULONG i = 0; i < 2; i++)
    {
        t.SrfDu(i) = rd.vec();
        t.SrfDv(i) = rd.vec();
    }
    t.CrvDeriv() = rd.vec();
    for (ULONG i = 0; i < 2; i++)
        t.UVDeriv(i) = rd.uv();
    o.push_back(st(fs.ComputeUVVectors(t)));
    for (ULONG i = 0; i < 2; i++)
    {
        o.push_back(t.UVDeriv(i).x);
        o.push_back(t.UVDeriv(i).y);
    }
    return 0;
}

int32_t run_split(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    ULONG idx = (ULONG)rd.idx();
    ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
    SmTArray<SmExtent1d> i1, i2;
    SmCurveClassification cc1, cc2;
    o.push_back(st(fs.SplitFilletGeom(idx, (SmBoundaryTrimmingType)0, i1, i2, cc1, cc2)));
    SmTArray<SmFilletGeom*> geoms;
    fs.GetFilletGeoms(geoms);
    o.push_back((double)geoms.GetSize());
    return 0;
}

int32_t run_finish(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* base = read_surface(rd, ctx);
    bool has0 = rd.flag();
    bool has1 = rd.flag();
    double off = rd.num();
    {
        ShimFS fs(ctx, 1.0e-6, 0.1, 0.01);
        if (has0)
            fs.SetSurface(0, new (ctx) SmOffsetSurface(off, *base));
        if (has1)
            fs.SetSurface(1, new (ctx) SmOffsetSurface(-off, *base));
        o.push_back(st(fs.finish()));
    } // the solver deletes its fillet geometries and surfaces before the base goes
    delete base;
    return 0;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_solver_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                 int64_t cap, int64_t* out_len)
{
    if (in == NULL || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    const SmContext& ctx = the_context();
    Rd rd{in, 0};
    std::vector<double> o;
    int32_t rc = SC_ERR_ARG;
    switch (op)
    {
    case OP_STATE: rc = run_state(rd, ctx, o); break;
    case OP_EXTENSION: rc = run_extension(rd, ctx, o); break;
    case OP_SPHERE: rc = run_sphere(rd, ctx, o); break;
    case OP_RAIL_UV: rc = run_rail_uv(rd, ctx, o); break;
    case OP_SURF_VALUES: rc = run_surf_values(rd, ctx, o); break;
    case OP_UV_VECTORS: rc = run_uv_vectors(rd, ctx, o); break;
    case OP_SPLIT: rc = run_split(rd, ctx, o); break;
    case OP_FINISH: rc = run_finish(rd, ctx, o); break;
    case OP_FSG: rc = run_fsg(rd, ctx, o); break;
    case OP_MISC: rc = run_misc(rd, ctx, o); break;
    default: break;
    }
    (void)n_in;
    if (rc != 0)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// ---- solvers for the static shim (blend_fillet_solver_p1_static.cpp) ----------------------

extern "C" void* sc_fsp1_solver_new(int32_t load, int32_t found, double tol)
{
    ShimFS* fs = new (the_context()) ShimFS(the_context(), tol, 0.1, 0.01);
    fs->load = load != 0;
    fs->found = found != 0;
    return (SmFilletSolver*)fs;
}

extern "C" void* sc_fsp1_dist_solver_new(double dist, double tol)
{
    return (SmFilletSolver*)new (the_context()) ShimDistFS(the_context(), tol, dist);
}

extern "C" void sc_fsp1_solver_delete(void* fs) { delete (SmFilletSolver*)fs; }

extern "C" void sc_fsp1_solver_set_edgeuses(void* fs, SmEdgeuse* eu0, SmEdgeuse* eu1)
{
    static_cast<ShimFS*>((SmFilletSolver*)fs)->set_edgeuses(eu0, eu1);
}

extern "C" void sc_fsp1_solver_set_surface(void* fs, uint32_t rail, SmOffsetSurface* s)
{
    ((SmFilletSolver*)fs)->SetSurface(rail, s);
}

extern "C" void sc_fsp1_dist_solver_set_orientation(void* fs, uint32_t rail, double o)
{
    static_cast<ShimDistFS*>((SmFilletSolver*)fs)->set_orientation((int)rail, o);
}

extern "C" SmVertex* sc_fsp1_solver_vertex(void* fs, uint32_t index) { return ((SmFilletSolver*)fs)->GetVertex(index); }

extern "C" uint32_t sc_fsp1_solver_rail_on_face(void* fs, SmFace* face)
{
    return (uint32_t)((SmFilletSolver*)fs)->FindIndexOfRailOnFace(face);
}

extern "C" uint32_t sc_fsp1_solver_rail_x_side(void* fs, SmEdgeuse* side)
{
    return (uint32_t)((SmFilletSolver*)fs)->FindIndexOfRailXSideEdgeuse(side);
}
