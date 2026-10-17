// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.hash_table`. Wraps upstream SmObjsInVoxels<TYPE> (and the
// SmHashTable / SmBucketItem / SmPointItem template members it uses), all in
// source/SMLib/inc/SmHashTable.h.
//
// sc_geom_hash_table_{ints,floats}(prog, n, out, cap, out_len) run the same flat double
// program against one SmObjsInVoxels<SmVertex*> and return its integer or float output
// stream. The encoding is shared with crates/solidcore-ffi/src/units/geom_hash_table.rs
// (which validates its shape first). Op (operand count):
//   0 (11) x y z has_bbox bbox6 est   constructor SmObjsInVoxels(x,y,z,bbox?,est); first only
//   1 (11) x y z has_bbox bbox6 est   SetSize
//   2 (6)  item px py pz tol track    Add(item, p, SmZoneTol3d(tol), track ? &updated : NULL)
//                                     ints: returned voxel index (SM_UNDEF_ULONG as is)
//   3 (1)  item                       Remove; ints: removed count
//   4 (4+n) limit mode hist n idx..   FindCoincidentPairs(limit < 0 ? NULL : &limit,
//                                     mode 0: NULL, 1: &updated, 2: the n indices,
//                                     hist ? &histogram : NULL)
//                                     ints: n_pairs, (coin1, coin2) point-item indices,
//                                     then (hist) the 11 histogram counts
//   5 (3)  px py pz                   MapPointToVoxel; ints: index ix iy iz; floats: extent
//   6 (1)  index                      MapVoxelIndexToBBox; floats: extent
//   7 (1)  item                       FindPointItems; ints: n, indices
//   8 (0)                             dump (layout in dump() below)
//   9 (3)  ix iy iz                   MapIndicesToVoxelIndex; ints: index
//  10 (1)  index                      MapVoxelIndexToIndices; ints: ix iy iz
//  11 (0)                             clear the accumulated updated-bucket list
// Items are fake SmVertex pointers g_objs + id (never dereferenced); id -1 is NULL.
// The histogram and the updated-bucket list persist over the program.
//
// Rejected with SC_ERR_ARG (upstream undefined behaviour):
//  * counts above 2^20 each / 2^20 voxels, estimates above 2^20;
//  * Add on a sized model with zero voxels (indexes m_sBuckets[SM_UNDEF_ULONG]);
//  * Add that grows a model whose hash table still has items: SmObjsInVoxels::IncMaxItemCount
//    re-points the bucket items at the old point-item array and then frees it;
//  * FindCoincidentPairs with an empty bucket list (GetAt(0)) or a bucket index out of range,
//    including a tracked bucket (mode 1) recorded before a SetSize that shrank the model;
//  * MapVoxelIndexToIndices with x*y == 0 (division by zero).
#include "sc_shim.h"

#include <SmExtent3d.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector3d.h>
#include <SmHashTable.h>

#include <vector>

