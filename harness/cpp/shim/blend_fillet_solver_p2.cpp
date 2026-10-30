// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_solver_p2`.
// Wraps source/SMLib/src/SmFilletSolver.cpp: PointOnPlaneSolve, RailEdgeuseIntersect,
// RailRailIntersect, CalcCylinderFilletGeom, CalcConeFilletGeom, CalcTorusFilletGeom,
// GetFilletCorner (index > 1) and RecordFilletError.
//
// One entry point, sc_blend_fillet_solver_p2_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/blend_fillet_solver_p2.rs, which
// validates every input. Surface, curve and B-rep blocks are those of unit topology.edgeuse_p1
// (sc_edgeuse_graph.h); entity indices are its traversal order.
//
// The solves call the solver's virtuals (GetJacobianSize, LoadInitialValues, LoadJacobian,
// SetupOffsetValues), pure or failing in the base class; the derived solvers of the library
// inherit them from units that are not ported. The shim's P2FS implements them as a small
// rolling-ball solver, mirrored record by record by the runner's `TestSolver`:
// - GetJacobianSize: 4 + extra.
// - SetupOffsetValues(t, d): r = radius + slope * t.m_dCurveParameter; each present offset
//   surface i gets offset r * orientation_i; *d = r.
// - LoadInitialValues: with rbDoSurf2Calcs, rpSurface2 = GetSurface(1 - rail),
//   rlSurface2Index = guess size, the second surface's base domain into the extents at that
//   index, two FALSE periodicities and the guess (guess[idx1] + gu, guess[idx1 + 1] + gv);
//   then (always) `extra` unknowns: extent [-100, 100], FALSE, guess 0.5 * i.
// - LoadJacobian: rpSurface2 = GetSurface(1 - rail), rlSurf2Offset = np; three rows
//   S_rail(x[o1], x[o1+1]) - S_other(x[o2], x[o2+1]) (offset surfaces,
//   Evaluate1stDerivatives(uv, TRUE, TRUE)) with their partials; np += 2; then per extra
//   unknown a row x[np] - (target + i) with partial 1 (np += 1); found when every row it
//   wrote is below GetConversionTol() * (1 + max |S_rail component|).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmAxis2Placement.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtentNd.h>
#include <SmFilletExecutive.h>
#include <SmFilletGeom.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmMatrix.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>

#include <cmath>
#include <string>
#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_solver_p2.rs).
enum : int32_t
{
    OP_PLANE = 0,
    OP_RAIL_EU = 1,
    OP_RAIL_RAIL = 2,
    OP_GEOM = 3,
    OP_MISC = 4,
};

