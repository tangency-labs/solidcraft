// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.tol`.
// Wraps upstream SmTol (source/SMLib/inc/SmTol.h, source/SMLib/src/SmTol.cpp) and
// SmRelation (source/SMLib/inc/SmRelation.h).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <nurbs.h>

#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmRelation.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

#include <algorithm>
#include <stdexcept>
#include <vector>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_tol.rs).
enum ScTolOp : int32_t
{
    OP_SIZE_ESTIMATE = 0,      // SizeEstimateToZoneTol3d(a0)
    OP_IN_TOL = 1,             // InTol(a0, SmScaledZero(a1))
    OP_CALC_FACE_ZONE = 2,     // CalcFaceZoneTol3d()
    OP_CALC_EDGE_ZONE = 3,     // CalcEdgeZoneTol3d(a0, a1, flag ? &SmZoneTol3d(a2) : NULL)
    OP_CALC_VERTEX_ZONE = 4,   // CalcVertexZoneTol3d(a0, a1, flag ? &SmZoneTol3d(a2) : NULL)
    OP_EFF_ZEROS = 5,          // GetEffZero, GetEffZeroSq, GetEffZeroParam, GetEffZeroAngRad
    OP_XSECT_FROM_ZONES = 6,   // GetXSectTol3d(SmZoneTol3d(a0), SmZoneTol3d(a1))
    OP_ANG_TOL_RAD = 7,        // GetAngTolRad()
    OP_MODEL_SIZE = 8,         // GetModelSizeEstimate()
    OP_ARE_PARALLEL = 9,       // AreParallel(v(a0..a2), v(a3..a5), flag ? &a6 : NULL)
    OP_ARE_PERPENDICULAR = 10, // ArePerpendicular(..)
    OP_IS_ZERO_SPEED = 11,     // IsZeroSpeed(a0, a1, flag ? &SmScaledZero(a2) : NULL)
    OP_IS_DEGENERATE_V3 = 12,  // IsDegenerate(v(a0..a2), flag ? &SmXSectTol3d(a3) : NULL)
    OP_GAP_GAIN = 13,          // GetGapGain()
    OP_ZONE_SYS = 14,          // GetZoneTol3d(small)
    OP_ZONE_CTX = 15,          // GetZoneTol3d((const SmContext*)NULL, small)
    OP_APPROX_SYS = 16,        // GetApproxTol3d(small)
    OP_APPROX_CTX = 17,        // GetApproxTol3d((const SmContext*)NULL, small)
    OP_XSECT_SYS = 18,         // GetXSectTol3d(small)
    OP_XSECT_CTX = 19,         // GetXSectTol3d((const SmContext*)NULL, small)
    OP_SCALED_ZERO_SYS = 20,   // GetScaledZero(), GetScaledZeroSq(), GetScaledZeroSqrt()
    OP_SCALED_ZERO_N = 21,     // GetScaledZero{,Sq,Sqrt}(o1[, o2[, o3[, o4]]]) (n = small)
    OP_TOL_CTORS = 22,         // all tolerance constructors and operator=(double) of a0
    OP_TOL_MUL = 23,           // SmTol3d t(a0); t *= a1
    OP_MAP_TO_1D = 24,         // MapTo1d(SmTol3d(a0), v(a1..a3), SmExtent1d(a4, a5))
};

// Object kinds for OP_SCALED_ZERO_N (6 doubles each in `objs`).
enum ScTolKind : int32_t
{
    KIND_DOUBLE = 0,  // d[0]
    KIND_VEC2 = 1,    // SmVector2d(d[0], d[1])
    KIND_VEC3 = 2,    // SmVector3d(d[0], d[1], d[2])
    KIND_EXTENT3 = 3, // SmExtent3d(d[0..6])
    KIND_EXTENT1 = 4, // SmExtent1d(d[0], d[1])
    KIND_EXTENT2 = 5, // SmExtent2d(d[0], d[1], d[2], d[3])
};

