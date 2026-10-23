// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.grid`. Wraps upstream SmGrid / SmVoxel / SmGridElement /
// SmHierarchyElement (source/SMLib/src/SmGrid.cpp, source/SMLib/inc/SmGrid.h).
//
// sc_geom_grid_{ints,floats}(prog, n, out, cap, out_len) run the same flat double program
// against one grid and return its integer or float output stream. The encoding is shared
// with crates/solidcore-ffi/src/units/geom_grid.rs, which validates it first (so nothing
// here can index outside the grid or dereference a fake pointer).
//   0  e6 nx ny nz        SmGrid(bbox, lSize)                     (first, exactly once)
//   1  type obj e6        AddElement(fake ptr obj, SM_GE_POLYBREP (1) | SM_GE_USERDATA (3), bbox)
//                         ints: status
//   2  kind surf tol e6   AddHierarchyElement(new SmHierarchyElement(tol) (kind 0) |
//                         new SmSurfaceHierarchyElement(fake surf, tol) (kind 1), bbox)
//                         ints: status
//   3  e6                 FindVoxelsInBox                          ints: status, n, n x (ix iy iz)
//   4  p3                 GetVoxelAddress                          ints: ix iy iz outside
//   5  ix iy iz           GetVoxelBBox                             floats: e6
//   6  p3 v3              Setup3DDDA                               ints: status, hits_nothing
//                         [+ dda dump when status 0 and the ray hits the grid]
//   7  t                  Step3DDDA(t)  ints: -3 when the walk is not ready (no hitting setup,
//                         or the last step failed or left the grid); else status, and on
//                         success exited, n, n x (type obj); floats: mins, maxs
//                         [+ dda dump when not exited]
//   8                     dump: per voxel (x-major) n_elems (-1 = null list) + (type obj)...,
//                         n_hier (-1 = null) + hierarchy indices...; per distinct element in
//                         that voxel order: (type obj) ints, centre radius e6 floats; per
//                         hierarchy element: type, back-ref index ints, centre radius e6
//                         floats; grid: voxel size, voxel tolerance, current mark (int)
//   9  h                  ExpandHierarchy of hierarchy element h:  ints: status, or -2 (not
//                         run: a surface element needs a real SmSurface / SmSurfaceCache)
//   10 e6                 AddElement(SM_GE_SURFACE): ints: -2 (not run: needs a real
//                         surface subdivision tree node)
// Explicit, unit-local cache extension (legacy opcodes above are unchanged):
//   11 id origin3 u3 v3 domain4 spans2   define/build a real affine B-spline cache
//   12 id tol e6                      add its real SmSurfaceHierarchyElement
//   13 h                              call that hierarchy's real ExpandHierarchy
//   14 id leaf_ordinal e6              AddElement(actual cache node, SM_GE_SURFACE)
//   15                                observe caches, surface pool and voxel lists
// Ops 11–14 return their status (construction 0 only after a valid cache exists).
// 15 ints: surface count, (input id, eligible-node count)...; hierarchy count and
// real owner ids (-1 for legacy); surface-pool count, (owner id, leaf ordinal)...;
// then x-major voxels: element count/-1, (legacy type/id, surface-pool index/-1)...,
// hierarchy count/-1 and hierarchy indices. Floats: each cache leaf's domain4/box6,
// then each pool entry's sphere4/base box6 and 51 leaf values (domain4, node box6,
// midpoint/DU/DV/normal12, pseudo basis9/intervals6, polar state1/domain4/basis9).
// dda dump: ints address, then per axis with next_t < SM_BIG_DOUBLE: step out; floats ray
// point, ray vector, ray t, current point, next_t, delta_t, the three delta vectors, then
// per axis with next_t < SM_BIG_DOUBLE: next plane point.
// Objects are fake pointers g_objs + id (stored, never dereferenced); elements are
// reported as (type code, object id).
#include "sc_shim.h"

#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmGrid.h>
#include <SmNurbsSrf.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <map>
#include <memory>
#include <set>
#include <stdexcept>
#include <utility>
#include <vector>

