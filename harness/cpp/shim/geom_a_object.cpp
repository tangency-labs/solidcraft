// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.a_object`. Wraps upstream SmAObject (Notify, copy
// constructor, AssertValid, type system, cache slot), SmCacheMgr / SmCacheCheckOutIn
// (source/SMLib/src/SmAObject.cpp, source/SMLib/src/SmCacheMgr.cpp), SmSAGObject
// (source/SMLib/src/SmSAGObject.cpp) and the NLI_Math.h inline helpers.
//
// sc_geom_a_object_run runs a script over attributed objects (kind 0: a bare SmRegion,
// 1: a plain SmAObject, 2: an empty SmBrep), attributes ("fresh" handles until first
// added to an object; afterwards reachable only through object lists) and cache
// objects (ShimCache; their memory is never released so addresses stay unique, and
// their destructor records the deletion). Each op is 8 int64s [op, a, b, c, d, e, f, g].
// Attribute references: ref >= 0 a fresh handle, ref < 0 entry (k % 1024) of object
// (k / 1024)'s list, k = -ref - 1.
//
// Statuses: 0 ok, ST_INVALID (-1) a reference does not resolve, ST_SKIP (-2) the op
// would hit undefined behaviour upstream (the Rust runner applies the same rules to
// its own state), otherwise the upstream SmStatus.
#include "sc_shim.h"

#include <SmAObject.h>
#include <SmAttribute.h>
#include <SmBrep.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmRegion.h>
#include <SmSAGObject.h>

#include <nurbs.h>
#include <NL_Globals.h>
#include <NLI_Math.h>

#include <cstring>
#include <map>
#include <memory>
#include <set>
#include <vector>

namespace
{
enum Op : int64_t
{
    OP_NEW_PLAIN = 0,
    OP_NEW_LONG = 1,
    OP_NEW_VECTOR3D = 2,
    OP_NEW_COLOR = 3,
    OP_NEW_MERGE_REGION = 4,
    OP_MR_SET_OPERATION = 5,
    OP_ADD = 6,
    OP_REMOVE = 7,
    OP_CLEAR = 8,
    OP_FIND = 9,
    OP_NOTIFY = 10,
    OP_COPY_CONSTRUCT = 11,
    OP_ASSERT_VALID = 12,
    OP_NEW_CACHE = 13,
    OP_SET_CACHE = 14,
    OP_GET_CACHE = 15,
    OP_CHECK_OUT = 16,
    OP_CHECK_IN = 17,
    OP_SOFT_DELETE = 18,
    OP_DELETE_OBJECTS_CACHE = 19,
    OP_GET_OR_CREATE = 20,
    OP_GUARD_OPEN = 21,
    OP_GUARD_CLOSE = 22,
    OP_SAG_MAKE = 23,
    OP_SAG_NOTIFY = 24,
    OP_LAST = 24,
};

const int64_t ST_INVALID = -1;
const int64_t ST_SKIP = -2;
const int64_t REF_STRIDE = 1024;
const int64_t MAX_OBJECTS = 64;

enum Kind : int64_t
{
    K_PLAIN = 0,
    K_LONG = 1,
    K_VECTOR3D = 2,
    K_COLOR = 3,
    K_MERGE_REGION = 4,
    K_OTHER = 5,
};

enum ObjKind : int64_t
{
    O_REGION = 0,
    O_AOBJECT = 1,
    O_BREP = 2,
};

struct ShimRegion : public SmRegion
{
    ShimRegion() : SmRegion() {}
    static void* operator new(size_t size, const SmContext& c) { return SmObject::operator new(size, c); }
    static void operator delete(void* p, const SmContext&) { smos_Free(p); }
    static void operator delete(void* p) { smos_Free(p); }
};

// A cache object whose deletion is recorded and whose memory is never released.
struct ShimCache : public SmCacheObj
{
    std::vector<bool>* m_pAlive = nullptr;
    size_t m_lIndex = 0;
    ULONG UseCount() const { return m_lUseCount; }
    bool Deleted() const { return m_bDeleted != FALSE; }
    ~ShimCache() override
    {
        if (m_pAlive) (*m_pAlive)[m_lIndex] = false;
    }
    static void* operator new(size_t size, const SmContext& c) { return SmObject::operator new(size, c); }
    static void operator delete(void*, const SmContext&) {}
    static void operator delete(void*) {}
};

int64_t kind_of(const SmAttribute* a)
{
    switch (a->GetType())
    {
    case SmAttribute_TYPE:
        return K_PLAIN;
    case SmLongAttribute_TYPE:
        return K_LONG;
    case SmPointerAttribute_TYPE:
        return a->IsKindOf(SmMergeRegionAttribute_TYPE) ? K_MERGE_REGION : K_OTHER;
    case SmVector3dAttribute_TYPE:
        return a->IsKindOf(SmColorAttribute_TYPE) ? K_COLOR : K_VECTOR3D;
    default:
        return K_OTHER;
    }
}

bool is_vector(const SmAttribute* a)
{
    int64_t k = kind_of(a);
    return k == K_VECTOR3D || k == K_COLOR;
}

SmContext* shim_context()
{
    static SmContext* s_pCtx = new SmContext();
    return s_pCtx;
}

// A notify callback recording (tag, op, caller, caller type, data1, data2, data3).
struct Recorder : public SmNotifyCallback
{
    int64_t m_lTag = 0;
    std::vector<int64_t>* m_pOut = nullptr;
    const std::vector<SmAObject*>* m_pObjs = nullptr;

