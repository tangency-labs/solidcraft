// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.math`.
// Wraps the NLib arithmetic / linear-algebra routines of upstream
// source/SMLib/src/NL_Math.cpp, NL_SrfPoly.cpp and NL_Util.cpp (see
// crates/solidcore-ffi/src/units/nlib_math.rs for the list).
//
// Status convention: 0 = ok; when an upstream routine returns its error flag
// the shim returns NL_ERROR.eno (cleared first; 1000 if the flag was set
// without a number); SC_ERR_* for shim problems.
//
// Matrices cross the boundary as a flat row-major storage block plus
// meta = {rows, cols, n, m, mtp, bw}; the shim builds row pointers into a
// private copy and calls N_CreateRealMatrix. Results are written as their
// "defined region" (emit_region): the entries upstream writes for the
// result's type, so storage upstream leaves uninitialised is never compared.
// The Rust wrapper validates every argument so that upstream only indexes
// inside the storage it is given.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

namespace
{
int32_t nl_status(NL_FLAG err)
{
    if (err == NL_NO)
        return SC_OK;
    return NL_ERROR.eno != 0 ? (int32_t)NL_ERROR.eno : 1000;
}

// A matrix over a private copy of caller storage.
struct Mat
{
    std::vector<double> data;
    std::vector<double*> rows;
    NL_RMATRIX r;

    // meta = {rows, cols, n, m, mtp, bw}; data has rows*cols entries.
    void create(const double* src, const int64_t* meta)
    {
        int64_t nr = meta[0], nc = meta[1];
        data.assign(src, src + nr * nc);
        rows.resize((size_t)(nr > 0 ? nr : 1));
        for (int64_t i = 0; i < nr; ++i)
            rows[(size_t)i] = data.data() + i * nc;
        N_CreateRealMatrix(&r, (NL_INDEX)meta[2], (NL_INDEX)meta[3], rows.data(), (NL_MATRIXTYPE)meta[4],
                           (NL_INDEX)meta[5]);
    }