struct P2FS : SmFilletSolver
{
    ULONG extra = 0;
    double radius = 0.0;
    double slope = 0.0;
    double target = 0.0;
    double gu = 0.0;
    double gv = 0.0;
    P2FS(const SmContext& ctx, double tol) : SmFilletSolver(ctx, tol, 0.1, 0.01)
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
    virtual ULONG GetJacobianSize() const { return 4 + extra; }
    virtual SmStatus SetupOffsetValues(const SmTsectPnt& t, double* d = NULL)
    {
        double r = radius + slope * t.m_dCurveParameter;
        for (ULONG i = 0; i < 2; i++)
            if (GetSurface(i) != NULL)
                GetSurface(i)->SetOffsetDistance(r * m_dOrientations[i]);
        if (d)
            *d = r;
        return SM_SUCCESS;
    }
    virtual SmStatus LoadInitialValues(ULONG& do2, ULONG rail, const SmSurface&, ULONG idx1, SmSurface*& s2,
                                       ULONG& idx2, SmExtentNd& ext, SmTArray<SmBoolean>& per,
                                       SmTArray<double>& guess)
    {
        if (do2)
        {
            SmOffsetSurface* other = GetSurface(1 - rail);
            s2 = other;
            idx2 = guess.GetSize();
            SmExtent2d dom = other->GetBaseSurface()->GetNaturalUVDomain();
            ext[idx2] = SmExtent1d(dom.GetMin().x, dom.GetMax().x);
            ext[idx2 + 1] = SmExtent1d(dom.GetMin().y, dom.GetMax().y);
            per.Add(FALSE);
            per.Add(FALSE);
            double u = guess[idx1] + gu;
            double v = guess[idx1 + 1] + gv;
            guess.Add(u);
            guess.Add(v);
        }
        for (ULONG i = 0; i < extra; i++)
        {
            ext[guess.GetSize()] = SmExtent1d(-100.0, 100.0);
            per.Add(FALSE);
            guess.Add(0.5 * (double)i);
        }
        return SM_SUCCESS;
    }
    virtual SmStatus LoadJacobian(ULONG& ne, ULONG& np, ULONG rail, SmSurface*&, ULONG& o1, SmSurface*& s2,
                                  ULONG& o2, const SmTArray<double>& x, SmTArray<double>& f, SmMatrix* j,
                                  SmBoolean& rbFound)
    {
        SmOffsetSurface* a = GetSurface(rail);
        SmOffsetSurface* b = GetSurface(1 - rail);
        s2 = b;
        o2 = np;
        SmPoint3d pa, pb;
        SmVector3d dua, dva, dub, dvb;
        SER(a->Evaluate1stDerivatives(SmPoint2d(x[o1], x[o1 + 1]), TRUE, TRUE, pa, dua, dva));
        SER(b->Evaluate1stDerivatives(SmPoint2d(x[o2], x[o2 + 1]), TRUE, TRUE, pb, dub, dvb));
        double scale = std::fabs(pa.x);
        if (std::fabs(pa.y) > scale)
            scale = std::fabs(pa.y);
        if (std::fabs(pa.z) > scale)
            scale = std::fabs(pa.z);
        double tol = GetConversionTol() * (1.0 + scale);
        SmBoolean found = TRUE;
        const double va[3] = {pa.x, pa.y, pa.z}, vb[3] = {pb.x, pb.y, pb.z};
        const double ua[3] = {dua.x, dua.y, dua.z}, wa[3] = {dva.x, dva.y, dva.z};
        const double ub[3] = {dub.x, dub.y, dub.z}, wb[3] = {dvb.x, dvb.y, dvb.z};
        for (ULONG c = 0; c < 3; c++)
        {
            f[ne + c] = va[c] - vb[c];
            if (std::fabs(f[ne + c]) >= tol)
                found = FALSE;
            if (j)
            {
                (*j)[ne + c][o1] = ua[c];
                (*j)[ne + c][o1 + 1] = wa[c];
                (*j)[ne + c][o2] = -ub[c];
                (*j)[ne + c][o2 + 1] = -wb[c];
            }
        }
        ne += 3;
        np += 2;
        for (ULONG i = 0; i < extra; i++)
        {
            f[ne] = x[np] - (target + (double)i);
            if (std::fabs(f[ne]) >= tol)
                found = FALSE;
            if (j)
                (*j)[ne][np] = 1.0;
            ne += 1;
            np += 1;
        }
        rbFound = found;
        return SM_SUCCESS;
    }
    void set_orientation(int i, double o) { m_dOrientations[i] = o; }
    void set_edgeuses(SmEdgeuse* e0, SmEdgeuse* e1)
    {
        m_pEdgeuses[0] = e0;
        m_pEdgeuses[1] = e1;
    }
    void set_extend(SmBoolean before, SmBoolean after)
    {
        m_bExtendBefore = before;
        m_bExtendAfter = after;
    }
};

// `tol extra radius slope target gu gv`
P2FS* read_solver(Rd& rd, const SmContext& ctx)
{
    double tol = rd.num();
    P2FS* fs = new (ctx) P2FS(ctx, tol);
    fs->extra = (ULONG)rd.idx();
    fs->radius = rd.num();
    fs->slope = rd.num();
    fs->target = rd.num();
    fs->gu = rd.num();
    fs->gv = rd.num();
    return fs;
}

