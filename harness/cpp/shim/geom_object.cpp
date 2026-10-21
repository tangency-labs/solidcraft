// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.object`. Wraps upstream SmObject (type system, Notify,
// AssertValid), SmTemporaryChangeValue<TYPE> (source/SMLib/inc/SmObject.h,
// source/SMLib/src/SmObject.cpp) and SmShape (source/SMLib/inc/SmShape.h,
// source/SMLib/src/SmShape.cpp). B-reps for shapes come from the shared B-rep fixture
// (support/topology_fixture.h), whose walk numbers their entities as the Rust import does.
#include "sc_shim.h"
#include "support/topology_fixture.h"
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmRegion.h>
#include <SmVertex.h>
#include <SmObject.h>
#include <SmShape.h>
#include <SmTree.h>

#include <cstring>
#include <memory>
#include <vector>

namespace
{

const int64_t kNumObjects = 6;

// A notify callback recording (tag, op, caller, data1, data2, data3, caller type).
struct ScRecorder : public SmNotifyCallback
{
    int64_t m_lTag = 0;
    std::vector<int64_t>* m_pOut = nullptr;
    std::vector<SmObject*>* m_pObjs = nullptr;

    int64_t Index(SmObject* p) const
    {
        if (p == nullptr)
            return -1;
        for (size_t i = 0; i < m_pObjs->size(); ++i)
            if ((*m_pObjs)[i] == p)
                return (int64_t)i;
        return -2;
    }

    SmStatus Execute(SmNotifyOperation eOp, SmObject* pObj, SmObject* p1, SmObject* p2,
                     SmObject* p3) override
    {
        m_pOut->push_back(m_lTag);
        m_pOut->push_back((int64_t)eOp);
        m_pOut->push_back(Index(pObj));
        m_pOut->push_back(pObj ? (int64_t)pObj->GetType() : 0);
        m_pOut->push_back(Index(p1));
        m_pOut->push_back(Index(p2));
        m_pOut->push_back(Index(p3));
        return SM_ERR; // ignored by Notify
    }
};

enum
{
    TCV_OPEN = 0,     // push SmTemporaryChangeValue(var, x)
    TCV_OPEN_SET = 1, // push SmTemporaryChangeValue(); then Set(var, x)
    TCV_CLOSE = 2,    // pop (destructor)
    TCV_CLEAR = 3,
    TCV_USE_ORIG = 4,
    TCV_USE_TEMP = 5,
    TCV_SET_NEW = 6, // SetNewValue(x)
    TCV_WRITE = 7,   // var = x
};

template <typename T>
int32_t run_tcv(T init, const int64_t* ops, const double* xs, int64_t n_ops,
                std::vector<double>& res)
{
    T var = init;
    std::vector<std::unique_ptr<SmTemporaryChangeValue<T>>> stack;
    res.push_back((double)var);
    for (int64_t i = 0; i < n_ops; ++i)
    {
        T x = (T)xs[i];
        switch (ops[i])
        {
        case TCV_OPEN: stack.emplace_back(new SmTemporaryChangeValue<T>(var, x)); break;
        case TCV_OPEN_SET:
            stack.emplace_back(new SmTemporaryChangeValue<T>());
            stack.back()->Set(var, x);
            break;
        case TCV_CLOSE: stack.pop_back(); break;
        case TCV_CLEAR: stack.back()->Clear(); break;
        case TCV_USE_ORIG: stack.back()->UseOrigValue(); break;
        case TCV_USE_TEMP: stack.back()->UseTempValue(); break;
        case TCV_SET_NEW: stack.back()->SetNewValue(x); break;
        case TCV_WRITE: var = x; break;
        default: return SC_ERR_ARG;
        }
        res.push_back((double)var);
    }
    while (!stack.empty())
    {
        stack.pop_back();
        res.push_back((double)var);
    }
    return SC_OK;
}

void put_box(std::vector<double>& res, const SmExtent3d& b)
{
    const SmPoint3d& lo = b.GetMin();
    const SmPoint3d& hi = b.GetMax();
    res.push_back(lo.x);
    res.push_back(lo.y);
    res.push_back(lo.z);
    res.push_back(hi.x);
    res.push_back(hi.y);
    res.push_back(hi.z);
}

void put_tree(std::vector<double>& res, const SmTree* pTree)
{
    res.push_back((double)pTree->GetNodeCount());
    SmTArray<SmObjectList*> sList;
    pTree->GetObjectList(sList);
    res.push_back((double)sList.GetSize());
    SmTreeNode* pTop = pTree->GetTopNode();
    res.push_back(pTop ? 1.0 : 0.0);
    if (pTop)
        put_box(res, pTop->GetBoundingBox());
}

template <typename P>
int64_t index_of(const std::vector<P*>& all, P* p)
{
    for (size_t i = 0; i < all.size(); ++i)
        if (all[i] == p)
            return (int64_t)i;
    return -1;
}

} // namespace

