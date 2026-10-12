// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.solution_array`. Wraps upstream SmSolutionEnd / SmSolution /
// SmSolutionArray (source/SMLib/src/SmSolutionArray.cpp, source/SMLib/inc/SmSolutionArray.h).
//
// sc_geom_solution_array_{ints,floats}(prog, n, out, cap, out_len) run the same flat double
// program against one SmSolutionArray and return its integer or float output stream. The
// encoding is shared with crates/solidcore-ffi/src/units/geom_solution_array.rs, which checks
// the static shape first; the checks that need run-time state are made here (SC_ERR_ARG).
//
// A solution `S` is 35 doubles: type nvars nobjs, start value + 10 params, end value + 10
// params, 5 objects, 5 nodes. An object is -1 (NULL) or kind*100 + id (kind 0..14, id 0..99):
//   0 curve (SmLine)   1 surface (SmPlane)   2 volume*        3 vertex (SmVertex)
//   4 edge (SmEdge on the line)   5 face (SmFace on the plane)   6 poly vertex*
//   7 poly edge*   8 poly loop*   9 poly face*   10 C poly face*   11 poly shell*
//   12 poly region*   13 poly brep*   14 other*
// (* = SmObject stand-in answering IsKindOf for that class; never dereferenced on the paths
// the run-time checks allow). Ops 3, 21 and 26 reject volumes and, when the first object
// has parameters, NULL objects (GetParameterInterval would dereference them). A node is -1 (NULL) or a fake SmTreeNode pointer id 0..4095.
// Output of a solution: ints type nvars nobjs objects[5] nodes[5]; floats start value,
// params[10], end value, params[10].
//
// Ops (op code, operands):
//   1 cap          array = SmSolutionArray(cap, NULL)
//   2 S            Add                          ints: index
//   3 key S        AddSortedSolution            ints: index (SM_BIG_ULONG = rejected)
//   4 idx cnt S    InsertAt(idx, S, cnt)
//   5 idx cnt      RemoveAt(idx, cnt)
//   6 i j          Swap
//   7 n            SetSize
//   8 idx S        SetAt
//   9 idx          GetAt                        solution
//  10              ReSet
//  11 m S*m        Append(array of the m solutions)   ints: old size
//  12              dump: ints GetSize; every solution via operator[]
//  13              ints GetType, IsKindOf(SmSolutionArray/SmObject/SmCurve _TYPE)
//  20 start S S    S1.ClassifySolutionEnd(S2, start)      ints: class
//  21 S S          S1.IsEqual(S2)                          ints: 0/1
//  22 obj flag S   GetIndex                     ints: status [, object index, param index]
//  23 idx end S    GetPoint(idx, &p, &q, end)   ints: status; floats [p, q] on success
//  24 obj flag end S  GetPoint(obj, flag, &p, &q, end)  ints: status; floats [p, q]
//  25 pidx S       GetObjectIndexForParameterIndex  ints: status [, object index, dir]
//  26 pidx S       GetParameterInterval         ints: status; floats [min, max]
//  27 idx S        GetObjectTolerance           floats: tolerance
//  28 S            copy ctor + operator= (solution), ReSet (solution), SmSolutionEnd
//                  operator= + operator[] (floats params) + ReSet (floats value, params)
// Statuses: 0 = SM_SUCCESS, else the SmStatus value.
#include "sc_shim.h"

#include <SmCurve.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmPlane.h>
#include <SmPoly.h>
#include <SmSolutionArray.h>
#include <SmVertex.h>
#include <SmVolume.h>

#include <map>
#include <vector>

namespace
{
const int kKinds = 15;
const int kVolume = 2, kPolyVertex = 6;

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

// SmObject stand-in of one class family; answers IsKindOf like that class.
class ScStandIn : public SmObject
{
public:
    explicit ScStandIn(int kind) : m_kind(kind) {}
    SmBoolean IsKindOf(SM_TYPE t) const override
    {
        switch (m_kind)
        {
        case 2: return t == SmVolume_TYPE;
        case 6: return t == SmPolyVertex_TYPE;
        case 7: return t == SmPolyEdge_TYPE;
        case 8: return t == SmPolyLoop_TYPE;
        case 9: return t == SmPolyFace_TYPE;
        case 10: return t == SmCPolyFace_TYPE || t == SmPolyFace_TYPE;
        case 11: return t == SmPolyShell_TYPE;
        case 12: return t == SmPolyRegion_TYPE;
        case 13: return t == SmPolyBrep_TYPE;
        default: return FALSE;
        }
    }

private:
    int m_kind;
};

const SmContext* const kNoContext = nullptr;

const SmContext* the_context()
{
    static SmContext* p = new SmContext();
    return p;
}

SmLine* new_line()
{
    return new (the_context()) SmLine(SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 2.0, 3.0), 3, the_context());
}