    int64_t Index(SmObject* p) const
    {
        if (p == nullptr) return -1;
        for (size_t i = 0; i < m_pObjs->size(); ++i)
            if (static_cast<SmObject*>((*m_pObjs)[i]) == p) return (int64_t)i;
        return -2;
    }

    SmStatus Execute(SmNotifyOperation eOp, SmObject* pObj, SmObject* p1, SmObject* p2, SmObject* p3) override
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

struct Run
{
    SmContext* ctx = nullptr;
    std::vector<SmAObject*> objs;
    std::vector<int64_t> kinds;
    std::vector<SmAttribute*> handles;
    std::vector<bool> consumed;
    std::vector<ShimCache*> caches;
    std::vector<bool> cache_alive;
    std::vector<std::unique_ptr<SmCacheCheckOutIn>> guards;
    std::vector<ShimCache*> guard_cache; // nullptr for a guard of no cache
    std::vector<double> pay_d;
    size_t pd = 0;
    bool bad_payload = false;
    std::vector<int64_t> rec;

    double next_d()
    {
        if (pd >= pay_d.size())
        {
            bad_payload = true;
            return 0.0;
        }
        return pay_d[pd++];
    }

    bool valid_obj(int64_t o) const { return o >= 0 && (size_t)o < objs.size(); }

    std::vector<SmAttribute*> list(size_t o) const
    {
        SmTArray<SmAttribute*> t;
        objs[o]->GetAttributes(t);
        std::vector<SmAttribute*> v;
        for (ULONG i = 0; i < t.GetSize(); ++i)
            v.push_back(t[i]);
        return v;
    }

    std::vector<SmAObject*> users(const SmAttribute* a) const
    {
        SmTArray<SmAObject*> t;
        a->GetUsers(t);
        std::vector<SmAObject*> v;
        for (ULONG i = 0; i < t.GetSize(); ++i)
            v.push_back(t[i]);
        return v;
    }

    int64_t obj_index(const void* p) const
    {
        for (size_t i = 0; i < objs.size(); ++i)
            if ((const void*)objs[i] == p) return (int64_t)i;
        return -999;
    }

    int64_t cache_index(const SmCacheObj* p) const
    {
        if (p == nullptr) return -1;
        for (size_t i = 0; i < caches.size(); ++i)
            if ((const SmCacheObj*)caches[i] == p) return (int64_t)i;
        return -2;
    }

    // Resolves a reference; nullptr when it does not resolve.
    SmAttribute* resolve(int64_t ref) const
    {
        if (ref >= 0)
        {
            if ((size_t)ref >= handles.size() || consumed[(size_t)ref]) return nullptr;
            return handles[(size_t)ref];
        }
        int64_t k = -ref - 1;
        int64_t o = k / REF_STRIDE, i = k % REF_STRIDE;
        if (!valid_obj(o)) return nullptr;
        std::vector<SmAttribute*> l = list((size_t)o);
        if ((size_t)i >= l.size()) return nullptr;
        return l[(size_t)i];
    }

