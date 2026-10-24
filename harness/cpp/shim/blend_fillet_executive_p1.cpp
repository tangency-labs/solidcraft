// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_executive_p1`. `sc_blend_fillet_executive_p1_law_eval`
// wraps upstream SmFilletLaw / SmLinearFilletLaw / SmBSplineFilletLaw
// (source/SMLib/src/SmFilletExecutive.cpp 5126-6132, source/SMLib/inc/SmFilletExecutive.h
// 660-899); `sc_blend_fillet_executive_p1_object_eval` runs scripts on SmFilletErrorInfo and
// SmFilletExecutive (cpp 30-292, 488-570, header 380-530); its layout is in
// crates/solidcore-ffi/src/units/blend_fillet_executive_p1.rs.
//
// Law input:
// Input (flat doubles; validated by crates/solidcore-ffi/src/units/blend_fillet_executive_p1.rs):
//   kind                 0 linear `r0 r1`; 1 B-spline `n p m cpts(x y z w)... knots...`; 2 base
//   has_map e0 e1        the edge map (ignored when has_map == 0 or kind == 2)
//   ncalls               then per call: `t i0 i1 reverse` (parameter, edge interval, orientation)
//   nk kinds[nk]         type codes for IsKindOf
// Output: `0 edge_min edge_max is_linear type_code`, per call `status [v0 v1 v2]`, then
// `has_ext [n p m cpts knots]` (GetExtendedCurve of a B-spline law), then one flag per kind.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmFilletCorner.h>
#include <SmFilletExecutive.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>

namespace
{
struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

// Reads `n p m cpts knots` into an SmBSplineCurve (SmBSplineCurve(gw_CURVE*, 3, FALSE)).
SmBSplineCurve* read_bcurve(Rd& rd, const SmContext& ctx)
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

void put_curve(std::vector<double>& o, SmBSplineCurve* c)
{
    gw_CURVE* cur = c ? c->GetOrCreateGwNurbPointer() : NULL;
    if (cur == NULL)
    {
        o.push_back(0.0);
        return;
    }
    o.push_back(1.0);
    o.push_back((double)cur->pol->n);
    o.push_back((double)cur->p);
    o.push_back((double)cur->knt->m);
    for (gw_INDEX i = 0; i <= cur->pol->n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= cur->knt->m; i++)
        o.push_back(cur->knt->U[i]);
}

} // namespace

SC_EXPORT int32_t sc_blend_fillet_executive_p1_law_eval(const double* in, int64_t n_in, double* out, int64_t cap,
                                                        int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};

    int64_t kind = rd.idx();
    SmFilletLaw* law = NULL;
    SmBSplineFilletLaw* bs = NULL;
    if (kind == 0)
    {
        double r0 = rd.num();
        double r1 = rd.num();
        bool has_map = rd.flag();
        double e0 = rd.num();
        double e1 = rd.num();
        SmExtent1d map(e0, e1);
        law = new (ctx) SmLinearFilletLaw(r0, r1, has_map ? &map : NULL);
    }
    else if (kind == 1)
    {
        SmBSplineCurve* lc = read_bcurve(rd, ctx);
        bool has_map = rd.flag();
        double e0 = rd.num();
        double e1 = rd.num();
        bs = new (ctx) SmBSplineFilletLaw(lc, has_map ? SmExtent1d(e0, e1) : SmExtent1d(0.0, 0.0));
        law = bs;
    }
    else
    {
        rd.flag();
        rd.num();
        rd.num();
        law = new (ctx) SmFilletLaw();
    }

    o.push_back(0.0);
    o.push_back(law->GetEdgeMap().GetMin());
    o.push_back(law->GetEdgeMap().GetMax());
    o.push_back(law->IsLinearLaw() ? 1.0 : 0.0);
    o.push_back((double)law->GetType());

    int64_t calls = rd.idx();
    for (int64_t i = 0; i < calls; i++)
    {
        double t = rd.num();
        double i0 = rd.num();
        double i1 = rd.num();
        bool reverse = rd.flag();
        SmExtent1d ivl(i0, i1);
        double v[3] = {0.0, 0.0, 0.0};
        SmStatus s = law->Evaluate(t, ivl, reverse ? TRUE : FALSE, v);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(v[0]);
            o.push_back(v[1]);
            o.push_back(v[2]);
        }
    }

    put_curve(o, bs ? bs->GetExtendedCurve() : NULL);

    int64_t nk = rd.idx();
    for (int64_t i = 0; i < nk; i++)
        o.push_back(law->IsKindOf((SM_TYPE)rd.idx()) ? 1.0 : 0.0);

    delete law;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// ---- error info and executive object scripts ---------------------------------------------