SmPlane* new_plane()
{
    return new (the_context()) SmPlane(SmPoint3d(0.0, 0.0, 1.0), SmVector3d(0.0, 0.0, 1.0), the_context());
}

SmCurve* the_line()
{
    static SmLine* p = new_line();
    return p;
}

SmSurface* the_plane()
{
    static SmPlane* p = new_plane();
    return p;
}

// Objects live for the whole process (never deleted: topology destructors notify owners).
SmObject* object(int64_t code)
{
    static std::map<int64_t, SmObject*> objs;
    if (code < 0)
        return nullptr;
    auto it = objs.find(code);
    if (it != objs.end())
        return it->second;
    SmObject* o = nullptr;
    switch ((int)(code / 100))
    {
    case 0: o = new_line(); break;
    case 1: o = new_plane(); break;
    case 3: o = new (static_cast<SmObject*>(the_line())) ScVertex(); break;
    case 4: o = new (static_cast<SmObject*>(the_line())) ScEdge(the_line()); break;
    case 5: o = new (static_cast<SmObject*>(the_line())) ScFace(the_plane()); break;
    default: o = new (kNoContext) ScStandIn((int)(code / 100)); break;
    }
    objs[code] = o;
    return o;
}

std::map<const SmObject*, int64_t>& object_codes()
{
    static std::map<const SmObject*, int64_t> m;
    return m;
}

SmObject* obj(double d)
{
    int64_t code = (int64_t)d;
    SmObject* o = object(code);
    if (o)
        object_codes()[o] = code;
    return o;
}

int64_t obj_code(const SmObject* o)
{
    if (o == nullptr)
        return -1;
    auto it = object_codes().find(o);
    return it == object_codes().end() ? -2 : it->second;
}

int kind_of(double d) { return d < 0 ? -1 : (int)((int64_t)d / 100); }

const int64_t kMaxNodes = 4096;
alignas(16) char g_nodes[kMaxNodes];

SmTreeNode* node_ptr(double d)
{
    long long i = (long long)d;
    return i < 0 ? nullptr : reinterpret_cast<SmTreeNode*>(g_nodes + i);
}

int64_t node_id(const SmTreeNode* p)
{
    return p == nullptr ? -1 : (int64_t)(reinterpret_cast<const char*>(p) - g_nodes);
}

const int64_t kSol = 35;

SmSolution sol(const double* a)
{
    SmSolution s;
    s.m_eSolutionType = (SmSolutionType)(int)a[0];
    s.m_lNumVariables = (ULONG)a[1];
    s.m_lNumObjects = (ULONG)a[2];
    s.m_vStart.m_dSolutionValue = a[3];
    for (int i = 0; i < SM_SA_MAX_TREES * 2; i++)
        s.m_vStart[i] = a[4 + i];
    s.m_vEnd.m_dSolutionValue = a[14];
    for (int i = 0; i < SM_SA_MAX_TREES * 2; i++)
        s.m_vEnd[i] = a[15 + i];
    for (int i = 0; i < SM_SA_MAX_TREES; i++)
    {
        s.m_apObjects[i] = obj(a[25 + i]);
        s.m_apNodes[i] = node_ptr(a[30 + i]);
    }
    return s;
}

void put_end(std::vector<double>& of, const SmSolutionEnd& e)
{
    of.push_back(e.m_dSolutionValue);
    for (int i = 0; i < SM_SA_MAX_TREES * 2; i++)
        of.push_back(e[i]);
}

void put_sol(std::vector<int64_t>& oi, std::vector<double>& of, const SmSolution& s)
{
    oi.push_back((int64_t)s.m_eSolutionType);
    oi.push_back((int64_t)s.m_lNumVariables);
    oi.push_back((int64_t)s.m_lNumObjects);
    for (int i = 0; i < SM_SA_MAX_TREES; i++)
        oi.push_back(obj_code(s.m_apObjects[i]));
    for (int i = 0; i < SM_SA_MAX_TREES; i++)
        oi.push_back(node_id(s.m_apNodes[i]));
    put_end(of, s.m_vStart);
    put_end(of, s.m_vEnd);
}

void put_v(std::vector<double>& of, const SmVector3d& v)
{
    of.push_back(v.x);
    of.push_back(v.y);
    of.push_back(v.z);
}