    int64_t add_handle(SmAttribute* a)
    {
        handles.push_back(a);
        consumed.push_back(false);
        return (int64_t)handles.size() - 1;
    }

    // A cache index argument: -1 null, else a cache (live or not); false when out of range.
    bool cache_arg(int64_t c, ShimCache*& out) const
    {
        if (c == -1)
        {
            out = nullptr;
            return true;
        }
        if (c < 0 || (size_t)c >= caches.size()) return false;
        out = caches[(size_t)c];
        return true;
    }

    bool live(const ShimCache* c) const
    {
        for (size_t i = 0; i < caches.size(); ++i)
            if (caches[i] == c) return cache_alive[i];
        return true; // not a shim cache (an SmBrepCache): owned by upstream
    }

    SmObject* data(int64_t k) const { return k < 0 ? nullptr : objs[(size_t)k]; }
};

bool has_id(const Run& r, size_t o, ULONG id)
{
    for (SmAttribute* a : r.list(o))
        if (a->GetAttributeID() == id) return true;
    return false;
}

// Whether SmAObject::Notify(op, ...) on `caller` can run without undefined behaviour
// in the attribute handlers (see crates/difftest/src/units/geom_a_object.rs).
bool notify_is_safe(const Run& r, int64_t op, int64_t caller, int64_t d1, int64_t d2, int64_t d3)
{
    switch ((SmNotifyOperation)op)
    {
    case SM_NO_SPLIT_IN_BREP:
        return d2 >= 0 && d3 >= 0;
    case SM_NO_MERGE_IN_BREP:
    case SM_NO_MERGE:
        if (d1 < 0 || d2 < 0) return false;
        return !has_id(r, (size_t)d1, SM_AI_OTHER_REGION_ID) && !has_id(r, (size_t)d2, SM_AI_OTHER_REGION_ID);
    case SM_NO_COINCIDENT:
    case SM_NO_COPY:
        return d1 >= 0;
    case SM_NO_SPLIT:
        return d1 >= 0 && d2 >= 0;
    case SM_NO_REG_PROPAGATION:
    {
        if (r.kinds[(size_t)caller] != O_REGION) return true;
        SmAttribute* m = r.objs[(size_t)caller]->FindAttribute(SM_AI_OTHER_REGION_ID);
        if (m == nullptr || kind_of(m) != K_MERGE_REGION) return false;
        SmMergeRegionAttribute* mr = (SmMergeRegionAttribute*)m;
        const SmTArray<SmRegion*>& others = mr->GetOtherRegions();
        if (others.GetSize() > 1) return false;
        std::vector<SmAttribute*> l = r.list((size_t)caller);
        std::set<SmAttribute*> seen(l.begin(), l.end());
        if (seen.size() != l.size()) return false;
        std::vector<size_t> regions{(size_t)caller};
        for (ULONG i = 0; i < others.GetSize(); ++i)
        {
            int64_t o = r.obj_index((SmAObject*)others[i]);
            if (o < 0 || o == caller) return false;
            regions.push_back((size_t)o);
        }
        for (size_t o : regions)
            for (SmAttribute* a : r.list(o))
                if (a->GetAttributeID() == SM_AI_COLOR && !is_vector(a)) return false;
        return true;
    }
    default:
        return true;
    }
}

int64_t run_op(Run& r, const int64_t* a, std::vector<int64_t>& out)
{
    const SmContext& ctx = *r.ctx;
    switch (a[0])
    {
    case OP_NEW_PLAIN:
        out.push_back(r.add_handle(new (ctx) SmAttribute((ULONG)a[1], (SmAttributeBehaviorType)a[2])));
        return 0;
    case OP_NEW_LONG:
        out.push_back(r.add_handle(new (ctx) SmLongAttribute((ULONG)a[1], (long)a[2], (SmAttributeBehaviorType)a[3])));
        return 0;
    case OP_NEW_VECTOR3D:
    case OP_NEW_COLOR:
    {
        double x = r.next_d(), y = r.next_d(), z = r.next_d();
        if (r.bad_payload) return SC_ERR_ARG;
        SmVector3d v(x, y, z);
        SmAttribute* at = a[0] == OP_NEW_COLOR
                              ? (SmAttribute*)new (ctx) SmColorAttribute((ULONG)a[1], v, (SmAttributeBehaviorType)a[2])
                              : (SmAttribute*)new (ctx) SmVector3dAttribute((ULONG)a[1], v, (SmAttributeBehaviorType)a[2]);
        out.push_back(r.add_handle(at));
        return 0;
    }
    case OP_NEW_MERGE_REGION:
        if (!r.valid_obj(a[1]) || !r.valid_obj(a[3])) return SC_ERR_ARG;
        if (r.kinds[(size_t)a[1]] != O_REGION || r.kinds[(size_t)a[3]] != O_REGION) return SC_ERR_ARG;
        out.push_back(r.add_handle(new (ctx) SmMergeRegionAttribute(
            (SmRegion*)r.objs[(size_t)a[1]], (ULONG)a[2], (SmRegion*)r.objs[(size_t)a[3]], (ULONG)a[4])));
        return 0;
    case OP_MR_SET_OPERATION:
    {
        SmAttribute* at = r.resolve(a[1]);
        if (at == nullptr) return ST_INVALID;
        if (kind_of(at) != K_MERGE_REGION) return ST_SKIP;
        ((SmMergeRegionAttribute*)at)->SetOperation((SmBooleanOperationType)a[2]);
        return 0;
    }
    case OP_ADD:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmAttribute* at = r.resolve(a[2]);
        if (at == nullptr) return ST_INVALID;
        if (a[2] >= 0) r.consumed[(size_t)a[2]] = true;
        r.objs[(size_t)a[1]]->AddAttribute(at);
        return 0;
    }
    case OP_REMOVE:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmAttribute* at = r.resolve(a[2]);
        if (at == nullptr) return ST_INVALID;
        r.objs[(size_t)a[1]]->RemoveAttribute(at, a[3] != 0 ? TRUE : FALSE);
        return 0;
    }
    case OP_CLEAR:
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        r.objs[(size_t)a[1]]->ClearAttributes();
        return 0;
    case OP_FIND:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmAttribute* f = r.objs[(size_t)a[1]]->FindAttribute((ULONG)a[2]);
        std::vector<SmAttribute*> l = r.list((size_t)a[1]);
        int64_t pos = -1;
        for (size_t i = 0; i < l.size() && f != nullptr; ++i)
            if (l[i] == f)
            {
                pos = (int64_t)i;
                break;
            }
        out.push_back(pos);
        return 0;
    }
    case OP_NOTIFY:
    case OP_SAG_NOTIFY:
    {
        if (a[1] < 0 || a[1] > (int64_t)SM_NO_UNKNOWN) return SC_ERR_ARG;
        if (!r.valid_obj(a[2])) return SC_ERR_ARG;
        for (int k = 3; k < 6; ++k)
            if (a[k] < -1 || a[k] >= (int64_t)r.objs.size()) return SC_ERR_ARG;
        if (a[0] == OP_SAG_NOTIFY)
        {
            // only the edit-like events with no data: SmTopology::Notify must not walk a B-rep
            SmNotifyOperation e = (SmNotifyOperation)a[1];
            if (r.kinds[(size_t)a[2]] != O_BREP || a[3] != -1 || a[4] != -1 || a[5] != -1) return SC_ERR_ARG;
            if (e != SM_NO_PRE_EDIT && e != SM_NO_POST_EDIT && e != SM_NO_CHANGE_OWNER && e != SM_NO_UNKNOWN)
                return SC_ERR_ARG;
            r.rec.clear();
            ((SmSAGObject*)(SmBrep*)r.objs[(size_t)a[2]])->SmSAGObject::Notify(e, nullptr, nullptr, nullptr);
            out.push_back((int64_t)(r.rec.size() / 7));
            out.insert(out.end(), r.rec.begin(), r.rec.end());
            return 0;
        }
        if (!notify_is_safe(r, a[1], a[2], a[3], a[4], a[5])) return ST_SKIP;
        r.rec.clear();
        r.objs[(size_t)a[2]]->SmAObject::Notify((SmNotifyOperation)a[1], r.data(a[3]), r.data(a[4]), r.data(a[5]));
        out.push_back((int64_t)(r.rec.size() / 7));
        out.insert(out.end(), r.rec.begin(), r.rec.end());
        return 0;
    }
    case OP_COPY_CONSTRUCT:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        if ((int64_t)r.objs.size() >= MAX_OBJECTS) return ST_SKIP;
        SmAObject* p = new (ctx) SmAObject(*r.objs[(size_t)a[1]]);
        r.objs.push_back(p);
        r.kinds.push_back(O_AOBJECT);
        out.push_back((int64_t)r.objs.size() - 1);
        return 0;
    }
    case OP_ASSERT_VALID:
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        out.push_back(r.objs[(size_t)a[1]]->SmAObject::AssertValid(nullptr, a[2] != 0 ? SM_LEVEL_GIVEN : SM_LEVEL_0) ? 1
                                                                                                                 : 0);
        return 0;
    case OP_NEW_CACHE:
    {
        ShimCache* c = new (ctx) ShimCache();
        c->m_pAlive = &r.cache_alive;
        c->m_lIndex = r.caches.size();
        r.caches.push_back(c);
        r.cache_alive.push_back(true);
        out.push_back((int64_t)r.caches.size() - 1);
        return 0;
    }
    default:
        break;
    }
    // cache ops
    switch (a[0])
    {
    case OP_SET_CACHE:
    {
        ShimCache* c;
        if (!r.valid_obj(a[1]) || !r.cache_arg(a[2], c)) return SC_ERR_ARG;
        r.objs[(size_t)a[1]]->SetCacheObj(c);
        return 0;
    }
    case OP_GET_CACHE:
    {
        if (a[1] != -1 && !r.valid_obj(a[1])) return SC_ERR_ARG;
        const SmAObject* o = a[1] < 0 ? nullptr : r.objs[(size_t)a[1]];
        out.push_back(r.cache_index(SmCacheMgr::GetObjectCache((SmObjectCacheType)a[2], o)));
        return 0;
    }
    case OP_CHECK_OUT:
    case OP_CHECK_IN:
    case OP_SOFT_DELETE:
    case OP_GUARD_OPEN:
    {
        ShimCache* c;
        if (!r.cache_arg(a[1], c)) return SC_ERR_ARG;
        if (c != nullptr && !r.live(c)) return ST_SKIP;
        if (a[0] == OP_CHECK_OUT)
            SmCacheMgr::CheckOutCache(c);
        else if (a[0] == OP_CHECK_IN)
            SmCacheMgr::CheckInCache(c);
        else if (a[0] == OP_SOFT_DELETE)
        {
            if (c == nullptr) return SC_ERR_ARG;
            SmCacheMgr::SoftDelete(c);
        }
        else
        {
            r.guards.emplace_back(new SmCacheCheckOutIn(c));
            r.guard_cache.push_back(c);
        }
        return 0;
    }
    case OP_GUARD_CLOSE:
    {
        if (r.guards.empty()) return ST_SKIP;
        ShimCache* c = r.guard_cache.back();
        r.guard_cache.pop_back();
        if (c != nullptr && !r.live(c))
        {
            (void)r.guards.back().release(); // leaked: its destructor would touch a dead cache
            r.guards.pop_back();
            return ST_SKIP;
        }
        r.guards.pop_back();
        return 0;
    }
    case OP_DELETE_OBJECTS_CACHE:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmAObject* o = r.objs[(size_t)a[1]];
        SmCacheObj* c = o->GetCacheObj();
        if (c != nullptr && !r.live((ShimCache*)c)) return ST_SKIP;
        if (a[2] == 0)
            SmCacheMgr::DeleteObjectsCache(o);
        else
            SmCacheMgr::DeleteObjectsCache((SmObjectCacheType)a[3], o);
        return 0;
    }
    case OP_GET_OR_CREATE:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmCacheObj* c = SmCacheMgr::GetOrCreateObjectCache((SmObjectCacheType)a[2], r.objs[(size_t)a[1]]);
        out.push_back(r.cache_index(c));
        return 0;
    }
    case OP_SAG_MAKE:
    {
        ShimCache* old;
        if (!r.valid_obj(a[1]) || !r.valid_obj(a[2]) || !r.cache_arg(a[3], old)) return SC_ERR_ARG;
        if (r.kinds[(size_t)a[1]] != O_BREP) return SC_ERR_ARG;
        SmSAGObject* self = (SmBrep*)r.objs[(size_t)a[1]];
        SmCacheObj* res = nullptr;
        SmStatus st = self->SmSAGObject::CacheMakeOrValidate((SmObjectCacheType)a[4], r.objs[(size_t)a[2]], old, res);
        out.push_back(r.cache_index(res));
        return st == SM_SUCCESS ? 0 : (int64_t)st;
    }
    default:
        return SC_ERR_ARG;
    }
}

