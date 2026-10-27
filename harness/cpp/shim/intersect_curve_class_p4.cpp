// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.curve_class_p4`: the part-4 members of SmCurveClassification
// (the full constructor, GetLoopCrossings, GetLoopTouches), SmCurveInterval::GetIvlBound_EdgeuseObj
// and the static function sm_SqueezeOutSmallestInterval of source/SMLib/src/SmCurveClass.cpp.
//
// The static function cannot be linked against, so it is a VERBATIM copy of upstream
// SmCurveClass.cpp (commit c9c5979) inside the namespace sc_cc_p4 (line range noted at the
// block; generated, do not edit by hand).
//
// One entry point, sc_intersect_curve_class_p4_run(in, n_in, out, cap, out_len): a script over a
// pool of objects and two default-constructed classifications cc[0], cc[1] (the setup ops are
// those of unit intersect.curve_class_p1; the layout is documented in
// crates/solidcore-ffi/src/units/intersect_curve_class_p4.rs, which validates the static shape).
// The pool's topology is a real one-face B-rep built with SmBrepConstructor (sc_one_face_brep.h,
// as the heal.face_props shim: a plane or surface of revolution with a vertex loop or edge loops
// of line edges); its face, loops, edges, vertices and edge uses follow the script's objects.
// The run-time checks below reject (SC_ERR_ARG) the ops that are undefined behaviour upstream:
// an empty classification for the squeeze (FindBestSqueezeEnd(0) reads m_pData[0]); for
// GetIvlBound_EdgeuseObj an interval index past the intervals, an interval whose back pointer is
// not its classification, a classification without face (UB-124: GetUpwardFaceuse / GetSurface
// on NULL) and, when the end is on an edge of a face with a surface, a NULL classification or
// edge curve (both are evaluated); for GetLoopCrossings or GetLoopTouches over loops a
// classification without curve (UB-124: GetNaturalInterval on NULL) or without intervals
// (GetPointClassifiesConnectedTo reads m_pData[0]). Geometry objects live for the whole process
// (never deleted); the B-rep is deleted at the end of the run.

#include "sc_shim.h"
#include "sc_one_face_brep.h"

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
#include <SmEdgeuse.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmFace.h>
#include <SmFaceProps.h>
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
#include <new>
#include <vector>

// global in the library (SmCurveClass.cpp), not declared in a header
SmXSectTol3d sm_GetXSectTol2dOr3d_ForClassifyPair(const SmCurve* cpSrcCurve, const SmSurface* cpSurface1,
                                                  double dSrcCurveOrObj1_Param,
                                                  SmZoneTol3d sSrcCurveOrObj1_ZoneTol3d,
                                                  double dObj2_SrcCurveParam, SmZoneTol3d sObj2_ZoneTol3d);

namespace sc_cc_p4
{
// clang-format off
// ---- verbatim: SmCurveClass.cpp lines 3710-3741: sm_SqueezeOutSmallestInterval
/*******************************************************************//**
PURPOSE: Remove the smallest interval by squeezing it

NOTES:
***********************************************************************/
static SmStatus sm_SqueezeOutSmallestInterval
  (SmCurveClassification & rCurveClass)    // in : target CurveClassification
{
  // find the shortest classification interval
  ULONG lSmallest = 0;
  for (ULONG j=1; j<rCurveClass.GetSize(); j++)
    {
      SmCurveInterval & rCIvl1 = rCurveClass[lSmallest];
      SmCurveInterval & rCIvl2 = rCurveClass[j];
      if (rCIvl2.m_vInterval.GetLength() < rCIvl1.m_vInterval.GetLength())
        {
          lSmallest = j;
        }
    }

  // pick end of interval to remove from classification
  SmIntervalPosition eSqueezePos = rCurveClass.FindBestSqueezeEnd(lSmallest);

  // remove smallest interval target endBoundary
  SER(rCurveClass.ConnectIntervals(lSmallest,   // in : interval to be removed (thought of as squeezed to zero length)
                                   eSqueezePos, // in : interval endPoint to be removed
                                   FALSE));     // in : TRUE = OK to delete end interval and shorten m_vInterval
                                                //      FALSE= only merge end_intervals with their neighbors
  // all done
  return SM_SUCCESS;

} // end sm_SqueezeOutSmallestInterval
// clang-format on
} // namespace sc_cc_p4

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

// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_curve_class_p3.rs).
enum : int64_t
{
    OP_SET_SIZE = 0,
    OP_REMOVE_AT = 1,
    OP_IVL_SET = 2,
    OP_IVL_SET_MIN = 3,
    OP_IVL_SET_MAX = 4,
    OP_IVL_RESET = 5,
    OP_IVL_ASSIGN = 6,
    OP_IVL_NEW = 7,
    OP_PC_SET_CLASS = 8,
    OP_PC_SET_T = 9,
    OP_PC_SET_UV = 10,
    OP_PC_SET_GAP = 11,
    OP_PC_SET_SSS = 12,
    OP_PC_SET_PRESNAP = 13,
    OP_PC_SET_SRC_TOL = 14,
    OP_SET_CURVE = 15,
    OP_SET_SURFACE1 = 16,
    OP_SET_SURFACE2 = 17,
    OP_SET_FACE = 18,
    OP_SET_INTERSECT_DATA = 19,
    OP_SET_IS_CURVE_CURRENT = 20,
    OP_SET_IGNORE_TANGENCY = 21,
    OP_SET_IS_CURVE_OWNED = 22,
    OP_SET_OK_TO_SET_TOL = 23,
    OP_SET_FILLET_DATA = 24,
    OP_SET_TGT_LOOPS_ONLY = 25,
    OP_ADD_TGT_LOOP = 26,
    OP_SET_SRC_TOL = 27,
    OP_SET_UV_CURVE1 = 28,
    OP_SET_UV_CURVE2 = 29,
    OP_SQUEEZE = 30,
    OP_IVL_BOUND = 31,
    OP_LOOP_CROSSINGS = 32,
    OP_CONSTRUCT = 33,
    OP_LOOP_TOUCHES = 34,
};

// Access to private / protected members through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct IvlSetMin
{
    typedef SmStatus (SmCurveInterval::*type)(double);
    friend type get(IvlSetMin);
};
template struct Rob<IvlSetMin, &SmCurveInterval::SetIntervalMin>;

struct IvlSetMax
{
    typedef SmStatus (SmCurveInterval::*type)(double);
    friend type get(IvlSetMax);
};
template struct Rob<IvlSetMax, &SmCurveInterval::SetIntervalMax>;

struct CcOwned
{
    typedef SmBoolean SmCurveClassification::*type;
    friend type get(CcOwned);
};
template struct Rob<CcOwned, &SmCurveClassification::m_bIsCurveOwned>;

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

bool is_surface(int64_t k) { return k == K_PLANE || k == K_BSURF; }

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
    return w == 0 ? &ivl.m_vStart : w == 1 ? &ivl.m_vMid : w == 2 ? &ivl.m_vEnd : NULL;
}

void put_touches(std::vector<double>& o, const SmTArray<SmTouchData>& l, const Pool& pool)
{
    o.push_back((double)l.GetSize());
    for (ULONG i = 0; i < l.GetSize(); i++)
    {
        o.push_back(pool.id(l[i].m_pConnectedTo));
        o.push_back((double)l[i].m_eLoopDir);
        o.push_back((double)l[i].m_lLoopIndx);
        o.push_back(l[i].m_dParam);
        o.push_back((double)l[i].m_lPtClassifyIndx);
    }
}

