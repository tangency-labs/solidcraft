// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_solver_p3`. Wraps SmFilletSolver::StepSolve
// (source/SMLib/src/SmFilletSolver.cpp).
//
// SmFilletSolver is abstract and every solver of the library belongs to an unported unit, so
// the shim derives a deterministic solver (`P3FS`, mirrored by the runner's `HarnessSolver`):
//   GetSolverType      the configured SmFilletSolverType (unknown, constant radius, constant
//                      distance or constant radius assisted; never variable radius, whose
//                      cast to SmVariableRadiusFS would be undefined);
//   GetJacobianSize    4, or 5 with the `extra` unknown;
//   SetupOffsetValues  sets each offset surface to radius * orientation, returns the radius;
//   LoadInitialValues  SM_ERR when `init_fail`; else, with `extra`, appends the extra
//                      unknown (interval [0, 1000], not periodic, guess = radius);
//   LoadJacobian       rows ne..ne+2: S0(x[o1], x[o1+1]) - S1(x[np], x[np+1]) with their
//                      partials (offset surfaces, Evaluate1stDerivatives(uv, TRUE, TRUE)),
//                      rlSurf2Offset = np, ne += 3, np += 2; with `extra` one more row
//                      x[np] - radius; found when every row is below 1e-12.
// Its first member after the base is m_dDistance at the offset SmConstantDistanceFS keeps
// it, because SmFindClippedRadiusEFO's constructor casts the solver to SmConstantDistanceFS
// and calls the inline GetDistance().
//
// One entry point, sc_blend_fillet_solver_p3_eval(op, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/blend_fillet_solver_p3.rs, which validates
// every input. Surfaces use the blocks of unit topology.edgeuse_p1 (sc_edgeuse_graph.h).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmMatrix.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <cmath>
#include <cstddef>
#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_solver_p3.rs).
enum : int32_t
{
    OP_STEP = 0,
};

struct P3FS : SmFilletSolver
{
    double m_dDistance; // first member: SmConstantDistanceFS::m_dDistance
    SmFilletSolverType m_eType;
    double m_dRadius;
    bool m_bExtra;
    bool m_bInitFail;

    P3FS(const SmContext& ctx, double tol, SmFilletSolverType ty, double radius, double dist, bool extra,
         bool init_fail)
        : SmFilletSolver(ctx, tol, 0.1, 0.01), m_dDistance(dist), m_eType(ty), m_dRadius(radius),
          m_bExtra(extra), m_bInitFail(init_fail)
    {
        m_pEdgeuses[0] = m_pEdgeuses[1] = NULL;
        m_pExecutive = NULL;
        m_dOrientations[0] = m_dOrientations[1] = 1.0;
        m_dSavedOffsets[0] = m_dSavedOffsets[1] = 0.0;
        m_bExtendBefore = m_bExtendAfter = FALSE;
    }
    virtual SmFilletSolverType GetSolverType() { return m_eType; }
    virtual void GetName(TCHAR* pc, size_t n)
    {
        if (n > 0)
            pc[0] = 0;
    }
    virtual ULONG GetJacobianSize() const { return m_bExtra ? 5 : 4; }
    virtual SmStatus SetupOffsetValues(const SmTsectPnt&, double* pOffsetDist = NULL)
    {
        for (int i = 0; i < 2; i++)
            if (m_pSurfaces[i])
                m_pSurfaces[i]->SetOffsetDistance(m_dRadius * m_dOrientations[i]);
        if (pOffsetDist)
            *pOffsetDist = m_dRadius;
        return SM_SUCCESS;
    }
    virtual SmStatus LoadInitialValues(ULONG&, ULONG, const SmSurface&, ULONG, SmSurface*&, ULONG&,
                                       SmExtentNd& rExtents, SmTArray<SmBoolean>& rPer, SmTArray<double>& rGuess)
    {
        if (m_bInitFail)
            return SM_ERR;
        if (m_bExtra)
        {
            rExtents[4] = SmExtent1d(0.0, 1000.0);
            rPer.Add(FALSE);
            rGuess.Add(m_dRadius);
        }
        return SM_SUCCESS;
    }
    virtual SmStatus LoadJacobian(ULONG& ne, ULONG& np, ULONG, SmSurface*&, ULONG& o1, SmSurface*&, ULONG& o2,
                                  const SmTArray<double>& x, SmTArray<double>& f, SmMatrix* j, SmBoolean& rbFound)
    {
        if (o1 + 1 >= x.GetSize() || np + 1 >= x.GetSize() || ne + 2 >= f.GetSize())
            return SM_ERR;
        if (m_pSurfaces[0] == NULL || m_pSurfaces[1] == NULL)
            return SM_ERR;
        SmPoint3d P, Q;
        SmVector3d Pu, Pv, Qu, Qv;
        SER(m_pSurfaces[0]->Evaluate1stDerivatives(SmPoint2d(x[o1], x[o1 + 1]), TRUE, TRUE, P, Pu, Pv));
        SER(m_pSurfaces[1]->Evaluate1stDerivatives(SmPoint2d(x[np], x[np + 1]), TRUE, TRUE, Q, Qu, Qv));
        SmVector3d D = P - Q;
        f[ne] = D.x;
        f[ne + 1] = D.y;
        f[ne + 2] = D.z;
        if (j && ne + 2 < j->GetNumRows() && np + 1 < j->GetNumColumns() && o1 + 1 < j->GetNumColumns())
        {
            const double pu[3] = {Pu.x, Pu.y, Pu.z};
            const double pv[3] = {Pv.x, Pv.y, Pv.z};
            const double qu[3] = {Qu.x, Qu.y, Qu.z};
            const double qv[3] = {Qv.x, Qv.y, Qv.z};
            for (ULONG i = 0; i < 3; i++)
            {
                (*j)[ne + i][o1] = pu[i];
                (*j)[ne + i][o1 + 1] = pv[i];
                (*j)[ne + i][np] = -qu[i];
                (*j)[ne + i][np + 1] = -qv[i];
            }
        }
        o2 = np;
        double big = std::fabs(D.x);
        if (std::fabs(D.y) > big)
            big = std::fabs(D.y);
        if (std::fabs(D.z) > big)
            big = std::fabs(D.z);
        ne += 3;
        np += 2;
        if (m_bExtra)
        {
            if (np >= x.GetSize() || ne >= f.GetSize())
                return SM_ERR;
            f[ne] = x[np] - m_dRadius;
            if (j && ne < j->GetNumRows() && np < j->GetNumColumns())
                (*j)[ne][np] = 1.0;
            if (std::fabs(f[ne]) > big)
                big = std::fabs(f[ne]);
            ne += 1;
            np += 1;
        }
        rbFound = big < 1.0e-12 ? TRUE : FALSE;
        return SM_SUCCESS;
    }
    void set_orientation(int i, double o) { m_dOrientations[i] = o; }
    double offset(int i) const { return m_pSurfaces[i] ? m_pSurfaces[i]->GetOffsetDistance() : 0.0; }
};

