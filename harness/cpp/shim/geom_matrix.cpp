// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.matrix`.
// Wraps upstream SmMatrix (source/SMLib/inc/SmMatrix.h, source/SMLib/src/SmMatrix.cpp).
#include "sc_shim.h"

#include <SmMatrix.h>
#include <SmVector3d.h>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_matrix.rs).
enum ScMatrixOp : int32_t
{
    OP_COPY = 0,         // SmMatrix(const SmMatrix&): out = elements, ints = [rows, cols]
    OP_RESIZE = 1,       // SetSize(i, j); MakeIdentity() (Clear() when not square)
    OP_ACCESS = 2,       // AddTo(i, j, s); GetAt; operator[]; SetAt(i, j, -s); GetAt
    OP_IS_SYMMETRIC = 3, // ints = [IsSymmetric()]
    OP_CLEAR = 4,        // Clear()
    OP_IDENTITY = 5,     // MakeIdentity()
    OP_LU = 6,           // LUDecompose(idx): out = elements, ints = idx
    OP_SOLVE_LU = 7,     // LUDecompose then SolveWithLUMatrix(idx, v): out = x
    OP_SOLVE = 8,        // SolveLinearSystem(v): out = x
    OP_EIGEN = 9,        // EigenSystemJacobiSolver: out = values, vectors, matrix; ints = [iter]
    OP_EIGEN3 = 10,      // Compute3x3EigenVectors: out = values, vectors (rows = vectors), matrix
    OP_SVD = 11,         // SVDecompose(W, V): out = W, V, matrix after (= U)
};

static void put_matrix(const SmMatrix& m, double* out, int64_t& k)
{
    for (ULONG r = 0; r < m.GetNumRows(); r++)
        for (ULONG c = 0; c < m.GetNumColumns(); c++)
            out[k++] = m.GetAt(r, c);
}

