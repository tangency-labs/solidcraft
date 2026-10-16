// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.point_class`: SmPointClassification
// (source/SMLib/src/SmPointClass.cpp, source/SMLib/inc/SmPointClass.h), the
// SmTol::Get{Src,Obj}ZoneTol3d / GetXSectTol3d overloads of a point classification
// (source/SMLib/src/SmTol.cpp) and the SmSpaceBend / SmSpaceUnbend type queries.
//
// One entry point, sc_intersect_point_class_run(in, n_in, out, cap, out_len): a script
// over a pool of objects and two classifications pc[0], pc[1]. The layout is documented
// in crates/solidcore-ffi/src/units/intersect_point_class.rs, which validates the static
// shape; the run-time checks below reject (SC_ERR_ARG) the operations that would call a
// member of a stand-in object or of an SmPoint3d cast to SmObject (undefined behaviour
// upstream, never generated). `out` receives, per op, its status (0 = SM_SUCCESS, else
// the SmStatus), its outputs and the state of the target classification.
//
// Objects (kind code, data): 0 curve (curve block, as unit curve.curve_p6), 1 plane
// SmPlane(O, N), 2 volume*, 3 point P (an SmPoint3d), 4 vertex (an SmVertex), 5 edge
// (an SmEdge on pool curve `c`, -1 none), 6 face (an SmFace on pool surface `s`, -1
// none), 7 region*, 8 edgeuse*, 9 other* (* = SmObject stand-in answering GetType and
// IsKindOf for that class). Objects live for the whole process (never deleted:
// topology destructors notify owners).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmEllipse.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmSolutionArray.h>
#include <SmSpaceBend.h>
#include <SmSpaceUnbend.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <map>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace
{
enum : int64_t
{
    K_CURVE = 0,
    K_PLANE = 1,
    K_VOLUME = 2,
    K_POINT = 3,
    K_VERTEX = 4,
    K_EDGE = 5,
    K_FACE = 6,
    K_REGION = 7,
    K_EDGEUSE = 8,
    K_OTHER = 9,
};

// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_point_class.rs).
enum : int64_t
{
    OP_RESET = 0,
    OP_SET_CLASS_OBJECT = 1,
    OP_SET_T = 2,
    OP_SET_UV = 3,
    OP_SET_UVW = 4,
    OP_UNSET = 5,
    OP_SET_GAP = 6,
    OP_SET_PRESNAP = 7,
    OP_SET_SSS = 8,
    OP_SET_SRC_TOL = 9,
    OP_SET_FACE = 10,
    OP_ASSIGN = 11,
    OP_COPY_CLASS_ONLY = 12,
    OP_COPY_SSS = 13,
    OP_COMBINE = 14,
    OP_COMPUTE = 15,
    OP_FIND = 16,
    OP_GAP = 17,
    OP_LOAD = 18,
    OP_MERGE = 19,
    OP_BREP = 20,
    OP_CONNECTED = 21,
    OP_CONNECTED_ANY = 22,
    OP_CURVE_OBJECT = 23,
    OP_OBJ_TOL = 24,
    OP_XSECT_TOL = 25,
    OP_SRC_TOL = 26,
    OP_SET_DEVIATION = 27,
    OP_BEND_TYPES = 28,
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

// A curve block `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w) knots`;
// 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 3 || kind == 4)
    {
        // `c[3] x[3] y[3] d0 d1 rx [ry] io`: SmEllipse / SmCircle (no pOptNurb)
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
    std::map<const void*, int64_t> ids;

    int64_t kind_of(const void* p) const
    {
        auto it = ids.find(p);
        return it == ids.end() ? -1 : kinds[(size_t)it->second];
    }
    double id(const void* p) const
    {
        if (p == NULL)
            return -1.0;
        auto it = ids.find(p);
        return it == ids.end() ? -2.0 : (double)it->second;
    }
    SmObject* obj(int64_t i) const
    {
        if (i < 0 || i >= (int64_t)ptrs.size())
            return NULL;
        return (SmObject*)ptrs[(size_t)i];
    }
};

void put_state(std::vector<double>& o, const SmPointClassification& pc, const Pool& pool)
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
    o.push_back(pc.GetTolerance());
    o.push_back(pool.id(pc.GetVertexObject()));
    o.push_back(pool.id(pc.GetEdgeObject()));
    o.push_back(pool.id(pc.GetFaceObject()));
}

// Kind of the classification's object; -1 for none.
int64_t obj_kind(const SmPointClassification& pc, const Pool& pool)
{
    return pc.GetObject() == NULL ? -1 : pool.kind_of(pc.GetObject());
}

bool is_stand_in(int64_t k) { return k == K_VOLUME || k == K_REGION || k == K_EDGEUSE || k == K_OTHER; }

int32_t run(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Pool pool;

    int64_t n_obj = rd.idx();
    for (int64_t i = 0; i < n_obj && !rd.bad; i++)
    {
        int64_t kind = rd.idx();
        void* p = NULL;
        switch (kind)
        {
        case K_CURVE: p = read_curve(rd, ctx); break;
        case K_PLANE:
        {
            SmPoint3d org = rd.vec();
            SmVector3d nrm = rd.vec();
            p = new (ctx) SmPlane(org, nrm, &ctx);
            break;
        }
        case K_VOLUME: p = new (&ctx) ScStandIn(SmVolume_TYPE); break;
        case K_POINT: p = new SmPoint3d(rd.vec()); break;
        case K_VERTEX: p = new (anchor()) ScVertex(); break;
        case K_EDGE:
        {
            int64_t c = rd.idx();
            SmObject* co = pool.obj(c);
            if (c >= 0 && pool.kinds[(size_t)c] != K_CURVE)
                return SC_ERR_ARG;
            p = new (anchor()) ScEdge((SmCurve*)co);
            break;
        }
        case K_FACE:
        {
            int64_t s = rd.idx();
            SmObject* so = pool.obj(s);
            if (s >= 0 && pool.kinds[(size_t)s] != K_PLANE)
                return SC_ERR_ARG;
            p = new (anchor()) ScFace((SmSurface*)so);
            break;
        }
        case K_REGION: p = new (&ctx) ScStandIn(SmRegion_TYPE); break;
        case K_EDGEUSE: p = new (&ctx) ScStandIn(SmEdgeuse_TYPE); break;
        case K_OTHER: p = new (&ctx) ScStandIn(SmObject_TYPE + 7); break;
        default: return SC_ERR_ARG;
        }
        pool.ids[p] = (int64_t)pool.ptrs.size();
        pool.kinds.push_back(kind);
        pool.ptrs.push_back(p);
    }

    double tol0 = rd.num();
    double tol1 = rd.num();
    SmPointClassification pcs[2] = {SmPointClassification(SmZoneTol3d(tol0)),
                                    SmPointClassification(SmZoneTol3d(tol1))};

    int64_t n_ops = rd.idx();
    for (int64_t i = 0; i < n_ops && !rd.bad; i++)
    {
        int64_t op = rd.idx();
        int64_t t = rd.idx();
        if (t < 0 || t > 1)
            return SC_ERR_ARG;
        SmPointClassification& pc = pcs[t];
        SmPointClassification& other = pcs[1 - t];
        int64_t ok = obj_kind(pc, pool);
        o.push_back((double)op);
        switch (op)
        {
        case OP_RESET:
        {
            bool has = rd.flag();
            SmZoneTol3d tol(rd.num());
            pc.ReSet(has ? &tol : NULL);
            o.push_back(0.0);
            break;
        }
        case OP_SET_CLASS_OBJECT:
        {
            int64_t cls = rd.idx();
            int64_t obj = rd.idx();
            pc.SetClassObject((SmPointClassificationType)cls, (SmObject*)pool.obj(obj));
            o.push_back(0.0);
            break;
        }
        case OP_SET_T:
        {
            double v = rd.num();
            bool snap = rd.flag();
            pc.SetTParam(v, snap ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        }
        case OP_SET_UV:
        {
            double u = rd.num();
            double v = rd.num();
            pc.SetUVParam(SmPoint2d(u, v));
            o.push_back(0.0);
            break;
        }
        case OP_SET_UVW:
            pc.SetUVWParam(rd.vec());
            o.push_back(0.0);
            break;
        case OP_UNSET:
            pc.UnSetParameters();
            o.push_back(0.0);
            break;
        case OP_SET_GAP:
            pc.SetGap3d(rd.num());
            o.push_back(0.0);
            break;
        case OP_SET_DEVIATION:
            pc.SetDeviation(rd.num());
            o.push_back(0.0);
            break;
        case OP_SET_PRESNAP:
            pc.SetPreSnapParam(rd.num());
            o.push_back(0.0);
            break;
        case OP_SET_SSS:
            pc.SetSSSPoint(rd.vec());
            o.push_back(0.0);
            break;
        case OP_SET_SRC_TOL:
            pc.SetSrcZoneTol3d(SmZoneTol3d(rd.num()));
            o.push_back(0.0);
            break;
        case OP_SET_FACE:
            pc.SetFace((const SmFace*)pool.obj(rd.idx()));
            o.push_back(0.0);
            break;
        case OP_ASSIGN:
            pc = other;
            o.push_back(0.0);
            break;
        case OP_COPY_CLASS_ONLY:
            pc.CopyClassificationOnly(other);
            o.push_back(0.0);
            break;
        case OP_COPY_SSS:
            pc.CopySSSPoint(other);
            o.push_back(0.0);
            break;
        case OP_COMBINE:
        {
            // the SSS point goes to the vertex only for a real vertex
            if (pc.GetPointClass() == SM_PC_VERTEX && ok != K_VERTEX)
                return SC_ERR_ARG;
            const SmPointClassification& r = pc.Combine(other);
            o.push_back(0.0);
            o.push_back(&r == &pc ? 0.0 : 1.0);
            break;
        }
        case OP_COMPUTE:
        {
            SmPoint3d p = rd.vec();
            if (ok == K_POINT || is_stand_in(ok) || pc.GetPointClass() == SM_PC_EDGEUSE)
                return SC_ERR_ARG;
            o.push_back(st(pc.ComputePointParameters(p)));
            break;
        }
        case OP_FIND:
        {
            if (pc.GetPointClass() == SM_PC_EDGEUSE || pc.GetPointClass() == SM_PC_VOLUME)
                return SC_ERR_ARG;
            SmPoint3d p;
            SmStatus s = pc.FindObjPoint3d(p);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                put_vec(o, p);
            break;
        }
        case OP_GAP:
        {
            int64_t s = rd.idx();
            if (s >= 0 && pool.kinds[(size_t)s] != K_PLANE)
                return SC_ERR_ARG;
            if (s >= 0 && (ok == K_POINT || pc.GetPointClass() == SM_PC_EDGEUSE ||
                           pc.GetPointClass() == SM_PC_VOLUME))
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(pc.GetGap3d((const SmSurface*)pool.obj(s)));
            break;
        }
        case OP_LOAD:
        {
            double tol = rd.num();
            int64_t obj = rd.idx();
            SmSolutionEnd end;
            end.m_dSolutionValue = rd.num();
            for (int k = 0; k < 10; k++)
                end[(ULONG)k] = rd.num();
            ULONG off = (ULONG)rd.idx();
            double pre = rd.num();
            SmObject* po = pool.obj(obj);
            if (po == NULL || pool.kinds[(size_t)obj] == K_POINT)
                return SC_ERR_ARG;
            o.push_back(st(pc.LoadFromIntersection(SmZoneTol3d(tol), po, end, off, pre)));
            break;
        }
        case OP_MERGE:
        {
            SmPoint3d p = rd.vec();
            if (ok == K_POINT || ok == K_REGION || ok == K_FACE || ok == K_EDGE)
                return SC_ERR_ARG;
            SmTArray<SmEdge*> edges;
            SmTArray<SmVertex*> verts;
            SmStatus s = pc.MergeIntoObject(p, &edges, &verts);
            o.push_back(st(s));
            o.push_back((double)edges.GetSize());
            o.push_back((double)verts.GetSize());
            break;
        }
        case OP_BREP:
        {
            if (pc.GetPointClass() == SM_PC_EDGEUSE)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(pc.GetBrep() == NULL ? 0.0 : 1.0);
            break;
        }
        case OP_CONNECTED:
        case OP_CONNECTED_ANY:
        {
            int64_t cnt = op == OP_CONNECTED ? 1 : rd.idx();
            SmTArray<SmTopology*> tgts;
            for (int64_t k = 0; k < cnt; k++)
            {
                int64_t j = rd.idx();
                if (j < 0 || j >= (int64_t)pool.ptrs.size())
                    return SC_ERR_ARG;
                int64_t kj = pool.kinds[(size_t)j];
                if (kj != K_VERTEX && kj != K_EDGE && kj != K_FACE)
                    return SC_ERR_ARG;
                tgts.Add((SmTopology*)pool.ptrs[(size_t)j]);
            }
            if (ok == K_POINT || ok == K_EDGEUSE)
                return SC_ERR_ARG;
            o.push_back(0.0);
            if (op == OP_CONNECTED)
                o.push_back(flg(pc.IsConnectedTo(tgts[0])));
            else
            {
                ULONG found = 0;
                SmBoolean r = pc.IsConnectedTo(tgts, found);
                o.push_back(flg(r));
                o.push_back(found == SM_UNDEF_ULONG ? -1.0 : (double)found);
            }
            break;
        }
        case OP_CURVE_OBJECT:
            if (pc.GetPointClass() == SM_PC_EDGEUSE)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(pool.id(pc.GetCurveObject()));
            break;
        case OP_OBJ_TOL:
        case OP_XSECT_TOL:
            if (ok == K_POINT)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(op == OP_OBJ_TOL ? (double)SmTol::GetObjZoneTol3d(&pc) : (double)SmTol::GetXSectTol3d(&pc));
            break;
        case OP_SRC_TOL:
            o.push_back(0.0);
            o.push_back(SmTol::GetSrcZoneTol3d(&pc));
            break;
        case OP_BEND_TYPES:
        {
            // the static class queries return what the virtual GetType / GetTypeString
            // return on an instance (SM_COMMON)
            o.push_back(0.0);
            o.push_back((double)SmSpaceBend::GetClassType());
            o.push_back((double)SmSpaceUnbend::GetClassType());
            for (const TCHAR* str : {SmSpaceBend::GetClassTypeString(), SmSpaceUnbend::GetClassTypeString()})
            {
                std::vector<double> cs;
                for (const TCHAR* c = str; *c; c++)
                    cs.push_back((double)*c);
                o.push_back((double)cs.size());
                o.insert(o.end(), cs.begin(), cs.end());
            }
            break;
        }
        default: return SC_ERR_ARG;
        }
        put_state(o, pc, pool);
    }
    if (rd.bad || rd.at != rd.n)
        return SC_ERR_ARG;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_intersect_point_class_run(const double* in, int64_t n_in, double* out, int64_t cap,
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
