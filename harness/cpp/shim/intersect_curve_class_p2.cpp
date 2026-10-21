// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.curve_class_p2`: the part-2 members of SmCurveClassification
// (operator=, GetPointObjZoneTol3d, HasDeadTopologyPointObject, GetClassifyCrvLoc(ULONG, double*),
// InsertAt, ReSet, ConnectIntervals, UpdatePointClass, EditIntervalMin, EditIntervalMax),
// SmCurveInterval::GetMinUV / GetMaxUV and sm_RefineCCIToSurfaces (global in
// source/SMLib/src/SmCurveClass.cpp).
//
// One entry point, sc_intersect_curve_class_p2_run(in, n_in, out, cap, out_len): a pool of objects
// (the format of unit intersect.curve_class_p1, whose object helpers are copied below), curves on
// surfaces appended to it, two classifications cc[0], cc[1] built from the script, then ops on
// them. The layout is documented in crates/solidcore-ffi/src/units/intersect_curve_class_p2.rs,
// which validates the static shape; the run-time checks below reject (SC_ERR_ARG) what is
// undefined upstream (indices outside the intervals, NULL curves that are dereferenced, zone
// tolerances of SmPoint3d objects, InsertAt(0) into a non-empty classification) and the
// refinement past its entry checks (it walks the bare stand-in topology). Objects live for the
// whole process (never deleted: topology destructors notify owners); a curve a classification
// owns is disowned before the ops that would delete it.
//
// With has_face, the pool also holds a real one-face B-rep built with SmBrepConstructor
// (sc_one_face_brep.h, as the intersect.curve_class_p4 shim): its face, loops, edges, vertices and
// edge uses follow the curves on surfaces, and HasDeadTopologyPointObject can test against it. The
// B-rep is deleted when the run returns.
#include "sc_shim.h"
#include "sc_one_face_brep.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
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
SmStatus sm_RefineCCIToSurfaces(SmCurveClassification* pCurveClassification, SmSolution& rSolution, SmEdge* pEdge,
                                SmBoolean& bSSSPointFlag, SmPoint3d& vSSSPoint, double& dDeviation,
                                ULONG& lRecursionCount);

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
    K_EDGEUSE = 9,
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

// Access to the private SmCurveClassification::m_bIsCurveOwned through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct CcOwned
{
    typedef SmBoolean SmCurveClassification::*type;
    friend type get(CcOwned);
};
template struct Rob<CcOwned, &SmCurveClassification::m_bIsCurveOwned>;

// The full state of a classification (the record of unit intersect.curve_class_p1).
void put_cc(std::vector<double>& o, const SmCurveClassification& cc, const Pool& pool)
{
    o.push_back((double)cc.GetSize());
    SmExtent1d ivl = cc.GetInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    o.push_back(SmTol::GetSrcZoneTol3d(&cc));
    o.push_back(flg(cc.*get(CcOwned())));
    o.push_back(flg(cc.GetIsCurveCurrent()));
    o.push_back(flg(cc.GetIgnoreTangency()));
    o.push_back(flg(cc.GetDoingCrossHatch()));
    o.push_back(flg(cc.GetDoingFillets()));
    o.push_back(flg(cc.GetOKToSetTolerance()));
    o.push_back(flg(cc.GetTgtLoopsOnly()));
    o.push_back(pool.id(cc.GetCurve()));
    o.push_back(pool.id(cc.GetUVCurve1()));
    o.push_back(pool.id(cc.GetUVCurve2()));
    o.push_back(pool.id(cc.GetAnyUVCurve1()));
    o.push_back(pool.id(cc.GetAnyUVCurve2()));
    o.push_back(pool.id(cc.GetSurface1()));
    o.push_back(pool.id(cc.GetSurface2()));
    o.push_back(pool.id(cc.GetFace()));
    o.push_back(pool.id(cc.GetTopologyIntersector()));
    o.push_back(pool.id(cc.GetOtherClassification()));
    const SmTArray<SmLoop*>& loops = cc.GetTgtLoops();
    o.push_back((double)loops.GetSize());
    for (ULONG i = 0; i < loops.GetSize(); i++)
        o.push_back(pool.id(loops[i]));
    for (ULONG i = 0; i < cc.GetSize(); i++)
        put_ivl(o, cc.GetAt(i), pool);
}

