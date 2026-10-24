// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.tree`. Wraps upstream SmTree / SmTreeNode / SmObjectList
// (source/SMLib/src/SmTree.cpp, source/SMLib/inc/SmTree.h).
//
// Two program interpreters; the program encodings are shared with
// crates/solidcore-ffi/src/units/geom_tree.rs (which validates them first).
//
// Spatial trees: sc_geom_tree_spatial_{ints,floats}(prog, n, out, cap, out_len) run the
// same flat double program and return its integer or float output stream.
//   0 e6            SmTree(extent)                       (must be first, or op 1)
//   1 p3 size lst   SmTree(point, size, NULL, lst ? &list : NULL)
//   2 e6 obj geom   AddToSpatialTree                      ints: status
//   3 e6 obj        RmFromSpatialTree                     ints: result
//   4 e6            FindNode                              ints: node index or -1
//   5 e6            GetObjectsInBox                       ints: status, n, objs
//   6 e6            IntersectsBox                         ints: result
//   7               dump                                  ints: top, n_nodes, per node parent c1 c2
//                                                         aux geom n_items items..., then
//                                                         GetObjectList: status, n, objs;
//                                                         floats: node boxes, item boxes, list boxes
//   8 node          SmTreeNode::GetObjectList             ints: status, n, objs
//   9               type/owner queries                    ints: GetType, IsKindOf x3, owner null,
//                                                         owner object null, node owner object null
// Objects are fake SmObject pointers g_objs + id (never dereferenced on these paths);
// obj -1 is NULL. Nodes are reported by their index in GetAllTreeNodes (allocation order).
//
// Subdivision trees: sc_geom_tree_subdiv(nodes, n_nodes, prog, n_prog, out, cap, out_len)
// builds n_nodes SmTreeNodes (node 0 = top) of an SmTree(NULL), each from 9 int64s:
// parent c1 c2 aux_type split_dir marked node_class face surface (-1 = NULL), every node
// with its own SmBezierPatch (a SmBezierAux2d subclass). Program (int64):
//   0 node   neighbours: Left, Right, Bot, Top, GetNeighbors (each n, indices)
//   1 node   PropagateToParents
//   2 node marked class face   set aux fields
//   3        dump marks: per node marked, class, face
//   4 node   GetFirstBezierPatch: ints: 0 = NULL, 1 = non-NULL;
//            floats: native n m p q r s, homogeneous control net, U/V knots.
//   5 node   SmTreeNode::GetOwnerObject: 0 = NULL
//   6 node   clear construction Bezier pointer (keep subdivision surface).
//   7 surface attach a real SmSurfaceCache owner for surface id (-1 = no owner).
// Native payloads are rational bilinear surfaces: construction seed = node + 1,
// subdivision seed = surface id + n_nodes + 1. See native_patch in the runner.
#include "sc_shim.h"

#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsSrf.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTree.h>

#include <map>
#include <memory>
#include <vector>
#include <nurbs.h>