// `base0 base1 has0 off0 has1 off1 o0 o1`: offset surface i of base i; the bases are
// returned for deletion after the solver (which deletes its offset surfaces).
struct Bases
{
    SmSurface* s[2] = {NULL, NULL};
    ~Bases()
    {
        delete s[0];
        delete s[1];
    }
};

void read_surfaces(Rd& rd, const SmContext& ctx, P2FS& fs, Bases& bases)
{
    bases.s[0] = read_surface(rd, ctx);
    bases.s[1] = read_surface(rd, ctx);
    for (ULONG i = 0; i < 2; i++)
    {
        bool has = rd.flag();
        double off = rd.num();
        if (has)
            fs.SetSurface(i, new (ctx) SmOffsetSurface(off, *bases.s[i]));
    }
    fs.set_orientation(0, rd.num());
    fs.set_orientation(1, rd.num());
}

void put_offsets(std::vector<double>& o, P2FS& fs)
{
    for (ULONG i = 0; i < 2; i++)
        o.push_back(fs.GetSurface(i) ? fs.GetSurface(i)->GetOffsetDistance() : 0.0);
}

void put_tsect(std::vector<double>& o, SmTsectPnt& t, ULONG n_user)
{
    for (ULONG i = 0; i < 2; i++)
    {
        o.push_back(t.UVPos(i).x);
        o.push_back(t.UVPos(i).y);
    }
    for (ULONG i = 0; i < n_user && i < SM_MAX_USER_DOUBLES; i++)
        o.push_back(t.m_adUserDoubles[i]);
}

// A curve's fingerprint through SmCurve: `1 t0 t1 P(t0) P(mid) P(t1)`, `-1` for none.
void put_crv(std::vector<double>& o, const SmCurve* c)
{
    if (c == NULL)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back(1.0);
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (double t : {ivl.GetMin(), ivl.GetMid(), ivl.GetMax()})
    {
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        c->EvaluatePoint(t, p);
        put_vec(o, p);
    }
}

// A surface's fingerprint: `1 u0 v0 u1 v1` and the points at the 3 x 3 grid of the domain
// (u outer), `-1` for none.
void put_srf(std::vector<double>& o, const SmSurface* s)
{
    if (s == NULL)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back(1.0);
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    for (double a : {0.0, 0.5, 1.0})
        for (double b : {0.0, 0.5, 1.0})
        {
            SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
            s->EvaluatePoint(d.Evaluate(a, b), p);
            put_vec(o, p);
        }
}

template <class T> T* at(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : NULL;
}

// `cfg surfs orig[3] normal[3] dom1[4] dom2[4] uv1[2] uv2[2]` -> `status off0 off1`, on
// success `found` and, when found, the tsect UV points and `extra` user doubles.
void run_plane(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Bases bases;
    P2FS* fs = read_solver(rd, ctx);
    read_surfaces(rd, ctx, *fs, bases);
    SmPoint3d orig = rd.vec();
    SmVector3d normal = rd.vec();
    SmPoint2d a1 = rd.uv(), b1 = rd.uv(), a2 = rd.uv(), b2 = rd.uv();
    SmVector2d uv1 = rd.uv(), uv2 = rd.uv();
    SmTsectPnt t;
    SmBoolean found = FALSE;
    SmStatus s = fs->PointOnPlaneSolve(orig, normal, SmExtent2d(a1, b1), SmExtent2d(a2, b2), uv1, uv2, found, t);
    o.push_back(st(s));
    put_offsets(o, *fs);
    if (s == SM_SUCCESS)
    {
        o.push_back(flg(found));
        if (found)
            put_tsect(o, t, fs->extra);
    }
    delete fs;
}

