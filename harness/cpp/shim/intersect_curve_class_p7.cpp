// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.curve_class_p7`: the part-7 members of SmCurveClassification
// (GetPointClassByIndex, GetPointClassMateByIndex, CollapseInterval, GetPointClassifiesConnectedTo,
// FindVertexObjects, FindEdgeObjects, IsCrossedByEdge, IsCrossingFace, HasNearMisses,
// AreCoincidentMaybe, ClassifyFaceInterval, TestForPartialCoincidentEdges) and
// sm_SetDoFilletingGeometry (global in source/SMLib/src/SmCurveClass.cpp).
//
// One entry point, sc_intersect_curve_class_p7_run(in, n_in, out, cap, out_len): a pool of objects
// (the format of unit intersect.curve_class_p1, whose object helpers are copied below), two
// classifications cc[0], cc[1] built from the script, then ops on them. The layout is documented in
// crates/solidcore-ffi/src/units/intersect_curve_class_p7.rs, which validates the static shape; the
// run-time checks below reject (SC_ERR_ARG) what is undefined upstream (indices outside the intervals,
// NULL curves that are dereferenced, edge curves that are not B-splines where SM_CAST_PTR is
// dereferenced) and the topology walks the bare stand-in topology cannot answer (face queries,
// connection tests across kinds, the edge-curve replacement that deletes a pool curve). Objects live
// for the whole process (never deleted: topology destructors notify owners).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCacheMgr.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCrvOnSurf.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmFace.h>
#include <SmGeomUtility.h>
#include <SmGraphicsExtern.h>
#include <SmGraphicsOutput.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTopologyIntersector.h>
#include <SmTrimSrfCache.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <map>
#include <vector>

// global in the library (SmCurveClass.cpp), not declared in a header
SmStatus sm_SetDoFilletingGeometry(ULONG lTgtIndx, SmCurveClassification& rThisCurveClass, ULONG lTodoList);

namespace
{
enum : int64_t
{
    K_CURVE = 0,
    K_PLANE = 1,
    K_BSURF = 2,
    K_POINT = 3,
    K_VERTEX = 4,
    K_EDGE = 5,
    K_FACE = 6,
    K_LOOP = 7,
    K_OTHER = 8,
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool bad = false;
    double num()
    {
        if (at >= n)
        {
            bad = true;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    bool flag() { return num() != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

struct ScVertex : public SmVertex
{
    ScVertex() : SmVertex() {}
};

struct ScEdge : public SmEdge
{
    explicit ScEdge(SmCurve* c) : SmEdge() { m_pCurve = c; }
};

struct ScFace : public SmFace
{
    explicit ScFace(SmSurface* s) : SmFace() { m_pSurface = s; }
};

// SmObject stand-in of one class; answers GetType and IsKindOf like that class.
class ScStandIn : public SmObject
{
public:
    explicit ScStandIn(SM_TYPE t) : m_type(t) {}
    SM_TYPE GetType() const override { return m_type; }
    SmBoolean IsKindOf(SM_TYPE t) const override { return t == m_type || SmObject::IsKindOf(t); }

private:
    SM_TYPE m_type;
};

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

// An object carrying the context, for the topology `operator new(size_t, SmObject*)`.
SmObject* anchor()
{
    static SmLine* p = new (&the_context())
        SmLine(SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 0.0, 0.0), 3, &the_context());
    return p;
}

gw_CURVE* read_nurb(Rd& rd)
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
    return cur;
}

// `n m p q r s cpts(x y z w, v fastest) u v`.
gw_SURFACE* read_nurb_surface(Rd& rd)
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
    return sur;
}

// A curve block `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w) knots`;
// 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`; 3 / 4: SmEllipse /
// SmCircle `C X Y d0 d1 rx [ry] io`).
SmCurve* read_curve(Rd& rd, const SmContext& ctx, bool& is_bspline, ULONG& dim)
{
    int64_t kind = rd.idx();
    dim = (ULONG)rd.idx();
    is_bspline = kind == 0;
    if (kind == 3 || kind == 4)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = kind == 3 ? rd.num() : rx;
        bool io = rd.flag();
        if (kind == 3)
            return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL, io ? TRUE : FALSE);
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), rx, dim, &ctx, NULL, io ? TRUE : FALSE);
    }
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 2)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