enum : int64_t
{
    OP_ASSIGN = 0,
    OP_OBJ_TOL = 1,
    OP_DEAD = 2,
    OP_CRV_LOC = 3,
    OP_INSERT = 4,
    OP_RESET = 5,
    OP_CONNECT = 6,
    OP_UPDATE_PC = 7,
    OP_EDIT_MIN = 8,
    OP_EDIT_MAX = 9,
    OP_MIN_UV = 10,
    OP_MAX_UV = 11,
    OP_REFINE = 12,
};

const int64_t MAX_IVLS = 16;
const int64_t MAX_LOOPS = 4;

// the kind of the object of a point classification (-1 none / outside the pool)
int64_t pc_kind(const SmPointClassification& pc, const Pool& pool)
{
    return pc.GetObject() == NULL ? -1 : pool.kind_of(pc.GetObject());
}

// `class obj t gap` into a point classification; false for an object of the wrong kind.
bool read_pc(Rd& rd, const Pool& pool, SmPointClassification& pc)
{
    int64_t cls = rd.idx();
    int64_t obj = rd.idx();
    double t = rd.num();
    double g = rd.num();
    int64_t want = kind_of_class(cls);
    if (want == -2)
        return false;
    bool ok = true;
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
    return true;
}

// `a b pc*3` into an interval.
bool read_ivl(Rd& rd, const Pool& pool, SmCurveInterval& ivl)
{
    double a = rd.num();
    double b = rd.num();
    ivl.SetInterval(SmExtent1d(a, b));
    for (int64_t w = 0; w < 3; w++)
        if (!read_pc(rd, pool, *pick(ivl, w)))
            return false;
    return !rd.bad;
}

// One classification: `curve s1 uv1 s2 uv2 face ti other fillet current ignore ok_tol tgt_only
// n_loops loop*n_loops tol n ivl*n`.
bool read_cc(Rd& rd, const Pool& pool, SmCurveClassification& cc)
{
    bool ok = true;
    SmCurve* c = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
    SmSurface* s1 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
    SmCurve* uv1 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
    SmSurface* s2 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
    SmCurve* uv2 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
    SmFace* face = (SmFace*)pool.get(rd.idx(), {K_FACE}, ok);
    void* ti = pool.get(rd.idx(), {K_OTHER}, ok);
    void* other = pool.get(rd.idx(), {K_OTHER}, ok);
    bool fillet = rd.flag();
    bool current = rd.flag();
    bool ignore = rd.flag();
    bool ok_tol = rd.flag();
    bool tgt_only = rd.flag();
    int64_t n_loops = rd.idx();
    if (!ok || rd.bad || n_loops < 0 || n_loops > MAX_LOOPS)
        return false;
    cc.SetCurve(c, FALSE);
    cc.SetUVCurve1(uv1);
    // never dereferenced: stored and returned only
    cc.SetIntersectData(s1, s2, uv2, current ? TRUE : FALSE, (const SmTopologyIntersector*)ti);
    cc.SetFace(face);
    if (other != NULL || fillet)
        cc.SetFilletData((SmCurveClassification*)other, fillet ? TRUE : FALSE);
    cc.SetIgnoreTangency(ignore ? TRUE : FALSE);
    cc.SetOKToSetTolerance(ok_tol ? TRUE : FALSE);
    cc.SetTgtLoopsOnly(tgt_only ? TRUE : FALSE);
    for (int64_t i = 0; i < n_loops; i++)
    {
        SmLoop* l = (SmLoop*)pool.get(rd.idx(), {K_LOOP}, ok);
        if (!ok || l == NULL)
            return false;
        cc.AddTgtLoop(l);
    }
    double tol = rd.num();
    int64_t n = rd.idx();
    if (rd.bad || n < 0 || n > MAX_IVLS)
        return false;
    SmTol::SetZoneTol3d(&cc, SmZoneTol3d(tol));
    cc.SetSize((ULONG)n);
    for (int64_t i = 0; i < n; i++)
        if (!read_ivl(rd, pool, cc.GetAt((ULONG)i)))
            return false;
    return !rd.bad;
}

// Whether the point classification's zone tolerance can be taken (the SmPoint3d of a point
// classification is not an SmObject: SmTol::GetZoneTol3d would cast it).
bool tol_ok(const SmPointClassification& pc, const Pool& pool) { return pc_kind(pc, pool) != K_POINT; }

// A curve the classification owns is never deleted here (it may be a pool object): the flag is
// cleared before the ops that delete it, which then overwrite it (unobservable).
void disown(SmCurveClassification& cc) { cc.*get(CcOwned()) = FALSE; }