namespace
{
const int64_t kMaxObjects = 4096;
const double kMaxCount = 1048576.0;
const int kHistBuckets = SM_HISTOGRAM_GAPSIZE_BUCKETCNT;
alignas(16) char g_objs[kMaxObjects];

typedef SmVertex* Item;
typedef SmPointItem<Item> PItem;
typedef SmBucketItem<PItem*> BItem;

Item item_ptr(double id)
{
    long long i = (long long)id;
    return i < 0 ? nullptr : reinterpret_cast<Item>(g_objs + i);
}

int64_t item_id(Item p) { return p == nullptr ? -1 : (int64_t)(reinterpret_cast<const char*>(p) - g_objs); }

// Exposes the protected hash table without changing any behaviour.
struct Probe : public SmObjsInVoxels<Item>
{
    Probe(ULONG x, ULONG y, ULONG z, SmExtent3d* b, ULONG e) : SmObjsInVoxels<Item>(x, y, z, b, e) {}
    SmHashTable<PItem*>& hash() { return m_sHashTable; }
    ULONG removed() const { return m_lRemovedPointItemCount; }
};

void put_ext(std::vector<double>& o, const SmExtent3d& e)
{
    o.push_back(e.GetMin().x);
    o.push_back(e.GetMin().y);
    o.push_back(e.GetMin().z);
    o.push_back(e.GetMax().x);
    o.push_back(e.GetMax().y);
    o.push_back(e.GetMax().z);
}

int64_t op_size(int op)
{
    switch (op)
    {
    case 0:
    case 1: return 11;
    case 2: return 6;
    case 3: return 1;
    case 4: return 4;
    case 5: return 3;
    case 6: return 1;
    case 7: return 1;
    case 8: return 0;
    case 9: return 3;
    case 10: return 1;
    case 11: return 0;
    default: return -1;
    }
}

bool sizes_ok(const double* a)
{
    for (int k = 0; k < 3; k++)
        if (!(a[k] >= 0.0 && a[k] <= kMaxCount))
            return false;
    if (a[0] * a[1] * a[2] > kMaxCount)
        return false;
    return a[10] >= 0.0 && a[10] <= kMaxCount;
}

int64_t ptr_index(const void* p, const void* base, size_t sz)
{
    if (p == nullptr)
        return -1;
    return (int64_t)((reinterpret_cast<const char*>(p) - reinterpret_cast<const char*>(base)) / (ptrdiff_t)sz);
}

void dump(Probe& v, const SmTArray<ULONG>& upd, std::vector<int64_t>& oi, std::vector<double>& of)
{
    SmHashTable<PItem*>& h = v.hash();
    SmTArray<PItem>& pts = v.GetPointItems();
    SmTArray<BItem>& bis = h.GetBucketItems();
    const PItem* pbase = pts.GetDataArray();
    const BItem* bbase = bis.GetDataArray();
    oi.push_back((int64_t)pts.GetSize());
    oi.push_back((int64_t)pts.GetDataSize());
    oi.push_back((int64_t)bis.GetDataSize());
    oi.push_back((int64_t)v.removed());
    oi.push_back((int64_t)h.GetBucketCount());
    oi.push_back((int64_t)h.GetUsedBucketCount());
    oi.push_back((int64_t)h.GetRemovedBucketItemCount());
    oi.push_back((int64_t)h.GetTotalBucketItemCount());
    oi.push_back(ptr_index(h.GetUsedBucketCount() > 0 ? h.GetUsedBucket(0) : nullptr, bbase, sizeof(BItem)));
    for (ULONG i = 0; i < h.GetBucketCount(); i++)
        oi.push_back(ptr_index(h.GetBucket(i), bbase, sizeof(BItem)));
    for (ULONG i = 0; i < h.GetTotalBucketItemCount(); i++)
    {
        BItem& b = h.GetBucketItem(i);
        oi.push_back(ptr_index(b.GetNextUsedBucket(), bbase, sizeof(BItem)));
        oi.push_back(ptr_index(b.GetNextBucketItem(), bbase, sizeof(BItem)));
        oi.push_back((int64_t)b.GetBucketIndex());
        oi.push_back(ptr_index(b.GetItem(), pbase, sizeof(PItem)));
        oi.push_back((int64_t)b.GetItemId());
    }
    for (ULONG i = 0; i < pts.GetSize(); i++)
    {
        PItem& p = pts.GetAt(i);
        oi.push_back(item_id(p.m_sTgtItem));
        of.push_back(p.m_sTgtPoint3d.x);
        of.push_back(p.m_sTgtPoint3d.y);
        of.push_back(p.m_sTgtPoint3d.z);
        of.push_back((double)p.m_sTgtZoneTol3d);
    }
    // used-bucket walk
    std::vector<int64_t> walk;
    for (BItem* b = h.GetUsedBucketCount() > 0 ? h.GetUsedBucket(0) : nullptr; b != nullptr; b = b->GetNextUsedBucket())
        walk.push_back(ptr_index(b, bbase, sizeof(BItem)));
    oi.push_back((int64_t)walk.size());
    oi.insert(oi.end(), walk.begin(), walk.end());
    oi.push_back((int64_t)upd.GetSize());
    for (ULONG i = 0; i < upd.GetSize(); i++)
        oi.push_back((int64_t)upd.GetAt(i));
}

int32_t run(const double* p, int64_t n, std::vector<int64_t>& oi, std::vector<double>& of)
{
    if (n < 12 || (int)p[0] != 0)
        return SC_ERR_ARG;
    const double* a0 = p + 1;
    if (!sizes_ok(a0))
        return SC_ERR_ARG;
    SmExtent3d bbox0(a0[4], a0[5], a0[6], a0[7], a0[8], a0[9]);
    Probe v((ULONG)a0[0], (ULONG)a0[1], (ULONG)a0[2], a0[3] != 0.0 ? &bbox0 : NULL, (ULONG)a0[10]);
    ULONG cnt[3] = {(ULONG)a0[0], (ULONG)a0[1], (ULONG)a0[2]};
    SmExtent3d box;
    if (a0[3] != 0.0)
        box = bbox0;
    SmTArray<ULONG> upd;
    SmTArray<ULONG> hist;
    hist.SetSize(kHistBuckets);
    hist.SetAll(0);
    int64_t i = 12;
    while (i < n)
    {
        int op = (int)p[i];
        int64_t sz = op_size(op);
        if (sz < 0 || i + 1 + sz > n)
            return SC_ERR_ARG;
        const double* a = p + i + 1;
        if (op == 4)
        {
            if (a[3] < 0.0 || i + 1 + sz + (int64_t)a[3] > n)
                return SC_ERR_ARG;
            sz += (int64_t)a[3];
        }
        switch (op)
        {
        case 0: return SC_ERR_ARG;
        case 1:
        {
            if (!sizes_ok(a))
                return SC_ERR_ARG;
            SmExtent3d b(a[4], a[5], a[6], a[7], a[8], a[9]);
            v.SetSize((ULONG)a[0], (ULONG)a[1], (ULONG)a[2], a[3] != 0.0 ? &b : NULL, (ULONG)a[10]);
            cnt[0] = (ULONG)a[0];
            cnt[1] = (ULONG)a[1];
            cnt[2] = (ULONG)a[2];
            box = SmExtent3d();
            if (a[3] != 0.0)
                box = b;
            break;
        }
        case 2:
        {
            SmPoint3d pt(a[1], a[2], a[3]);
            bool live = !box.IsInit();
            if (live && cnt[0] * cnt[1] * cnt[2] == 0)
                return SC_ERR_ARG;
            ULONG size = v.GetPointItems().GetSize();
            ULONG data = v.GetPointItems().GetDataSize();
            if (live && size > 0 && size + 7 > data && v.hash().GetUsedBucketItemCount() > 0)
                return SC_ERR_ARG;
            ULONG r = v.Add(item_ptr(a[0]), pt, SmZoneTol3d(a[4]), a[5] != 0.0 ? &upd : NULL, NULL);
            oi.push_back((int64_t)r);
            break;
        }
        case 3: oi.push_back((int64_t)v.Remove(item_ptr(a[0]))); break;
        case 4:
        {
            int mode = (int)a[1];
            ULONG lim = a[0] < 0.0 ? 0 : (ULONG)a[0];
            SmTArray<ULONG> list;
            SmTArray<ULONG>* pl = NULL;
            if (mode == 1)
            {
                if (upd.GetSize() == 0)
                    return SC_ERR_ARG;
                for (ULONG k = 0; k < upd.GetSize(); k++)
                    if (upd[k] >= v.hash().GetBucketCount())
                        return SC_ERR_ARG;
                pl = &upd;
            }
            else if (mode == 2)
            {
                int64_t m = (int64_t)a[3];
                if (m == 0)
                    return SC_ERR_ARG;
                for (int64_t k = 0; k < m; k++)
                {
                    double b = a[4 + k];
                    if (!(b >= 0.0) || b >= (double)v.hash().GetBucketCount())
                        return SC_ERR_ARG;
                    list.Add((ULONG)b);
                }
                pl = &list;
            }
            else if (mode != 0)
                return SC_ERR_ARG;
            SmTArray<PItem*> c1, c2;
            const PItem* base = v.GetPointItems().GetDataArray();
            v.FindCoincidentPairs(c1, c2, a[0] < 0.0 ? NULL : &lim, pl, a[2] != 0.0 ? &hist : NULL);
            oi.push_back((int64_t)c1.GetSize());
            for (ULONG k = 0; k < c1.GetSize(); k++)
            {
                oi.push_back(ptr_index(c1[k], base, sizeof(PItem)));
                oi.push_back(ptr_index(c2[k], base, sizeof(PItem)));
            }
            if (a[2] != 0.0)
                for (int k = 0; k < kHistBuckets; k++)
                    oi.push_back((int64_t)hist[k]);
            break;
        }
        case 5:
        {
            SmPoint3d pt(a[0], a[1], a[2]);
            SmExtent3d e;
            ULONG ix = 0, iy = 0, iz = 0;
            ULONG idx = v.MapPointToVoxel(pt, &e, &ix, &iy, &iz);
            oi.push_back((int64_t)idx);
            oi.push_back((int64_t)ix);
            oi.push_back((int64_t)iy);
            oi.push_back((int64_t)iz);
            put_ext(of, e);
            break;
        }
        case 6:
        {
            if (!(a[0] >= 0.0 && a[0] < 1e15))
                return SC_ERR_ARG;
            SmExtent3d e;
            v.MapVoxelIndexToBBox((ULONG)a[0], e);
            put_ext(of, e);
            break;
        }
        case 7:
        {
            SmTArray<ULONG> f;
            v.FindPointItems(item_ptr(a[0]), f);
            oi.push_back((int64_t)f.GetSize());
            for (ULONG k = 0; k < f.GetSize(); k++)
                oi.push_back((int64_t)f[k]);
            break;
        }
        case 8:
            dump(v, upd, oi, of);
            break;
        case 9:
            for (int k = 0; k < 3; k++)
                if (!(a[k] >= 0.0 && a[k] < 1e15))
                    return SC_ERR_ARG;
            oi.push_back((int64_t)v.MapIndicesToVoxelIndex((ULONG)a[0], (ULONG)a[1], (ULONG)a[2]));
            break;
        case 10:
        {
            if (cnt[0] * cnt[1] == 0 || !(a[0] >= 0.0 && a[0] < 1e15))
                return SC_ERR_ARG;
            ULONG ix, iy, iz;
            v.MapVoxelIndexToIndices((ULONG)a[0], ix, iy, iz);
            oi.push_back((int64_t)ix);
            oi.push_back((int64_t)iy);
            oi.push_back((int64_t)iz);
            break;
        }
        case 11: upd.RemoveAll(); break;
        default: return SC_ERR_ARG;
        }
        i += 1 + sz;
    }
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_geom_hash_table_ints(const double* prog, int64_t n, int64_t* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t s = run(prog, n, oi, of);
    if (s != SC_OK)
    {
        *out_len = 0;
        return s;
    }
    return sc_copy_out(oi.data(), (int64_t)oi.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_hash_table_floats(const double* prog, int64_t n, double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t s = run(prog, n, oi, of);
    if (s != SC_OK)
    {
        *out_len = 0;
        return s;
    }
    return sc_copy_out(of.data(), (int64_t)of.size(), out, cap, out_len);
    SC_CATCH
}