struct Pool
{
    std::vector<int64_t> kinds;
    std::vector<void*> ptrs; // SmObject* or SmPoint3d*
    std::vector<bool> bspline;
    std::vector<ULONG> dims;
    std::map<const void*, int64_t> ids;

    double id(const void* p) const
    {
        if (p == NULL)
            return -1.0;
        auto it = ids.find(p);
        return it == ids.end() ? -2.0 : (double)it->second;
    }
    bool valid(int64_t i) const { return i >= 0 && i < (int64_t)ptrs.size(); }
    // pool object i (NULL for -1); `ok` false for an index out of range or of the wrong kind
    void* get(int64_t i, std::initializer_list<int64_t> want, bool& ok) const
    {
        if (i == -1)
            return NULL;
        if (!valid(i))
        {
            ok = false;
            return NULL;
        }
        bool match = want.size() == 0;
        for (int64_t w : want)
            match = match || kinds[(size_t)i] == w;
        if (!match)
            ok = false;
        return ptrs[(size_t)i];
    }
    int64_t kind_of(const void* p) const
    {
        auto it = ids.find(p);
        return it == ids.end() ? -1 : kinds[(size_t)it->second];
    }
};

void put_pc(std::vector<double>& o, const SmPointClassification& pc, const Pool& pool)
{
    o.push_back((double)pc.GetPointClass());
    o.push_back(pool.id(pc.GetObject()));
    o.push_back(pool.id(pc.GetFace()));
    o.push_back(pc.GetDeviation());
    o.push_back(pc.GetTParam());
    SmPoint2d uv = pc.GetUVParam();
    o.push_back(uv.x);
    o.push_back(uv.y);
    put_vec(o, pc.GetUVWParam());
    o.push_back(flg(pc.AreParametersSet()));
    o.push_back(pc.GetPreSnapParam());
    o.push_back(flg(pc.GetSSSPointFlag()));
    put_vec(o, pc.GetSSSPoint());
    o.push_back(SmTol::GetSrcZoneTol3d(&pc));
}

void put_ivl(std::vector<double>& o, const SmCurveInterval& ivl, const Pool& pool)
{
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    put_pc(o, ivl.m_vStart, pool);
    put_pc(o, ivl.m_vMid, pool);
    put_pc(o, ivl.m_vEnd, pool);
}