namespace
{
const int64_t kMaxObjects = 4096;
alignas(16) char g_objs[kMaxObjects];

SmObject* obj_ptr(double id)
{
    long long i = (long long)id;
    return i < 0 ? nullptr : reinterpret_cast<SmObject*>(g_objs + i);
}

int64_t obj_id(const void* p)
{
    return p == nullptr ? -1 : (int64_t)(static_cast<const char*>(p) - g_objs);
}

SmExtent3d ext(const double* d) { return SmExtent3d(d[0], d[1], d[2], d[3], d[4], d[5]); }

void put_ext(std::vector<double>& o, const SmExtent3d& e)
{
    o.push_back(e.GetMin().x);
    o.push_back(e.GetMin().y);
    o.push_back(e.GetMin().z);
    o.push_back(e.GetMax().x);
    o.push_back(e.GetMax().y);
    o.push_back(e.GetMax().z);
}

int64_t st(SmStatus s) { return s == SM_SUCCESS ? 0 : (int64_t)s; }

int64_t op_size(int op)
{
    switch (op)
    {
    case 0: return 6;
    case 1: return 5;
    case 2: return 8;
    case 3: return 7;
    case 4: return 6;
    case 5: return 6;
    case 6: return 6;
    case 7: return 0;
    case 8: return 1;
    case 9: return 0;
    default: return -1;
    }
}

struct Index
{
    SmTArray<SmTreeNode*> nodes;
    std::map<const SmTreeNode*, int64_t> idx;
    explicit Index(const SmTree& t)
    {
        t.GetAllTreeNodes(nodes);
        for (ULONG i = 0; i < nodes.GetSize(); i++)
            idx[nodes[i]] = (int64_t)i;
    }
    int64_t of(const SmTreeNode* n) const
    {
        if (n == nullptr)
            return -1;
        auto it = idx.find(n);
        return it == idx.end() ? -2 : it->second;
    }
};

// Runs the ops of a spatial program after its constructor (at p[i]).
int32_t run_ops(SmTree* tree, bool point, const double* p, int64_t n, int64_t i, std::vector<int64_t>& oi,
                std::vector<double>& of)
{
    while (i < n)
    {
        int op = (int)p[i];
        int64_t sz = op_size(op);
        if (sz < 0 || i + 1 + sz > n)
            return SC_ERR_ARG;
        const double* a = p + i + 1;
        if (op <= 1)
            return SC_ERR_ARG; // exactly one constructor, first
        switch (op)
        {
        case 2:
        {
            if (point)
                return SC_ERR_ARG; // point trees have no object memory manager
            int g = (int)a[7];
            if (g < 0 || g > 3)
                return SC_ERR_ARG;
            oi.push_back(st(tree->AddToSpatialTree(ext(a), obj_ptr(a[6]), (SmNodeGeomType)g)));
            break;
        }
        case 3: oi.push_back(tree->RmFromSpatialTree(ext(a), obj_ptr(a[6])) ? 1 : 0); break;
        case 4:
        {
            Index ix(*tree);
            oi.push_back(ix.of(tree->FindNode(ext(a))));
            break;
        }
        case 5:
        {
            SmTArray<SmObject*> objs;
            oi.push_back(st(tree->GetObjectsInBox(ext(a), objs)));
            oi.push_back((int64_t)objs.GetSize());
            for (ULONG k = 0; k < objs.GetSize(); k++)
                oi.push_back(obj_id(objs[k]));
            break;
        }
        case 6: oi.push_back(tree->IntersectsBox(ext(a)) ? 1 : 0); break;
        case 7:
        {
            Index ix(*tree);
            oi.push_back(ix.of(tree->GetTopNode()));
            oi.push_back((int64_t)ix.nodes.GetSize());
            for (ULONG k = 0; k < ix.nodes.GetSize(); k++)
            {
                const SmTreeNode* nd = ix.nodes[k];
                oi.push_back(ix.of(nd->m_pParent));
                oi.push_back(ix.of(nd->m_pChild1));
                oi.push_back(ix.of(nd->m_pChild2));
                oi.push_back((int64_t)nd->m_eAuxDataType);
                oi.push_back((int64_t)nd->m_eGeomType);
                put_ext(of, ((SmTreeNode*)nd)->GetBoundingBox());
                SmTArray<SmObjectList*> items;
                if (nd->m_eAuxDataType == SM_AD_OBJECT_LIST)
                    nd->GetObjectList(items);
                oi.push_back((int64_t)items.GetSize());
                for (ULONG m = 0; m < items.GetSize(); m++)
                {
                    oi.push_back(obj_id(items[m]->m_pObject));
                    put_ext(of, items[m]->m_sBBox);
                }
            }
            SmTArray<SmObjectList*> all;
            oi.push_back(st(tree->GetObjectList(all)));
            oi.push_back((int64_t)all.GetSize());
            for (ULONG m = 0; m < all.GetSize(); m++)
            {
                oi.push_back(obj_id(all[m]->m_pObject));
                put_ext(of, all[m]->m_sBBox);
            }
            break;
        }
        case 8:
        {
            Index ix(*tree);
            long long k = (long long)a[0];
            if (k < 0 || k >= (long long)ix.nodes.GetSize())
            {
                oi.push_back(-3);
                break;
            }
            SmTArray<SmObjectList*> items;
            SmStatus s = ix.nodes[(ULONG)k]->GetObjectList(items);
            oi.push_back(st(s));
            oi.push_back((int64_t)items.GetSize());
            for (ULONG m = 0; m < items.GetSize(); m++)
                oi.push_back(obj_id(items[m]->m_pObject));
            break;
        }
        case 9:
        {
            oi.push_back((int64_t)tree->GetType());
            oi.push_back(tree->IsKindOf(SmTree_TYPE) ? 1 : 0);
            oi.push_back(tree->IsKindOf(SmObject_TYPE) ? 1 : 0);
            oi.push_back(tree->IsKindOf(SmTree_TYPE + 1) ? 1 : 0);
            oi.push_back(tree->GetOwner() == nullptr ? 1 : 0);
            oi.push_back(tree->GetOwnerObject() == nullptr ? 1 : 0);
            oi.push_back(tree->GetTopNode()->GetOwnerObject() == nullptr ? 1 : 0);
            break;
        }
        default: return SC_ERR_ARG;
        }
        i += 1 + sz;
    }
    return SC_OK;
}

// Runs a spatial program; returns SC_OK or SC_ERR_ARG.
int32_t run_spatial(const double* p, int64_t n, std::vector<int64_t>& oi, std::vector<double>& of)
{
    if (n < 1)
        return SC_ERR_ARG;
    int op = (int)p[0];
    if (op == 0)
    {
        if (n < 7)
            return SC_ERR_ARG;
        SmTree tree(ext(p + 1));
        return run_ops(&tree, false, p, n, 7, oi, of);
    }
    if (op == 1)
    {
        if (n < 6)
            return SC_ERR_ARG;
        SmObjectList list; // the point tree's caller-owned object list
        SmTree tree(SmPoint3d(p[1], p[2], p[3]), p[4], nullptr, p[5] != 0.0 ? &list : nullptr);
        return run_ops(&tree, true, p, n, 6, oi, of);
    }
    return SC_ERR_ARG;
}

void put_list(std::vector<int64_t>& o, const SmTArray<SmTreeNode*>& l, const std::vector<SmTreeNode>& nodes)
{
    o.push_back((int64_t)l.GetSize());
    for (ULONG k = 0; k < l.GetSize(); k++)
        o.push_back((int64_t)(l[k] - nodes.data()));
}
} // namespace