// `cfg surfs block side_eu frac rail ptype` -> `0 status off0 off1`, on success `found`
// and, when found, `param tsect curve_param point_type` (the guess is the side edge's
// interval at `frac`).
void run_rail_eu(Rd& rd, const char* path, const SmContext& ctx, std::vector<double>& o)
{
    Bases bases;
    P2FS* fs = read_solver(rd, ctx);
    read_surfaces(rd, ctx, *fs, bases);
    SmStatus bst = SM_SUCCESS;
    SmBrep* brep = build(rd, path, bst);
    if (brep == NULL)
    {
        o.push_back(-1.0);
        o.push_back((double)bst);
        delete fs;
        return;
    }
    Graph g;
    walk(brep, g);
    o.push_back(0.0);
    SmEdgeuse* side = at(g.edgeuses, rd.idx());
    double frac = rd.num();
    ULONG rail = (ULONG)rd.idx();
    SmTsectPnt t;
    t.m_ePointType = (SmIntersectionPointType)rd.idx();
    SmExtent1d ivl = side->GetEdge()->GetInterval();
    double guess = ivl.GetMin() + (ivl.GetMax() - ivl.GetMin()) * frac;
    SmBoolean found = FALSE;
    double param = -7.0;
    SmStatus s = fs->RailEdgeuseIntersect(rail, side, guess, found, t, param);
    o.push_back(st(s));
    put_offsets(o, *fs);
    if (s == SM_SUCCESS)
        o.push_back(flg(found));
    if (s == SM_SUCCESS && found)
    {
        o.push_back(param);
        put_tsect(o, t, fs->extra);
        o.push_back(t.m_dCurveParameter);
        o.push_back((double)t.m_ePointType);
    }
    delete fs;
    delete brep;
}

// `cfgA surfsA cfgB surfsB uv[2] rail other` -> `status offA0 offA1 offB0 offB1`, on
// success `found` and, when found, both tsect points (`extra` of A, `extra` of B user
// doubles).
void run_rail_rail(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Bases ba, bb;
    P2FS* fa = read_solver(rd, ctx);
    read_surfaces(rd, ctx, *fa, ba);
    P2FS* fb = read_solver(rd, ctx);
    read_surfaces(rd, ctx, *fb, bb);
    SmPoint2d uv = rd.uv();
    ULONG rail = (ULONG)rd.idx();
    ULONG other = (ULONG)rd.idx();
    SmTsectPnt ta, tb;
    SmBoolean found = FALSE;
    SmStatus s = fa->RailRailIntersect(uv, rail, fb, other, found, ta, tb);
    o.push_back(st(s));
    put_offsets(o, *fa);
    put_offsets(o, *fb);
    if (s == SM_SUCCESS)
        o.push_back(flg(found));
    if (s == SM_SUCCESS && found)
    {
        put_tsect(o, ta, fa->extra);
        put_tsect(o, tb, fb->extra);
    }
    delete fa;
    delete fb;
}