// The run-time checks of EditIntervalMin / EditIntervalMax: the curve is dereferenced and the
// end objects' zone tolerances are taken of every interval the edit can reach.
bool edit_ok(const SmCurveClassification& cc, const Pool& pool)
{
    if (cc.GetCurve() == NULL)
        return false;
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        const SmCurveInterval& v = cc.GetAt(i);
        if (!tol_ok(v.m_vStart, pool) || !tol_ok(v.m_vEnd, pool))
            return false;
    }
    return true;
}

int32_t run(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Pool pool;

    // the one-face B-rep (deleted on every return; its objects join the pool after the
    // curves on surfaces)
    SmBrep* brep = NULL;
    sc_one_face::OneFace of;
    bool has_face = rd.flag();
    bool built = !has_face || sc_one_face::build(rd, ctx, brep, of);
    SmObjDelete clean(brep);
    if (rd.bad || !built)
        return SC_ERR_ARG;

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

    // curves on surfaces: `n (uv surface)*n`, appended to the pool
    int64_t n_cos = rd.idx();
    for (int64_t i = 0; i < n_cos && !rd.bad; i++)
    {
        bool ok = true;
        SmCurve* uv = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
        SmSurface* s = (SmSurface*)pool.get(rd.idx(), {K_BSURF}, ok);
        if (!ok || uv == NULL || s == NULL)
            return SC_ERR_ARG;
        SmCrvOnSurf* cos = new (ctx) SmCrvOnSurf(*uv, *s, NULL, 0, &ctx);
        pool.ids[cos] = (int64_t)pool.ptrs.size();
        pool.kinds.push_back(K_CURVE);
        pool.ptrs.push_back(cos);
        pool.bspline.push_back(false);
        pool.dims.push_back(3);
    }
    if (rd.bad)
        return SC_ERR_ARG;
    // the B-rep's topology: the face, then per loop the loop and its vertex (a vertex loop) or
    // per edge use its edge, its vertex use's vertex and the edge use (an edge loop)
    auto add = [&pool](void* q, int64_t kind) {
        pool.ids[q] = (int64_t)pool.ptrs.size();
        pool.kinds.push_back(kind);
        pool.ptrs.push_back(q);
        pool.bspline.push_back(false);
        pool.dims.push_back(0);
    };
    if (has_face)
    {
        add(of.face, K_FACE);
        for (size_t li = 0; li < of.loops.size(); li++)
        {
            add(of.loops[li], K_LOOP);
            if (of.loop_vertex[li] != NULL)
            {
                add(of.loop_vertex[li], K_VERTEX);
                continue;
            }
            for (SmEdgeuse* eu : of.loop_edgeuses[li])
            {
                SmEdge* e = eu->GetEdge();
                SmVertexuse* vu = eu->GetVertexuse();
                SmPoint3d start;
                // the edge use's vertex is its edge's start vertex (the Rust B-rep is built so)
                if (e == NULL || vu == NULL || vu->GetVertex() == NULL ||
                    e->GetCurve()->EvaluatePoint(e->GetInterval().GetMin(), start) != SM_SUCCESS ||
                    vu->GetVertex()->GetPoint().DistanceBetweenSquared(start) > 1e-18)
                    return SC_ERR_ARG;
                // the edge line with its NURB made, as the Rust side makes it
                SmBSplineCurve* line = dynamic_cast<SmBSplineCurve*>(e->GetCurve());
                if (line == NULL)
                    return SC_ERR_ARG;
                line->GetOrCreateGwNurbPointer();
                add(e, K_EDGE);
                add(vu->GetVertex(), K_VERTEX);
                add(eu, K_EDGEUSE);
            }
        }
    }

    SmCurveClassification ccs[2];
    for (int t = 0; t < 2; t++)
        if (!read_cc(rd, pool, ccs[t]))
        {
            disown(ccs[0]);
            disown(ccs[1]);
            return SC_ERR_ARG;
        }

    int32_t rc = SC_OK;
    int64_t n_ops = rd.idx();
    for (int64_t i = 0; i < n_ops && !rd.bad && rc == SC_OK; i++)
    {
        int64_t op = rd.idx();
        int64_t t = rd.idx();
        if (t < 0 || t > 1)
        {
            rc = SC_ERR_ARG;
            break;
        }
        SmCurveClassification& cc = ccs[t];
        SmCurveClassification& other = ccs[1 - t];
        int64_t size = (int64_t)cc.GetSize();
        bool ok = true;
        o.push_back((double)op);
        switch (op)
        {
        case OP_ASSIGN:
        {
            disown(cc);
            cc = other;
            // an owned curve is deep-copied: the copy stands for the original
            if (cc.GetCurve() != NULL && pool.ids.find(cc.GetCurve()) == pool.ids.end())
                pool.ids[cc.GetCurve()] = (int64_t)pool.id(other.GetCurve());
            o.push_back(0.0);
            break;
        }
        case OP_OBJ_TOL:
        {
            int64_t j = rd.idx();
            if (size == 0 || j < 0 || j > size)
            {
                rc = SC_ERR_ARG;
                break;
            }
            if (!tol_ok(*cc.GetPointClassByIndex((ULONG)j), pool))
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(0.0);
            o.push_back(cc.GetPointObjZoneTol3d((ULONG)j));
            break;
        }
        case OP_DEAD:
        {
            int64_t with = rd.idx();
            static SmBrep* empty = new (ctx) SmBrep();
            if (with == 2 && brep == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(0.0);
            o.push_back(flg(cc.HasDeadTopologyPointObject(with == 2 ? brep : with == 1 ? empty : NULL)));
            break;
        }
        case OP_CRV_LOC:
        {
            int64_t j = rd.idx();
            if (size == 0 || j < 0 || j > size)
            {
                rc = SC_ERR_ARG;
                break;
            }
            if (!cc.GetPointClassByIndex((ULONG)j)->GetSSSPointFlag() && cc.GetCurve() == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            double prm = -7.0;
            SmPoint3d p = cc.GetClassifyCrvLoc((ULONG)j, &prm);
            o.push_back(0.0);
            o.push_back(prm);
            put_vec(o, p);
            break;
        }
        case OP_INSERT:
        {
            int64_t j = rd.idx();
            SmCurveInterval ivl(&ctx, &cc);
            if (!read_ivl(rd, pool, ivl) || j < 0 || j > MAX_IVLS || (j == 0 && size > 0))
            {
                rc = SC_ERR_ARG;
                break;
            }
            cc.InsertAt((ULONG)j, ivl);
            o.push_back(0.0);
            break;
        }
        case OP_RESET:
        {
            SmCurve* c = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            bool has_ivl = rd.flag();
            double a = rd.num();
            double b = rd.num();
            SmCurve* uv = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            double tol = rd.num();
            bool owned = rd.flag();
            bool current = rd.flag();
            SmSurface* s1 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmSurface* s2 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmCurve* uv2 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            void* ti = pool.get(rd.idx(), {K_OTHER}, ok);
            bool fresh = c != NULL || uv != NULL;
            if (!ok || rd.bad || (fresh && !has_ivl))
            {
                rc = SC_ERR_ARG;
                break;
            }
            SmExtent1d ivl(a, b);
            if (fresh)
                disown(cc);
            cc.ReSet(c, has_ivl ? &ivl : NULL, uv, SmZoneTol3d(tol), 0, NULL, owned ? TRUE : FALSE,
                     current ? TRUE : FALSE, s1, s2, uv2, (const SmTopologyIntersector*)ti);
            o.push_back(0.0);
            break;
        }
        case OP_CONNECT:
        {
            int64_t j = rd.idx();
            int64_t end = rd.idx();
            bool allow = rd.flag();
            if (j < 0 || j > MAX_IVLS || end < 0 || end > 4)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(st(cc.ConnectIntervals((ULONG)j, (SmIntervalPosition)end, allow ? TRUE : FALSE)));
            break;
        }
        case OP_UPDATE_PC:
        {
            int64_t j = rd.idx();
            double tol = rd.num();
            SmPointClassification pc(SmZoneTol3d(tol), &ctx);
            if (!read_pc(rd, pool, pc) || size == 0 || j < 0 || j > size)
            {
                rc = SC_ERR_ARG;
                break;
            }
            // the mate of the first and last boundary needs the curve
            if ((j == 0 || j == size) && cc.GetCurve() == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(st(cc.UpdatePointClass((ULONG)j, pc)));
            break;
        }
        case OP_EDIT_MIN:
        case OP_EDIT_MAX:
        {
            int64_t j = rd.idx();
            double prm = rd.num();
            bool collapse = rd.flag();
            if (j < 0 || j > MAX_IVLS)
            {
                rc = SC_ERR_ARG;
                break;
            }
            if (j < size)
            {
                // reachable only past the input checks (the allowed range)
                SmExtent1d allowed = op == OP_EDIT_MIN
                                         ? SmExtent1d(j > 0 ? cc.GetAt((ULONG)j - 1).m_vInterval.GetMin()
                                                            : cc.GetInterval().GetMin(),
                                                      cc.GetAt((ULONG)j).m_vInterval.GetMax())
                                         : SmExtent1d(cc.GetAt((ULONG)j).m_vInterval.GetMin(),
                                                      j < size - 1 ? cc.GetAt((ULONG)j + 1).m_vInterval.GetMax()
                                                                   : cc.GetInterval().GetMax());
                if (allowed.ContainsValue(prm) && !edit_ok(cc, pool))
                {
                    rc = SC_ERR_ARG;
                    break;
                }
            }
            SmStatus s = op == OP_EDIT_MIN ? cc.EditIntervalMin((ULONG)j, prm, collapse ? TRUE : FALSE)
                                           : cc.EditIntervalMax((ULONG)j, prm, collapse ? TRUE : FALSE);
            o.push_back(st(s));
            break;
        }
        case OP_MIN_UV:
        case OP_MAX_UV:
        {
            int64_t j = rd.idx();
            double u = rd.num();
            double v = rd.num();
            if (j < 0 || j >= size || cc.GetCurve() == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            SmPoint2d uv(u, v);
            const SmCurveInterval& ivl = cc.GetAt((ULONG)j);
            SmStatus s = op == OP_MIN_UV ? ivl.GetMinUV(uv) : ivl.GetMaxUV(uv);
            o.push_back(st(s));
            o.push_back(uv.x);
            o.push_back(uv.y);
            break;
        }
        case OP_REFINE:
        {
            SmSolution sol;
            int64_t ty = rd.idx();
            double s0 = rd.num();
            double s1 = rd.num();
            double e0 = rd.num();
            double e1 = rd.num();
            double val = rd.num();
            SmEdge* e = (SmEdge*)pool.get(rd.idx(), {K_EDGE}, ok);
            SmPoint3d sss = rd.vec();
            int64_t cnt = rd.idx();
            if (!ok || rd.bad || ty < 0 || ty > 4 || cnt < 0 || cnt > 100)
            {
                rc = SC_ERR_ARG;
                break;
            }
            sol.m_eSolutionType = (SmSolutionType)ty;
            sol.m_vStart.m_adParameters[0] = s0;
            sol.m_vStart.m_adParameters[1] = s1;
            sol.m_vEnd.m_adParameters[0] = e0;
            sol.m_vEnd.m_adParameters[1] = e1;
            sol.m_vStart.m_dSolutionValue = val;
            // past the entry checks the bare stand-in edge's topology (faces, edge uses,
            // vertices) is walked and its curve may be edited: only the entry checks run
            bool point = ty == SM_ST_SINGLE_VALUE || SM_ARE_SAME(s0, e0) || SM_ARE_SAME(s1, e1);
            if (cnt <= 5 && point && cc.GetSurface1() != NULL && cc.GetSurface2() != NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            SmBoolean flag = TRUE;
            double dev = -7.0;
            ULONG count = (ULONG)cnt;
            SmStatus s = sm_RefineCCIToSurfaces(&cc, sol, e, flag, sss, dev, count);
            o.push_back(st(s));
            o.push_back(flg(flag));
            put_vec(o, sss);
            o.push_back(dev);
            o.push_back((double)count);
            o.push_back((double)sol.m_eSolutionType);
            o.push_back(sol.m_vStart.m_adParameters[0]);
            o.push_back(sol.m_vStart.m_adParameters[1]);
            o.push_back(sol.m_vEnd.m_adParameters[0]);
            o.push_back(sol.m_vEnd.m_adParameters[1]);
            o.push_back(sol.m_vStart.m_dSolutionValue);
            break;
        }
        default: rc = SC_ERR_ARG; break;
        }
        if (rc != SC_OK)
            break;
        if (rd.bad)
        {
            rc = SC_ERR_ARG;
            break;
        }
        put_cc(o, cc, pool);
    }
    if (rc == SC_OK && (rd.bad || rd.at != rd.n))
        rc = SC_ERR_ARG;
    disown(ccs[0]);
    disown(ccs[1]);
    return rc;
}
} // namespace

SC_EXPORT int32_t sc_intersect_curve_class_p2_run(const double* in, int64_t n_in, double* out, int64_t cap,
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