SC_EXPORT int32_t sc_geom_tree_spatial_ints(const double* prog, int64_t n, int64_t* out, int64_t cap,
                                            int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t s = run_spatial(prog, n, oi, of);
    if (s != SC_OK)
        return s;
    return sc_copy_out(oi.data(), (int64_t)oi.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_tree_spatial_floats(const double* prog, int64_t n, double* out, int64_t cap,
                                              int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t s = run_spatial(prog, n, oi, of);
    if (s != SC_OK)
        return s;
    return sc_copy_out(of.data(), (int64_t)of.size(), out, cap, out_len);
    SC_CATCH
}

static int32_t run_subdiv(const int64_t* desc, int64_t n_nodes, const int64_t* prog, int64_t n_prog,
                          double scale, std::vector<int64_t>& o, std::vector<double>& values)
{
    if (n_nodes < 1 || n_prog < 0)
        return SC_ERR_ARG;
    for (int64_t k = 0; k < n_nodes; k++)
    {
        const int64_t* d = desc + 9 * k;
        for (int j = 0; j < 3; j++)
            if (d[j] < -1 || d[j] >= n_nodes)
                return SC_ERR_ARG;
        if (d[3] < 0 || d[3] > 8 || d[4] < 0 || d[4] > 2 || d[6] < 0 || d[6] > 3)
            return SC_ERR_ARG;
        if (d[7] < -1 || d[7] >= kMaxObjects || d[8] < -1 || d[8] >= kMaxObjects)
            return SC_ERR_ARG;
    }
    SC_TRY
    SmContext ctx;
    std::vector<std::unique_ptr<SmBSplineSurface>> surfaces;
    auto make_surface = [&](int64_t seed) -> SmBSplineSurface* {
        gw_SURFACE* s = sm_AllocateNurbSurface(1, 1, 1, 1, 3, 3);
        if (!s)
            throw std::bad_alloc();
        for (int u = 0; u < 2; ++u)
            for (int v = 0; v < 2; ++v)
            {
                auto& p = s->net->Pw[u][v];
                p.w = 1.0 + 0.25 * (seed % 3 + u + v);
                p.x = (seed + u) * scale * p.w;
                p.y = (v - seed) * scale * p.w;
                p.z = (seed + u * v) * scale * p.w;
            }
        for (int k = 0; k < 4; ++k)
        {
            s->knu->U[k] = k < 2 ? -2.0 : 3.0;
            s->knv->U[k] = k < 2 ? 0.5 : 4.0;
        }
        surfaces.emplace_back(new (ctx) SmBSplineSurface(s, FALSE, nullptr));
        return surfaces.back().get();
    };
    std::unique_ptr<SmSurfaceCache> owner_cache;
    SmTree tree((SmObject*)nullptr);
    std::vector<SmTreeNode> nodes((size_t)n_nodes);
    std::vector<SmBezierPatch> aux((size_t)n_nodes);
    std::map<int64_t, SmBSplineSurface*> subdivisions;
    auto surface_for = [&](int64_t id) -> SmBSplineSurface* {
        auto it = subdivisions.find(id);
        if (it == subdivisions.end())
            it = subdivisions.emplace(id, make_surface(id + n_nodes + 1)).first;
        return it->second;
    };
    auto ptr = [&](int64_t k) -> SmTreeNode* { return k < 0 ? nullptr : &nodes[(size_t)k]; };
    for (int64_t k = 0; k < n_nodes; k++)
    {
        const int64_t* d = desc + 9 * k;
        SmTreeNode& nd = nodes[(size_t)k];
        nd.m_pTree = &tree;
        nd.m_pParent = ptr(d[0]);
        nd.m_pChild1 = ptr(d[1]);
        nd.m_pChild2 = ptr(d[2]);
        nd.m_eAuxDataType = (SmAuxDataType)d[3];
        SmBezierPatch& a = aux[(size_t)k];
        a.m_eSplitDir = (SmSurfParamType)d[4];
        a.m_bIsMarked = d[5] != 0 ? TRUE : FALSE;
        a.m_eNodeClass = (SmNodeClassType)d[6];
        a.m_pFace = obj_ptr((double)d[7]);
        if (d[3] == SM_AD_BEZIER_SURFACE)
            a.m_pBezier = make_surface(k + 1)->GetGwNurbPointer();
        if (d[8] >= 0)
        {
            a.mBA_pSurface = surface_for(d[8]);
        }
        nd.m_pData = &a;
    }
    int64_t i = 0;
    while (i < n_prog)
    {
        int64_t op = prog[i];
        int64_t sz = op == 0 || op == 1 || (op >= 4 && op <= 7) ? 1 : op == 2 ? 4 : op == 3 ? 0 : -1;
        if (sz < 0 || i + 1 + sz > n_prog)
            return SC_ERR_ARG;
        const int64_t* a = prog + i + 1;
        if (sz >= 1 && op != 7 && (a[0] < 0 || a[0] >= n_nodes))
            return SC_ERR_ARG;
        switch (op)
        {
        case 0:
        {
            const SmTreeNode& nd = nodes[(size_t)a[0]];
            SmTArray<SmTreeNode*> l;
            nd.GetLeftNeighbors(l);
            put_list(o, l, nodes);
            l.ReSet();
            nd.GetRightNeighbors(l);
            put_list(o, l, nodes);
            l.ReSet();
            nd.GetBotNeighbors(l);
            put_list(o, l, nodes);
            l.ReSet();
            nd.GetTopNeighbors(l);
            put_list(o, l, nodes);
            l.ReSet();
            nd.GetNeighbors(l);
            put_list(o, l, nodes);
            break;
        }
        case 1: nodes[(size_t)a[0]].PropagateToParents(); break;
        case 2:
        {
            if (a[2] < 0 || a[2] > 3 || a[3] < -1 || a[3] >= kMaxObjects)
                return SC_ERR_ARG;
            SmBezierAux2d& x = aux[(size_t)a[0]];
            x.m_bIsMarked = a[1] != 0 ? TRUE : FALSE;
            x.m_eNodeClass = (SmNodeClassType)a[2];
            x.m_pFace = obj_ptr((double)a[3]);
            break;
        }
        case 3:
            for (int64_t k = 0; k < n_nodes; k++)
            {
                o.push_back(aux[(size_t)k].m_bIsMarked ? 1 : 0);
                o.push_back((int64_t)aux[(size_t)k].m_eNodeClass);
                o.push_back(obj_id(aux[(size_t)k].m_pFace));
            }
            break;
        case 4:
        {
            const gw_SURFACE* s = nodes[(size_t)a[0]].GetFirstBezierPatch();
            o.push_back(s == nullptr ? 0 : 1);
            if (s)
            {
                values.insert(values.end(), {(double)s->net->n, (double)s->net->m,
                    (double)s->p, (double)s->q, (double)s->knu->m, (double)s->knv->m});
                for (gw_INDEX u = 0; u <= s->net->n; ++u)
                    for (gw_INDEX v = 0; v <= s->net->m; ++v)
                    {
                        const auto& p = s->net->Pw[u][v];
                        values.insert(values.end(), {p.x, p.y, p.z, p.w});
                    }
                for (gw_INDEX k = 0; k <= s->knu->m; ++k)
                    values.push_back(s->knu->U[k]);
                for (gw_INDEX k = 0; k <= s->knv->m; ++k)
                    values.push_back(s->knv->U[k]);
            }
            break;
        }
        case 5: o.push_back(nodes[(size_t)a[0]].GetOwnerObject() == nullptr ? 0 : 1); break;
        case 6: aux[(size_t)a[0]].m_pBezier = nullptr; break;
        case 7:
            if (a[0] < -1 || a[0] >= kMaxObjects)
                return SC_ERR_ARG;
            tree.SetOwnerObject(nullptr);
            owner_cache.reset();
            if (a[0] >= 0)
            {
                owner_cache.reset(new (ctx) SmSurfaceCache(*surface_for(a[0]),
                                                          SmExtent2d(-2.0, 0.5, 3.0, 4.0)));
                tree.SetOwnerObject(owner_cache.get());
            }
            break;
        default: return SC_ERR_ARG;
        }
        i += 1 + sz;
    }
    return SC_OK;
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_tree_subdiv(const int64_t* desc, int64_t n_nodes, const int64_t* prog, int64_t n_prog,
                                      int64_t* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> ints;
    std::vector<double> values;
    int32_t status = run_subdiv(desc, n_nodes, prog, n_prog, 1.0, ints, values);
    if (status != SC_OK)
        return status;
    return sc_copy_out(ints.data(), (int64_t)ints.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_tree_subdiv_patches(const int64_t* desc, int64_t n_nodes, const int64_t* prog,
                                              int64_t n_prog, double scale, double* out, int64_t cap,
                                              int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> ints;
    std::vector<double> values;
    int32_t status = run_subdiv(desc, n_nodes, prog, n_prog, scale, ints, values);
    if (status != SC_OK)
        return status;
    return sc_copy_out(values.data(), (int64_t)values.size(), out, cap, out_len);
    SC_CATCH
}