// `which cfg surfs fsg before after factor block eu0 eu1 n n x [uv0[2] uv1[2] cparam]` ->
// `0 status` and, on success, `surface rail0 rail1 [ivl0 ivl1] center` (the rail edge
// intervals for the cone and the torus only). `which`: 0 cylinder, 1 cone, 2 torus. `fsg`:
// 0 SmLinearCrossSectionFSG, 1 SmCircularCrossSectionFSG. The fillet geometry is a fresh
// SmFilletGeom of the solver with two rails made by MakeRailEdge (fillet vertices of
// unknown type, unprocessed) in a fresh SmFilletBrep.
void run_geom(Rd& rd, const char* path, const SmContext& ctx, std::vector<double>& o)
{
    int64_t which = rd.idx();
    Bases bases;
    P2FS* fs = read_solver(rd, ctx);
    read_surfaces(rd, ctx, *fs, bases);
    int64_t fsg_kind = rd.idx();
    SmBoolean before = rd.bflag();
    SmBoolean after = rd.bflag();
    fs->SetExtensionFactor(rd.num());
    fs->set_extend(before, after);
    SmFilletSurfaceGenerator* fsg = NULL;
    if (fsg_kind == 0)
        fsg = new (ctx) SmLinearCrossSectionFSG();
    else
        fsg = new (ctx) SmCircularCrossSectionFSG();
    fs->SetFilletSurfaceGenerator(fsg);
    // rails: 0, or 1 and the four rail end points (typed RAIL_X_EDGEUSE, processed)
    int64_t rail_mode = rd.idx();
    SmPoint3d rail_pts[4];
    if (rail_mode == 1)
        for (int i = 0; i < 4; i++)
            rail_pts[i] = rd.vec();
    // the filleted edge: a B-rep block, an SmLine segment or an SmCircle arc
    int64_t edge_kind = rd.idx();
    SmStatus bst = SM_SUCCESS;
    SmBrep* brep = NULL;
    if (edge_kind == 0)
        brep = build(rd, path, bst);
    else
    {
        brep = new (ctx) SmBrep();
        SmBSplineCurve* curve = NULL;
        if (edge_kind == 1)
        {
            SmPoint3d a = rd.vec();
            SmPoint3d b = rd.vec();
            SmLine* line = NULL;
            bst = SmLine::CreateLineSegment(ctx, 3, a, b, line);
            curve = line;
        }
        else
        {
            double radius = rd.num();
            double span = rd.num();
            SmAxis2Placement frame;
            SmExtent1d ivl(0.0, span);
            SmCircle* circle = NULL;
            bst = SmCircle::CreateCanonical(ctx, frame, radius, circle, &ivl);
            curve = circle;
        }
        if (bst == SM_SUCCESS)
        {
            SmEdge* edge = NULL;
            bst = brep->CreateWireEdgeFromCurve(curve, curve->GetNaturalInterval(), edge);
        }
        if (bst != SM_SUCCESS)
        {
            delete brep;
            brep = NULL;
        }
    }
    if (brep == NULL)
    {
        o.push_back(-1.0);
        o.push_back((double)bst);
        fs->SetFilletSurfaceGenerator(NULL);
        delete fsg;
        delete fs;
        return;
    }
    Graph g;
    walk(brep, g);
    o.push_back(0.0);
    SmEdgeuse* eu0 = at(g.edgeuses, rd.idx());
    SmEdgeuse* eu1 = at(g.edgeuses, rd.idx());
    fs->set_edgeuses(eu0, eu1);
    int64_t n = rd.idx();
    std::vector<SmTsectPnt*> pts;
    SmTArray<SmTsectPnt*> arr;
    for (int64_t i = 0; i < n; i++)
    {
        SmTsectPnt* t = new SmTsectPnt();
        t->UVPos(0) = rd.uv();
        t->UVPos(1) = rd.uv();
        t->m_dCurveParameter = rd.num();
        pts.push_back(t);
        arr.Add(t);
    }
    SmFilletBrep* pb = new (ctx) SmFilletBrep();
    SmFilletGeom* fg = new SmFilletGeom(fs, SM_FG_DEFAULT);
    SmFilletEdge* rails[2] = {NULL, NULL};
    fg->MakeRailEdge(pb, 0, rails[0]);
    fg->MakeRailEdge(pb, 1, rails[1]);
    if (rail_mode == 1)
        for (int jj = 0; jj < 2; jj++)
        {
            SmFilletVertex* v0 = (SmFilletVertex*)rails[jj]->GetVertex();
            SmFilletVertex* v1 = (SmFilletVertex*)rails[jj]->GetOtherVertex(v0);
            SmFilletVertex* vs[2] = {v0, v1};
            for (int e = 0; e < 2; e++)
            {
                vs[e]->SetFilletVertexType(SM_FV_RAIL_X_EDGEUSE);
                vs[e]->SetStatus(SM_FIL_PROCESSED);
                vs[e]->SetPoint(rail_pts[2 * jj + e]);
            }
        }
    SmStatus s = SM_SUCCESS;
    switch (which)
    {
    case 0: s = fs->CalcCylinderFilletGeom(arr, fg); break;
    case 1: s = fs->CalcConeFilletGeom(arr, fg); break;
    default: s = fs->CalcTorusFilletGeom(arr, fg); break;
    }
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        put_srf(o, fg->GetFilletSurface());
        put_crv(o, fg->GetRail(0)->GetCurve());
        put_crv(o, fg->GetRail(1)->GetCurve());
        if (which != 0)
            for (ULONG i = 0; i < 2; i++)
            {
                SmExtent1d ivl = fg->GetRail(i)->GetInterval();
                o.push_back(ivl.GetMin());
                o.push_back(ivl.GetMax());
            }
        put_crv(o, fg->GetCenterLineCurve());
    }
    SmSurface* fillet_surface = fg->GetFilletSurface();
    delete pb; // the rails (and their edge and vertex uses) before the vertices
    delete fg;
    delete fillet_surface;
    for (SmTsectPnt* t : pts)
        delete t;
    fs->SetFilletSurfaceGenerator(NULL);
    delete fsg;
    delete fs;
    delete brep;
}

