// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.geom`.
// Wraps the NLib Euclidean geometry routines of upstream source/SMLib/src/NL_Geom.cpp
// and the polygon / net routines of NL_FrameBasic.cpp owned by the unit.
//
// One entry point, sc_nlib_geom_eval(op, in, n_in, out, cap, out_len): `op` selects
// a group of upstream calls, `in` holds its inputs, `out` receives a flat record of
// doubles (layouts in crates/difftest/src/units/nlib_geom.rs). Statuses of the
// upstream calls are written into the record as doubles: 0 = ok, else NL_ERROR.eno
// (cleared before each call; 1000 if the flag was set without a number). Outputs
// upstream leaves unwritten are pre-set to NaN.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <cmath>
#include <limits>
#include <vector>

#include "sc_nlib.h"

using sc::nl_status;

namespace
{
const double kNaN = std::numeric_limits<double>::quiet_NaN();

NL_POINT pt(const double* d) { return NL_POINT{d[0], d[1], d[2]}; }
NL_CPOINT cpt(const double* d) { return NL_CPOINT{d[0], d[1], d[2], d[3]}; }

void push_pt(std::vector<double>& o, const NL_POINT& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

void push_cpt(std::vector<double>& o, const NL_CPOINT& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
    o.push_back(p.w);
}

void push_box(std::vector<double>& o, NL_MINMAXBOX* box)
{
    NL_REAL xl, xr, yb, yt, zn, zf, diag;
    N_GetBBoxData(box, &xl, &xr, &yb, &yt, &zn, &zf);
    N_BBoxGetDiagonal(box, &diag);
    o.push_back(xl);
    o.push_back(xr);
    o.push_back(yb);
    o.push_back(yt);
    o.push_back(zn);
    o.push_back(zf);
    o.push_back(diag);
}

// Line from in[0..7]: P[3] V[3] bounded.
NL_LINESEG line(const double* d)
{
    NL_LINESEG l;
    N_CreateLineStartDirVector(&l, pt(d), pt(d + 3), d[6] != 0.0 ? NL_TRUE : NL_FALSE);
    return l;
}

// Plane from in[0..6]: P[3] N[3].
NL_PLANE plane(const double* d)
{
    NL_PLANE p;
    N_CreatePlanePtNormal(&p, pt(d), pt(d + 3));
    return p;
}

// 4x4 matrix entries row-major.
void push_mat4(std::vector<double>& o, NL_RMATRIX* rm)
{
    NL_INDEX n, m, bw;
    NL_REAL** RM;
    NL_MATRIXTYPE mtp;
    N_GetRealMatrixData(rm, &n, &m, &RM, &mtp, &bw);
    for (int i = 0; i < 4; ++i)
        for (int j = 0; j < 4; ++j)
            o.push_back(RM[i][j]);
}

void push_nan(std::vector<double>& o, int k)
{
    for (int i = 0; i < k; ++i)
        o.push_back(kNaN);
}

// op 0: vector arithmetic and structure accessors.
// in: a[3] b[3] alpha beta pw[4] qw[4]  (16)
void vector_ops(const double* in, std::vector<double>& o)
{
    NL_VECTOR a = pt(in), b = pt(in + 3), c;
    double alpha = in[6], beta = in[7];
    NL_CPOINT pw = cpt(in + 8), qw = cpt(in + 12);
    double s;

    N_VectorDot(a, b, &s);
    o.push_back(s);
    N_VectorDotRef(&a, &b, &s);
    o.push_back(s);
    N_VectorCross(a, b, &c);
    push_pt(o, c);
    N_VectorCrossRef(&a, &b, &c);
    push_pt(o, c);
    N_VectorScale(a, alpha, &c);
    push_pt(o, c);
    N_VectorScaleRef(&a, alpha, &c);
    push_pt(o, c);
    N_VectorReverse(a, &c);
    push_pt(o, c);
    c = a;
    N_VectorReverseInPlace(&c);
    push_pt(o, c);
    N_VectorSum(a, b, &c);
    push_pt(o, c);
    N_VectorCombine(alpha, a, beta, b, &c);
    push_pt(o, c);
    N_VectorDiff(a, b, &c);
    push_pt(o, c);
    N_VectorDiffCPts(pw, qw, &c);
    push_pt(o, c);
    N_VectorDir(a, b, &c);
    push_pt(o, c);
    N_VectorCreate(alpha, beta, a.z, &c);
    push_pt(o, c);
    N_VectorCopy(b, &c);
    push_pt(o, c);
    N_VectorMagnitude(a, &s);
    o.push_back(s);
    N_VectorMagnitudeRef(&b, &s);
    o.push_back(s);
    N_VectorPtAlongVector(a, alpha, b, &c);
    push_pt(o, c);

    // N_VectorNormalize: status, vector, magnitude (NaN on error).
    {
        N_ErrClear();
        NL_FLAG e = N_VectorNormalize(a, &c, &s);
        o.push_back(nl_status(e));
        if (e == NL_NO)
        {
            push_pt(o, c);
            o.push_back(s);
        }
        else
            push_nan(o, 4);
    }
    // N_VectorNormalizeRef on b.
    {
        c = b;
        N_ErrClear();
        NL_FLAG e = N_VectorNormalizeRef(&c);
        o.push_back(nl_status(e));
        if (e == NL_NO)
            push_pt(o, c);
        else
            push_nan(o, 3);
    }
    {
        N_ErrClear();
        s = kNaN;
        NL_FLAG e = N_VectorsCosAngle(a, b, &s);
        o.push_back(nl_status(e));
        o.push_back(e == NL_NO ? s : kNaN);
    }
    {
        N_ErrClear();
        s = kNaN;
        NL_FLAG e = N_VectorsAngle(a, b, &s);
        o.push_back(nl_status(e));
        o.push_back(e == NL_NO ? s : kNaN);
    }

    N_DistPtPt(a, b, &s);
    o.push_back(s);
    N_DistSqPtPt(a, b, &s);
    o.push_back(s);
    o.push_back((double)N_PtsAreEqual(a, b));
    o.push_back((double)N_PtsAreEqual(a, a));
    N_DistCptCpt(pw, qw, &s);
    o.push_back(s);
    N_DistCptCptHomo(pw, qw, &s);
    o.push_back(s);
    N_DistSignedPtPlane(a.x, a.y, a.z, alpha, b, &s);
    o.push_back(s);

    // Structures.
    {
        NL_INTERVAL iv;
        NL_REAL ul, ur;
        N_CreateInterval(&iv, alpha, beta);
        N_IntervalGetData(&iv, &ul, &ur);
        o.push_back(ul);
        o.push_back(ur);
    }
    {
        NL_RECTANGLE r;
        NL_REAL ul, ur, vb, vt;
        N_CreateRectangle(&r, a.x, a.y, b.x, b.y);
        N_RectangleGetData(&r, &ul, &ur, &vb, &vt);
        o.push_back(ul);
        o.push_back(ur);
        o.push_back(vb);
        o.push_back(vt);
    }
    {
        NL_MINMAXBOX box;
        N_BBoxDefine(&box, a.x, b.x, a.y, b.y, a.z, b.z);
        push_box(o, &box);
    }
    {
        NL_LINESEG l;
        NL_POINT P;
        NL_VECTOR V;
        NL_FLAG bd;
        N_CreateLineStartDirVector(&l, a, b, NL_TRUE);
        N_LineSetBoundingFlag(&l, NL_FALSE);
        N_LineGetData(&l, &P, &V, &bd);
        push_pt(o, P);
        push_pt(o, V);
        o.push_back((double)bd);
    }
    {
        NL_PLANE pl;
        NL_POINT P;
        NL_VECTOR N, N2;
        N_CreatePlanePtNormal(&pl, a, b);
        N_PlaneGetData(&pl, &P, &N);
        N_PlaneGetNormal(&pl, &N2);
        push_pt(o, P);
        push_pt(o, N);
        push_pt(o, N2);
    }
}

// op 1: N_IsectLineLine. in: line1[7] line2[7]. out: status flg P[3] t1 t2.
void isect_ll(const double* in, std::vector<double>& o)
{
    NL_LINESEG l1 = line(in), l2 = line(in + 7);
    NL_POINT P{kNaN, kNaN, kNaN};
    NL_REAL t1 = kNaN, t2 = kNaN;
    NL_FLAG flg = -1;
    N_ErrClear();
    NL_FLAG e = N_IsectLineLine(l1, l2, &P, &t1, &t2, &flg);
    o.push_back(nl_status(e));
    if (e != NL_NO)
    {
        push_nan(o, 6);
        return;
    }
    o.push_back((double)flg);
    push_pt(o, P);
    o.push_back(t1);
    o.push_back(t2);
}

// Line/plane style result: status flg P[3] t.
void push_lp(std::vector<double>& o, NL_FLAG e, NL_FLAG flg, const NL_POINT& P, NL_REAL t)
{
    o.push_back(nl_status(e));
    if (e != NL_NO)
    {
        push_nan(o, 5);
        return;
    }
    o.push_back((double)flg);
    push_pt(o, P);
    o.push_back(t);
}

// op 2: N_IsectLinePlane. in: line[7] plane[6].
void isect_lp(const double* in, std::vector<double>& o)
{
    NL_LINESEG l = line(in);
    NL_PLANE pl = plane(in + 7);
    NL_POINT P{kNaN, kNaN, kNaN};
    NL_REAL t = kNaN;
    NL_FLAG flg = -1;
    N_ErrClear();
    NL_FLAG e = N_IsectLinePlane(l, pl, &P, &t, &flg);
    push_lp(o, e, flg, P, t);
}

// op 3: N_ProjectPtLine. in: line[7] P[3].
void project_pl(const double* in, std::vector<double>& o)
{
    NL_LINESEG l = line(in);
    NL_POINT Q{kNaN, kNaN, kNaN};
    NL_REAL t = kNaN;
    NL_FLAG flg = -1;
    N_ErrClear();
    NL_FLAG e = N_ProjectPtLine(l, pt(in + 7), &Q, &t, &flg);
    push_lp(o, e, flg, Q, t);
}

// op 4: N_ProjectPtPlane, N_DistPtInfLine (line = P along N), N_PlanePtNormalToImplicit.
// in: plane[6] P[3] bounded. out: status Q[3] | status d | status a b c d.
void plane_ops(const double* in, std::vector<double>& o)
{
    NL_PLANE pl = plane(in);
    NL_POINT P = pt(in + 6);
    {
        NL_POINT Q{kNaN, kNaN, kNaN};
        N_ErrClear();
        NL_FLAG e = N_ProjectPtPlane(pl, P, &Q);
        o.push_back(nl_status(e));
        if (e == NL_NO)
            push_pt(o, Q);
        else
            push_nan(o, 3);
    }
    {
        NL_LINESEG l;
        N_CreateLineStartDirVector(&l, pt(in), pt(in + 3), in[9] != 0.0 ? NL_TRUE : NL_FALSE);
        NL_REAL d = kNaN;
        N_ErrClear();
        NL_FLAG e = N_DistPtInfLine(l, P, &d);
        o.push_back(nl_status(e));
        o.push_back(e == NL_NO ? d : kNaN);
    }
    {
        NL_REAL a = kNaN, b = kNaN, c = kNaN, d = kNaN;
        N_ErrClear();
        NL_FLAG e = N_PlanePtNormalToImplicit(P, pt(in + 3), &a, &b, &c, &d);
        o.push_back(nl_status(e));
        if (e == NL_NO)
        {
            o.push_back(a);
            o.push_back(b);
            o.push_back(c);
            o.push_back(d);
        }
        else
            push_nan(o, 4);
    }
}

// op 5: N_DistPtLineSeg, N_DistPerpPtLineSeg. in: P[3] A[3] B[3].
void dist_seg(const double* in, std::vector<double>& o)
{
    NL_POINT P = pt(in), A = pt(in + 3), B = pt(in + 6);
    {
        NL_REAL d = kNaN;
        N_ErrClear();
        NL_FLAG e = N_DistPtLineSeg(P, A, B, &d);
        o.push_back(nl_status(e));
        o.push_back(e == NL_NO ? d : kNaN);
    }
    {
        NL_REAL d = kNaN;
        N_ErrClear();
        NL_FLAG e = N_DistPerpPtLineSeg(P, A, B, &d);
        o.push_back(nl_status(e));
        o.push_back(e == NL_NO ? d : kNaN);
    }
}

// op 6: polygon. in: n_pts points[3]. out: bbox[6] diag len.
void op_polygon(const double* in, int64_t n_in, std::vector<double>& o)
{
    std::vector<NL_POINT> P;
    for (int64_t i = 0; i + 2 < n_in; i += 3)
        P.push_back(pt(in + i));
    NL_EPOLYGON pol;
    N_EPolygonFromPts(&pol, (NL_INDEX)P.size() - 1, P.data());
    NL_MINMAXBOX box;
    N_PolygonGetBBox(&pol, &box);
    push_box(o, &box);
    NL_INDEX n;
    NL_POINT* Pp;
    N_EPolygonGetPts(&pol, &n, &Pp);
    NL_REAL len;
    N_DistPolygon(Pp, n, &len);
    o.push_back(len);
}

// op 7: control polygon. in: cpts[4]. out: len, then the round trip of the points.
void op_cpolygon(const double* in, int64_t n_in, std::vector<double>& o)
{
    std::vector<NL_CPOINT> Pw;
    for (int64_t i = 0; i + 3 < n_in; i += 4)
        Pw.push_back(cpt(in + i));
    NL_CPOLYGON pol;
    N_CPolygonFromCPts(&pol, Pw.data(), (NL_INDEX)Pw.size() - 1);
    NL_INDEX n;
    NL_CPOINT* Q;
    N_CPolygonGetCPts(&pol, &n, &Q);
    NL_REAL len;
    N_DistCPolygon(Q, n, &len);
    o.push_back(len);
    o.push_back((double)n);
}

// op 8: net. in: rows cols points[3]. out: bbox[6] diag, n, m.
void op_net(const double* in, int64_t n_in, std::vector<double>& o)
{
    int64_t rows = (int64_t)in[0], cols = (int64_t)in[1];
    std::vector<NL_POINT> data;
    for (int64_t i = 2; i + 2 < n_in; i += 3)
        data.push_back(pt(in + i));
    std::vector<NL_POINT*> rp((size_t)rows);
    for (int64_t i = 0; i < rows; ++i)
        rp[(size_t)i] = data.data() + i * cols;
    NL_ENET ntl;
    N_ENetFromPts(&ntl, (NL_INDEX)rows - 1, (NL_INDEX)cols - 1, rp.data());
    NL_MINMAXBOX box;
    N_ENetGetBBox(&ntl, &box);
    push_box(o, &box);
    // N_CNetFromCPts round trip on the same shape.
    std::vector<NL_CPOINT> cdata((size_t)(rows * cols), NL_CPOINT{0, 0, 0, 0});
    std::vector<NL_CPOINT*> crp((size_t)rows);
    for (int64_t i = 0; i < rows; ++i)
        crp[(size_t)i] = cdata.data() + i * cols;
    NL_CNET cn;
    N_CNetFromCPts(&cn, crp.data(), (NL_INDEX)rows - 1, (NL_INDEX)cols - 1);
    o.push_back((double)cn.n);
    o.push_back((double)cn.m);
}

// op 9: N_CreateRotationMatrixAboutAxis. in: P[3] V[3] al. out: status M[16].
void rotation(const double* in, std::vector<double>& o)
{
    NL_STACKS S;
    N_InitNurbs(&S);
    NL_RMATRIX rma;
    N_InitRealMatrix(&rma);
    N_ErrClear();
    NL_FLAG e = N_CreateRotationMatrixAboutAxis(pt(in), pt(in + 3), in[6], &rma, &S);
    o.push_back(nl_status(e));
    if (e == NL_NO)
        push_mat4(o, &rma);
    else
        push_nan(o, 16);
    N_EndNurbs(&S);
}

// op 10: N_CreateTransformMatrixFromAxes. in: O1 X1 Y1 Z1 O2 X2 Y2 Z2 (24). out: status M[16].
void transform(const double* in, std::vector<double>& o)
{
    NL_STACKS S;
    N_InitNurbs(&S);
    NL_RMATRIX rma;
    N_InitRealMatrix(&rma);
    N_ErrClear();
    NL_FLAG e = N_CreateTransformMatrixFromAxes(pt(in), pt(in + 3), pt(in + 6), pt(in + 9), pt(in + 12),
                                                pt(in + 15), pt(in + 18), pt(in + 21), &rma, &S);
    o.push_back(nl_status(e));
    if (e == NL_NO)
        push_mat4(o, &rma);
    else
        push_nan(o, 16);
    N_EndNurbs(&S);
}

// op 11: N_CptReflect, N_CPtsAreColinear. in: Pw[4] Qw[4] Rw[4] plane[6].
// out: status Qw'[4] col3 col4.
void cpt_ops(const double* in, std::vector<double>& o)
{
    NL_CPOINT Pw = cpt(in), Qw = cpt(in + 4), Rw = cpt(in + 8);
    NL_PLANE pl = plane(in + 12);
    {
        NL_CPOINT R{kNaN, kNaN, kNaN, kNaN};
        N_ErrClear();
        NL_FLAG e = N_CptReflect(Pw, pl, &R);
        o.push_back(nl_status(e));
        if (e == NL_NO)
            push_cpt(o, R);
        else
            push_nan(o, 4);
    }
    o.push_back((double)N_CPtsAreColinear(Pw, Qw, Rw, NL_THREED));
    o.push_back((double)N_CPtsAreColinear(Pw, Qw, Rw, NL_FOURD));
}
} // namespace

// Number of input doubles of each fixed-size op (-1 = variable).
SC_EXPORT int32_t sc_nlib_geom_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                    int64_t* out_len)
{
    static const int64_t kInputs[] = {16, 14, 13, 10, 10, 9, -1, -1, -1, 7, 24, 18};
    if (op < 0 || op > 11)
        return SC_ERR_ARG;
    if (kInputs[op] >= 0 && n_in != kInputs[op])
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    switch (op)
    {
    case 0:
        vector_ops(in, o);
        break;
    case 1:
        isect_ll(in, o);
        break;
    case 2:
        isect_lp(in, o);
        break;
    case 3:
        project_pl(in, o);
        break;
    case 4:
        plane_ops(in, o);
        break;
    case 5:
        dist_seg(in, o);
        break;
    case 6:
        if (n_in < 3 || n_in % 3 != 0)
            return SC_ERR_ARG;
        op_polygon(in, n_in, o);
        break;
    case 7:
        if (n_in % 4 != 0)
            return SC_ERR_ARG;
        op_cpolygon(in, n_in, o);
        break;
    case 8:
    {
        if (n_in < 2)
            return SC_ERR_ARG;
        int64_t rows = (int64_t)in[0], cols = (int64_t)in[1];
        if (rows < 1 || cols < 1 || rows > 4096 || cols > 4096 || n_in != 2 + 3 * rows * cols)
            return SC_ERR_ARG;
        op_net(in, n_in, o);
        break;
    }
    case 9:
        rotation(in, o);
        break;
    case 10:
        transform(in, o);
        break;
    case 11:
        cpt_ops(in, o);
        break;
    default:
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