void dump(const Run& r, std::vector<int64_t>& ints, std::vector<double>& dbls)
{
    std::vector<SmAttribute*> canon;
    std::map<SmAttribute*, int64_t> index;
    auto see = [&](SmAttribute* a) {
        if (!index.count(a))
        {
            index[a] = (int64_t)canon.size();
            canon.push_back(a);
        }
    };
    std::vector<std::vector<SmAttribute*>> lists;
    for (size_t o = 0; o < r.objs.size(); ++o)
    {
        lists.push_back(r.list(o));
        for (SmAttribute* a : lists.back())
            see(a);
    }
    for (size_t h = 0; h < r.handles.size(); ++h)
        if (!r.consumed[h]) see(r.handles[h]);

    ints.push_back((int64_t)r.objs.size());
    for (size_t o = 0; o < r.objs.size(); ++o)
    {
        ints.push_back((int64_t)r.objs[o]->GetType());
        ints.push_back((int64_t)lists[o].size());
        for (SmAttribute* a : lists[o])
            ints.push_back(index[a]);
        ints.push_back(r.cache_index(r.objs[o]->GetCacheObj()));
    }
    ints.push_back((int64_t)canon.size());
    for (SmAttribute* a : canon)
    {
        int64_t k = kind_of(a);
        ints.push_back(k);
        ints.push_back((int64_t)a->GetAttributeID());
        ints.push_back((int64_t)a->GetBehavior());
        std::vector<SmAObject*> u = r.users(a);
        ints.push_back((int64_t)u.size());
        for (SmAObject* x : u)
            ints.push_back(r.obj_index(x));
        std::vector<int64_t> v;
        std::vector<double> d;
        if (k == K_LONG) v.push_back((int64_t)((SmLongAttribute*)a)->GetValue());
        if (k == K_VECTOR3D || k == K_COLOR)
        {
            const SmVector3d& x = ((SmVector3dAttribute*)a)->GetValue();
            d.push_back(x.x);
            d.push_back(x.y);
            d.push_back(x.z);
        }
        if (k == K_MERGE_REGION)
        {
            SmMergeRegionAttribute* m = (SmMergeRegionAttribute*)a;
            v.push_back((int64_t)m->GetOperation());
            v.push_back((int64_t)m->GetThisRegions().GetSize());
            for (ULONG i = 0; i < m->GetThisRegions().GetSize(); ++i)
                v.push_back(r.obj_index((SmAObject*)m->GetThisRegions()[i]));
            for (ULONG i = 0; i < m->GetThisRegionTypes().GetSize(); ++i)
                v.push_back((int64_t)m->GetThisRegionTypes()[i]);
            v.push_back((int64_t)m->GetOtherRegions().GetSize());
            for (ULONG i = 0; i < m->GetOtherRegions().GetSize(); ++i)
                v.push_back(r.obj_index((SmAObject*)m->GetOtherRegions()[i]));
            for (ULONG i = 0; i < m->GetOtherRegionTypes().GetSize(); ++i)
                v.push_back((int64_t)m->GetOtherRegionTypes()[i]);
        }
        ints.push_back((int64_t)v.size());
        ints.insert(ints.end(), v.begin(), v.end());
        ints.push_back((int64_t)d.size());
        dbls.insert(dbls.end(), d.begin(), d.end());
    }
    ints.push_back((int64_t)r.handles.size());
    for (size_t h = 0; h < r.handles.size(); ++h)
        ints.push_back(r.consumed[h] ? -1 : index[r.handles[h]]);
    ints.push_back((int64_t)r.caches.size());
    for (size_t c = 0; c < r.caches.size(); ++c)
    {
        bool alive = r.cache_alive[c];
        ints.push_back(alive ? 1 : 0);
        ints.push_back(alive ? (int64_t)r.caches[c]->UseCount() : -1);
        ints.push_back(alive ? (r.caches[c]->Deleted() ? 1 : 0) : -1);
    }
    ints.push_back((int64_t)r.guards.size());
}
} // namespace