namespace
{
// Handle h > 0 stands for the (never dereferenced) pointer h << 6; 0 for NULL. The script
// calls only store and compare these pointers.
template <typename T> T* ptr(int64_t h)
{
    return h == 0 ? NULL : (T*)(uintptr_t)((uint64_t)h << 6);
}

// The real fillet edges a script made (call 20); output as `-2 - k`.
std::vector<const void*> g_made;

// `-1` for the executive itself, `-2 - k` for the k-th fillet edge the script made, else
// the handle.
double hnd(const void* p, const void* self)
{
    if (p != NULL && p == self)
        return -1.0;
    for (size_t k = 0; p != NULL && k < g_made.size(); k++)
        if (g_made[k] == p)
            return -2.0 - (double)k;
    return (double)((uintptr_t)p >> 6);
}

// `len c...`: len = -1 is NULL.
struct Str
{
    bool null;
    std::string s;
    const char* c() const { return null ? NULL : s.c_str(); }
};

Str read_str(Rd& rd)
{
    Str out{false, std::string()};
    int64_t len = rd.idx();
    if (len < 0)
    {
        out.null = true;
        return out;
    }
    for (int64_t i = 0; i < len; i++)
        out.s.push_back((char)rd.idx());
    return out;
}

void put_text(std::vector<double>& o, const char* text)
{
    std::string t(text);
    o.push_back((double)t.size());
    for (unsigned char ch : t)
        o.push_back((double)ch);
}

// The private m_eFilletStatus (no getter), through an explicit instantiation.
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

// One NoteError call of kind 0-5; false for another kind.
// With `fx`, calls 2, 3 and 5 go through the matching SmFilletExecutive::NoteFilletError.
bool note(Rd& rd, int64_t kind, SmFilletErrorInfo& e, SmFilletExecutive* fx, bool& fv_set, bool& loc_set)
{
    switch (kind)
    {
    case 0:
    {
        SmFilletErrorType code = (SmFilletErrorType)rd.idx();
        Str c = read_str(rd);
        e.NoteError(code, c.c());
        return true;
    }
    case 1:
    {
        const SmEdge* edge = ptr<const SmEdge>(rd.idx());
        SmFilStatus st = (SmFilStatus)rd.idx();
        SmFilletErrorType code = (SmFilletErrorType)rd.idx();
        Str c = read_str(rd);
        e.NoteError(edge, st, code, c.c());
        return true;
    }
    case 2:
    {
        SmFilletErrorType code = (SmFilletErrorType)rd.idx();
        SmFilStatus st = (SmFilStatus)rd.idx();
        const SmEdge* edge = ptr<const SmEdge>(rd.idx());
        const SmFace* face = ptr<const SmFace>(rd.idx());
        const SmFilletEdge* fe = ptr<const SmFilletEdge>(rd.idx());
        const SmSurface* surf = ptr<const SmSurface>(rd.idx());
        Str c = read_str(rd);
        if (fx)
            fx->NoteFilletError(code, st, edge, face, fe, surf, c.c());
        else
            e.NoteError(code, st, edge, face, fe, surf, c.c());
        return true;
    }
    case 3:
    {
        SmFilletErrorType code = (SmFilletErrorType)rd.idx();
        SmFilletCornerType ct = (SmFilletCornerType)rd.idx();
        SmFilStatus st = (SmFilStatus)rd.idx();
        Str name = read_str(rd);
        const SmVertex* v = ptr<const SmVertex>(rd.idx());
        const SmSurface* s1 = ptr<const SmSurface>(rd.idx());
        const SmSurface* s2 = ptr<const SmSurface>(rd.idx());
        const SmFilletVertex* fv1 = ptr<const SmFilletVertex>(rd.idx());
        const SmFilletVertex* fv2 = ptr<const SmFilletVertex>(rd.idx());
        Str c = read_str(rd);
        if (fx)
            fx->NoteFilletError(code, ct, st, (TCHAR*)name.c(), v, s1, s2, fv1, fv2, c.c());
        else
            e.NoteError(code, ct, st, (TCHAR*)name.c(), v, s1, s2, fv1, fv2, c.c());
        fv_set = true;
        return true;
    }
    case 4:
    {
        SmFilletErrorType code = (SmFilletErrorType)rd.idx();
        SmFilStatus st = (SmFilStatus)rd.idx();
        SmFilletEdgeType et = (SmFilletEdgeType)rd.idx();
        const SmFilletEdge* fe = ptr<const SmFilletEdge>(rd.idx());
        Str name = read_str(rd);
        Str c = read_str(rd);
        e.NoteError(code, st, et, fe, name.c(), c.c());
        return true;
    }
    case 5:
    {
        SmFilletErrorType code = (SmFilletErrorType)rd.idx();
        SmFilStatus st = (SmFilStatus)rd.idx();
        const SmEdge* edge = ptr<const SmEdge>(rd.idx());
        SmTArray<SM_FILLETSURF_TYPE*> surfs;
        SmTArray<SmBSplineCurve*> curves;
        SmTArray<SmFilletEdge*> edges;
        int64_t n = rd.idx();
        for (int64_t i = 0; i < n; i++)
            surfs.Add(ptr<SM_FILLETSURF_TYPE>(rd.idx()));
        n = rd.idx();
        for (int64_t i = 0; i < n; i++)
            curves.Add(ptr<SmBSplineCurve>(rd.idx()));
        n = rd.idx();
        for (int64_t i = 0; i < n; i++)
            edges.Add(ptr<SmFilletEdge>(rd.idx()));
        double x = rd.num();
        double y = rd.num();
        double z = rd.num();
        SmPoint3d at(x, y, z);
        Str c = read_str(rd);
        if (fx)
            fx->NoteFilletError(code, st, edge, surfs, curves, edges, at, c.c());
        else
            e.NoteError(code, st, edge, surfs, curves, edges, at, c.c());
        loc_set = true;
        return true;
    }
    default:
        return false;
    }
}

// `code status corner edge_type exec vertex edge side end has_fv [fv1 fv2] ns s* ne e* nc c*
// has_loc [x y z] comment name`.
void put_error(std::vector<double>& o, SmFilletErrorInfo& e, const void* self, bool fv_set, bool loc_set)
{
    o.push_back((double)e.GetErrorCode());
    o.push_back((double)(e.*get(StatusTag())));
    o.push_back((double)e.m_eCornerType);
    o.push_back((double)e.m_eEdgeType);
    o.push_back(hnd(e.m_pFilExec, self));
    o.push_back(hnd(e.m_pFilletedVertex, self));
    o.push_back(hnd(e.m_pFilletedEdge, self));
    o.push_back(hnd(e.m_pSideFace, self));
    o.push_back(hnd(e.m_pEndFace, self));
    o.push_back(fv_set ? 1.0 : 0.0);
    if (fv_set)
    {
        o.push_back(hnd(e.m_pFilletVertex1, self));
        o.push_back(hnd(e.m_pFilletVertex2, self));
    }
    o.push_back((double)e.m_pFilletSurfaces.GetSize());
    for (ULONG i = 0; i < e.m_pFilletSurfaces.GetSize(); i++)
        o.push_back(hnd(e.m_pFilletSurfaces[i], self));
    o.push_back((double)e.m_pFilletEdges.GetSize());
    for (ULONG i = 0; i < e.m_pFilletEdges.GetSize(); i++)
        o.push_back(hnd(e.m_pFilletEdges[i], self));
    o.push_back((double)e.m_pFilletCurves.GetSize());
    for (ULONG i = 0; i < e.m_pFilletCurves.GetSize(); i++)
        o.push_back(hnd(e.m_pFilletCurves[i], self));
    o.push_back(loc_set ? 1.0 : 0.0);
    if (loc_set)
    {
        o.push_back(e.m_vLocation.x);
        o.push_back(e.m_vLocation.y);
        o.push_back(e.m_vLocation.z);
    }
    put_text(o, e.m_cComment);
    put_text(o, e.m_cEntityName);
}

// Reads the protected members the constructor and the setters change.
struct Exec : public SmFilletExecutive
{
    Exec(const SmContext& ctx, SmBrep* target) : SmFilletExecutive(ctx, target) {}