int64_t st(SmStatus s) { return s == SM_SUCCESS ? 0 : (int64_t)s; }

// Fixed operand count of an op (before any embedded solutions), or -1.
int64_t op_fixed(int op)
{
    switch (op)
    {
    case 1: return 1;
    case 2: return 0;
    case 3: return 1;
    case 4: return 2;
    case 5: return 2;
    case 6: return 2;
    case 7: return 1;
    case 8: return 1;
    case 9: return 1;
    case 10: return 0;
    case 11: return 1;
    case 12: return 0;
    case 13: return 0;
    case 20: return 1;
    case 21: return 0;
    case 22: return 2;
    case 23: return 2;
    case 24: return 3;
    case 25: return 1;
    case 26: return 1;
    case 27: return 1;
    case 28: return 0;
    default: return -1;
    }
}

int64_t op_sols(int op, const double* a)
{
    switch (op)
    {
    case 2:
    case 3:
    case 4:
    case 8:
    case 22:
    case 23:
    case 24:
    case 25:
    case 26:
    case 27:
    case 28: return 1;
    case 11: return (int64_t)a[0];
    case 20:
    case 21: return 2;
    default: return 0;
    }
}

// Solution checks for paths that would dereference a NULL object or a stand-in.
bool objects_non_null(const double* s)
{
    for (int i = 0; i < (int)s[2]; i++)
        if (s[25 + i] < 0)
            return false;
    return true;
}

bool has_kind(const double* s, int kind)
{
    for (int i = 0; i < SM_SA_MAX_TREES; i++)
        if (kind_of(s[25 + i]) == kind)
            return true;
    return false;
}

// GetParameterInterval dereferences the object at the index GetObjectIndexForParameterIndex
// returns; that search can stop past a NULL slot when the first object has parameters.
bool interval_safe(const double* s)
{
    if (has_kind(s, kVolume))
        return false;
    int k0 = kind_of(s[25]);
    bool params = k0 == 0 || k0 == 1 || k0 == 4 || k0 == 5;
    return !(s[2] > 0 && params) || objects_non_null(s);
}

