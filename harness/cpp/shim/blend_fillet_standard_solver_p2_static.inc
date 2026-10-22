// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp: the difftest shim of unit
// `blend.fillet_standard_solver_p2`, SmVariableRadiusFS::ReCalcFilletGeom. The op builds its
// fillet geometry with blend_fillet_geom_p1.cpp's Model (anonymous namespace there), so it
// lives in that translation unit; the rest of the unit's shim is
// blend_fillet_standard_solver_p2.cpp.
//
// Entry point sc_blend_fillet_standard_solver_p2_recalc(in, n_in, out, cap, out_len). Input
// (layout in crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p2.rs): the Model
// block; the geometry put into m_vFilletGeoms (-1: none); the intersection points of some
// vertex uses (`edge which  crv_pos[3] crv_deriv[3]  t  uv0[2] uv1[2]`); base surfaces A and B, the centre
// curve; offset distances, orientations; approx, angle and tangency tolerances; the linear
// law's start and end radius and the law orientation; `has_ref [ref curve]`.
//
// The solver is a real SmVariableRadiusFS (its surfaces constructor over A, B, the centre
// curve and an SmLinearFilletLaw); its offsets are then replaced by offsets of A and B at the
// given distances (allowing extension, as the constructor makes them) so the Rust side, which
// cannot evaluate the unported law, builds the same solver, and its first fillet geometry
// (the constructor's default SmFilletGeom(this), deleted) by the Model's.
// ReCalcFilletGeom(FALSE, NULL, ref) runs on it. Output: `-2` (refused: no fillet geometry, which upstream reads out of
// bounds), else `status  extend_before extend_after`, then for every edge of the Model and
// both its edge uses the vertex use's intersection point `t crv_pos[3] uv0[2] uv1[2]`.

#include <SmFilletExecutive.h>
#include <SmFilletStandardSolver.h>
#include <SmOffsetSurface.h>

namespace
{
const double P2R_REFUSE = -2.0;

struct P2VrPeek : SmVariableRadiusFS
{
    P2VrPeek(const SmContext& ctx, double tol, double ang, double tan, SmSurface* a, SmSurface* b, double o0,
             double o1, SmCurve* center, SmFilletLaw& law, SmBoolean law_orient)
        : SmVariableRadiusFS(ctx, tol, ang, tan, a, b, o0, o1, center, law, law_orient)
    {
    }
    void set_offsets(SmOffsetSurface* s0, SmOffsetSurface* s1)
    {
        delete m_pSurfaces[0];
        delete m_pSurfaces[1];
        m_pSurfaces[0] = s0;
        m_pSurfaces[1] = s1;
    }
    // m_vFilletGeoms[0] is the constructor's default geometry (owned by the solver): the
    // Model's replaces it
    void set_first_geom(SmFilletGeom* g)
    {
        delete m_vFilletGeoms[0];
        m_vFilletGeoms[0] = g;
    }
    // the Model owns its geometries: ~SmFilletSolver deletes m_vFilletGeoms
    void release_geoms() { m_vFilletGeoms.RemoveAll(); }
    bool extend_before() const { return m_bExtendBefore != FALSE; }
    bool extend_after() const { return m_bExtendAfter != FALSE; }
};

void p2r_put_tsect(std::vector<double>& o, SmFilletEdgeuse* eu)
{
    SmFilletVertexuse* vu = eu ? (SmFilletVertexuse*)eu->GetVertexuse() : NULL;
    if (vu == NULL)
    {
        for (int i = 0; i < 8; i++)
            o.push_back(-1.0);
        return;
    }
    SmTsectPnt& tp = vu->GetTsectPnt();
    o.push_back(tp.m_dCurveParameter);
    o.push_back(tp.CrvPos().x);
    o.push_back(tp.CrvPos().y);
    o.push_back(tp.CrvPos().z);
    o.push_back(tp.UVPos(0).x);
    o.push_back(tp.UVPos(0).y);
    o.push_back(tp.UVPos(1).x);
    o.push_back(tp.UVPos(1).y);
}

int32_t p2r_run(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Model M(ctx);
    if (!M.build(rd))
        return SC_ERR_ARG;
    int64_t g = rd.idx();
    int64_t nt = rd.idx();
    bool ok = true;
    for (int64_t i = 0; i < nt && !rd.bad && ok; ++i)
    {
        int64_t e = rd.idx(), which = rd.idx();
        SmPoint3d pos = rd.pt3();
        SmPoint3d deriv = rd.pt3();
        double t = rd.num();
        SmPoint2d uv0 = rd.uv(), uv1 = rd.uv();
        SmFilletEdgeuse* eu = M.edgeuse(e, which);
        SmFilletVertexuse* vu = eu ? (SmFilletVertexuse*)eu->GetVertexuse() : NULL;
        if (vu == NULL)
        {
            ok = false;
            break;
        }
        SmTsectPnt& tp = vu->GetTsectPnt();
        tp.CrvPos() = pos;
        tp.CrvDeriv() = SmVector3d(deriv.x, deriv.y, deriv.z);
        tp.m_dCurveParameter = t;
        tp.UVPos(0) = uv0;
        tp.UVPos(1) = uv1;
    }
    SmSurface* a = read_surface(rd, ctx);
    SmSurface* b = read_surface(rd, ctx);
    SmCurve* center = read_curve(rd, ctx);
    double d0 = rd.num(), d1 = rd.num(), o0 = rd.num(), o1 = rd.num();
    double tol = rd.num(), ang = rd.num(), tan = rd.num();
    double r0 = rd.num(), r1 = rd.num();
    bool law_orient = rd.flag();
    SmCurve* ref = NULL;
    if (rd.flag())
        ref = read_curve(rd, ctx);
    if (!ok || rd.bad || a == NULL || b == NULL || center == NULL)
    {
        delete a;
        delete b;
        delete center;
        delete ref;
        return SC_ERR_ARG;
    }
    SmFilletGeom* geom = M.geom(g);
    if (geom == NULL)
    {
        o.push_back(P2R_REFUSE);
    }
    else
    {
        SmLinearFilletLaw law(r0, r1);
        {
            P2VrPeek vr(ctx, tol, ang, tan, a, b, o0, o1, center, law, law_orient ? TRUE : FALSE);
            SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(d0 * o0, *a);
            SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(d1 * o1, *b);
            s0->SetAllowExtension(TRUE);
            s1->SetAllowExtension(TRUE);
            vr.set_offsets(s0, s1);
            vr.set_first_geom(geom);
            SmStatus st = vr.ReCalcFilletGeom(FALSE, NULL, ref);
            vr.release_geoms();
            o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
            o.push_back(vr.extend_before() ? 1.0 : 0.0);
            o.push_back(vr.extend_after() ? 1.0 : 0.0);
        }
        std::vector<SmFilletEdge*> es = M.edges();
        for (size_t e = 0; e < es.size(); ++e)
            for (int64_t which = 0; which < 2; ++which)
                p2r_put_tsect(o, Model::eu_of(es[e], which));
    }
    delete ref;
    delete center;
    delete a;
    delete b;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_standard_solver_p2_recalc(const double* in, int64_t n_in, double* out,
                                                            int64_t cap, int64_t* out_len)
{
    if (out == NULL || out_len == NULL || n_in < 0 || cap < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd(in, n_in);
    std::vector<double> o;
    int32_t rc = p2r_run(rd, o);
    if (rc != SC_OK)
        return rc;
    if (rd.at != n_in)
        return SC_ERR_ARG; // trailing input
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