// The private SmFilletErrorInfo::m_eFilletStatus (no getter), via explicit instantiation.
template <typename Tag, typename Tag::type M> struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};
struct StatusTag
{
    typedef SmFilStatus SmFilletErrorInfo::*type;
    friend type get(StatusTag);
};
template struct Rob<StatusTag, &SmFilletErrorInfo::m_eFilletStatus>;

void put_text(std::vector<double>& o, const char* text)
{
    std::string t(text);
    o.push_back((double)t.size());
    for (unsigned char ch : t)
        o.push_back((double)ch);
}

// `0 cfg index` -> GetFilletCorner(index > 1): `null jacobian_size`;
// `1 cfg block eu0` -> RecordFilletError into a live SmFilletExecutive's error info.
void run_misc(Rd& rd, const char* path, const SmContext& ctx, std::vector<double>& o)
{
    int64_t which = rd.idx();
    P2FS* fs = read_solver(rd, ctx);
    if (which == 0)
    {
        ULONG index = (ULONG)rd.idx();
        o.push_back(flg(fs->GetFilletCorner(index) == NULL));
        o.push_back((double)fs->GetJacobianSize());
        delete fs;
        return;
    }
    SmStatus bst = SM_SUCCESS;
    SmBrep* brep = build(rd, path, bst);
    if (brep == NULL)
    {
        o.push_back(-1.0);
        o.push_back((double)bst);
        delete fs;
        return;
    }
    Graph g;
    walk(brep, g);
    o.push_back(0.0);
    int64_t eu0_idx = rd.idx();
    SmEdgeuse* eu0 = at(g.edgeuses, eu0_idx);
    fs->set_edgeuses(eu0, NULL);
    fs->SetStatus((eu0_idx % 2 == 0) ? SM_FS_SURF_TRACING_FAILURE : SM_FIL_UNPROCESSED);
    SmFilletErrorType err_code = (eu0_idx % 3 == 0) ? SM_FILERR_EDGE_PROBLEM : SM_FILERR_BAD_INT;

    SmPlane* fillet_surface = NULL;
    {
        SmFilletExecutive exec(ctx, brep);
        SmFilletBrep* pb = exec.GetPseudoBrep();
        SmFilletGeom* fg = fs->GetFirstFilletGeom();
        SmFilletEdge* rails[2] = {NULL, NULL};
        fg->MakeRailEdge(pb, 0, rails[0]);
        fg->MakeRailEdge(pb, 1, rails[1]);

        SmTsectPnt t;
        SmTArray<SmTsectPnt*> arr;
        if (fs->target < -1.5)
        {
            SmBSplineCurve* r0 = NULL;
            SmBSplineCurve::CreateLineSegment(ctx, 3, SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 0.0, 0.0), r0);
            rails[0]->SetCurve(r0, TRUE);
            r0->SetOwner(rails[0]); // as upstream's rail producers do after SetCurve
            t.CrvPos() = SmPoint3d(fs->target, fs->radius, fs->GetThisApproxTol3d());
            arr.Add(&t);
        }
        else if (fs->target < 0.0)
        {
            SmBSplineCurve* r1 = NULL;
            SmBSplineCurve::CreateLineSegment(ctx, 3, SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(0.0, 1.0, 0.0), r1);
            rails[1]->SetCurve(r1, TRUE);
            r1->SetOwner(rails[1]);
            fillet_surface = new (ctx) SmPlane(SmPoint3d(0.0, 0.0, 0.0), SmVector3d(0.0, 0.0, 1.0));
            fillet_surface->MakeNurb();
            fg->SetFilletSurface(fillet_surface);
            SmBSplineCurve* cl = NULL;
            SmBSplineCurve::CreateLineSegment(ctx, 3, SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(2.0, 0.0, 0.0), cl);
            fg->SetCenterLineCurve(cl);
            t.CrvPos() = SmPoint3d(fs->target, -fs->radius, fs->GetThisApproxTol3d());
            arr.Add(&t);
        }
        else if (fs->target < 1.5)
        {
            SmBSplineCurve* r0 = NULL;
            SmBSplineCurve* r1 = NULL;
            SmBSplineCurve::CreateLineSegment(ctx, 3, SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 0.0, 0.0), r0);
            SmBSplineCurve::CreateLineSegment(ctx, 3, SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(0.0, 1.0, 0.0), r1);
            rails[0]->SetCurve(r0, TRUE);
            r0->SetOwner(rails[0]);
            rails[1]->SetCurve(r1, TRUE);
            r1->SetOwner(rails[1]);
            arr.Add(&t); // CrvPos left uninitialized
        }

        fs->SetFilletExecutive(&exec);
        fs->RecordFilletError(err_code, arr, _T("difftest"));

        SmFilletErrorInfo* ei = exec.GetFilletErrorInfo();
        o.push_back((double)ei->GetErrorCode());
        o.push_back((double)(*ei.*get(StatusTag())));
        o.push_back((double)ei->m_eEdgeType);
        o.push_back(flg(ei->m_pFilletedEdge == eu0->GetEdge()));
        o.push_back((double)ei->m_pFilletSurfaces.GetSize());
        o.push_back(flg(ei->m_pFilletSurfaces.GetSize() == 1 && ei->m_pFilletSurfaces[0] == fg->GetFilletSurface()));
        o.push_back((double)ei->m_pFilletCurves.GetSize());
        o.push_back(flg(ei->m_pFilletCurves.GetSize() == 1 && ei->m_pFilletCurves[0] == fg->GetCenterLineCurve()));
        o.push_back((double)ei->m_pFilletEdges.GetSize());
        o.push_back(flg(ei->m_pFilletEdges.GetSize() == 1 && ei->m_pFilletEdges[0] == rails[0]));
        o.push_back(flg(ei->m_vLocation.IsInitialized()));
        if (ei->m_vLocation.IsInitialized())
        {
            o.push_back(ei->m_vLocation.x);
            o.push_back(ei->m_vLocation.y);
            o.push_back(ei->m_vLocation.z);
        }
        put_text(o, ei->m_cComment);

        fs->SetFilletExecutive(NULL);
    }
    delete fillet_surface;
    delete fs;
    delete brep;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_solver_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                                 double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    const SmContext& ctx = the_context();
    Rd rd{in, 0};
    std::vector<double> o;
    switch (op)
    {
    case OP_PLANE: run_plane(rd, ctx, o); break;
    case OP_RAIL_EU: run_rail_eu(rd, path, ctx, o); break;
    case OP_RAIL_RAIL: run_rail_rail(rd, ctx, o); break;
    case OP_GEOM: run_geom(rd, path, ctx, o); break;
    case OP_MISC: run_misc(rd, path, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    (void)n_in;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