int32_t run(const double* p, int64_t n, std::vector<int64_t>& oi, std::vector<double>& of)
{
    SmSolutionArray* arr = new (kNoContext) SmSolutionArray();
    int32_t rc = SC_OK;
    int64_t i = 0;
    while (i < n && rc == SC_OK)
    {
        int op = (int)p[i];
        int64_t fixed = op_fixed(op);
        if (fixed < 0 || i + 1 + fixed > n)
        {
            rc = SC_ERR_ARG;
            break;
        }
        const double* a = p + i + 1;
        int64_t nsols = op_sols(op, a);
        if (nsols < 0 || i + 1 + fixed + nsols * kSol > n)
        {
            rc = SC_ERR_ARG;
            break;
        }
        const double* s = a + fixed;
        const double* s2 = s + kSol;
        ULONG size = arr->GetSize();
        switch (op)
        {
        case 1:
            delete arr;
            arr = new (kNoContext) SmSolutionArray((ULONG)a[0], NULL);
            break;
        case 2: oi.push_back((int64_t)arr->Add(sol(s))); break;
        case 3:
            if (!interval_safe(s))
                rc = SC_ERR_ARG;
            else
                oi.push_back((int64_t)arr->AddSortedSolution(sol(s), (SmSortKeyType)(int)a[0]));
            break;
        case 4: arr->InsertAt((ULONG)a[0], sol(s), (ULONG)a[1]); break;
        case 5: arr->RemoveAt((ULONG)a[0], (ULONG)a[1]); break;
        case 6:
            if ((ULONG)a[0] >= size || (ULONG)a[1] >= size)
                rc = SC_ERR_ARG;
            else
                arr->Swap((ULONG)a[0], (ULONG)a[1]);
            break;
        case 7: arr->SetSize((ULONG)a[0]); break;
        case 8:
            if ((ULONG)a[0] >= size)
                rc = SC_ERR_ARG;
            else
                arr->SetAt((ULONG)a[0], sol(s));
            break;
        case 9:
            if ((ULONG)a[0] >= size)
                rc = SC_ERR_ARG;
            else
                put_sol(oi, of, arr->GetAt((ULONG)a[0]));
            break;
        case 10: arr->ReSet(); break;
        case 11:
        {
            SmSolutionArray src;
            for (int64_t k = 0; k < nsols; k++)
                src.Add(sol(s + k * kSol));
            oi.push_back((int64_t)arr->Append(src));
            break;
        }
        case 12:
            oi.push_back((int64_t)size);
            for (ULONG k = 0; k < size; k++)
                put_sol(oi, of, (*arr)[k]);
            break;
        case 13:
            oi.push_back((int64_t)arr->GetType());
            oi.push_back(arr->IsKindOf(SmSolutionArray_TYPE) ? 1 : 0);
            oi.push_back(arr->IsKindOf(SmObject_TYPE) ? 1 : 0);
            oi.push_back(arr->IsKindOf(SmCurve_TYPE) ? 1 : 0);
            break;
        case 20: oi.push_back((int64_t)sol(s).ClassifySolutionEnd(sol(s2), a[0] != 0.0 ? TRUE : FALSE)); break;
        case 21:
            if (!interval_safe(s) || !interval_safe(s2))
                rc = SC_ERR_ARG;
            else
                oi.push_back(sol(s).IsEqual(sol(s2)) ? 1 : 0);
            break;
        case 22:
        {
            if (!objects_non_null(s))
            {
                rc = SC_ERR_ARG;
                break;
            }
            ULONG lo = 0, lp = 0;
            SmStatus r = sol(s).GetIndex(obj(a[0]), a[1] != 0.0 ? TRUE : FALSE, lo, lp);
            oi.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                oi.push_back((int64_t)lo);
                oi.push_back((int64_t)lp);
            }
            break;
        }
        case 23:
        case 24:
        {
            ULONG idx = (ULONG)a[0];
            bool ok = op == 23 ? (idx >= (ULONG)s[2] || (objects_non_null(s) && kind_of(s[25 + idx]) != kPolyVertex))
                               : (objects_non_null(s) && !has_kind(s, kPolyVertex));
            if (!ok)
            {
                rc = SC_ERR_ARG;
                break;
            }
            SmPoint3d pt;
            SmPoint3d prm(0.0, 0.0, 0.0);
            SmStatus r = op == 23 ? sol(s).GetPoint(idx, pt, &prm, a[1] != 0.0 ? TRUE : FALSE)
                                  : sol(s).GetPoint(obj(a[0]), a[1] != 0.0 ? TRUE : FALSE, pt, &prm,
                                                    a[2] != 0.0 ? TRUE : FALSE);
            oi.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                put_v(of, pt);
                put_v(of, prm);
            }
            break;
        }
        case 25:
        {
            ULONG lo = 0, ld = 0;
            SmStatus r = sol(s).GetObjectIndexForParameterIndex((ULONG)a[0], lo, ld);
            oi.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                oi.push_back((int64_t)lo);
                oi.push_back((int64_t)ld);
            }
            break;
        }
        case 26:
        {
            if (!interval_safe(s))
            {
                rc = SC_ERR_ARG;
                break;
            }
            SmExtent1d ivl;
            SmStatus r = sol(s).GetParameterInterval((ULONG)a[0], ivl);
            oi.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                of.push_back(ivl.GetMin());
                of.push_back(ivl.GetMax());
            }
            break;
        }
        case 27:
        {
            ULONG idx = (ULONG)a[0];
            if (idx < (ULONG)s[2] && s[25 + idx] >= 0)
            {
                rc = SC_ERR_ARG;
                break;
            }
            of.push_back(sol(s).GetObjectTolerance(idx));
            break;
        }
        case 28:
        {
            SmSolution src = sol(s);
            SmSolution b(src);
            SmSolution c;
            c = b;
            put_sol(oi, of, c);
            c.ReSet();
            put_sol(oi, of, c);
            SmSolutionEnd e;
            e = src.m_vStart;
            const SmSolutionEnd& ce = e;
            for (ULONG k = 0; k < SM_SA_MAX_TREES * 2; k++)
                of.push_back(ce[k]);
            e.ReSet();
            put_end(of, e);
            break;
        }
        default: rc = SC_ERR_ARG; break;
        }
        i += 1 + fixed + nsols * kSol;
    }
    delete arr;
    return rc;
}
} // namespace

SC_EXPORT int32_t sc_geom_solution_array_ints(const double* prog, int64_t n, int64_t* out, int64_t cap,
                                              int64_t* out_len)
{
    if (n < 0 || (n > 0 && prog == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t rc = run(prog, n, oi, of);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(oi.data(), (int64_t)oi.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_solution_array_floats(const double* prog, int64_t n, double* out, int64_t cap,
                                                int64_t* out_len)
{
    if (n < 0 || (n > 0 && prog == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t rc = run(prog, n, oi, of);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(of.data(), (int64_t)of.size(), out, cap, out_len);
    SC_CATCH
}