    // `piecewise insertion global trimming classification target fillet n_corners n_solvers
    // n_topo topo* n_fsg fsg* handler ignore_tangency approx_tol ang_tol has_pseudo
    // has_attribute`, then the error info.
    void put(std::vector<double>& o, bool fv_set, bool loc_set)
    {
        o.push_back(m_bDoPiecewiseMerge ? 1.0 : 0.0);
        o.push_back(m_bDoTopologyInsertion ? 1.0 : 0.0);
        o.push_back(m_bDoGlobalMerge ? 1.0 : 0.0);
        o.push_back(m_bDoTrimming ? 1.0 : 0.0);
        o.push_back(m_bDoClassification ? 1.0 : 0.0);
        o.push_back(hnd(m_pTargetBrep, this));
        o.push_back(hnd(m_pFilletBrep, this));
        o.push_back((double)m_vFilletCorners.GetSize());
        o.push_back((double)m_vFilletSolvers.GetSize());
        o.push_back((double)m_vTopoEdges.GetSize());
        for (ULONG i = 0; i < m_vTopoEdges.GetSize(); i++)
            o.push_back(hnd(m_vTopoEdges[i], this));
        o.push_back((double)m_vFSGs.GetSize());
        for (ULONG i = 0; i < m_vFSGs.GetSize(); i++)
            o.push_back(hnd(m_vFSGs[i], this));
        o.push_back(hnd(m_pSelfIntersectionHandler, this));
        o.push_back(m_vTI.m_bIgnoreTangency ? 1.0 : 0.0);
        o.push_back(m_vTI.GetThisApproxTol3d());
        o.push_back(m_vTI.GetThisAngTolRad());
        o.push_back(m_pPseudoBrep != NULL ? 1.0 : 0.0);
        o.push_back(m_pAttributeE != NULL ? 1.0 : 0.0);
        put_error(o, m_sErrorInfo, this, fv_set, loc_set);
    }