// Runs one SmMatrix operation on the row-major `rows x cols` matrix `m`.
//   v[n_v]        : right-hand side (OP_SOLVE*)
//   i, j, s       : index / scalar operands (OP_RESIZE, OP_ACCESS)
//   out[cap]      : float results, *out_len receives the count
//   ints[ints_cap]: integer results, *ints_len receives the count
//   *status       : upstream SmStatus of the operation, SM_SUCCESS (1000) reported as 0
// Returns 0, SC_ERR_ARG or SC_ERR_BUFFER. The caller guarantees 1 <= rows, cols <= 64,
// i, j in range for OP_ACCESS, 1 <= i, j <= 64 for OP_RESIZE, n_v == rows for
// OP_SOLVE_LU, 3x3 for OP_EIGEN3, and cap / ints_cap of at least
// 3 * rows * cols + rows + 8 / rows + 8.
SC_EXPORT int32_t sc_geom_matrix_op(int32_t op, int64_t rows, int64_t cols, const double* m,
                                    const double* v, int64_t n_v, int64_t i, int64_t j, double s,
                                    double* out, int64_t cap, int64_t* out_len, int64_t* ints,
                                    int64_t ints_cap, int64_t* ints_len, int32_t* status)
{
    if (rows < 1 || cols < 1 || rows > 64 || cols > 64 || n_v < 0 || n_v > 64)
        return SC_ERR_ARG;
    if (cap < 3 * rows * cols + rows + 8 || ints_cap < rows + 8)
        return SC_ERR_BUFFER;
    SC_TRY
    SmMatrix sM((ULONG)rows, (ULONG)cols);
    for (int64_t r = 0; r < rows; r++)
        for (int64_t c = 0; c < cols; c++)
            sM.SetAt((ULONG)r, (ULONG)c, m[r * cols + c]);
    SmTArray<double> sV;
    for (int64_t k = 0; k < n_v; k++)
        sV.Add(v[k]);

    int64_t k = 0;
    int64_t ni = 0;
    *status = 0;
    switch (op)
    {
    case OP_COPY:
    {
        SmMatrix sCopy(sM);
        put_matrix(sCopy, out, k);
        ints[ni++] = (int64_t)sCopy.GetNumRows();
        ints[ni++] = (int64_t)sCopy.GetNumColumns();
        break;
    }
    case OP_RESIZE:
    {
        if (i < 1 || j < 1 || i > 64 || j > 64)
            return SC_ERR_ARG;
        if (i == rows && j == cols)
        {
            sM.SetSize((ULONG)i, (ULONG)j); // same size: keeps the values
        }
        else
        {
            sM.SetSize((ULONG)i, (ULONG)j);
            if (sM.MakeIdentity() != SM_SUCCESS)
                sM.Clear();
        }
        put_matrix(sM, out, k);
        ints[ni++] = (int64_t)sM.GetNumRows();
        ints[ni++] = (int64_t)sM.GetNumColumns();
        break;
    }
    case OP_ACCESS:
    {
        if (i < 0 || j < 0 || i >= rows || j >= cols)
            return SC_ERR_ARG;
        sM.AddTo((ULONG)i, (ULONG)j, s);
        out[k++] = sM.GetAt((ULONG)i, (ULONG)j);
        const SmMatrix& crM = sM;
        out[k++] = crM[(ULONG)i][j];
        sM.SetAt((ULONG)i, (ULONG)j, -s);
        out[k++] = sM[(ULONG)i][j];
        put_matrix(sM, out, k);
        break;
    }
    case OP_IS_SYMMETRIC:
        ints[ni++] = sM.IsSymmetric() ? 1 : 0;
        break;
    case OP_CLEAR:
        *status = sM.Clear();
        put_matrix(sM, out, k);
        break;
    case OP_IDENTITY:
        *status = sM.MakeIdentity();
        put_matrix(sM, out, k);
        break;
    case OP_LU:
    {
        if (rows != cols)
        {
            ULONG dummy[1];
            *status = sM.LUDecompose(dummy); // rejects before touching the index
            break;
        }
        ULONG aIdx[64];
        *status = sM.LUDecompose(aIdx);
        put_matrix(sM, out, k);
        if (*status == SM_SUCCESS || *status == SM_ERR_WARNING)
            for (int64_t r = 0; r < rows; r++)
                ints[ni++] = (int64_t)aIdx[r];
        break;
    }
    case OP_SOLVE_LU:
    {
        if (rows != cols || n_v != rows)
            return SC_ERR_ARG;
        ULONG aIdx[64];
        SmStatus sLu = sM.LUDecompose(aIdx);
        if (sLu != SM_SUCCESS && sLu != SM_ERR_WARNING)
        {
            *status = sLu;
            break;
        }
        SmTArray<double> sX;
        sX.SetSize((ULONG)n_v);
        *status = sM.SolveWithLUMatrix(aIdx, sV, sX);
        if (*status == SM_SUCCESS)
        {
            for (ULONG r = 0; r < sX.GetSize(); r++)
                out[k++] = sX[r];
            ints[ni++] = sLu == SM_ERR_WARNING ? 1 : 0;
        }
        break;
    }
    case OP_SOLVE:
    {
        SmTArray<double> sX;
        *status = sM.SolveLinearSystem(sV, sX);
        if (*status == SM_SUCCESS || *status == SM_ERR_WARNING)
        {
            for (ULONG r = 0; r < sX.GetSize(); r++)
                out[k++] = sX[r];
            ints[ni++] = *status == SM_ERR_WARNING ? 1 : 0;
            *status = 0;
        }
        break;
    }
    case OP_EIGEN:
    {
        SmTArray<double> sVals;
        SmMatrix sVecs(1, 1);
        ULONG lIter = 0;
        *status = sM.EigenSystemJacobiSolver(sVals, sVecs, lIter);
        if (*status == SM_SUCCESS)
        {
            for (ULONG r = 0; r < sVals.GetSize(); r++)
                out[k++] = sVals[r];
            put_matrix(sVecs, out, k);
            put_matrix(sM, out, k);
            ints[ni++] = (int64_t)lIter;
        }
        break;
    }
    case OP_EIGEN3:
    {
        if (rows != 3 || cols != 3)
            return SC_ERR_ARG;
        // Upstream ignores the solver status; for a non-symmetric matrix it returns
        // uninitialized vectors. Report SM_ERR there instead (the port does too).
        if (!sM.IsSymmetric())
        {
            *status = SM_ERR;
            break;
        }
        ULONG lNum = 0;
        double adVals[3] = {0.0, 0.0, 0.0};
        SmVector3d aVecs[3];
        *status = sM.Compute3x3EigenVectors(lNum, adVals, aVecs);
        if (*status == SM_SUCCESS)
        {
            for (int r = 0; r < 3; r++)
                out[k++] = adVals[r];
            for (int r = 0; r < 3; r++)
            {
                out[k++] = aVecs[r].x;
                out[k++] = aVecs[r].y;
                out[k++] = aVecs[r].z;
            }
            put_matrix(sM, out, k);
        }
        break;
    }
    case OP_SVD:
    {
        SmTArray<double> sW;
        SmMatrix sVm(1, 1);
        *status = sM.SVDecompose(sW, sVm);
        if (*status == SM_SUCCESS)
        {
            for (ULONG r = 0; r < sW.GetSize(); r++)
                out[k++] = sW[r];
            put_matrix(sVm, out, k);
            put_matrix(sM, out, k);
        }
        break;
    }
    default:
        return SC_ERR_ARG;
    }
    if (*status == SM_SUCCESS)
        *status = 0;
    *out_len = k;
    *ints_len = ni;
    return SC_OK;
    SC_CATCH
}