// Type system: per t in ts, (SmAObject::IsKindOf(t), SmSAGObject::IsKindOf(t)); then
// SmAObject GetType / GetClassType, SmSAGObject GetType / GetClassType,
// SmAObject::AssertValid at SM_LEVEL_0 and SM_LEVEL_GIVEN, and the strings
// SmAObject GetTypeString / GetClassString, SmSAGObject GetTypeString /
// GetClassString as (length, chars...).
SC_EXPORT int32_t sc_geom_a_object_types(const int64_t* ts, int64_t n, int64_t* out, int64_t cap, int64_t* out_len)
{
    if (n < 0) return SC_ERR_ARG;
    SC_TRY
    SmContext* ctx = shim_context();
    SmAObject sObj;
    SmBrep* pBrep = new (*ctx) SmBrep();
    SmSAGObject* pSag = pBrep;
    std::vector<int64_t> res;
    for (int64_t i = 0; i < n; ++i)
    {
        res.push_back(sObj.SmAObject::IsKindOf((SM_TYPE)ts[i]) ? 1 : 0);
        res.push_back(pSag->SmSAGObject::IsKindOf((SM_TYPE)ts[i]) ? 1 : 0);
    }
    res.push_back((int64_t)sObj.SmAObject::GetType());
    res.push_back((int64_t)SmAObject::GetClassType());
    res.push_back((int64_t)pSag->SmSAGObject::GetType());
    res.push_back((int64_t)SmSAGObject::GetClassType());
    res.push_back(sObj.SmAObject::AssertValid(nullptr, SM_LEVEL_0) ? 1 : 0);
    res.push_back(sObj.SmAObject::AssertValid(nullptr, SM_LEVEL_GIVEN) ? 1 : 0);
    for (const TCHAR* s : {sObj.SmAObject::GetTypeString(), sObj.SmAObject::GetClassString(),
                           pSag->SmSAGObject::GetTypeString(), pSag->SmSAGObject::GetClassString()})
    {
        size_t len = strlen(s);
        res.push_back((int64_t)len);
        for (size_t k = 0; k < len; ++k)
            res.push_back((int64_t)(unsigned char)s[k]);
    }
    // pBrep is leaked on purpose (deleting it runs the B-rep teardown).
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}