    // The fake generator pointers must not reach the base destructor's delete.
    ~Exec() { m_vFSGs.ReSet(); }

    // Calls 16-22; false for another kind. Queries append to `q`. The lookups cast the
    // fake map values to SmFilletCorner* / SmFilletSolver* (single inheritance from
    // SmObject: the address is unchanged) and never dereference them.
    bool step(Rd& rd, int64_t kind, std::vector<double>& q)
    {
        switch (kind)
        {
        case 16:
        {
            SmVertex* v = ptr<SmVertex>(rd.idx());
            m_vVertexCornerMap.Insert(v, ptr<SmObject>(rd.idx()));
            return true;
        }
        case 17:
        {
            SmEdgeuse* eu = ptr<SmEdgeuse>(rd.idx());
            m_vEdgeuseSolverMap.Insert(eu, ptr<SmFilletSolver>(rd.idx()));
            return true;
        }
        case 18:
            q.push_back(hnd(GetFilletCornerOfVertex(ptr<SmVertex>(rd.idx())), this));
            return true;
        case 19:
            q.push_back(hnd(GetFilletSolverOfEdgeuse(ptr<SmEdgeuse>(rd.idx())), this));
            return true;
        case 20:
        {
            SmFilletErrorType code = (SmFilletErrorType)rd.idx();
            SmFilletEdgeType et = (SmFilletEdgeType)rd.idx();
            SmFilStatus st = (SmFilStatus)rd.idx();
            Str c = read_str(rd);
            SmFilletEdge* fe = new (m_pPseudoBrep) SmFilletEdge(m_pPseudoBrep);
            fe->SetFilletEdgeType(et);
            fe->SetStatus(st);
            g_made.push_back(fe);
            NoteFilletError(code, fe, c.c());
            return true;
        }
        case 21:
        {
            // a real corner (the base destructor deletes m_vFilletCorners)
            SmVertex* v = ptr<SmVertex>(rd.idx());
            SmFilletCorner* c =
                new (m_crContext) SmFilletConvexNxNCorner(m_crContext, NULL, NULL, NULL, this, 1e-6, 1e-6);
            m_vFilletCorners.Add(c);
            m_vVertexCornerMap.Insert(v, (SmObject*)c);
            g_made.push_back(c);
            return true;
        }
        case 22:
        {
            // the wrapper refuses a vertex mapped to a fake corner pointer
            SmVertex* v = ptr<SmVertex>(rd.idx());
            SmStatus s = SetBevelCorner(v);
            q.push_back(st(s));
            if (s == SM_SUCCESS)
                q.push_back(((SmFilletCorner*)m_vVertexCornerMap.At(v))->IsBevelCorner() ? 1.0 : 0.0);
            return true;
        }
        default:
            return false;
        }
    }
};

} // namespace