namespace
{

// Calls f with the upstream object of kind `kind` built from d[0..6].
template <class F> void sc_with_obj(int32_t kind, const double* d, F f)
{
    switch (kind)
    {
    case KIND_VEC2: f(SmVector2d(d[0], d[1])); break;
    case KIND_VEC3: f(SmVector3d(d[0], d[1], d[2])); break;
    case KIND_EXTENT3: f(SmExtent3d(d[0], d[1], d[2], d[3], d[4], d[5])); break;
    case KIND_EXTENT1: f(SmExtent1d(d[0], d[1])); break;
    case KIND_EXTENT2: f(SmExtent2d(d[0], d[1], d[2], d[3])); break;
    default: f(d[0]); break;
    }
}

// Sets the system tolerance statics, restoring the defaults on scope exit.
struct ScSystemTol
{
    ScSystemTol(double dModelSize, double dRatio)
    {
        SmTol::SetModelSizeEstimate(dModelSize);
        SmTol::SetLargeSmallSizeRatio(dRatio);
    }
    ~ScSystemTol()
    {
        SmTol::SetModelSizeEstimate(SM_USE_DEFAULT);
        SmTol::SetLargeSmallSizeRatio(SM_USE_DEFAULT);
    }
};

} // namespace

// Runs one SmTol operation with the system tolerances set from (model_size, ratio).
//   a[8]   : scalar operands
//   flag   : optional-argument switch (non-zero = pass the pointer)
//   small  : SmSmallTopoType (0..4), or the object count for OP_SCALED_ZERO_N (1..4)
//   kinds[4], objs[24] : objects of OP_SCALED_ZERO_N
//   out[16]: result values; *out_n receives how many were written
SC_EXPORT int32_t sc_geom_tol_op(int32_t op, double model_size, double ratio, const double* a,
                                 int32_t flag, int32_t small, const int32_t* kinds,
                                 const double* objs, double* out, int32_t* out_n)
{
    SC_TRY
    ScSystemTol sSys(model_size, ratio);
    SmSmallTopoType eSmall = (SmSmallTopoType)small;
    int32_t n = 0;
    auto put = [&](double v) { out[n++] = v; };
    switch (op)
    {
    case OP_SIZE_ESTIMATE: put(SmTol::SizeEstimateToZoneTol3d(a[0])); break;
    case OP_IN_TOL:
    {
        SmScaledZero sTol(a[1]);
        put(SmTol::InTol(a[0], sTol) ? 1.0 : 0.0);
        break;
    }
    case OP_CALC_FACE_ZONE: put(SmTol::CalcFaceZoneTol3d()); break;
    case OP_CALC_EDGE_ZONE:
    {
        SmZoneTol3d sMax(a[2]);
        put(SmTol::CalcEdgeZoneTol3d(a[0], a[1], flag ? &sMax : (SmZoneTol3d*)NULL));
        break;
    }
    case OP_CALC_VERTEX_ZONE:
    {
        SmZoneTol3d sMax(a[2]);
        put(SmTol::CalcVertexZoneTol3d(a[0], a[1], flag ? &sMax : (SmZoneTol3d*)NULL));
        break;
    }
    case OP_EFF_ZEROS:
        put(SmTol::GetEffZero());
        put(SmTol::GetEffZeroSq());
        put(SmTol::GetEffZeroParam());
        put(SmTol::GetEffZeroAngRad());
        break;
    case OP_XSECT_FROM_ZONES:
    {
        SmZoneTol3d s1(a[0]), s2(a[1]);
        put(SmTol::GetXSectTol3d(s1, s2));
        break;
    }
    case OP_ANG_TOL_RAD: put(SmTol::GetAngTolRad()); break;
    case OP_MODEL_SIZE: put(SmTol::GetModelSizeEstimate()); break;
    case OP_ARE_PARALLEL:
    case OP_ARE_PERPENDICULAR:
    {
        SmVector3d v1(a[0], a[1], a[2]), v2(a[3], a[4], a[5]);
        double dAng = a[6];
        double* pAng = flag ? &dAng : NULL;
        SmBoolean b = op == OP_ARE_PARALLEL ? SmTol::AreParallel(&v1, &v2, pAng)
                                            : SmTol::ArePerpendicular(&v1, &v2, pAng);
        put(b ? 1.0 : 0.0);
        break;
    }
    case OP_IS_ZERO_SPEED:
    {
        SmScaledZero sZero(a[2]);
        put(SmTol::IsZeroSpeed(a[0], a[1], flag ? &sZero : NULL) ? 1.0 : 0.0);
        break;
    }
    case OP_IS_DEGENERATE_V3:
    {
        SmVector3d v(a[0], a[1], a[2]);
        SmXSectTol3d sTol(a[3]);
        put(SmTol::IsDegenerate(v, flag ? &sTol : NULL) ? 1.0 : 0.0);
        break;
    }
    case OP_GAP_GAIN: put(SmTol::GetGapGain()); break;
    case OP_ZONE_SYS: put(SmTol::GetZoneTol3d(eSmall)); break;
    case OP_ZONE_CTX: put(SmTol::GetZoneTol3d((const SmContext*)NULL, eSmall)); break;
    case OP_APPROX_SYS: put(SmTol::GetApproxTol3d(eSmall)); break;
    case OP_APPROX_CTX: put(SmTol::GetApproxTol3d((const SmContext*)NULL, eSmall)); break;
    case OP_XSECT_SYS: put(SmTol::GetXSectTol3d(eSmall)); break;
    case OP_XSECT_CTX: put(SmTol::GetXSectTol3d((const SmContext*)NULL, eSmall)); break;
    case OP_SCALED_ZERO_SYS:
        put(SmTol::GetScaledZero());
        put(SmTol::GetScaledZeroSq());
        put(SmTol::GetScaledZeroSqrt());
        break;
    case OP_SCALED_ZERO_N:
    {
        const double* d0 = objs;
        const double* d1 = objs + 6;
        const double* d2 = objs + 12;
        const double* d3 = objs + 18;
        if (small == 1)
        {
            sc_with_obj(kinds[0], d0, [&](const auto& o0) {
                put(SmTol::GetScaledZero(o0));
                put(SmTol::GetScaledZeroSq(o0));
                put(SmTol::GetScaledZeroSqrt(o0));
            });
        }
        else if (small == 2)
        {
            sc_with_obj(kinds[0], d0, [&](const auto& o0) {
                sc_with_obj(kinds[1], d1, [&](const auto& o1) {
                    put(SmTol::GetScaledZero(o0, o1));
                    put(SmTol::GetScaledZeroSq(o0, o1));
                    put(SmTol::GetScaledZeroSqrt(o0, o1));
                });
            });
        }
        else if (small == 3)
        {
            sc_with_obj(kinds[0], d0, [&](const auto& o0) {
                sc_with_obj(kinds[1], d1, [&](const auto& o1) {
                    sc_with_obj(kinds[2], d2, [&](const auto& o2) {
                        put(SmTol::GetScaledZero(o0, o1, o2));
                        put(SmTol::GetScaledZeroSq(o0, o1, o2));
                        put(SmTol::GetScaledZeroSqrt(o0, o1, o2));
                    });
                });
            });
        }
        else if (small == 4)
        {
            sc_with_obj(kinds[0], d0, [&](const auto& o0) {
                sc_with_obj(kinds[1], d1, [&](const auto& o1) {
                    sc_with_obj(kinds[2], d2, [&](const auto& o2) {
                        sc_with_obj(kinds[3], d3, [&](const auto& o3) {
                            put(SmTol::GetScaledZero(o0, o1, o2, o3));
                            put(SmTol::GetScaledZeroSq(o0, o1, o2, o3));
                            put(SmTol::GetScaledZeroSqrt(o0, o1, o2, o3));
                        });
                    });
                });
            });
        }
        else
            return SC_ERR_ARG;
        break;
    }
    case OP_MAP_TO_1D:
    {
        SmTol3d sTol(a[0]);
        SmVector3d sDeriv(a[1], a[2], a[3]);
        const SmExtent1d sIvl(a[4], a[5]);
        put(SmTol::MapTo1d(sTol, sDeriv, sIvl));
        break;
    }
    case OP_TOL_CTORS:
    {
        SmTol3d t3(a[0]);
        SmZoneTol3d tz(a[0]);
        SmXSectTol3d tx(a[0]);
        SmApproxTol3d ta(a[0]);
        SmScaledZero ts(a[0]);
        SmTol2d t2(a[0]);
        SmTol1d t1(a[0]);
        put(t3);
        put(tz);
        put(tx);
        put(ta);
        put(ts);
        put(t2);
        put(t1);
        // operator=(double) of SM_TOL_CALLS; SmTol3d::operator=(const SmTol3d)
        SmZoneTol3d az(1.0);
        SmXSectTol3d ax(1.0);
        SmApproxTol3d aa(1.0);
        SmScaledZero as(1.0);
        SmTol2d a2(1.0);
        SmTol1d a1(1.0);
        SmTol3d a3(1.0);
        az = a[0];
        ax = a[0];
        aa = a[0];
        as = a[0];
        a2 = a[0];
        a1 = a[0];
        a3 = tz;
        put(az);
        put(ax);
        put(aa);
        put(as);
        put(a2);
        put(a1);
        put(a3);
        break;
    }
    case OP_TOL_MUL:
    {
        SmTol3d t(a[0]);
        t *= a[1];
        put(t);
        break;
    }
    default: return SC_ERR_ARG;
    }
    *out_n = n;
    return SC_OK;
    SC_CATCH
}