// Type system: per t in ts, (SmObject::IsKindOf(t), SmShape::IsKindOf(t)); then
// SmObject::GetType, SmShape::GetType, SmObject::AssertValid, and the two type
// strings as (length, chars...).
SC_EXPORT int32_t sc_geom_object_types(const int64_t* ts, int64_t n, int64_t* out, int64_t cap,
                                       int64_t* out_len)
{
    if (n < 0)
        return SC_ERR_ARG;
    SC_TRY
    SmObject sObj;
    SmTArray<SmBrep*> sB;
    SmTArray<SmFace*> sF;
    SmTArray<SmEdge*> sE;
    SmTArray<SmVertex*> sV;
    SmShape sShape(sB, sF, sE, sV);
    std::vector<int64_t> res;
    for (int64_t i = 0; i < n; ++i)
    {
        res.push_back(sObj.IsKindOf((SM_TYPE)ts[i]) ? 1 : 0);
        res.push_back(sShape.IsKindOf((SM_TYPE)ts[i]) ? 1 : 0);
    }
    res.push_back((int64_t)sObj.GetType());
    res.push_back((int64_t)sShape.GetType());
    res.push_back(sObj.AssertValid() ? 1 : 0);
    for (const TCHAR* s : {sObj.GetTypeString(), sShape.GetTypeString()})
    {
        size_t len = strlen(s);
        res.push_back((int64_t)len);
        for (size_t k = 0; k < len; ++k)
            res.push_back((int64_t)(unsigned char)s[k]);
    }
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

// Notify script. ops[8*n_ops] per op: (has_ctx, user_cb, sys_cb, op, caller, d1, d2, d3);
// object indices in [0, 6), data -1 = NULL. The caller's context is the shared
// SmContext (with the given callbacks installed) or NULL. Output: per op the count of
// Execute records, then each record (tag 1=user 2=sys, op, caller, caller type, d1, d2, d3).
SC_EXPORT int32_t sc_geom_object_notify(const int64_t* ops, int64_t n_ops, int64_t* out,
                                        int64_t cap, int64_t* out_len)
{
    if (n_ops < 0)
        return SC_ERR_ARG;
    for (int64_t i = 0; i < n_ops; ++i)
    {
        const int64_t* o = ops + 8 * i;
        for (int k = 0; k < 3; ++k)
            if (o[k] < 0 || o[k] > 1)
                return SC_ERR_ARG;
        if (o[3] < 0 || o[3] > (int64_t)SM_NO_UNKNOWN)
            return SC_ERR_ARG;
        if (o[4] < 0 || o[4] >= kNumObjects)
            return SC_ERR_ARG;
        for (int k = 5; k < 8; ++k)
            if (o[k] < -1 || o[k] >= kNumObjects)
                return SC_ERR_ARG;
    }
    SC_TRY
    SmContext sCtx;
    std::vector<SmObject> vObjs((size_t)kNumObjects);
    std::vector<SmObject*> vPtrs;
    for (SmObject& r : vObjs)
        vPtrs.push_back(&r);
    std::vector<int64_t> res;
    std::vector<int64_t> rec;
    ScRecorder sUser, sSys;
    sUser.m_lTag = 1;
    sSys.m_lTag = 2;
    sUser.m_pOut = sSys.m_pOut = &rec;
    sUser.m_pObjs = sSys.m_pObjs = &vPtrs;
    for (int64_t i = 0; i < n_ops; ++i)
    {
        const int64_t* o = ops + 8 * i;
        sCtx.SetUserNotifyCallback(o[1] ? &sUser : nullptr);
        sCtx.SetSysNotifyCallback(nullptr);
        sCtx.SetSysNotifyCallback(o[2] ? &sSys : nullptr);
        SmObject* pCaller = vPtrs[(size_t)o[4]];
        pCaller->SetContext(o[0] ? &sCtx : nullptr);
        auto data = [&](int64_t k) { return k < 0 ? (SmObject*)nullptr : vPtrs[(size_t)k]; };
        rec.clear();
        pCaller->Notify((SmNotifyOperation)o[3], data(o[5]), data(o[6]), data(o[7]));
        res.push_back((int64_t)(rec.size() / 7));
        res.insert(res.end(), rec.begin(), rec.end());
    }
    sCtx.SetUserNotifyCallback(nullptr);
    sCtx.SetSysNotifyCallback(nullptr);
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

// SmTemporaryChangeValue script on one variable. type 0 = double, 1 = SmBoolean (int),
// 2 = ULONG. ops[n_ops] and xs[n_ops] (operand, converted to the type). Output: the
// variable after construction, after every op, and after each remaining guard is
// destroyed (innermost first).
SC_EXPORT int32_t sc_geom_object_tcv(int32_t type, double init, const int64_t* ops,
                                     const double* xs, int64_t n_ops, double* out, int64_t cap,
                                     int64_t* out_len)
{
    if (type < 0 || type > 2 || n_ops < 0)
        return SC_ERR_ARG;
    int64_t depth = 0;
    for (int64_t i = 0; i < n_ops; ++i)
    {
        int64_t op = ops[i];
        if (op < TCV_OPEN || op > TCV_WRITE)
            return SC_ERR_ARG;
        if (type != 0 && !(xs[i] >= 0.0 && xs[i] <= 1e9 && xs[i] == (double)(int64_t)xs[i]))
            return SC_ERR_ARG;
        if (op == TCV_OPEN || op == TCV_OPEN_SET)
            ++depth;
        else if (op != TCV_WRITE && depth == 0)
            return SC_ERR_ARG;
        if (op == TCV_CLOSE)
            --depth;
    }
    if (type != 0 && !(init >= 0.0 && init <= 1e9 && init == (double)(int64_t)init))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> res;
    int32_t st = type == 0   ? run_tcv<double>(init, ops, xs, n_ops, res)
                 : type == 1 ? run_tcv<SmBoolean>((SmBoolean)init, ops, xs, n_ops, res)
                             : run_tcv<ULONG>((ULONG)init, ops, xs, n_ops, res);
    if (st != SC_OK)
        return st;
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

namespace
{
// The shape of `sel` over `pBrep` (null: no B-rep) whose faces, edges and vertices
// are numbered by `vAllF` / `vAllE` / `vAllV`; see sc_geom_object_shape.
int32_t shape_of(SmBrep* pBrep, const std::vector<SmFace*>& vAllF,
                 const std::vector<SmEdge*>& vAllE, const std::vector<SmVertex*>& vAllV,
                 const int64_t* sel, int64_t nf, int64_t ne, int64_t nv,
                 std::vector<double>& res)
{
    SmTArray<SmBrep*> sB;
    SmTArray<SmFace*> sF;
    SmTArray<SmEdge*> sE;
    SmTArray<SmVertex*> sV;
    if (sel[0])
        sB.Add(pBrep);
    const int64_t* idx = sel + 4;
    for (int64_t i = 0; i < nf; ++i, ++idx)
    {
        if (*idx < 0 || *idx >= (int64_t)vAllF.size())
            return SC_ERR_ARG;
        sF.Add(vAllF[(size_t)*idx]);
    }
    for (int64_t i = 0; i < ne; ++i, ++idx)
    {
        if (*idx < 0 || *idx >= (int64_t)vAllE.size())
            return SC_ERR_ARG;
        sE.Add(vAllE[(size_t)*idx]);
    }
    for (int64_t i = 0; i < nv; ++i, ++idx)
    {
        if (*idx < 0 || *idx >= (int64_t)vAllV.size())
            return SC_ERR_ARG;
        sV.Add(vAllV[(size_t)*idx]);
    }
    // SmShape dereferences a face's surface and an edge's curve (a listed face's edges
    // included)
    for (ULONG i = 0; i < sF.GetSize(); ++i)
    {
        if (sF[i]->GetSurface() == nullptr)
            return SC_ERR_ARG;
        SmTArray<SmEdge*> vFaceE;
        sF[i]->GetEdges(vFaceE);
        for (ULONG j = 0; j < vFaceE.GetSize(); ++j)
            if (vFaceE[j]->GetCurve() == nullptr)
                return SC_ERR_ARG;
    }
    for (ULONG i = 0; i < sE.GetSize(); ++i)
        if (sE[i]->GetCurve() == nullptr)
            return SC_ERR_ARG;
    if (sel[0])
    {
        for (SmFace* f : vAllF)
            if (f->GetSurface() == nullptr)
                return SC_ERR_ARG;
        for (SmEdge* e : vAllE)
            if (e->GetCurve() == nullptr)
                return SC_ERR_ARG;
        // the last listed object gives the trees their context
        if (nf + ne + nv == 0 && pBrep->GetInfiniteRegion() == nullptr)
            return SC_ERR_ARG;
    }
    SmShape sShape(sB, sF, sE, sV);
    SmTArray<SmFace*> vF;
    SmTArray<SmEdge*> vE;
    SmTArray<SmVertex*> vV;
    sShape.GetFaces(vF);
    sShape.GetEdges(vE);
    sShape.GetVertices(vV);
    res.push_back((double)vF.GetSize());
    res.push_back((double)vE.GetSize());
    res.push_back((double)vV.GetSize());
    for (ULONG i = 0; i < vF.GetSize(); ++i)
        res.push_back((double)index_of(vAllF, vF[i]));
    for (ULONG i = 0; i < vE.GetSize(); ++i)
        res.push_back((double)index_of(vAllE, vE[i]));
    for (ULONG i = 0; i < vV.GetSize(); ++i)
        res.push_back((double)index_of(vAllV, vV[i]));
    put_tree(res, sShape.GetVertexTree());
    put_tree(res, sShape.GetCurveTree());
    put_tree(res, sShape.GetSurfaceTree());
    for (SmFace* f : vAllF)
        res.push_back(sShape.IsInShape(f) ? 1.0 : 0.0);
    for (SmEdge* e : vAllE)
        res.push_back(sShape.IsInShape(e) ? 1.0 : 0.0);
    for (SmVertex* v : vAllV)
        res.push_back(sShape.IsInShape(v) ? 1.0 : 0.0);
    return 0;
}
} // namespace

// SmShape over the B-rep of the shared fixture `path` / `block` (`n_block == 0`: no
// B-rep, all lists empty). sel = (whole_brep, n_f, n_e, n_v, face indices..., edge
// indices..., vertex indices...) into the fixture walk's face / edge / vertex lists.
// Output: (#faces, #edges, #vertices of the shape, their walk indices in shape order),
// each tree (node count, object count, has top, top box), then IsInShape of every
// face, edge and vertex of the B-rep.
SC_EXPORT int32_t sc_geom_object_shape(const char* path, const double* block,
                                       int64_t n_block, const int64_t* sel, int64_t n_sel,
                                       double* out, int64_t cap, int64_t* out_len)
{
    if (n_block < 0 || (n_block > 0 && (path == nullptr || block == nullptr)) || sel == nullptr ||
        n_sel < 4 || out_len == nullptr)
        return SC_ERR_ARG;
    int64_t nf = sel[1], ne = sel[2], nv = sel[3];
    if (sel[0] < 0 || sel[0] > 1 || nf < 0 || ne < 0 || nv < 0 || 4 + nf + ne + nv != n_sel)
        return SC_ERR_ARG;
    if (n_block == 0 && (sel[0] != 0 || nf + ne + nv != 0))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> res;
    int32_t rc = 0;
    if (n_block == 0)
        rc = shape_of(nullptr, {}, {}, {}, sel, nf, ne, nv, res);
    else
        rc = sc_topology_fixture::apply(path, block, n_block,
                                        [&](const sc_topology_fixture::View& g) -> int32_t {
                                            return shape_of(&g.brep, g.faces, g.edges,
                                                            g.vertices, sel, nf, ne, nv, res);
                                        });
    if (rc != 0)
        return rc;
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

// The fixture B-rep's GetFaces, GetEdges and GetVertices orders as fixture walk indices:
// (n, indices...) per kind (the Rust import's list orders).
SC_EXPORT int32_t sc_geom_object_list_orders(const char* path, const double* block,
                                             int64_t n_block, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (path == nullptr || block == nullptr || n_block < 1 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> res;
    int32_t rc = sc_topology_fixture::apply(
        path, block, n_block, [&](const sc_topology_fixture::View& g) -> int32_t {
            SmTArray<SmFace*> vF;
            SmTArray<SmEdge*> vE;
            SmTArray<SmVertex*> vV;
            g.brep.GetFaces(vF);
            g.brep.GetEdges(vE);
            g.brep.GetVertices(vV);
            res.push_back((double)vF.GetSize());
            for (ULONG i = 0; i < vF.GetSize(); ++i)
                res.push_back((double)index_of(g.faces, vF[i]));
            res.push_back((double)vE.GetSize());
            for (ULONG i = 0; i < vE.GetSize(); ++i)
                res.push_back((double)index_of(g.edges, vE[i]));
            res.push_back((double)vV.GetSize());
            for (ULONG i = 0; i < vV.GetSize(); ++i)
                res.push_back((double)index_of(g.vertices, vV[i]));
            return 0;
        });
    if (rc != 0)
        return rc;
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}