void put_curve(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    const SmBSplineCurve* b = c->IsKindOf(SmBSplineCurve_TYPE) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(cur ? (double)cur->pol->n : -1.0);
    o.push_back(cur ? (double)cur->knt->m : -1.0);
    for (int i = 0; i <= 6; i++)
    {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(ivl.Evaluate(i / 6.0), p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
    }
}


// the object kind a classification type needs; -1 for none (UNKNOWN, NOTHING), -2 invalid
int64_t kind_of_class(int64_t c)
{
    switch (c)
    {
    case 100:
    case 110: return -1;
    case 102: return K_FACE;
    case 103: return K_EDGE;
    case 104: return K_VERTEX;
    case 105: return K_POINT;
    case 106: return K_CURVE;
    case 107: return K_BSURF; // or K_PLANE
    default: return -2;
    }
}

SmPointClassification* pick(SmCurveInterval& ivl, int64_t w)
{
    return w == 0 ? &ivl.m_vStart : w == 1 ? &ivl.m_vMid : &ivl.m_vEnd;
}

enum : int64_t
{
    OP_PC_BY_INDEX = 0,
    OP_MATE = 1,
    OP_COLLAPSE = 2,
    OP_TOUCHES = 3,
    OP_FIND_VERTEX = 4,
    OP_FIND_EDGE = 5,
    OP_CROSSED_BY_EDGE = 6,
    OP_CROSSING_FACE = 7,
    OP_NEAR_MISSES = 8,
    OP_COIN_MAYBE = 9,
    OP_CLASSIFY_FACE = 10,
    OP_PARTIAL = 11,
    OP_FILLET = 12,
};

const int64_t MAX_IVLS = 16;

// the kind of the object of a point classification (-1 none / outside the pool)
int64_t pc_kind(const SmPointClassification& pc, const Pool& pool)
{
    return pc.GetObject() == NULL ? -1 : pool.kind_of(pc.GetObject());
}

void put_state(std::vector<double>& o, const SmCurveClassification& cc, const Pool& pool)
{
    o.push_back((double)cc.GetSize());
    for (ULONG i = 0; i < cc.GetSize(); i++)
        put_ivl(o, cc.GetAt(i), pool);
}

// One classification: `curve s1 uv1 ignore tol fillet n (a b (class obj t gap)*3)*n`.
bool read_cc(Rd& rd, const Pool& pool, SmCurveClassification& cc, SmCurveClassification& other)
{
    bool ok = true;
    SmCurve* c = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
    SmSurface* s1 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
    SmCurve* uv1 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
    bool ignore = rd.flag();
    double tol = rd.num();
    int64_t fillet = rd.idx();
    int64_t n = rd.idx();
    if (!ok || rd.bad || fillet < -1 || fillet > 1 || n < 0 || n > MAX_IVLS)
        return false;
    cc.SetCurve(c, FALSE);
    cc.SetSurface1(s1);
    cc.SetUVCurve1(uv1);
    cc.SetIgnoreTangency(ignore ? TRUE : FALSE);
    SmTol::SetZoneTol3d(&cc, SmZoneTol3d(tol));
    if (fillet >= 0)
        cc.SetFilletData(&other, fillet == 1 ? TRUE : FALSE);
    cc.SetSize((ULONG)n);
    for (int64_t i = 0; i < n; i++)
    {
        SmCurveInterval& ivl = cc.GetAt((ULONG)i);
        double a = rd.num();
        double b = rd.num();
        ivl.SetInterval(SmExtent1d(a, b));
        for (int64_t w = 0; w < 3; w++)
        {
            SmPointClassification& pc = *pick(ivl, w);
            int64_t cls = rd.idx();
            int64_t obj = rd.idx();
            double t = rd.num();
            double g = rd.num();
            int64_t want = kind_of_class(cls);
            if (want == -2)
                return false;
            void* po = pool.get(obj, {}, ok);
            if (!ok)
                return false;
            if (obj != -1)
            {
                int64_t k = pool.kinds[(size_t)obj];
                if (!(k == want || (want == K_BSURF && k == K_PLANE)))
                    return false;
            }
            pc.SetClassObject((SmPointClassificationType)cls, (SmObject*)po);
            pc.SetTParam(t);
            pc.SetGap3d(g);
        }
    }
    return !rd.bad;
}

bool is_ve(const SmPointClassification& pc)
{
    return pc.GetPointClass() == SM_PC_VERTEX || pc.GetPointClass() == SM_PC_EDGE;
}

// Whether a coincidence block of TestForPartialCoincidentEdges over these interiors could
// reach `SM_CAST_PTR(SmBSplineCurve, pEdge->GetCurve())` with a curve that is not a B-spline
// (a NULL dereference upstream).
bool bad_edge_curve(const SmPointClassification& mid, const SmPointClassification& omid)
{
    if (mid.GetPointClass() != SM_PC_EDGE || omid.GetPointClass() != SM_PC_EDGE)
        return false;
    const SmEdge* e = (const SmEdge*)mid.GetObject();
    const SmEdge* oe = (const SmEdge*)omid.GetObject();
    if (e == NULL || oe == NULL || e->GetCurve() == NULL || oe->GetCurve() == NULL)
        return true;
    return !e->GetCurve()->IsKindOf(SmBSplineCurve_TYPE);
}

int32_t run(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Pool pool;

    int64_t n_obj = rd.idx();
    for (int64_t i = 0; i < n_obj && !rd.bad; i++)
    {
        int64_t kind = rd.idx();
        void* p = NULL;
        bool bs = false;
        ULONG dim = 0;
        switch (kind)
        {
        case K_CURVE: p = read_curve(rd, ctx, bs, dim); break;
        case K_PLANE:
        {
            SmPoint3d org = rd.vec();
            SmVector3d nrm = rd.vec();
            p = new (ctx) SmPlane(org, nrm, &ctx);
            break;
        }
        case K_BSURF:
        {
            bool oob = rd.flag();
            SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb_surface(rd), FALSE, NULL);
            s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
            p = s;
            break;
        }
        case K_POINT: p = new SmPoint3d(rd.vec()); break;
        case K_VERTEX: p = new (anchor()) ScVertex(); break;
        case K_EDGE:
        {
            int64_t c = rd.idx();
            bool ok = true;
            SmCurve* co = (SmCurve*)pool.get(c, {K_CURVE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            p = new (anchor()) ScEdge(co);
            break;
        }
        case K_FACE:
        {
            int64_t s = rd.idx();
            bool ok = true;
            SmSurface* so = (SmSurface*)pool.get(s, {K_PLANE, K_BSURF}, ok);
            if (!ok)
                return SC_ERR_ARG;
            p = new (anchor()) ScFace(so);
            break;
        }
        case K_LOOP: p = new (&ctx) ScStandIn(SmLoop_TYPE); break;
        case K_OTHER: p = new (&ctx) ScStandIn(SmObject_TYPE + 7); break;
        default: return SC_ERR_ARG;
        }
        pool.ids[p] = (int64_t)pool.ptrs.size();
        pool.kinds.push_back(kind);
        pool.ptrs.push_back(p);
        pool.bspline.push_back(bs);
        pool.dims.push_back(dim);
    }
    if (rd.bad)
        return SC_ERR_ARG;

    SmCurveClassification ccs[2];
    for (int t = 0; t < 2; t++)
        if (!read_cc(rd, pool, ccs[t], ccs[1 - t]))
            return SC_ERR_ARG;

    int64_t n_ops = rd.idx();
    for (int64_t i = 0; i < n_ops && !rd.bad; i++)
    {
        int64_t op = rd.idx();
        int64_t t = rd.idx();
        if (t < 0 || t > 1)
            return SC_ERR_ARG;
        SmCurveClassification& cc = ccs[t];
        int64_t size = (int64_t)cc.GetSize();
        bool ok = true;
        o.push_back((double)op);
        switch (op)
        {
        case OP_PC_BY_INDEX:
        {
            int64_t j = rd.idx();
            if (size == 0 || j < 0 || j > size)
                return SC_ERR_ARG;
            double prm = -7.0;
            SmPointClassification* pc = cc.GetPointClassByIndex((ULONG)j, &prm);
            o.push_back(0.0);
            o.push_back(prm);
            put_pc(o, *pc, pool);
            break;
        }
        case OP_MATE:
        {
            int64_t j = rd.idx();
            if (j < 0 || j > size)
                return SC_ERR_ARG;
            if ((j == 0 || j == size) && cc.GetCurve() == NULL)
                return SC_ERR_ARG;
            if (size == 0)
            {
                SmZoneTol3d z = SmTol::GetSrcZoneTol3d(&cc);
                SmXSectTol3d x = SmTol::GetXSectTol3d(z, z);
                if (cc.GetCurve()->IsClosed(cc.GetInterval(), x))
                    return SC_ERR_ARG;
            }
            SmPointClassification* pc = cc.GetPointClassMateByIndex((ULONG)j);
            o.push_back(0.0);
            double at = -1.0;
            for (int64_t k = 0; k < size; k++)
                if (pc == &cc.GetAt((ULONG)k).m_vEnd)
                    at = (double)k;
            o.push_back(pc == NULL ? -1.0 : at);
            break;
        }
        case OP_COLLAPSE:
        {
            int64_t j = rd.idx();
            if (j < 0 || j > MAX_IVLS)
                return SC_ERR_ARG;
            if (j < size && size >= 2)
            {
                // the curve is dereferenced; the end objects' zone tolerances are taken (the
                // SmPoint3d of a point classification is not an SmObject)
                const SmCurveInterval& v = cc.GetAt((ULONG)j);
                if (cc.GetCurve() == NULL || pc_kind(v.m_vStart, pool) == K_POINT
                    || pc_kind(v.m_vEnd, pool) == K_POINT)
                    return SC_ERR_ARG;
            }
            o.push_back(st(cc.CollapseInterval((ULONG)j)));
            break;
        }
        case OP_TOUCHES:
        {
            int64_t n = rd.idx();
            if (n < 0 || n > 8 || size == 0)
                return SC_ERR_ARG;
            SmTArray<SmTopology*> tgts;
            std::vector<int64_t> tkinds;
            for (int64_t k = 0; k < n; k++)
            {
                int64_t ti = rd.idx();
                SmTopology* tp = (SmTopology*)pool.get(ti, {K_VERTEX, K_EDGE}, ok);
                if (!ok || ti == -1)
                    return SC_ERR_ARG;
                tgts.Add(tp);
                tkinds.push_back(pool.kinds[(size_t)ti]);
            }
            // connection tests between kinds other than vertex/vertex and edge/edge walk the
            // (missing) topology of the bare stand-ins
            if (n > 0)
            {
                auto check = [&](const SmPointClassification& pc) {
                    if (!is_ve(pc))
                        return true;
                    int64_t k = pc_kind(pc, pool);
                    for (int64_t tk : tkinds)
                        if (tk != k)
                            return false;
                    return true;
                };
                if (!check(cc.GetAt(0).m_vStart))
                    return SC_ERR_ARG;
                for (int64_t k = 0; k < size; k++)
                    if (!check(cc.GetAt((ULONG)k).m_vEnd))
                        return SC_ERR_ARG;
            }
            SmTArray<SmTouchData> td;
            SmStatus s = cc.GetPointClassifiesConnectedTo(tgts, td);
            o.push_back(st(s));
            o.push_back((double)td.GetSize());
            for (ULONG k = 0; k < td.GetSize(); k++)
            {
                const SmTouchData& d = td[k];
                o.push_back(pool.id(d.m_pConnectedTo));
                o.push_back((double)d.m_eLoopDir);
                o.push_back((double)d.m_lLoopIndx);
                o.push_back(d.m_dParam);
                o.push_back((double)d.m_lPtClassifyIndx);
                // which point classification: 3 * interval + (0 start, 1 mid, 2 end)
                double where = -1.0;
                for (int64_t q = 0; q < size; q++)
                    for (int64_t w = 0; w < 3; w++)
                        if (d.m_pPtClassify == pick(cc.GetAt((ULONG)q), w))
                            where = (double)(3 * q + w);
                o.push_back(where);
            }
            break;
        }
        case OP_FIND_VERTEX:
        {
            SmTopology* tgt = (SmTopology*)pool.get(rd.idx(), {K_VERTEX}, ok);
            if (!ok || size == 0)
                return SC_ERR_ARG;
            SmTArray<SmVertex*> vs;
            SmTArray<double> prms;
            ULONG cnt = cc.FindVertexObjects(vs, tgt, &prms);
            o.push_back(0.0);
            o.push_back((double)cnt);
            for (ULONG k = 0; k < vs.GetSize(); k++)
                o.push_back(pool.id(vs[k]));
            o.push_back((double)prms.GetSize());
            for (ULONG k = 0; k < prms.GetSize(); k++)
                o.push_back(prms[k]);
            break;
        }
        case OP_FIND_EDGE:
        {
            SmTopology* tgt = (SmTopology*)pool.get(rd.idx(), {K_EDGE}, ok);
            int64_t flag = rd.idx();
            if (!ok || size == 0 || flag < 0 || flag > 3)
                return SC_ERR_ARG;
            SmTArray<SmEdge*> es;
            ULONG cnt = cc.FindEdgeObjects(es, tgt, (ULONG)flag);
            o.push_back(0.0);
            o.push_back((double)cnt);
            for (ULONG k = 0; k < es.GetSize(); k++)
                o.push_back(pool.id(es[k]));
            break;
        }
        case OP_CROSSED_BY_EDGE:
            o.push_back(0.0);
            o.push_back(flg(cc.IsCrossedByEdge()));
            break;
        case OP_CROSSING_FACE:
        {
            SmFace* f = (SmFace*)pool.get(rd.idx(), {K_FACE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(flg(cc.IsCrossingFace(f)));
            break;
        }
        case OP_NEAR_MISSES:
        {
            bool b = rd.flag();
            o.push_back(0.0);
            o.push_back(flg(cc.HasNearMisses(b ? TRUE : FALSE)));
            break;
        }
        case OP_COIN_MAYBE:
        {
            int64_t j = rd.idx();
            bool self = rd.flag();
            bool report = rd.flag();
            const SmCurveClassification& other = self ? cc : ccs[1 - t];
            if (j < 0 || j >= size || j >= (int64_t)other.GetSize())
                return SC_ERR_ARG;
            if (cc.GetCurve() == NULL || other.GetCurve() == NULL)
            {
                // the curves are dereferenced unless the early return is taken
                const SmCurveInterval& a = cc.GetAt((ULONG)j);
                const SmCurveInterval& b2 = other.GetAt((ULONG)j);
                auto edgy = [](const SmCurveInterval& v) {
                    return v.m_vMid.GetPointClass() == SM_PC_EDGE
                           && (v.m_vEnd.GetPointClass() == SM_PC_EDGE || v.m_vStart.GetPointClass() == SM_PC_EDGE);
                };
                if (edgy(a) || edgy(b2))
                    return SC_ERR_ARG;
            }
            SmIntervalPosition pos = SM_IP_UNINIT;
            SmBoolean r = cc.AreCoincidentMaybe(other, (ULONG)j, pos, report ? TRUE : FALSE);
            o.push_back(0.0);
            o.push_back((double)r);
            o.push_back((double)pos);
            break;
        }
        case OP_CLASSIFY_FACE:
        {
            int64_t j = rd.idx();
            bool do_point = rd.flag();
            int64_t fi = rd.idx();
            SmFace* f = (SmFace*)pool.get(fi, {K_FACE}, ok);
            SmFace* old = (SmFace*)pool.get(rd.idx(), {K_FACE}, ok);
            bool uv_ok = rd.flag();
            if (!ok || j < 0 || j >= size || f == NULL)
                return SC_ERR_ARG;
            // the face queries walk the (missing) loops of the bare stand-in faces: only the
            // paths that return before them are run
            const SmCurveInterval& ivl = cc.GetAt((ULONG)j);
            SmPointClassificationType mc = ivl.m_vMid.GetPointClass();
            bool early = !(mc == SM_PC_UNKNOWN || (old != NULL && mc == SM_PC_FACE))
                         || (old != NULL && ivl.m_vMid.GetFaceObject() != NULL && old != ivl.m_vMid.GetFaceObject());
            if (!early && (is_ve(ivl.m_vStart) || is_ve(ivl.m_vEnd) || do_point))
                return SC_ERR_ARG;
            o.push_back(st(cc.ClassifyFaceInterval((ULONG)j, do_point ? TRUE : FALSE, f, old, uv_ok ? TRUE : FALSE)));
            break;
        }
        case OP_PARTIAL:
        {
            bool self = rd.flag();
            SmCurveClassification& other = self ? cc : ccs[1 - t];
            int64_t osize = (int64_t)other.GetSize();
            if (size > 0 && osize > 0)
            {
                if (bad_edge_curve(cc.GetAt(0).m_vMid, other.GetAt(0).m_vMid)
                    || bad_edge_curve(cc.GetAt((ULONG)size - 1).m_vMid, other.GetAt((ULONG)osize - 1).m_vMid))
                    return SC_ERR_ARG;
            }
            double gap = -7.0;
            SmBSplineCurve* nc = NULL;
            SmStatus s = cc.TestForPartialCoincidentEdges(other, gap, nc);
            o.push_back(st(s));
            o.push_back(gap);
            o.push_back(flg(nc != NULL));
            if (nc != NULL)
            {
                put_curve(o, nc); // (never deleted, like the pool objects)
            }
            break;
        }
        case OP_FILLET:
        {
            int64_t j = rd.idx();
            int64_t todo = rd.idx();
            if (j < 0 || j > MAX_IVLS || todo < 0 || todo > 7)
                return SC_ERR_ARG;
            if (cc.GetDoingFillets() && cc.GetOtherClassification() != NULL)
            {
                const SmCurveClassification& other = *cc.GetOtherClassification();
                if (j >= size || j >= (int64_t)other.GetSize())
                    return SC_ERR_ARG;
                // the edge step replaces (deletes) the pool curve of this edge
                const SmCurveInterval& a = cc.GetAt((ULONG)j);
                const SmCurveInterval& b2 = other.GetAt((ULONG)j);
                bool v1 = !(todo & 1)
                          || (a.m_vStart.GetPointClass() == SM_PC_VERTEX && b2.m_vStart.GetPointClass() == SM_PC_VERTEX);
                bool v2 = !(todo & 2)
                          || (a.m_vEnd.GetPointClass() == SM_PC_VERTEX && b2.m_vEnd.GetPointClass() == SM_PC_VERTEX);
                if ((todo & 4) && v1 && v2 && a.m_vMid.GetPointClass() == SM_PC_EDGE
                    && b2.m_vMid.GetPointClass() == SM_PC_EDGE)
                    return SC_ERR_ARG;
            }
            o.push_back(st(sm_SetDoFilletingGeometry((ULONG)j, cc, (ULONG)todo)));
            break;
        }
        default: return SC_ERR_ARG;
        }
        if (rd.bad)
            return SC_ERR_ARG;
        put_state(o, cc, pool);
    }
    if (rd.bad || rd.at != rd.n)
        return SC_ERR_ARG;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_intersect_curve_class_p7_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                  int64_t* out_len)
{
    if (in == NULL || n_in < 0 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    int32_t s = run(rd, o);
    if (s != SC_OK)
    {
        *out_len = 0;
        return s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