SC_EXPORT int32_t sc_blend_fillet_executive_p1_object_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                           int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    bool fv_set = false;
    bool loc_set = false;
    g_made.clear();
    if (op == 0)
    {
        int64_t exec = rd.idx();
        SmFilletErrorInfo e(ptr<SmFilletExecutive>(exec));
        int64_t calls = rd.idx();
        for (int64_t i = 0; i < calls; i++)
            if (!note(rd, rd.idx(), e, NULL, fv_set, loc_set))
                throw std::runtime_error("bad call kind");
        o.push_back(0.0);
        put_error(o, e, NULL, fv_set, loc_set);
    }
    else
    {
        Exec x(ctx, ptr<SmBrep>(rd.idx()));
        std::vector<double> q;
        int64_t calls = rd.idx();
        for (int64_t i = 0; i < calls; i++)
        {
            int64_t kind = rd.idx();
            if (note(rd, kind, *x.GetFilletErrorInfo(), &x, fv_set, loc_set) || x.step(rd, kind, q))
                continue;
            switch (kind)
            {
            case 10:
                x.SetDoGlobalMerge(rd.flag() ? TRUE : FALSE);
                break;
            case 11:
                x.SetDoTrimming(rd.flag() ? TRUE : FALSE);
                break;
            case 12:
                x.SetSelfIntersectionHandler(ptr<SmSelfIntersectionHandler>(rd.idx()));
                break;
            case 13:
                x.AddFilletSurfaceGenerator(ptr<SmFilletSurfaceGenerator>(rd.idx()));
                break;
            case 14:
                x.AddTopoEdges(ptr<SmFilletEdge>(rd.idx()));
                break;
            case 15:
                x.RemoveFromTopoEdges(ptr<SmFilletEdge>(rd.idx()));
                break;
            default:
                throw std::runtime_error("bad call kind");
            }
        }
        o.push_back(0.0);
        x.put(o, fv_set, loc_set);
        o.push_back((double)q.size());
        o.insert(o.end(), q.begin(), q.end());
    }
    g_made.clear();
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