// NI_CPtToPtEuclid_specialized(noz, now, pw) -> out[0..3]; then
// NI_VectorBlendCPt_specialized(noz, now, alpha, aw, &bw) -> out[3..7].
SC_EXPORT int32_t sc_geom_a_object_nli(int32_t noz, int32_t now, const double* pw, double alpha, const double* aw,
                                       const double* bw, double* out)
{
    SC_TRY
    NL_CPOINT Pw;
    Pw.x = pw[0];
    Pw.y = pw[1];
    Pw.z = pw[2];
    Pw.w = pw[3];
    NL_POINT P;
    NI_CPtToPtEuclid_specialized(noz ? TRUE : FALSE, now ? TRUE : FALSE, Pw, &P);
    out[0] = P.x;
    out[1] = P.y;
    out[2] = P.z;
    NL_CPOINT Aw, Bw;
    Aw.x = aw[0];
    Aw.y = aw[1];
    Aw.z = aw[2];
    Aw.w = aw[3];
    Bw.x = bw[0];
    Bw.y = bw[1];
    Bw.z = bw[2];
    Bw.w = bw[3];
    NI_VectorBlendCPt_specialized(noz ? TRUE : FALSE, now ? TRUE : FALSE, alpha, Aw, &Bw);
    out[3] = Bw.x;
    out[4] = Bw.y;
    out[5] = Bw.z;
    out[6] = Bw.w;
    return SC_OK;
    SC_CATCH
}