    // Null matrix when src/meta are null, else create().
    void init_or_create(const double* src, const int64_t* meta)
    {
        if (meta == nullptr)
        {
            N_InitRealMatrix(&r);
            r.mtp = NL_MT_FULL;
        }
        else
            create(src, meta);
    }
};

void push_cpt(std::vector<double>& o, const NL_CPOINT& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
    o.push_back(p.w);
}

void push_pt(std::vector<double>& o, const NL_POINT& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

NL_CPOINT cpt(const double* d) { return NL_CPOINT{d[0], d[1], d[2], d[3]}; }
NL_POINT pt(const double* d) { return NL_POINT{d[0], d[1], d[2]}; }

// Entries of the result region of r, row-major (see file comment).
void emit_region(NL_RMATRIX* rm, std::vector<double>& o)
{
    NL_INDEX n, m, bw;
    NL_REAL** RM;
    NL_MATRIXTYPE mtp;
    N_GetRealMatrixData(rm, &n, &m, &RM, &mtp, &bw);
    for (NL_INDEX i = 0; i <= n; ++i)
    {
        switch (mtp)
        {
        case NL_MT_FULL:
            for (NL_INDEX j = 0; j <= m; ++j)
                o.push_back(RM[i][j]);
            break;
        case NL_MT_LOWERLEFT:
            for (NL_INDEX j = 0; j <= (i < m ? i : m); ++j)
                o.push_back(RM[i][j]);
            break;
        case NL_MT_UPPERRIGHT:
            for (NL_INDEX j = i; j <= m; ++j)
                o.push_back(RM[i][j]);
            break;
        case NL_MT_BANDED:
        {
            NL_INDEX sbw = bw / 2;
            for (NL_INDEX c = 0; c < bw; ++c)
            {
                NL_INDEX k = i + c - sbw;
                if (k >= 0 && k <= n)
                    o.push_back(RM[i][c]);
            }
            break;
        }
        }
    }
}

void write_meta(NL_RMATRIX* rm, int64_t* out_meta)
{
    NL_INDEX n, m, bw;
    NL_REAL** RM;
    NL_MATRIXTYPE mtp;
    N_GetRealMatrixData(rm, &n, &m, &RM, &mtp, &bw);
    out_meta[0] = n;
    out_meta[1] = m;
    out_meta[2] = (int64_t)mtp;
    out_meta[3] = bw;
}

int32_t copy_vec(const std::vector<double>& v, double* out, int64_t cap, int64_t* out_len)
{
    return sc_copy_out(v.data(), (int64_t)v.size(), out, cap, out_len);
}
} // namespace

// All point / control-point arithmetic routines on one input record.
// in (49 doubles): A[4] B[4] C[4] D[4] PA[3] PB[3] PC[3] PD[3] alpha beta gamma delta w M[16]
// ints: i1, i2. Output layout: see POINT_OPS_LAYOUT in crates/difftest/src/units/nlib_math.rs.
SC_EXPORT int32_t sc_nlib_math_point_ops(const double* in, const int64_t* ints, double* out, int64_t cap,
                                         int64_t* out_len)
{
    SC_TRY
    NL_CPOINT A = cpt(in), B = cpt(in + 4), C = cpt(in + 8), D = cpt(in + 12);
    NL_POINT PA = pt(in + 16), PB = pt(in + 19), PC = pt(in + 22), PD = pt(in + 25);
    double alpha = in[28], beta = in[29], gamma = in[30], delta = in[31], w = in[32];
    std::vector<double> mdata(in + 33, in + 49);
    double* mrows[4] = {mdata.data(), mdata.data() + 4, mdata.data() + 8, mdata.data() + 12};
    NL_RMATRIX M;
    N_CreateRealMatrix(&M, 3, 3, mrows, NL_MT_FULL, 3);

    std::vector<double> o;
    NL_CPOINT q;
    NL_POINT p;
    double s, s2;

    N_Combine2CPts(alpha, A, beta, B, &q);
    push_cpt(o, q);
    N_Combine2Pts(alpha, PA, beta, PB, &p);
    push_pt(o, p);
    N_TranslateSum2Pts(PC, alpha, PA, beta, PB, &p);
    push_pt(o, p);
    N_TranslateSum2CPts(C, alpha, A, beta, B, &q);
    push_cpt(o, q);
    N_Combine4Pts(alpha, PA, beta, PB, gamma, PC, delta, PD, &p);
    push_pt(o, p);
    N_Combine4CPts(alpha, A, beta, B, gamma, C, delta, D, &q);
    push_cpt(o, q);
    N_CopyCPt(A, &q);
    push_cpt(o, q);
    N_CopyPt(PA, &p);
    push_pt(o, p);
    N_CPtToPtEuclid(A, &p);
    push_pt(o, p);
    N_CPtToPtAndW(A, &p, &s);
    push_pt(o, p);
    o.push_back(s);
    N_Weight(PA, w, &q);
    push_cpt(o, q);
    q = B;
    N_VectorBlendCPt(alpha, A, &q);
    push_cpt(o, q);
    N_ScaleCPt(alpha, A, &q);
    push_cpt(o, q);
    N_ScaleCPtXYZ(alpha, A, &q);
    push_cpt(o, q);
    p = PB;
    N_VectorBlendPt(alpha, PA, &p);
    push_pt(o, p);
    N_ScalePt(alpha, PA, &p);
    push_pt(o, p);
    N_TransformCPt(A, &M, &q);
    push_cpt(o, q);
    N_TranslateCPt(A, PB, &q);
    push_cpt(o, q);
    N_ScaleCPtWithPtAndVector(A, PC, PD, &q);
    push_cpt(o, q);
    N_PtToCPt(PA, &q);
    push_cpt(o, q);
    N_CPtGetW(A, &s);
    o.push_back(s);
    {
        double x, y, z;
        N_PtToXYZ(PA, &x, &y, &z);
        o.push_back(x);
        o.push_back(y);
        o.push_back(z);
    }
    N_PtFromXYZ(alpha, beta, gamma, &p);
    push_pt(o, p);
    {
        double x, y, z, ww;
        N_CPtToWxWyWz(A, &x, &y, &z, &ww);
        o.push_back(x);
        o.push_back(y);
        o.push_back(z);
        o.push_back(ww);
    }
    N_CPtFromWxWyWz(alpha, beta, gamma, delta, &q);
    push_cpt(o, q);
    q = A;
    N_CPtSetX(alpha, &q);
    push_cpt(o, q);
    q = A;
    N_CPtSetY(alpha, &q);
    push_cpt(o, q);
    q = A;
    N_CPtSetZ(alpha, &q);
    push_cpt(o, q);
    q = A;
    N_CPtSetW(alpha, &q);
    push_cpt(o, q);
    N_Sum2Pts(PA, PB, &p);
    push_pt(o, p);
    N_Sum2CPts(A, B, &q);
    push_cpt(o, q);
    N_Diff2Pts(PA, PB, &p);
    push_pt(o, p);
    N_Diff2CPts(A, B, &q);
    push_cpt(o, q);
    N_Dot2Pts(PA, PB, &s);
    o.push_back(s);
    N_Dot2CPtsIn4D(A, B, &s);
    o.push_back(s);
    N_PtMagnitude(PA, &s);
    o.push_back(s);
    N_CPtMagnitude(A, &s);
    o.push_back(s);
    N_CPtRatDeriv(A, PB, beta, &q);
    push_cpt(o, q);
    N_CPtToPtNoW(A, &q);
    push_cpt(o, q);
    N_CPtGetZ(A, &s);
    o.push_back(s);
    N_CPtToPt(A, &p);
    push_pt(o, p);
    s = alpha;
    s2 = beta;
    N_SwapReals(&s, &s2);
    o.push_back(s);
    o.push_back(s2);
    {
        NL_CPOINT a = A, b = B;
        N_SwapCPts(&a, &b);
        push_cpt(o, a);
        push_cpt(o, b);
    }
    {
        NL_INTEGER i1 = (NL_INTEGER)ints[0], i2 = (NL_INTEGER)ints[1];
        N_SwapIntegers(&i1, &i2);
        o.push_back((double)i1);
        o.push_back((double)i2);
    }
    o.push_back((double)N_CPtsAreEqual(A, B));
    o.push_back((double)N_CPtsAreEqual(A, C));
    return copy_vec(o, out, cap, out_len);
    SC_CATCH
}

// N_FloatOpIsBad(x, y, op) -> *out.
SC_EXPORT int32_t sc_nlib_math_float_op(double x, double y, int32_t op, int32_t* out)
{
    SC_TRY
    *out = (int32_t)N_FloatOpIsBad(x, y, (NL_FLAG)op);
    return SC_OK;
    SC_CATCH
}

// N_SolveQuadraticEq(a, b, c) -> roots[0..2], *n (roots are 0 where upstream leaves them).
SC_EXPORT int32_t sc_nlib_math_quadratic(double a, double b, double c, double* roots, int64_t* n)
{
    SC_TRY
    NL_REAL r1 = 0.0, r2 = 0.0;
    NL_INTEGER nn = -1;
    N_ErrClear();
    NL_FLAG err = N_SolveQuadraticEq(a, b, c, &r1, &r2, &nn);
    roots[0] = r1;
    roots[1] = r2;
    *n = (int64_t)nn;
    return nl_status(err);
    SC_CATCH
}

// N_PascalTriRow(tri, row) -> rows 0..=row concatenated (row k has k+1 entries).
SC_EXPORT int32_t sc_nlib_math_pascal(int64_t row, int64_t* out, int64_t cap, int64_t* out_len)
{
    if (row < 0 || row > 66)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<std::vector<NL_INTEGER>> store((size_t)row + 1, std::vector<NL_INTEGER>((size_t)row + 1, 0));
    std::vector<NL_INTEGER*> tri((size_t)row + 1);
    for (size_t k = 0; k <= (size_t)row; ++k)
        tri[k] = store[k].data();
    N_PascalTriRow(tri.data(), (NL_INDEX)row);
    std::vector<int64_t> o;
    for (int64_t k = 0; k <= row; ++k)
        for (int64_t j = 0; j <= k; ++j)
            o.push_back((int64_t)store[(size_t)k][(size_t)j]);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// N_InitRealMatrix or N_CreateRealMatrix, N_RealMatrixIsNULL, then
// N_CheckMemRealMatrix(req = {n, m, mtp, bw}); out = {n, m, mtp, bw, was_null}
// (N_GetRealMatrixData / N_GetMaxIndexRealMatrix after the call).
SC_EXPORT int32_t sc_nlib_math_check_mem(const double* data, const int64_t* meta, const int64_t* req,
                                         int64_t* out)
{
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    Mat a;
    a.init_or_create(data, meta);
    int64_t was_null = (int64_t)N_RealMatrixIsNULL(&a.r);
    N_ErrClear();
    NL_FLAG err = N_CheckMemRealMatrix(&a.r, (NL_INDEX)req[0], (NL_INDEX)req[1], (NL_MATRIXTYPE)req[2],
                                       (NL_INDEX)req[3], (NL_STRING)_T("sc_shim"), &S);
    int32_t st = nl_status(err);
    write_meta(&a.r, out);
    NL_INDEX mn, mm;
    N_GetMaxIndexRealMatrix(&a.r, &mn, &mm);
    if (mn != out[0] || mm != out[1])
        st = SC_ERR_EXCEPTION;
    out[4] = was_null;
    N_EndNurbs(&S);
    return st;
    SC_CATCH
}

// N_RealMatrixMultiply(A, B, C) with C null (c_meta == nullptr) or given storage.
SC_EXPORT int32_t sc_nlib_math_multiply(const double* a_data, const int64_t* a_meta, const double* b_data,
                                        const int64_t* b_meta, const double* c_data, const int64_t* c_meta,
                                        int64_t* out_meta, double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    Mat a, b, c;
    a.create(a_data, a_meta);
    b.create(b_data, b_meta);
    c.init_or_create(c_data, c_meta);
    N_ErrClear();
    NL_FLAG err = N_RealMatrixMultiply(&a.r, &b.r, &c.r, &S);
    int32_t st = nl_status(err);
    std::vector<double> o;
    if (st == SC_OK)
    {
        write_meta(&c.r, out_meta);
        emit_region(&c.r, o);
        st = copy_vec(o, out, cap, out_len);
    }
    N_EndNurbs(&S);
    return st;
    SC_CATCH
}

// N_RealMatrixTranspose(A, B): in place (B = A) when in_place != 0, else B null or given.
SC_EXPORT int32_t sc_nlib_math_transpose(const double* a_data, const int64_t* a_meta, int32_t in_place,
                                         const double* b_data, const int64_t* b_meta, int64_t* out_meta,
                                         double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    Mat a, b;
    a.create(a_data, a_meta);
    NL_RMATRIX* res = &a.r;
    N_ErrClear();
    NL_FLAG err;
    if (in_place)
        err = N_RealMatrixTranspose(&a.r, &a.r, &S, &S);
    else
    {
        b.init_or_create(b_data, b_meta);
        err = N_RealMatrixTranspose(&a.r, &b.r, &S, &S);
        res = &b.r;
    }
    int32_t st = nl_status(err);
    std::vector<double> o;
    if (st == SC_OK)
    {
        write_meta(res, out_meta);
        emit_region(res, o);
        st = copy_vec(o, out, cap, out_len);
    }
    N_EndNurbs(&S);
    return st;
    SC_CATCH
}

// N_RealMatrixMultiplyPtArray (homogeneous == 0, pts stride 3) or
// N_RealMatrixMultiplyCPtArray (stride 4); writes n+1 results.
SC_EXPORT int32_t sc_nlib_math_mul_pts(const double* a_data, const int64_t* a_meta, const double* pts,
                                       int64_t n_pts, int32_t homogeneous, double* out, int64_t cap,
                                       int64_t* out_len)
{
    SC_TRY
    Mat a;
    a.create(a_data, a_meta);
    int64_t nq = a_meta[2] + 1;
    std::vector<double> o;
    NL_FLAG err;
    if (homogeneous)
    {
        std::vector<NL_CPOINT> P((size_t)n_pts), Q((size_t)nq);
        for (int64_t i = 0; i < n_pts; ++i)
            P[(size_t)i] = cpt(pts + 4 * i);
        N_ErrClear();
        err = N_RealMatrixMultiplyCPtArray(&a.r, P.data(), Q.data());
        for (auto& q : Q)
            push_cpt(o, q);
    }
    else
    {
        std::vector<NL_POINT> P((size_t)n_pts), Q((size_t)nq);
        for (int64_t i = 0; i < n_pts; ++i)
            P[(size_t)i] = pt(pts + 3 * i);
        N_ErrClear();
        err = N_RealMatrixMultiplyPtArray(&a.r, P.data(), Q.data());
        for (auto& q : Q)
            push_pt(o, q);
    }
    int32_t st = nl_status(err);
    if (st != SC_OK)
        return st;
    return copy_vec(o, out, cap, out_len);
    SC_CATCH
}

// N_RealMatrixLuDecompose (pivot == 0) or N_RealMatrixLuDecomposePivot (ind -> out_ind):
// writes the region of the decomposed matrix.
SC_EXPORT int32_t sc_nlib_math_lu(const double* a_data, const int64_t* a_meta, int32_t pivot, int64_t* out_ind,
                                  double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    Mat a;
    a.create(a_data, a_meta);
    N_ErrClear();
    NL_FLAG err;
    if (pivot)
    {
        std::vector<NL_INDEX> ind((size_t)(a_meta[2] + 1), -1);
        err = N_RealMatrixLuDecomposePivot(&a.r, ind.data());
        for (size_t i = 0; i < ind.size(); ++i)
            out_ind[i] = (int64_t)ind[i];
    }
    else
        err = N_RealMatrixLuDecompose(&a.r);
    int32_t st = nl_status(err);
    if (st != SC_OK)
        return st;
    std::vector<double> o;
    emit_region(&a.r, o);
    return copy_vec(o, out, cap, out_len);
    SC_CATCH
}

// Optional N_RealMatrixLuDecompose, then N_RealMatrixForBack(A, rhs, ptp) with
// ptp NL_EPOINT (rhs stride 3) or NL_HPOINT (stride 4); writes n+1 control points.
SC_EXPORT int32_t sc_nlib_math_for_back(const double* a_data, const int64_t* a_meta, int32_t decompose,
                                        const double* rhs, int64_t n_rhs, int32_t ptp, double* out, int64_t cap,
                                        int64_t* out_len)
{
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    Mat a;
    a.create(a_data, a_meta);
    N_ErrClear();
    NL_FLAG err = NL_NO;
    if (decompose)
        err = N_RealMatrixLuDecompose(&a.r);
    int64_t nq = a_meta[2] + 1;
    std::vector<NL_CPOINT> Q((size_t)nq);
    if (err == NL_NO)
    {
        if (ptp == NL_EPOINT)
        {
            std::vector<NL_POINT> P((size_t)n_rhs);
            for (int64_t i = 0; i < n_rhs; ++i)
                P[(size_t)i] = pt(rhs + 3 * i);
            err = N_RealMatrixForBack(&a.r, (NL_VOID*)P.data(), NL_EPOINT, Q.data(), &S);
        }
        else
        {
            std::vector<NL_CPOINT> P((size_t)n_rhs);
            for (int64_t i = 0; i < n_rhs; ++i)
                P[(size_t)i] = cpt(rhs + 4 * i);
            err = N_RealMatrixForBack(&a.r, (NL_VOID*)P.data(), NL_HPOINT, Q.data(), &S);
        }
    }
    int32_t st = nl_status(err);
    if (st == SC_OK)
    {
        std::vector<double> o;
        for (auto& q : Q)
            push_cpt(o, q);
        st = copy_vec(o, out, cap, out_len);
    }
    N_EndNurbs(&S);
    return st;
    SC_CATCH
}

// Optional N_RealMatrixLuDecompose, then N_RealMatrixRightForBack(A, y, x); writes x.
SC_EXPORT int32_t sc_nlib_math_right_for_back(const double* a_data, const int64_t* a_meta, int32_t decompose,
                                              const double* y, int64_t n_y, double* out, int64_t cap,
                                              int64_t* out_len)
{
    SC_TRY
    Mat a;
    a.create(a_data, a_meta);
    N_ErrClear();
    NL_FLAG err = NL_NO;
    if (decompose)
        err = N_RealMatrixLuDecompose(&a.r);
    std::vector<double> yy(y, y + n_y);
    std::vector<double> x((size_t)(a_meta[2] + 1), 0.0);
    if (err == NL_NO)
        err = N_RealMatrixRightForBack(&a.r, yy.data(), x.data());
    int32_t st = nl_status(err);
    if (st != SC_OK)
        return st;
    return copy_vec(x, out, cap, out_len);
    SC_CATCH
}

// N_RealMatrixLuDecomposePivot then N_RealMatrixForBackPivot(A, ind, b); writes b.
SC_EXPORT int32_t sc_nlib_math_solve_pivot(const double* a_data, const int64_t* a_meta, const double* b,
                                           int64_t n_b, double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    Mat a;
    a.create(a_data, a_meta);
    std::vector<NL_INDEX> ind((size_t)(a_meta[2] + 1), -1);
    std::vector<double> bb(b, b + n_b);
    N_ErrClear();
    NL_FLAG err = N_RealMatrixLuDecomposePivot(&a.r, ind.data());
    if (err == NL_NO)
        err = N_RealMatrixForBackPivot(&a.r, ind.data(), bb.data());
    int32_t st = nl_status(err);
    if (st != SC_OK)
        return st;
    return copy_vec(bb, out, cap, out_len);
    SC_CATCH
}

// N_RealMatrixInverse(A, lud, B) with B null or given; writes region(B) then region(A).
SC_EXPORT int32_t sc_nlib_math_inverse(const double* a_data, const int64_t* a_meta, int32_t lud,
                                       const double* b_data, const int64_t* b_meta, int64_t* out_meta, double* out,
                                       int64_t cap, int64_t* out_len)
{
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    Mat a, b;
    a.create(a_data, a_meta);
    b.init_or_create(b_data, b_meta);
    N_ErrClear();
    NL_FLAG err = N_RealMatrixInverse(&a.r, lud ? NL_YES : NL_NO, &b.r, &S);
    int32_t st = nl_status(err);
    if (st == SC_OK)
    {
        std::vector<double> o;
        write_meta(&b.r, out_meta);
        emit_region(&b.r, o);
        emit_region(&a.r, o);
        st = copy_vec(o, out, cap, out_len);
    }
    N_EndNurbs(&S);
    return st;
    SC_CATCH
}

// N_RealMatrixLstSqSolve(A, rhs, sol); writes sol (A's highest column index + 1 entries).
SC_EXPORT int32_t sc_nlib_math_lstsq(const double* a_data, const int64_t* a_meta, const double* rhs,
                                     int64_t n_rhs, double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    Mat a;
    a.create(a_data, a_meta);
    std::vector<double> r(rhs, rhs + n_rhs);
    std::vector<double> sol((size_t)(a_meta[3] + 1), 0.0);
    N_ErrClear();
    NL_FLAG err = N_RealMatrixLstSqSolve(&a.r, r.data(), sol.data());
    int32_t st = nl_status(err);
    if (st != SC_OK)
        return st;
    return copy_vec(sol, out, cap, out_len);
    SC_CATCH
}

namespace
{
// Control net over net (stride 4, rows x cols) as NL_CPOINT rows.
struct Net
{
    std::vector<NL_CPOINT> pts;
    std::vector<NL_CPOINT*> rows;
    NL_CNET cnet;

    void build(const double* net, int64_t nr, int64_t nc)
    {
        pts.resize((size_t)(nr * nc));
        for (int64_t i = 0; i < nr * nc; ++i)
            pts[(size_t)i] = cpt(net + 4 * i);
        rows.resize((size_t)nr);
        for (int64_t i = 0; i < nr; ++i)
            rows[(size_t)i] = pts.data() + i * nc;
        cnet.n = (NL_INDEX)(nr - 1);
        cnet.m = (NL_INDEX)(nc - 1);
        cnet.Pw = rows.data();
    }
};
} // namespace

// N_PowerBasisSrfEvalPts(aw, n, m, u, v) -> out[0..3]; N_SrfPowerBasisEvalPt on an
// NL_SURFACE over the whole net (knot vectors [0,1]) -> out[3..6].
SC_EXPORT int32_t sc_nlib_math_srf_eval(const double* net, int64_t nr, int64_t nc, int64_t n, int64_t m, double u,
                                        double v, double* out)
{
    if (nr < 1 || nc < 1 || n < 0 || m < 0 || n >= nr || m >= nc)
        return SC_ERR_ARG;
    SC_TRY
    Net cn;
    cn.build(net, nr, nc);
    NL_POINT S;
    N_PowerBasisSrfEvalPts(cn.rows.data(), (NL_INDEX)n, (NL_INDEX)m, u, v, &S);
    out[0] = S.x;
    out[1] = S.y;
    out[2] = S.z;
    double U[2] = {0.0, 1.0}, V[2] = {0.0, 1.0};
    NL_KNOTVECTOR knu, knv;
    N_KnotVectorFromRealArray(&knu, U, 1);
    N_KnotVectorFromRealArray(&knv, V, 1);
    NL_SURFACE sur;
    sur.net = &cn.cnet;
    sur.p = (NL_DEGREE)(nr - 1);
    sur.q = (NL_DEGREE)(nc - 1);
    sur.knu = &knu;
    sur.knv = &knv;
    N_ErrClear();
    NL_FLAG err = N_SrfPowerBasisEvalPt(&sur, u, v, &S);
    out[3] = S.x;
    out[4] = S.y;
    out[5] = S.z;
    return nl_status(err);
    SC_CATCH
}

// N_SrfPowerBasisEvalDerivs on an NL_SURFACE (net rows x cols, degrees p, q);
// writes SD[0..=udr][0..=vdr] (stride 3).
SC_EXPORT int32_t sc_nlib_math_srf_derivs(const double* net, int64_t nr, int64_t nc, int32_t p, int32_t q, double u,
                                          double v, int32_t mfl, int64_t udr, int64_t vdr, double* out, int64_t cap,
                                          int64_t* out_len)
{
    if (nr < 1 || nc < 1 || udr < 0 || vdr < 0 || udr > 64 || vdr > 64)
        return SC_ERR_ARG;
    SC_TRY
    Net cn;
    cn.build(net, nr, nc);
    double U[2] = {0.0, 1.0}, V[2] = {0.0, 1.0};
    NL_KNOTVECTOR knu, knv;
    N_KnotVectorFromRealArray(&knu, U, 1);
    N_KnotVectorFromRealArray(&knv, V, 1);
    NL_SURFACE sur;
    sur.net = &cn.cnet;
    sur.p = (NL_DEGREE)p;
    sur.q = (NL_DEGREE)q;
    sur.knu = &knu;
    sur.knv = &knv;
    std::vector<NL_POINT> sd((size_t)((udr + 1) * (vdr + 1)), NL_POINT{0.0, 0.0, 0.0});
    std::vector<NL_POINT*> rows((size_t)(udr + 1));
    for (int64_t i = 0; i <= udr; ++i)
        rows[(size_t)i] = sd.data() + i * (vdr + 1);
    N_ErrClear();
    NL_FLAG err = N_SrfPowerBasisEvalDerivs(&sur, u, v, (NL_FLAG)mfl, (NL_INDEX)udr, (NL_INDEX)vdr, rows.data());
    int32_t st = nl_status(err);
    if (st != SC_OK)
        return st;
    std::vector<double> o;
    for (auto& s : sd)
        push_pt(o, s);
    return copy_vec(o, out, cap, out_len);
    SC_CATCH
}

// N_Realloc*: kind 0 flag-1d, 1 real-1d, 2 pt-1d, 3 cpt-1d, 4 real-2d, 5 cpt-2d.
// data holds the old array (1d: len items; 2d: rows x cols items; items are 1, 3
// or 4 doubles); dims = {n, m, k, l} (1d uses n, m). Writes the copied block.
SC_EXPORT int32_t sc_nlib_math_realloc(int32_t kind, const double* data, int64_t len, int64_t rows, int64_t cols,
                                       const int64_t* dims, double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    int64_t n = dims[0], m = dims[1], k = dims[2], l = dims[3];
    std::vector<double> o;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (kind)
    {
    case 0:
    {
        NL_FLAG* a = N_AllocFlag1dArray((NL_INDEX)(len - 1), &S);
        for (int64_t i = 0; i < len; ++i)
            a[i] = (NL_FLAG)data[i];
        err = N_Realloc1dFlagArray(&a, (NL_INDEX)n, (NL_INDEX)m, &S);
        if (err == NL_NO)
            for (int64_t i = 0; i <= (n < m ? n : m); ++i)
                o.push_back((double)a[i]);
        break;
    }
    case 1:
    {
        NL_REAL* a = N_AllocReal1dArray((NL_INDEX)(len - 1), &S);
        for (int64_t i = 0; i < len; ++i)
            a[i] = data[i];
        err = N_Realloc1dRealArray(&a, (NL_INDEX)n, (NL_INDEX)m, &S);
        if (err == NL_NO)
            for (int64_t i = 0; i <= (n < m ? n : m); ++i)
                o.push_back(a[i]);
        break;
    }
    case 2:
    {
        NL_POINT* a = N_AllocPt1dArray((NL_INDEX)(len - 1), &S);
        for (int64_t i = 0; i < len; ++i)
            a[i] = pt(data + 3 * i);
        err = N_Realloc1dPtArray(&a, (NL_INDEX)n, (NL_INDEX)m, &S);
        if (err == NL_NO)
            for (int64_t i = 0; i <= (n < m ? n : m); ++i)
                push_pt(o, a[i]);
        break;
    }
    case 3:
    {
        NL_CPOINT* a = N_AllocCPt1dArray((NL_INDEX)(len - 1), &S);
        for (int64_t i = 0; i < len; ++i)
            a[i] = cpt(data + 4 * i);
        err = N_Realloc1dCPtArray(&a, (NL_INDEX)n, (NL_INDEX)m, &S);
        if (err == NL_NO)
            for (int64_t i = 0; i <= (n < m ? n : m); ++i)
                push_cpt(o, a[i]);
        break;
    }
    case 4:
    {
        NL_REAL** a = N_AllocReal2dArray((NL_INDEX)(rows - 1), (NL_INDEX)(cols - 1), &S);
        for (int64_t i = 0; i < rows; ++i)
            for (int64_t j = 0; j < cols; ++j)
                a[i][j] = data[i * cols + j];
        err = N_Realloc2dRealArray(&a, (NL_INDEX)n, (NL_INDEX)m, (NL_INDEX)k, (NL_INDEX)l, &S);
        if (err == NL_NO)
            for (int64_t i = 0; i <= (k < n ? k : n); ++i)
                for (int64_t j = 0; j <= (l < m ? l : m); ++j)
                    o.push_back(a[i][j]);
        break;
    }
    case 5:
    {
        NL_CPOINT** a = N_AllocCPt2dArray((NL_INDEX)(rows - 1), (NL_INDEX)(cols - 1), &S);
        for (int64_t i = 0; i < rows; ++i)
            for (int64_t j = 0; j < cols; ++j)
                a[i][j] = cpt(data + 4 * (i * cols + j));
        err = N_Realloc2dCPtArray(&a, (NL_INDEX)n, (NL_INDEX)m, (NL_INDEX)k, (NL_INDEX)l, &S);
        if (err == NL_NO)
            for (int64_t i = 0; i <= (k < n ? k : n); ++i)
                for (int64_t j = 0; j <= (l < m ? l : m); ++j)
                    push_cpt(o, a[i][j]);
        break;
    }
    default:
        N_EndNurbs(&S);
        return SC_ERR_ARG;
    }
    int32_t st = nl_status(err);
    if (st == SC_OK)
        st = copy_vec(o, out, cap, out_len);
    N_EndNurbs(&S);
    return st;
    SC_CATCH
}