struct CDPeek : SmConstantDistanceFS
{
    static constexpr size_t distance_offset() { return offsetof(CDPeek, m_dDistance); }
};
static_assert(CDPeek::distance_offset() == offsetof(P3FS, m_dDistance),
              "P3FS must lay out its distance as SmConstantDistanceFS does");

SmExtent2d read_domain(Rd& rd)
{
    double u0 = rd.num();
    double v0 = rd.num();
    double u1 = rd.num();
    double v1 = rd.num();
    return SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
}

// OP_STEP
//   in : type, tol, radius, orient0, orient1, distance, extra, init_fail, allow_extension,
//        surface 0, surface 1 (bases of the offset surfaces),
//        current: uv0, uv1, duv0, duv1, crv_pos, crv_deriv; step, domain1, domain2,
//        next (before): uv0, uv1, user doubles[4]
//   out: status; on success found, clipped, boundary, step, next uv0, uv1, user doubles[4],
//        offset distance 0, offset distance 1
int32_t op_step(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmFilletSolverType ty = (SmFilletSolverType)rd.idx();
    double tol = rd.num();
    double radius = rd.num();
    double o0 = rd.num();
    double o1 = rd.num();
    double dist = rd.num();
    bool extra = rd.flag();
    bool init_fail = rd.flag();
    bool allow_ext = rd.flag();
    SmSurface* b0 = read_surface(rd, ctx);
    SmSurface* b1 = read_surface(rd, ctx);
    P3FS* fs = new (ctx) P3FS(ctx, tol, ty, radius, dist, extra, init_fail);
    fs->set_orientation(0, o0);
    fs->set_orientation(1, o1);
    SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(radius * o0, *b0);
    SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(radius * o1, *b1);
    if (allow_ext)
    {
        s0->SetAllowExtension(TRUE);
        s1->SetAllowExtension(TRUE);
    }
    fs->SetSurface(0, s0);
    fs->SetSurface(1, s1);

    SmTsectPnt cur;
    cur.UVPos(0) = rd.uv();
    cur.UVPos(1) = rd.uv();
    cur.UVDeriv(0) = rd.uv();
    cur.UVDeriv(1) = rd.uv();
    cur.CrvPos() = rd.vec();
    cur.CrvDeriv() = rd.vec();
    double step = rd.num();
    SmExtent2d d1 = read_domain(rd);
    SmExtent2d d2 = read_domain(rd);
    SmTsectPnt next;
    next.UVPos(0) = rd.uv();
    next.UVPos(1) = rd.uv();
    for (int i = 0; i < 4; i++)
        next.m_adUserDoubles[i] = rd.num();

    SmBoolean found = FALSE, clipped = FALSE, boundary = FALSE;
    SmStatus s = fs->StepSolve(cur, step, d1, d2, found, clipped, boundary, next);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back(flg(found));
        o.push_back(flg(clipped));
        o.push_back(flg(boundary));
        o.push_back(step);
        o.push_back(next.UVPos(0).x);
        o.push_back(next.UVPos(0).y);
        o.push_back(next.UVPos(1).x);
        o.push_back(next.UVPos(1).y);
        for (int i = 0; i < 4; i++)
            o.push_back(next.m_adUserDoubles[i]);
        o.push_back(fs->offset(0));
        o.push_back(fs->offset(1));
    }
    else
    {
        // a failed solve keeps the flags it set (SmFilletIntersector reads them)
        o.push_back(flg(found));
        o.push_back(flg(clipped));
        o.push_back(flg(boundary));
    }
    // the solver deletes its offset surfaces (they do not own their bases)
    delete fs;
    delete b0;
    delete b1;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_solver_p3_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                 int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    int32_t rc = SC_ERR_ARG;
    switch (op)
    {
    case OP_STEP:
        rc = op_step(rd, o);
        break;
    default:
        return SC_ERR_ARG;
    }
    if (rc != SC_OK)
        return rc;
    if (rd.at > n_in)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