namespace
{
const int64_t kMaxObjects = 4096;
alignas(16) char g_objs[kMaxObjects];

void* obj_ptr(double id) { return reinterpret_cast<void*>(g_objs + (long long)id); }

int64_t obj_id(const void* p) { return p == nullptr ? -1 : (int64_t)(static_cast<const char*>(p) - g_objs); }

SmExtent3d ext(const double* d) { return SmExtent3d(d[0], d[1], d[2], d[3], d[4], d[5]); }

void put3(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void put_ext(std::vector<double>& o, const SmExtent3d& e)
{
    put3(o, e.GetMin());
    put3(o, e.GetMax());
}

int64_t st(SmStatus s) { return s == SM_SUCCESS ? 0 : (int64_t)s; }

int64_t op_size(int op)
{
    switch (op)
    {
    case 0: return 9;
    case 1: return 8;
    case 2: return 9;
    case 3: return 6;
    case 4: return 3;
    case 5: return 3;
    case 6: return 6;
    case 7: return 1;
    case 8: return 0;
    case 9: return 1;
    case 10: return 6;
    case 11: return 16;
    case 12: return 8;
    case 13: return 1;
    case 14: return 8;
    case 15: return 0;
    default: return -1;
    }
}

// Exposes the protected members of SmGrid read by the dumps.
class ShimGrid : public SmGrid
{
  public:
    ShimGrid(const SmExtent3d& b, ULONG lSize[3]) : SmGrid(b, lSize) {}

    long size(int i) const { return m_lSize[i]; }

    std::vector<SmSurfaceGridElement*> surface_elements()
    {
        SmTArray<void*> active;
        m_sSurfaceElementMgr.GetActiveElements(active);
        std::vector<SmSurfaceGridElement*> out;
        for (ULONG i = 0; i < active.GetSize(); ++i)
            out.push_back(static_cast<SmSurfaceGridElement*>(active[i]));
        return out;
    }

    void dump_dda(std::vector<int64_t>& oi, std::vector<double>& of) const
    {
        for (int i = 0; i < 3; i++)
            oi.push_back(m_lAddress[i]);
        const double next[3] = {m_dNextT.x, m_dNextT.y, m_dNextT.z};
        const long step[3] = {m_lStepX, m_lStepY, m_lStepZ};
        const long out[3] = {m_lOutX, m_lOutY, m_lOutZ};
        const SmPoint3d* np[3] = {&m_sNextX, &m_sNextY, &m_sNextZ};
        for (int i = 0; i < 3; i++)
            if (next[i] < SM_BIG_DOUBLE)
            {
                oi.push_back(step[i]);
                oi.push_back(out[i]);
            }
        put3(of, m_sRayPoint);
        put3(of, m_sRayVector);
        of.push_back(m_dRayT);
        put3(of, m_sCurrPoss);
        put3(of, m_dNextT);
        put3(of, m_dDeltaT);
        put3(of, m_sDeltaXVec);
        put3(of, m_sDeltaYVec);
        put3(of, m_sDeltaZVec);
        for (int i = 0; i < 3; i++)
            if (next[i] < SM_BIG_DOUBLE)
                put3(of, *np[i]);
    }

    void dump_grid(std::vector<int64_t>& oi, std::vector<double>& of) const
    {
        put3(of, m_sVoxelSize);
        of.push_back(m_dVoxelTolerance);
        oi.push_back((int64_t)m_lCurrentMark);
    }
};

void put_elem(std::vector<int64_t>& oi, SmGridElement* e)
{
    oi.push_back((int64_t)e->GetType());
    if (e->GetType() == SmPolygonGridElement_TYPE)
        oi.push_back(obj_id(((SmPolygonGridElement*)e)->GetElement()));
    else if (e->GetType() == SmUserGridElement_TYPE)
        oi.push_back(obj_id(((SmUserGridElement*)e)->GetElement()));
    else
        oi.push_back(-2);
}

void put_sphere_box(std::vector<double>& of, const SmGridElement& e)
{
    put3(of, e.GetSphereCenter());
    of.push_back(e.GetSphereRadius());
    put_ext(of, e.GetBoundingBox());
}

#include "geom_grid_native.inc"

int32_t run(const double* p, int64_t n, std::vector<int64_t>& oi, std::vector<double>& of)
{
    if (n < 10 || (int)p[0] != 0)
        return SC_ERR_ARG;
    SmContext sCtx;
    NativeCaches caches;
    ULONG lSize[3] = {(ULONG)p[7], (ULONG)p[8], (ULONG)p[9]};
    std::unique_ptr<ShimGrid> owned_grid(new (sCtx) ShimGrid(ext(p + 1), lSize));
    ShimGrid* g = owned_grid.get();
    std::vector<SmHierarchyElement*> hier;
    std::map<const SmHierarchyElement*, int64_t> hidx;
    bool ready = false;

    int64_t i = 10;
    int32_t rc = SC_OK;
    while (i < n)
    {
        int op = (int)p[i];
        int64_t sz = op_size(op);
        if (sz < 0 || op == 0 || i + 1 + sz > n)
        {
            rc = SC_ERR_ARG;
            break;
        }
        const double* a = p + i + 1;
        switch (op)
        {
        case 1:
        {
            SmGridElementType t = (int)a[0] == 1 ? SM_GE_POLYBREP : SM_GE_USERDATA;
            oi.push_back(st(g->AddElement(obj_ptr(a[1]), t, ext(a + 2))));
            break;
        }
        case 2:
        {
            SmHierarchyElement* h = (int)a[0] == 0
                                        ? new (sCtx) SmHierarchyElement(a[2])
                                        : new (sCtx) SmSurfaceHierarchyElement((SmSurface*)obj_ptr(a[1]), a[2]);
            hidx[h] = (int64_t)hier.size();
            hier.push_back(h);
            oi.push_back(st(g->AddHierarchyElement(h, ext(a + 3))));
            break;
        }
        case 3:
        {
            SmTArray<SmVoxel*> sVoxels;
            oi.push_back(st(g->FindVoxelsInBox(ext(a), sVoxels)));
            oi.push_back((int64_t)sVoxels.GetSize());
            // map each voxel pointer back to its address
            for (ULONG k = 0; k < sVoxels.GetSize(); k++)
            {
                bool found = false;
                for (long ix = 0; ix < g->size(0) && !found; ix++)
                    for (long iy = 0; iy < g->size(1) && !found; iy++)
                        for (long iz = 0; iz < g->size(2) && !found; iz++)
                        {
                            long ad[3] = {ix, iy, iz};
                            if (g->GetVoxel(ad) == sVoxels[k])
                            {
                                oi.push_back(ix);
                                oi.push_back(iy);
                                oi.push_back(iz);
                                found = true;
                            }
                        }
                if (!found)
                {
                    oi.push_back(-1);
                    oi.push_back(-1);
                    oi.push_back(-1);
                }
            }
            break;
        }
        case 4:
        {
            long ad[3] = {0, 0, 0};
            SmBoolean bOut = FALSE;
            SmStatus s = g->GetVoxelAddress(SmPoint3d(a[0], a[1], a[2]), ad, bOut);
            (void)s;
            oi.push_back(ad[0]);
            oi.push_back(ad[1]);
            oi.push_back(ad[2]);
            oi.push_back(bOut ? 1 : 0);
            break;
        }
        case 5:
        {
            long ad[3] = {(long)a[0], (long)a[1], (long)a[2]};
            put_ext(of, g->GetVoxelBBox(ad));
            break;
        }
        case 6:
        {
            SmBoolean bHits = FALSE;
            SmStatus s = g->Setup3DDDA(SmPoint3d(a[0], a[1], a[2]), SmVector3d(a[3], a[4], a[5]), bHits);
            oi.push_back(st(s));
            oi.push_back(bHits ? 1 : 0);
            ready = s == SM_SUCCESS && !bHits;
            if (ready)
                g->dump_dda(oi, of);
            break;
        }
        case 7:
        {
            if (!ready)
            {
                oi.push_back(-3);
                break;
            }
            SmBoolean bExited = FALSE;
            long cur[3] = {0, 0, 0};
            SmTArray<SmGridElement*> sElems;
            SmTArray<double> sMin, sMax;
            SmStatus s = g->Step3DDDA(a[0], bExited, cur, sElems, sMin, sMax);
            oi.push_back(st(s));
            if (s != SM_SUCCESS)
            {
                ready = false;
                break;
            }
            oi.push_back(bExited ? 1 : 0);
            oi.push_back((int64_t)sElems.GetSize());
            for (ULONG k = 0; k < sElems.GetSize(); k++)
                put_elem(oi, sElems[k]);
            for (ULONG k = 0; k < sMin.GetSize(); k++)
                of.push_back(sMin[k]);
            for (ULONG k = 0; k < sMax.GetSize(); k++)
                of.push_back(sMax[k]);
            if (bExited)
                ready = false;
            else
                g->dump_dda(oi, of);
            break;
        }
        case 8:
        {
            std::vector<SmGridElement*> order;
            std::set<SmGridElement*> seen;
            for (long ix = 0; ix < g->size(0); ix++)
                for (long iy = 0; iy < g->size(1); iy++)
                    for (long iz = 0; iz < g->size(2); iz++)
                    {
                        long ad[3] = {ix, iy, iz};
                        SmVoxel* v = g->GetVoxel(ad);
                        SmTArray<SmGridElement*>* pe = v->GetGridElements();
                        if (pe == nullptr)
                            oi.push_back(-1);
                        else
                        {
                            oi.push_back((int64_t)pe->GetSize());
                            for (ULONG k = 0; k < pe->GetSize(); k++)
                            {
                                SmGridElement* e = (*pe)[k];
                                put_elem(oi, e);
                                if (seen.insert(e).second)
                                    order.push_back(e);
                            }
                        }
                        SmTArray<SmHierarchyElement*>* ph = v->GetHierarchyElements();
                        if (ph == nullptr)
                            oi.push_back(-1);
                        else
                        {
                            oi.push_back((int64_t)ph->GetSize());
                            for (ULONG k = 0; k < ph->GetSize(); k++)
                            {
                                auto it = hidx.find((*ph)[k]);
                                oi.push_back(it == hidx.end() ? -2 : it->second);
                            }
                        }
                    }
            for (SmGridElement* e : order)
            {
                put_elem(oi, e);
                put_sphere_box(of, *e);
            }
            for (SmHierarchyElement* h : hier)
            {
                SmHierarchyGridElement& ge = h->GetHierarchyGridElement();
                oi.push_back((int64_t)ge.GetType());
                auto it = hidx.find(ge.GetElement());
                oi.push_back(ge.GetElement() == nullptr ? -1 : (it == hidx.end() ? -2 : it->second));
                put_sphere_box(of, ge);
            }
            g->dump_grid(oi, of);
            break;
        }
        case 9:
        {
            SmHierarchyElement* h = hier[(size_t)a[0]];
            if (h->GetSurface() != nullptr)
                oi.push_back(-2);
            else
                oi.push_back(st(h->ExpandHierarchy(g)));
            break;
        }
        case 10:
            oi.push_back(-2);
            break;
        case 11:
            caches.define(sCtx, a);
            oi.push_back(0);
            break;
        case 12:
        {
            NativeSurface& s = caches.find((int64_t)a[0]);
            SmHierarchyElement* h = new (sCtx) SmSurfaceHierarchyElement(s.surface.get(), a[1]);
            hidx[h] = (int64_t)hier.size();
            hier.push_back(h);
            caches.hierarchies[h] = s.id;
            oi.push_back(st(g->AddHierarchyElement(h, ext(a + 2))));
            break;
        }
        case 13:
        {
            SmHierarchyElement* h = hier.at((size_t)a[0]);
            if (caches.hierarchies.count(h) != 1)
                return SC_ERR_ARG;
            oi.push_back(st(h->ExpandHierarchy(g)));
            break;
        }
        case 14:
        {
            NativeSurface& s = caches.find((int64_t)a[0]);
            // The ordinal is checked against this actual cache, not a fake node id.
            if (a[1] < 0 || a[1] >= (double)s.leaves.size())
                return SC_ERR_ARG;
            SmTreeNode* node = s.leaves.at((size_t)a[1]);
            if (node->GetOwnerObject() != s.surface.get())
                return SC_ERR_ARG;
            oi.push_back(st(g->AddElement(node, SM_GE_SURFACE, ext(a + 2))));
            break;
        }
        case 15:
            caches.observe(g, hier, hidx, oi, of);
            break;
        default:
            rc = SC_ERR_ARG;
            break;
        }
        if (rc != SC_OK)
            break;
        i += 1 + sz;
    }
    // Grid elements/hierarchies refer to the surfaces. Destroy the grid first;
    // surfaces then destroy their own caches before sCtx goes out of scope.
    owned_grid.reset();
    return rc;
}
} // namespace

SC_EXPORT int32_t sc_geom_grid_ints(const double* prog, int64_t n, int64_t* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t s = run(prog, n, oi, of);
    if (s != SC_OK)
        return s;
    return sc_copy_out(oi.data(), (int64_t)oi.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_grid_floats(const double* prog, int64_t n, double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t s = run(prog, n, oi, of);
    if (s != SC_OK)
        return s;
    return sc_copy_out(of.data(), (int64_t)of.size(), out, cap, out_len);
    SC_CATCH
}