// Runs a script of SmRelation<int, int> operations on objects first[0..n1) and
// second[0..n2) (identified by index) with cardinality `card`.
//   script: [0, f, s] = RelatePair, [1, f, k, s1..sk] = RelateToMany, [2] = RemoveAll
// Output (int64): per script op its status; then for every first f:
//   GetSecond(f) index (-1 = NULL), count, sorted GetSeconds(f);
// then count + sorted GetAllFirsts, count + sorted GetAllSeconds.
// (upstream returns these in hash order, which is unspecified; they are sorted here.)
SC_EXPORT int32_t sc_geom_tol_relation(int32_t card, int64_t n1, int64_t n2, const int64_t* script,
                                       int64_t n_script, int64_t* out, int64_t cap,
                                       int64_t* out_len)
{
    if (card < 0 || card > 3 || n1 < 0 || n2 < 0 || n_script < 0)
        return SC_ERR_ARG;
    SC_TRY
    static SmContext* s_pContext = new SmContext();
    std::vector<int> vFirsts((size_t)n1 + 1), vSeconds((size_t)n2 + 1);
    auto f_idx = [&](int* p) { return p ? (int64_t)(p - vFirsts.data()) : (int64_t)-1; };
    auto s_idx = [&](int* p) { return p ? (int64_t)(p - vSeconds.data()) : (int64_t)-1; };
    SmRelation<int, int> sRel(*s_pContext, (SmRelationCardinalityType)card);
    std::vector<int64_t> res;
    int64_t i = 0;
    while (i < n_script)
    {
        int64_t kind = script[i];
        if (kind == 0)
        {
            if (i + 3 > n_script) return SC_ERR_ARG;
            int64_t f = script[i + 1], s = script[i + 2];
            if (f < 0 || f >= n1 || s < 0 || s >= n2) return SC_ERR_ARG;
            res.push_back(sRel.RelatePair(&vFirsts[f], &vSeconds[s]));
            i += 3;
        }
        else if (kind == 1)
        {
            if (i + 3 > n_script) return SC_ERR_ARG;
            int64_t f = script[i + 1], k = script[i + 2];
            if (f < 0 || f >= n1 || k < 0 || i + 3 + k > n_script) return SC_ERR_ARG;
            SmTArray<int*> sSeconds;
            for (int64_t j = 0; j < k; ++j)
            {
                int64_t s = script[i + 3 + j];
                if (s < 0 || s >= n2) return SC_ERR_ARG;
                sSeconds.Add(&vSeconds[s]);
            }
            res.push_back(sRel.RelateToMany(&vFirsts[f], sSeconds));
            i += 3 + k;
        }
        else if (kind == 2)
        {
            sRel.RemoveAll();
            res.push_back(0);
            i += 1;
        }
        else
            return SC_ERR_ARG;
    }
    auto push_sorted = [&](std::vector<int64_t> v) {
        std::sort(v.begin(), v.end());
        res.push_back((int64_t)v.size());
        res.insert(res.end(), v.begin(), v.end());
    };
    for (int64_t f = 0; f < n1; ++f)
    {
        res.push_back(s_idx(sRel.GetSecond(&vFirsts[f])));
        SmTArray<int*> sSeconds;
        sRel.GetSeconds(&vFirsts[f], sSeconds);
        std::vector<int64_t> v;
        for (ULONG j = 0; j < sSeconds.GetSize(); ++j) v.push_back(s_idx(sSeconds[j]));
        push_sorted(v);
    }
    {
        SmTArray<int*> sAll;
        sRel.GetAllFirsts(sAll);
        std::vector<int64_t> v;
        for (ULONG j = 0; j < sAll.GetSize(); ++j) v.push_back(f_idx(sAll[j]));
        push_sorted(v);
    }
    {
        SmTArray<int*> sAll;
        sRel.GetAllSeconds(sAll);
        std::vector<int64_t> v;
        for (ULONG j = 0; j < sAll.GetSize(); ++j) v.push_back(s_idx(sAll[j]));
        push_sorted(v);
    }
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

// ---- tolerances of a curve ---------------------------------------------------------
//
// sc_geom_tol_curve(in, n_in, out, cap, out_len): one real SmLine / SmCircle /
// SmBSplineCurve and, on it, GetScaledZero(const SmCurve&), MapTo1d(SmTol3d, dParam,
// const SmCurve&) and IsDegenerate(const SmCurve*, SmExtent1d, [&xsect]). Input:
//   curve block: `0 a[3] b[3]` SmLine | `1 c[3] x[3] y[3] d0 d1 r` SmCircle |
//                `2 p ncp cpts(x y z w) knots` SmBSplineCurve (dim 3)
//   then: param tol ivl0 ivl1 has_xsect xsect_tol
// Output: scaled_zero map_to_1d is_degenerate (1 or 0).
namespace
{
struct ScBad : std::runtime_error
{
    ScBad() : std::runtime_error("bad program") {}
};

struct ScRd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw ScBad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw ScBad();
        return (int64_t)d;
    }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

SmCurve* sc_read_curve(ScRd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 2);
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    if (kind == 1)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    int64_t p = rd.idx(1, 8);
    int64_t ncp = rd.idx(2, 64);
    if (p >= ncp)
        throw ScBad();
    int64_t n = ncp - 1;
    int64_t m = n + p + 1;
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * ncp + m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)n, (gw_DEGREE)p, (gw_INDEX)m);
    for (int64_t i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[4 * i];
        cur->pol->Pw[i].y = v[4 * i + 1];
        cur->pol->Pw[i].z = v[4 * i + 2];
        cur->pol->Pw[i].w = v[4 * i + 3];
    }
    for (int64_t i = 0; i <= m; i++)
        cur->knt->U[i] = v[4 * ncp + i];
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}
} // namespace

SC_EXPORT int32_t sc_geom_tol_curve(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 1 || out_len == nullptr || (out == nullptr && cap > 0))
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    ScRd rd{in, n_in, 0};
    std::vector<double> o;
    SmCurve* c = NULL;
    try
    {
        c = sc_read_curve(rd, ctx);
        double param = rd.num();
        double tol = rd.num();
        double i0 = rd.num();
        double i1 = rd.num();
        int64_t has_x = rd.idx(0, 1);
        double xs = rd.num();
        if (rd.at != n_in)
        {
            delete c;
            return SC_ERR_ARG;
        }
        SmScaledZero sz = SmTol::GetScaledZero(*c);
        SmTol3d t3(tol);
        SmTol1d t1 = SmTol::MapTo1d(t3, param, *c);
        SmXSectTol3d xt(xs);
        SmBoolean deg = SmTol::IsDegenerate(c, SmExtent1d(i0, i1), has_x ? &xt : NULL);
        o.push_back((double)sz);
        o.push_back((double)t1);
        o.push_back(deg ? 1.0 : 0.0);
    }
    catch (const ScBad&)
    {
        delete c;
        return SC_ERR_ARG;
    }
    delete c;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