int32_t run(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Pool pool;

    // the one-face B-rep (deleted on every return; its objects join the pool after the
    // script's objects)
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
                // the edge line with its NURB made (SmPlane::DropCurve of a UV trim curve reads it)
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
    // the pool's curves are never deleted: clear the ownership flags before the
    // classifications are destroyed (on every return)
    struct Disown
    {
        SmCurveClassification* c;
        ~Disown()
        {
            c[0].*get(CcOwned()) = FALSE;
            c[1].*get(CcOwned()) = FALSE;
        }
    } disown{ccs};

    int64_t n_ops = rd.idx();
    for (int64_t i = 0; i < n_ops && !rd.bad; i++)
    {
        int64_t op = rd.idx();
        int64_t t = rd.idx();
        if (t < 0 || t > 1)
            return SC_ERR_ARG;
        SmCurveClassification& cc = ccs[t];
        SmCurveClassification& other = ccs[1 - t];
        int64_t size = (int64_t)cc.GetSize();
        bool ok = true;
        o.push_back((double)op);
        switch (op)
        {
        case OP_SET_SIZE:
        {
            int64_t n = rd.idx();
            if (n < 0 || n > 64)
                return SC_ERR_ARG;
            cc.SetSize((ULONG)n);
            o.push_back(0.0);
            break;
        }
        case OP_REMOVE_AT:
        {
            int64_t j = rd.idx();
            if (size == 0 || j < 0 || j > 64)
                return SC_ERR_ARG;
            cc.RemoveAt((ULONG)j);
            o.push_back(0.0);
            break;
        }
        case OP_IVL_SET:
        case OP_IVL_SET_MIN:
        case OP_IVL_SET_MAX:
        case OP_IVL_RESET:
        case OP_IVL_ASSIGN:
        case OP_IVL_NEW:
        {
            int64_t j = rd.idx();
            if (j < 0 || j >= size)
                return SC_ERR_ARG;
            SmCurveInterval& ivl = cc.GetAt((ULONG)j);
            if (op == OP_IVL_SET)
            {
                double a = rd.num();
                double b = rd.num();
                ivl.SetInterval(SmExtent1d(a, b));
                o.push_back(0.0);
            }
            else if (op == OP_IVL_SET_MIN)
                o.push_back(st((ivl.*get(IvlSetMin()))(rd.num())));
            else if (op == OP_IVL_SET_MAX)
                o.push_back(st((ivl.*get(IvlSetMax()))(rd.num())));
            else if (op == OP_IVL_RESET)
            {
                ivl.ReSet();
                o.push_back(0.0);
            }
            else if (op == OP_IVL_ASSIGN)
            {
                int64_t k = rd.idx();
                if (k < 0 || k >= (int64_t)other.GetSize())
                    return SC_ERR_ARG;
                ivl = other.GetAt((ULONG)k);
                o.push_back(0.0);
            }
            else
            {
                bool with = rd.flag();
                bool owner = rd.flag();
                double a = rd.num();
                double b = rd.num();
                const SmCurveClassification* po = owner ? &cc : NULL;
                if (with)
                    ivl = SmCurveInterval(SmExtent1d(a, b), &ctx, po);
                else
                    ivl = SmCurveInterval(&ctx, po);
                o.push_back(0.0);
            }
            break;
        }
        case OP_PC_SET_CLASS:
        case OP_PC_SET_T:
        case OP_PC_SET_UV:
        case OP_PC_SET_GAP:
        case OP_PC_SET_SSS:
        case OP_PC_SET_PRESNAP:
        case OP_PC_SET_SRC_TOL:
        {
            int64_t j = rd.idx();
            int64_t w = rd.idx();
            if (j < 0 || j >= size || w < 0 || w > 2)
                return SC_ERR_ARG;
            SmPointClassification& pc = *pick(cc.GetAt((ULONG)j), w);
            if (op == OP_PC_SET_CLASS)
            {
                int64_t c = rd.idx();
                int64_t obj = rd.idx();
                int64_t want = kind_of_class(c);
                if (want == -2)
                    return SC_ERR_ARG;
                void* po = pool.get(obj, {}, ok);
                if (!ok)
                    return SC_ERR_ARG;
                if (obj != -1)
                {
                    int64_t k = pool.kinds[(size_t)obj];
                    if (!(k == want || (want == K_BSURF && k == K_PLANE)))
                        return SC_ERR_ARG;
                }
                pc.SetClassObject((SmPointClassificationType)c, (SmObject*)po);
            }
            else if (op == OP_PC_SET_T)
                pc.SetTParam(rd.num());
            else if (op == OP_PC_SET_UV)
            {
                double u = rd.num();
                double v = rd.num();
                pc.SetUVParam(SmPoint2d(u, v));
            }
            else if (op == OP_PC_SET_GAP)
                pc.SetGap3d(rd.num());
            else if (op == OP_PC_SET_SSS)
                pc.SetSSSPoint(rd.vec());
            else if (op == OP_PC_SET_PRESNAP)
                pc.SetPreSnapParam(rd.num());
            else
                pc.SetSrcZoneTol3d(SmZoneTol3d(rd.num()));
            o.push_back(0.0);
            break;
        }
        case OP_SET_CURVE:
        {
            int64_t obj = rd.idx();
            bool owned = rd.flag();
            SmCurve* c = (SmCurve*)pool.get(obj, {K_CURVE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            // SetCurve deletes an owned previous curve: the pool's curves are never deleted,
            // so the previous one is disowned first (the flag is data here)
            cc.*get(CcOwned()) = FALSE;
            cc.SetCurve(c, owned ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        }
        case OP_SET_SURFACE1:
        case OP_SET_SURFACE2:
        {
            SmSurface* s = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            if (!ok)
                return SC_ERR_ARG;
            if (op == OP_SET_SURFACE1)
                cc.SetSurface1(s);
            else
                cc.SetSurface2(s);
            o.push_back(0.0);
            break;
        }
        case OP_SET_FACE:
        {
            SmFace* f = (SmFace*)pool.get(rd.idx(), {K_FACE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            cc.SetFace(f);
            o.push_back(0.0);
            break;
        }
        case OP_SET_INTERSECT_DATA:
        {
            SmSurface* s1 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmSurface* s2 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmCurve* uv2 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            bool cur = rd.flag();
            void* ti = pool.get(rd.idx(), {K_OTHER}, ok);
            if (!ok)
                return SC_ERR_ARG;
            cc.SetIntersectData(s1, s2, uv2, cur ? TRUE : FALSE, (const SmTopologyIntersector*)ti);
            o.push_back(0.0);
            break;
        }
        case OP_SET_IS_CURVE_CURRENT:
            cc.SetIsCurveCurrent(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_IGNORE_TANGENCY:
            cc.SetIgnoreTangency(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_IS_CURVE_OWNED:
            // the flag only: the pool's curves are never deleted (see OP_SET_CURVE)
            cc.SetIsCurveOwned(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_OK_TO_SET_TOL:
            cc.SetOKToSetTolerance(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_FILLET_DATA:
        {
            void* oc = pool.get(rd.idx(), {K_OTHER}, ok);
            bool b = rd.flag();
            if (!ok)
                return SC_ERR_ARG;
            cc.SetFilletData((SmCurveClassification*)oc, b ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        }
        case OP_SET_TGT_LOOPS_ONLY:
            cc.SetTgtLoopsOnly(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_ADD_TGT_LOOP:
        {
            int64_t j = rd.idx();
            void* l = pool.get(j, {K_LOOP}, ok);
            if (!ok || j == -1)
                return SC_ERR_ARG;
            cc.AddTgtLoop((SmLoop*)l);
            o.push_back(0.0);
            break;
        }
        case OP_SET_SRC_TOL:
            SmTol::SetZoneTol3d(&cc, SmZoneTol3d(rd.num()));
            o.push_back(0.0);
            break;
        case OP_SET_UV_CURVE1:
        case OP_SET_UV_CURVE2:
        {
            SmCurve* c = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            if (op == OP_SET_UV_CURVE1)
                cc.SetUVCurve1(c);
            else
                cc.SetUVCurve2(c);
            o.push_back(0.0);
            break;
        }
        // ---- the part-4 members ------------------------------------------------------------------
        case OP_SQUEEZE:
        {
            // FindBestSqueezeEnd(0) reads m_pData[0]
            if (size == 0)
                return SC_ERR_ARG;
            o.push_back(st(sc_cc_p4::sm_SqueezeOutSmallestInterval(cc)));
            break;
        }
        case OP_IVL_BOUND:
        {
            int64_t j = rd.idx();
            bool do_min = rd.flag();
            if (rd.bad || j < 0 || j >= size)
                return SC_ERR_ARG;
            const SmCurveInterval& ivl = cc.GetAt((ULONG)j);
            // the interval reaches its classification through its back pointer (the port has
            // none: the interval is addressed in its classification)
            if (ivl.GetCurveClassification() != &cc)
                return SC_ERR_ARG;
            // UB-124: GetUpwardFaceuse() and GetSurface() are called on the face before its
            // NULL check
            const SmFace* face = cc.GetFace();
            if (face == NULL)
                return SC_ERR_ARG;
            const SmPointClassification& pc = do_min ? ivl.m_vStart : ivl.m_vEnd;
            SmEdge* edge = pc.GetEdgeObject();
            if (edge != NULL && face->GetSurface() != NULL && (cc.GetCurve() == NULL || edge->GetCurve() == NULL))
                return SC_ERR_ARG;
            SmEdgeuse* eu = ivl.GetIvlBound_EdgeuseObj(do_min ? TRUE : FALSE, NULL);
            o.push_back(0.0);
            o.push_back(pool.id(eu));
            break;
        }
        case OP_LOOP_CROSSINGS:
        case OP_LOOP_TOUCHES:
        {
            int64_t n = rd.idx();
            if (rd.bad || n < 0 || n > 8)
                return SC_ERR_ARG;
            SmTArray<SmLoop*> loops;
            for (int64_t i = 0; i < n; i++)
            {
                SmLoop* l = (SmLoop*)pool.get(rd.idx(), {K_LOOP}, ok);
                if (!ok || l == NULL)
                    return SC_ERR_ARG;
                loops.SetSize((ULONG)i + 1);
                loops[(ULONG)i] = l;
            }
            bool with_props = rd.flag();
            // GetPointClassifiesConnectedTo reads GetAt(0) unchecked: an empty classification
            // (e.g. after a squeeze) reaches it with loops whose first loop's face has a
            // surface, and a curve. (Refused even for a degenerate curve, which returns
            // before it.)
            if (n > 0 && cc.GetSize() == 0 && loops[0]->GetFace() != NULL
                && loops[0]->GetFace()->GetSurface() != NULL && cc.GetCurve() != NULL)
                return SC_ERR_ARG;
            // the face props seeds (`field idx value`, as the heal.face_props shim), applied to
            // the props passed: a preset at SM_PROPSTAGE_2a or later skips SetProps_Stage2
            int64_t n_seeds = rd.idx();
            if (rd.bad || n_seeds < 0 || n_seeds > 32)
                return SC_ERR_ARG;
            SmFaceProps props(&ctx);
            for (int64_t i = 0; i < n_seeds; i++)
            {
                int64_t field = rd.idx();
                int64_t idx = rd.idx();
                double v = rd.num();
                sc_one_face::seed(props, of.face, field, idx, v);
            }
            // UB-124: the classification curve's GetNaturalInterval() before its NULL check
            if (rd.bad || (n > 0 && cc.GetCurve() == NULL))
                return SC_ERR_ARG;
            // GetPointClassifiesConnectedTo reads m_pData[0]: an empty classification (left by
            // a squeeze of its only interval, or never sized) reads a removed or missing
            // interval
            if (n > 0 && size == 0)
                return SC_ERR_ARG;
            if (op == OP_LOOP_TOUCHES)
            {
                // GetLoopTouches (new in 1.0.1): on a failing status the list may hold the
                // touches collected before it, so only the status is compared then
                SmTArray<SmTouchData> touches;
                SmStatus s = cc.GetLoopTouches(loops, with_props ? &props : NULL, touches);
                o.push_back(st(s));
                if (s != SM_SUCCESS)
                    break;
                put_touches(o, touches, pool);
                if (with_props)
                    sc_one_face::snapshot(o, props, of.face);
                break;
            }
            SmTArray<ULONG> counts;
            SmTArray<SmTouchData> raw, done;
            SmStatus s = cc.GetLoopCrossings(loops, with_props ? &props : NULL, &counts, &raw, &done);
            o.push_back(st(s));
            o.push_back((double)counts.GetSize());
            for (ULONG i = 0; i < counts.GetSize(); i++)
                o.push_back((double)counts[i]);
            put_touches(o, raw, pool);
            put_touches(o, done, pool);
            if (with_props)
                sc_one_face::snapshot(o, props, of.face);
            break;
        }
        case OP_CONSTRUCT:
        {
            SmCurve* c = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            double a = rd.num();
            double b = rd.num();
            SmCurve* uv = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            double tol = rd.num();
            int64_t data_size = rd.idx();
            bool owned = rd.flag();
            bool current = rd.flag();
            SmSurface* s1 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmSurface* s2 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmCurve* uv2 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            if (rd.bad || !ok || data_size < 0 || data_size > 64)
                return SC_ERR_ARG;
            // the pool's curves are never deleted: disown before the old classification goes
            cc.*get(CcOwned()) = FALSE;
            cc.~SmCurveClassification();
            ::new (&cc) SmCurveClassification(c, SmExtent1d(a, b), uv, SmZoneTol3d(tol), (ULONG)data_size, NULL,
                                            owned ? TRUE : FALSE, current ? TRUE : FALSE, s1, s2, uv2, NULL);
            o.push_back(0.0);
            break;
        }

        default: return SC_ERR_ARG;
        }
        if (rd.bad)
            return SC_ERR_ARG;
        put_cc(o, cc, pool);
    }
    if (rd.bad || rd.at != rd.n)
        return SC_ERR_ARG;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_intersect_curve_class_p4_run(const double* in, int64_t n_in, double* out, int64_t cap,
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