// kinds[n_kinds]: initial objects (0 region, 1 SmAObject, 2 SmBrep); user_cb / sys_cb:
// install a recording notify callback; ops[8*n_ops]; payload_d: vector payloads.
// Output (int64 stream, doubles in a second stream):
//   per op: status, n_out, out...
//   n_objects, per object: type, n_attrs, canonical indices..., cache index (-1 none, -2 foreign)
//   n_attrs, per attr: kind, id, behavior, n_users, users..., n_vals, vals..., n_dbls
//   n_handles, per handle: canonical index or -1 (consumed)
//   n_caches, per cache: alive, use count, deleted
//   open guards
SC_EXPORT int32_t sc_geom_a_object_run(const int64_t* kinds, int64_t n_kinds, int32_t user_cb, int32_t sys_cb,
                                       const int64_t* ops, int64_t n_ops, const double* payload_d,
                                       int64_t n_payload_d, int64_t* ints, int64_t ints_cap, int64_t* ints_len,
                                       double* dbls, int64_t dbls_cap, int64_t* dbls_len)
{
    if (n_kinds < 0 || n_kinds > MAX_OBJECTS || n_ops < 0 || n_payload_d < 0) return SC_ERR_ARG;
    for (int64_t i = 0; i < n_kinds; ++i)
        if (kinds[i] < 0 || kinds[i] > 2) return SC_ERR_ARG;
    for (int64_t k = 0; k < n_ops; ++k)
        if (ops[8 * k] < 0 || ops[8 * k] > OP_LAST) return SC_ERR_ARG;
    SC_TRY
    Run r;
    r.ctx = shim_context();
    for (int64_t i = 0; i < n_kinds; ++i)
    {
        SmAObject* p = kinds[i] == O_REGION   ? (SmAObject*)new (*r.ctx) ShimRegion()
                       : kinds[i] == O_AOBJECT ? new (*r.ctx) SmAObject()
                                               : (SmAObject*)new (*r.ctx) SmBrep();
        r.objs.push_back(p);
        r.kinds.push_back(kinds[i]);
    }
    r.pay_d.assign(payload_d, payload_d + n_payload_d);
    Recorder sUser, sSys;
    sUser.m_lTag = 1;
    sSys.m_lTag = 2;
    sUser.m_pOut = sSys.m_pOut = &r.rec;
    sUser.m_pObjs = sSys.m_pObjs = &r.objs;
    r.ctx->SetUserNotifyCallback(user_cb ? &sUser : nullptr);
    r.ctx->SetSysNotifyCallback(nullptr);
    r.ctx->SetSysNotifyCallback(sys_cb ? &sSys : nullptr);
    std::vector<int64_t> o_ints;
    std::vector<double> o_dbls;
    int32_t status = SC_OK;
    for (int64_t k = 0; k < n_ops; ++k)
    {
        std::vector<int64_t> out;
        int64_t st = run_op(r, ops + 8 * k, out);
        if (st == SC_ERR_ARG)
        {
            status = SC_ERR_ARG;
            break;
        }
        o_ints.push_back(st);
        o_ints.push_back((int64_t)out.size());
        o_ints.insert(o_ints.end(), out.begin(), out.end());
    }
    r.ctx->SetUserNotifyCallback(nullptr);
    r.ctx->SetSysNotifyCallback(nullptr);
    if (status != SC_OK)
    {
        for (auto& g : r.guards)
            (void)g.release();
        for (ShimCache* c : r.caches)
            c->m_pAlive = nullptr;
        return status;
    }
    dump(r, o_ints, o_dbls);
    // Objects, attributes, caches and guards are leaked on purpose: their destructors
    // run Notify / ClearAttributes / cache deletion chains that are not compared.
    for (auto& g : r.guards)
        (void)g.release();
    for (ShimCache* c : r.caches)
        c->m_pAlive = nullptr;
    *ints_len = (int64_t)o_ints.size();
    *dbls_len = (int64_t)o_dbls.size();
    if (*ints_len > ints_cap || *dbls_len > dbls_cap) return SC_ERR_BUFFER;
    for (size_t i = 0; i < o_ints.size(); ++i)
        ints[i] = o_ints[i];
    for (size_t i = 0; i < o_dbls.size(); ++i)
        dbls[i] = o_dbls[i];
    return SC_OK;
    SC_CATCH
}
