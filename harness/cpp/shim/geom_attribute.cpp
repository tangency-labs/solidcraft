// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.attribute`.
// Wraps upstream SmAttribute and its derived classes (source/SMLib/src/SmAttribute.cpp,
// source/SMLib/inc/SmAttribute.h) together with the SmAObject attribute-list calls
// that drive them (AddAttribute / RemoveAttribute / ClearAttributes / FindAttribute).
//
// sc_geom_attribute_run runs a script against `n_objects` attributed objects (bare
// SmRegion instances, never placed in a B-rep) and a table of attribute "handles"
// (attributes created by the script, MakeCopy results and the placeholders created by
// ReadIndexedAttributesFromDB). Each op is 7 int64s: [op, a, b, c, d, e, f].
// Attribute references (`ref`): ref >= 0 is a handle index; ref < 0 is entry
// (k % 1024) of object (k / 1024)'s attribute list, k = -ref - 1.
//
// Statuses: 0 ok, ST_INVALID (-1) the reference does not resolve, ST_SKIP (-2) the op
// would hit undefined behaviour upstream (the Rust runner applies the same rule to
// its own state), otherwise the upstream SmStatus.
//
// Memory safety rests on the invariant "objects listing an attribute are a sub-multiset
// of its users" (see crates/difftest/src/units/geom_attribute.rs): direct RemoveUser /
// fromClearAttributes run only on attributes no object lists, and `delete` only on
// attributes with no users or standalone attributes whose users all list them. Which
// handles get deleted is predicted from the state before the op (upstream gives no
// way to observe a deletion); everything else in the dump is read back from upstream.
//
// Output (int64 stream, doubles in a second stream):
//   per op: status, n_out, out...
//   n_handles, alive[n_handles], canon[n_handles]   (canonical index or -1)
//   n_attrs, per attr: kind, type, id, behavior, n_users, users..., n_vals, vals..., n_dbls
//   per object: n, canonical indices...
//   n_all, all[n_all] (handle index or -1)
//   stream state, stream position (-1 -1 without a stream)
#include "sc_shim.h"

#include <SmApiGeneral.h>
#include <SmAttribute.h>
#include <SmDatabaseIO.h>
#include <SmBrep.h>
#include <SmRegion.h>

#include <cctype>
#include <cstring>
#include <map>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

namespace
{
enum Op : int64_t
{
    OP_NEW_PLAIN = 0,
    OP_NEW_GENERIC = 1,
    OP_NEW_LONG = 2,
    OP_NEW_POINTER = 3,
    OP_NEW_POINTER_LIST = 4,
    OP_NEW_VECTOR3D = 5,
    OP_NEW_COLOR = 6,
    OP_NEW_TAG = 7,
    OP_NEW_MERGE_REGION = 8,
    OP_MAKE_COPY = 9,
    OP_ADD_ATTRIBUTE = 10,
    OP_REMOVE_ATTRIBUTE = 11,
    OP_CLEAR_ATTRIBUTES = 12,
    OP_SPLIT = 13,
    OP_COPY = 14,
    OP_MERGE = 15,
    OP_COINCIDENT = 16,
    OP_EDIT = 17,
    OP_DESTRUCTION = 18,
    OP_PROPAGATE = 19,
    OP_ADD_USER = 20,
    OP_REMOVE_USER = 21,
    OP_GET_USERS = 22,
    OP_QUERY = 23,
    OP_MR_QUERY = 24,
    OP_MR_SET_REGIONS = 25,
    OP_MR_SET_OPERATION = 26,
    OP_SET_VECTOR = 27,
    OP_DESTROY = 28,
    OP_READ_INDEXED = 29,
    OP_FIND = 30,
    OP_FROM_CLEAR = 31,
    OP_SET_IS_VOID = 32,
    OP_MAKE_INFINITE = 33,
    OP_LAST = 33,
};

const int64_t ST_INVALID = -1;
const int64_t ST_SKIP = -2;
const int64_t REF_STRIDE = 1024;
const unsigned long READ_CAP = 1ul << 20;

enum Kind : int64_t
{
    K_PLAIN = 0,
    K_GENERIC = 1,
    K_LONG = 2,
    K_POINTER = 3,
    K_POINTER_LIST = 4,
    K_VECTOR3D = 5,
    K_MERGE_REGION = 6,
    K_TAG = 7,
    K_COLOR = 8,
};

const SM_TYPE QUERY_TYPES[11] = {SmObject_TYPE,          SmAttribute_TYPE,        SmLongAttribute_TYPE,
                                 SmVector3dAttribute_TYPE, SmGenericAttribute_TYPE, SmPointerAttribute_TYPE,
                                 SmPointerListAttribute_TYPE, SmMergeRegionAttribute_TYPE,
                                 SmTagAttribute_TYPE,     SmColorAttribute_TYPE,   CONT_BASE_TYPE};

struct ShimRegion : public SmRegion
{
    ShimRegion() : SmRegion() {}
    // SmTopology only offers `new (SmObject*)`; allocate from the context directly.
    static void* operator new(size_t size, const SmContext& c) { return SmObject::operator new(size, c); }
    static void operator delete(void* p, const SmContext&) { smos_Free(p); }
    static void operator delete(void* p) { smos_Free(p); }
};

int64_t kind_of(const SmAttribute* a)
{
    switch (a->GetType())
    {
    case SmAttribute_TYPE:
        return K_PLAIN;
    case SmGenericAttribute_TYPE:
        return K_GENERIC;
    case SmLongAttribute_TYPE:
        return K_LONG;
    case SmPointerAttribute_TYPE:
        return a->IsKindOf(SmMergeRegionAttribute_TYPE) ? K_MERGE_REGION : K_POINTER;
    case SmPointerListAttribute_TYPE:
        return K_POINTER_LIST;
    case SmVector3dAttribute_TYPE:
        return a->IsKindOf(SmColorAttribute_TYPE) ? K_COLOR : K_VECTOR3D;
    case SmTagAttribute_TYPE:
        return K_TAG;
    default:
        return -1;
    }
}

bool is_vector(const SmAttribute* a)
{
    int64_t k = kind_of(a);
    return k == K_VECTOR3D || k == K_COLOR;
}

bool standalone(const SmAttribute* a)
{
    return a->GetBehavior() == SM_AB_STANDALONE_COPY || a->GetBehavior() == SM_AB_STANDALONE_REFERENCE;
}

struct Run
{
    const SmContext* ctx = nullptr;
    std::vector<SmRegion*> objs;
    std::vector<SmAttribute*> handles;
    std::vector<bool> alive;
    SmTArray<SmAttribute*> all;
    std::vector<int64_t> all_handle; // handle index of each placeholder slot (-1 empty)
    std::vector<int64_t> pay_i;
    std::vector<double> pay_d;
    size_t pi = 0, pd = 0;
    std::string data;
    std::unique_ptr<std::istringstream> iss;
    std::unique_ptr<SmDatabaseIO> db;
    bool bad_payload = false;

    int64_t next_i()
    {
        if (pi >= pay_i.size())
        {
            bad_payload = true;
            return 0;
        }
        return pay_i[pi++];
    }
    double next_d()
    {
        if (pd >= pay_d.size())
        {
            bad_payload = true;
            return 0.0;
        }
        return pay_d[pd++];
    }

    std::vector<SmAttribute*> list(int64_t o) const
    {
        SmTArray<SmAttribute*> t;
        objs[(size_t)o]->GetAttributes(t);
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
            if ((const void*)objs[i] == p || (const void*)static_cast<SmAObject*>(objs[i]) == p)
                return (int64_t)i;
        return -999;
    }

    // Occurrences of `a` over all object lists.
    int64_t listings(const SmAttribute* a) const
    {
        int64_t n = 0;
        for (size_t o = 0; o < objs.size(); ++o)
            for (SmAttribute* x : list((int64_t)o))
                if (x == a) ++n;
        return n;
    }

    SmAttribute* resolve(int64_t ref) const
    {
        if (ref >= 0)
        {
            if ((size_t)ref >= handles.size() || !alive[(size_t)ref]) return nullptr;
            return handles[(size_t)ref];
        }
        int64_t k = -ref - 1;
        int64_t o = k / REF_STRIDE, i = k % REF_STRIDE;
        if (o < 0 || (size_t)o >= objs.size()) return nullptr;
        std::vector<SmAttribute*> l = list(o);
        if ((size_t)i >= l.size()) return nullptr;
        return l[(size_t)i];
    }

    bool valid_obj(int64_t o) const { return o >= 0 && (size_t)o < objs.size(); }

    void mark_dead(const SmAttribute* a)
    {
        for (size_t i = 0; i < handles.size(); ++i)
            if (alive[i] && handles[i] == a) alive[i] = false;
    }

    int64_t add_handle(SmAttribute* a)
    {
        handles.push_back(a);
        alive.push_back(true);
        return (int64_t)handles.size() - 1;
    }
};

bool is_c_space(char c)
{
    return c == ' ' || c == '\t' || c == '\n' || c == '\v' || c == '\f' || c == '\r';
}

int64_t stream_pos(std::istream& in)
{
    return (int64_t)in.rdbuf()->pubseekoff(0, std::ios_base::cur, std::ios_base::in);
}

uint64_t load_u64(const std::string& d, size_t at, bool swap)
{
    unsigned char b[8];
    std::memcpy(b, d.data() + at, 8);
    if (swap)
        for (int i = 0; i < 4; ++i)
            std::swap(b[i], b[7 - i]);
    uint64_t v;
    std::memcpy(&v, b, 8);
    return v;
}

// Whether ReadIndexedAttributesFromDB can run without upstream undefined behaviour.
bool read_is_safe(Run& r)
{
    if (!r.db) return false;
    std::istream* in = r.db->GetInStreamPtr();
    if (in == nullptr || !in->good()) return false;
    int64_t pos = stream_pos(*in);
    if (pos < 0) return false;
    if (r.db->GetFileType() == SM_ASCII)
    {
        for (size_t i = (size_t)pos; i < r.data.size(); ++i)
            if (!is_c_space(r.data[i])) return true;
        return false;
    }
    bool swap = r.db->GetByteSwap() != 0;
    size_t p = (size_t)pos;
    if (r.data.size() < p + 8) return false;
    uint64_t n = load_u64(r.data, p, swap);
    if (n > READ_CAP) return false;
    if ((r.data.size() - p - 8) / 8 < n) return false;
    for (uint64_t i = 0; i < n; ++i)
        if (load_u64(r.data, p + 8 + 8 * (size_t)i, swap) > READ_CAP) return false;
    return true;
}

// Runs one op; returns its status and appends outputs.
int64_t run_op(Run& r, const int64_t* a, std::vector<int64_t>& out)
{
    const SmContext& ctx = *r.ctx;
    int64_t op = a[0];
    switch (op)
    {
    case OP_NEW_PLAIN:
        out.push_back(r.add_handle(new (ctx) SmAttribute((ULONG)a[1], (SmAttributeBehaviorType)a[2])));
        return 0;
    case OP_NEW_GENERIC:
    {
        SmTArray<long> ls;
        SmTArray<double> ds;
        SmTArray<char> cs;
        for (int64_t i = 0; i < a[3]; ++i)
            ls.Add((long)r.next_i());
        for (int64_t i = 0; i < a[4]; ++i)
            ds.Add(r.next_d());
        for (int64_t i = 0; i < a[5]; ++i)
            cs.Add((char)(unsigned char)r.next_i());
        if (r.bad_payload) return SC_ERR_ARG;
        out.push_back(
            r.add_handle(new (ctx) SmGenericAttribute((ULONG)a[1], (SmAttributeBehaviorType)a[2], ls, ds, cs)));
        return 0;
    }
    case OP_NEW_LONG:
        out.push_back(r.add_handle(new (ctx) SmLongAttribute((ULONG)a[1], (long)a[2], (SmAttributeBehaviorType)a[3])));
        return 0;
    case OP_NEW_POINTER:
        out.push_back(r.add_handle(
            new (ctx) SmPointerAttribute((ULONG)a[1], (void*)(uintptr_t)a[2], (SmAttributeBehaviorType)a[3])));
        return 0;
    case OP_NEW_POINTER_LIST:
    {
        SmTArray<SmObject*> l;
        for (int64_t i = 0; i < a[3]; ++i)
            l.Add((SmObject*)(uintptr_t)r.next_i());
        if (r.bad_payload) return SC_ERR_ARG;
        out.push_back(r.add_handle(
            new (ctx) SmPointerListAttribute((ULONG)a[1], a[4] != 0 ? nullptr : &l, (SmAttributeBehaviorType)a[2])));
        return 0;
    }
    case OP_NEW_VECTOR3D:
    case OP_NEW_COLOR:
    {
        double x = r.next_d(), y = r.next_d(), z = r.next_d();
        if (r.bad_payload) return SC_ERR_ARG;
        SmVector3d v(x, y, z);
        SmAttribute* at = op == OP_NEW_COLOR
                              ? (SmAttribute*)new (ctx) SmColorAttribute((ULONG)a[1], v, (SmAttributeBehaviorType)a[2])
                              : (SmAttribute*)new (ctx) SmVector3dAttribute((ULONG)a[1], v, (SmAttributeBehaviorType)a[2]);
        out.push_back(r.add_handle(at));
        return 0;
    }
    case OP_NEW_TAG:
    {
        long t[8];
        for (int i = 0; i < 8; ++i)
            t[i] = (long)r.next_i();
        if (r.bad_payload) return SC_ERR_ARG;
        out.push_back(r.add_handle(new (ctx) SmTagAttribute((ULONG)a[1], t[0], t[1], t[2], t[3], t[4], t[5], t[6],
                                                            t[7], (SmAttributeBehaviorType)a[2])));
        return 0;
    }
    case OP_SET_IS_VOID:
        // SmRegion::SetIsVoid(TRUE / UNSURE / FALSE) on a script object.
        if (!r.valid_obj(a[1]) || a[2] < 0 || a[2] > 2) return SC_ERR_ARG;
        r.objs[(size_t)a[1]]->SetIsVoid((SmBoolean)a[2]);
        return 0;
    case OP_MAKE_INFINITE:
    {
        // Replace the object with the infinite region of a fresh B-rep, so
        // IsInfiniteRegion()/GetBrep() are TRUE/non-NULL. Attributes created earlier
        // keep pointing at the old object, so this is only defined before any
        // attribute exists (the Rust runner applies the same rule).
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        if (!r.handles.empty() || r.all.GetSize() > 0) return ST_SKIP;
        SmBrep* brep = new (*r.ctx) SmBrep();
        if (brep->GetInfiniteRegion() == nullptr) return SC_ERR_ARG;
        r.objs[(size_t)a[1]] = brep->GetInfiniteRegion();
        return 0;
    }
    case OP_NEW_MERGE_REGION:
        if (!r.valid_obj(a[1]) || !r.valid_obj(a[3])) return SC_ERR_ARG;
        out.push_back(r.add_handle(new (ctx) SmMergeRegionAttribute(r.objs[(size_t)a[1]], (ULONG)a[2],
                                                                    r.objs[(size_t)a[3]], (ULONG)a[4])));
        return 0;
    case OP_READ_INDEXED:
    {
        if (!read_is_safe(r)) return ST_SKIP;
        ULONG before = r.all.GetSize();
        std::vector<SmAttribute*> old(before);
        for (ULONG i = 0; i < before; ++i)
            old[i] = r.all[i];
        SmTArray<ULONG> idx;
        SmStatus st = SmAttributeData::ReadIndexedAttributesFromDB(ctx, r.all, idx, *r.db);
        // Placeholders created by this call become handles, in slot order.
        r.all_handle.resize(r.all.GetSize(), -1);
        for (ULONG i = 0; i < r.all.GetSize(); ++i)
            if (r.all[i] != nullptr && (i >= before || old[i] == nullptr)) r.all_handle[i] = r.add_handle(r.all[i]);
        if (st != SM_SUCCESS) return st;
        for (ULONG i = 0; i < idx.GetSize(); ++i)
            out.push_back((int64_t)idx[i]);
        return 0;
    }
    case OP_FIND:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmAttribute* f = r.objs[(size_t)a[1]]->FindAttribute((ULONG)a[2]);
        std::vector<SmAttribute*> l = r.list(a[1]);
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
    case OP_CLEAR_ATTRIBUTES:
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        // Predict deletions: per entry, users != 1 -> RemoveUser (never deletes under
        // the invariant), users == 1 -> reset and delete unless standalone.
        std::map<SmAttribute*, int64_t> count;
        for (SmAttribute* x : r.list(a[1]))
        {
            if (!count.count(x)) count[x] = (int64_t)r.users(x).size();
            int64_t& u = count[x];
            if (u != 1)
                --u;
            else
            {
                u = 0;
                if (!standalone(x)) r.mark_dead(x);
            }
        }
        r.objs[(size_t)a[1]]->ClearAttributes();
        return 0;
    }
    default:
        break;
    }

    // Ops on an attribute reference (a[1]).
    if (op == OP_ADD_ATTRIBUTE || op == OP_REMOVE_ATTRIBUTE)
    {
        if (!r.valid_obj(a[1])) return SC_ERR_ARG;
        SmAttribute* at = r.resolve(a[2]);
        if (at == nullptr) return ST_INVALID;
        SmRegion* o = r.objs[(size_t)a[1]];
        if (op == OP_ADD_ATTRIBUTE)
        {
            o->AddAttribute(at);
            return 0;
        }
        std::vector<SmAttribute*> l = r.list(a[1]);
        bool listed = false;
        for (SmAttribute* x : l)
            listed |= x == at;
        if (listed && r.users(at).size() == 1 && a[3] == 0 && !standalone(at)) r.mark_dead(at);
        o->RemoveAttribute(at, a[3] != 0 ? TRUE : FALSE);
        return 0;
    }

    SmAttribute* at = r.resolve(a[1]);
    if (at == nullptr) return ST_INVALID;
    auto obj = [&](int64_t i) -> SmAObject* { return r.objs[(size_t)i]; };
    switch (op)
    {
    case OP_MAKE_COPY:
        out.push_back(r.add_handle(at->MakeCopy(ctx)));
        return 0;
    case OP_SPLIT:
        if (!r.valid_obj(a[2]) || !r.valid_obj(a[3]) || !r.valid_obj(a[4])) return SC_ERR_ARG;
        at->Split(obj(a[2]), obj(a[3]), obj(a[4]));
        return 0;
    case OP_COPY:
        if (!r.valid_obj(a[2]) || !r.valid_obj(a[3])) return SC_ERR_ARG;
        at->Copy(obj(a[2]), obj(a[3]));
        return 0;
    case OP_MERGE:
    {
        if (!r.valid_obj(a[2]) || !r.valid_obj(a[3]) || !r.valid_obj(a[4])) return SC_ERR_ARG;
        if (kind_of(at) == K_MERGE_REGION)
        {
            SmAttribute* f[3] = {obj(a[4])->FindAttribute(SM_AI_OTHER_REGION_ID),
                                 obj(a[2])->FindAttribute(SM_AI_OTHER_REGION_ID),
                                 obj(a[3])->FindAttribute(SM_AI_OTHER_REGION_ID)};
            for (SmAttribute* x : f)
                if (x != nullptr && kind_of(x) != K_MERGE_REGION) return ST_SKIP;
            if (f[1] != nullptr && f[2] != nullptr && f[0] == nullptr) return ST_SKIP;
        }
        at->Merge(obj(a[2]), obj(a[3]), obj(a[4]));
        return 0;
    }
    case OP_COINCIDENT:
        if (!r.valid_obj(a[2]) || !r.valid_obj(a[3])) return SC_ERR_ARG;
        at->Coincident(obj(a[2]), obj(a[3]), nullptr, nullptr);
        return 0;
    case OP_EDIT:
        if (!r.valid_obj(a[2])) return SC_ERR_ARG;
        at->Edit(obj(a[2]));
        return 0;
    case OP_DESTRUCTION:
        if (!r.valid_obj(a[2])) return SC_ERR_ARG;
        at->Destruction(obj(a[2]));
        return 0;
    case OP_PROPAGATE:
    {
        if (!r.valid_obj(a[2]) || !r.valid_obj(a[3]) || !r.valid_obj(a[4])) return SC_ERR_ARG;
        if (kind_of(at) == K_COLOR)
        {
            SmAttribute* c1 = obj(a[2])->FindAttribute(SM_AI_COLOR);
            SmAttribute* c2 = obj(a[3])->FindAttribute(SM_AI_COLOR);
            SmAttribute* cm = obj(a[4])->FindAttribute(SM_AI_COLOR);
            if (c1 != nullptr && c2 != nullptr && (!is_vector(c1) || !is_vector(c2) || cm == nullptr || !is_vector(cm)))
                return ST_SKIP;
        }
        at->PropagateRegionsThroughBooleans(obj(a[2]), obj(a[3]), obj(a[4]));
        return 0;
    }
    case OP_ADD_USER:
        if (!r.valid_obj(a[2])) return SC_ERR_ARG;
        at->AddUser(obj(a[2]));
        return 0;
    case OP_REMOVE_USER:
    case OP_FROM_CLEAR:
    {
        if (!r.valid_obj(a[2])) return SC_ERR_ARG;
        if (r.listings(at) > 0) return ST_SKIP;
        std::vector<SmAObject*> u = r.users(at);
        bool found = false;
        for (SmAObject* x : u)
            found |= x == obj(a[2]);
        if (op == OP_FROM_CLEAR && u.size() == 1)
        {
            if (!standalone(at)) r.mark_dead(at);
            at->fromClearAttributes(obj(a[2]));
            return 0;
        }
        bool dnd = op == OP_REMOVE_USER && a[3] != 0;
        if (found && u.size() == 1 && !dnd && !standalone(at)) r.mark_dead(at);
        if (op == OP_FROM_CLEAR)
            at->fromClearAttributes(obj(a[2]));
        else
            at->RemoveUser(obj(a[2]), dnd ? TRUE : FALSE);
        return 0;
    }
    case OP_GET_USERS:
        for (SmAObject* x : r.users(at))
            out.push_back(r.obj_index(x));
        return 0;
    case OP_QUERY:
    {
        out.push_back((int64_t)at->GetType());
        int64_t mask = 0;
        for (int i = 0; i < 11; ++i)
            if (at->IsKindOf(QUERY_TYPES[i])) mask |= (int64_t)1 << i;
        out.push_back(mask);
        out.push_back(at->IsPropagatedThroughBooleans() ? 1 : 0);
        out.push_back((int64_t)at->GetAttributeID());
        out.push_back((int64_t)at->GetBehavior());
        for (const TCHAR* s = at->GetTypeString(); *s; ++s)
            out.push_back((int64_t)(unsigned char)*s);
        return 0;
    }
    case OP_MR_QUERY:
    {
        if (kind_of(at) != K_MERGE_REGION) return ST_SKIP;
        SmMergeRegionAttribute* m = (SmMergeRegionAttribute*)at;
        out.push_back(m->HasThisInfiniteRegion() ? 1 : 0);
        out.push_back(m->HasOtherInfiniteRegion() ? 1 : 0);
        out.push_back(m->HasAnySolid((ULONG)a[2]) ? 1 : 0);
        out.push_back((int64_t)m->GetOperation());
        out.push_back(m->HasAnyOtherPropagatedAttribs() ? 1 : 0);
        out.push_back((int64_t)m->GetThisRegions().GetSize());
        out.push_back((int64_t)m->GetThisRegionTypes().GetSize());
        out.push_back((int64_t)m->GetOtherRegions().GetSize());
        if (m->GetThisRegions().GetSize() > 0) out.push_back(r.obj_index(m->GetThisRegion(0)));
        return 0;
    }
    case OP_MR_SET_REGIONS:
    {
        if (kind_of(at) != K_MERGE_REGION) return ST_SKIP;
        if (!r.valid_obj(a[3]) || !r.valid_obj(a[5])) return SC_ERR_ARG;
        SmMergeRegionAttribute* m = (SmMergeRegionAttribute*)at;
        ULONG idx = (ULONG)a[2];
        if (idx < m->GetThisRegions().GetSize() &&
            (idx >= m->GetThisRegionTypes().GetSize() || idx >= m->GetOtherRegions().GetSize() ||
             idx >= m->GetOtherRegionTypes().GetSize()))
            return ST_SKIP;
        SmStatus st = m->SetRegions(idx, r.objs[(size_t)a[3]], (ULONG)a[4], r.objs[(size_t)a[5]], (ULONG)a[6]);
        return st == SM_SUCCESS ? 0 : st;
    }
    case OP_MR_SET_OPERATION:
    {
        if (kind_of(at) != K_MERGE_REGION) return ST_SKIP;
        ((SmMergeRegionAttribute*)at)->SetOperation((SmBooleanOperationType)a[2]);
        return 0;
    }
    case OP_SET_VECTOR:
    {
        double x = r.next_d(), y = r.next_d(), z = r.next_d();
        if (r.bad_payload) return SC_ERR_ARG;
        if (!is_vector(at)) return ST_SKIP;
        SmVector3d v(x, y, z);
        ((SmVector3dAttribute*)at)->SetValue(v);
        return 0;
    }
    case OP_DESTROY:
    {
        std::vector<SmAObject*> u = r.users(at);
        if (!u.empty())
        {
            if (!standalone(at)) return ST_SKIP;
            // users must equal listings (as multisets) so the destructor's loop
            // unlinks every listing
            std::map<const void*, int64_t> diff;
            for (SmAObject* x : u)
                diff[x] += 1;
            for (size_t o = 0; o < r.objs.size(); ++o)
                for (SmAttribute* x : r.list((int64_t)o))
                    if (x == at) diff[static_cast<SmAObject*>(r.objs[o])] -= 1;
            for (auto& kv : diff)
                if (kv.second != 0) return ST_SKIP;
        }
        r.mark_dead(at);
        delete at;
        return 0;
    }
    default:
        return SC_ERR_ARG;
    }
}

void dump(Run& r, std::vector<int64_t>& ints, std::vector<double>& dbls)
{
    std::vector<SmAttribute*> canon;
    std::map<SmAttribute*, int64_t> index;
    auto visit = [&](SmAttribute* a) {
        if (index.count(a)) return;
        index[a] = (int64_t)canon.size();
        canon.push_back(a);
    };
    for (size_t i = 0; i < r.handles.size(); ++i)
        if (r.alive[i]) visit(r.handles[i]);
    std::vector<std::vector<SmAttribute*>> lists;
    for (size_t o = 0; o < r.objs.size(); ++o)
    {
        lists.push_back(r.list((int64_t)o));
        for (SmAttribute* a : lists.back())
            visit(a);
    }
    ints.push_back((int64_t)r.handles.size());
    for (size_t i = 0; i < r.handles.size(); ++i)
        ints.push_back(r.alive[i] ? 1 : 0);
    for (size_t i = 0; i < r.handles.size(); ++i)
        ints.push_back(r.alive[i] ? index[r.handles[i]] : -1);
    ints.push_back((int64_t)canon.size());
    for (SmAttribute* a : canon)
    {
        int64_t k = kind_of(a);
        ints.push_back(k);
        ints.push_back((int64_t)a->GetType());
        ints.push_back((int64_t)a->GetAttributeID());
        ints.push_back((int64_t)a->GetBehavior());
        std::vector<SmAObject*> u = r.users(a);
        ints.push_back((int64_t)u.size());
        for (SmAObject* x : u)
            ints.push_back(r.obj_index(x));
        std::vector<int64_t> v;
        std::vector<double> d;
        switch (k)
        {
        case K_GENERIC:
        {
            SmGenericAttribute* g = (SmGenericAttribute*)a;
            v.push_back((int64_t)g->GetLongTArray().GetSize());
            for (ULONG i = 0; i < g->GetLongTArray().GetSize(); ++i)
                v.push_back((int64_t)g->GetLongTArray()[i]);
            v.push_back((int64_t)g->GetCharTArray().GetSize());
            for (ULONG i = 0; i < g->GetCharTArray().GetSize(); ++i)
                v.push_back((int64_t)(unsigned char)g->GetCharTArray()[i]);
            for (ULONG i = 0; i < g->GetDoubleTArray().GetSize(); ++i)
                d.push_back(g->GetDoubleTArray()[i]);
            break;
        }
        case K_LONG:
            v.push_back((int64_t)((SmLongAttribute*)a)->GetValue());
            break;
        case K_POINTER:
            v.push_back((int64_t)(uintptr_t)((SmPointerAttribute*)a)->GetValue());
            break;
        case K_POINTER_LIST:
        {
            const SmTArray<SmObject*>& l = ((SmPointerListAttribute*)a)->GetValue();
            v.push_back((int64_t)l.GetSize());
            for (ULONG i = 0; i < l.GetSize(); ++i)
                v.push_back((int64_t)(uintptr_t)l.GetAt(i));
            break;
        }
        case K_VECTOR3D:
        case K_COLOR:
        {
            const SmVector3d& x = ((SmVector3dAttribute*)a)->GetValue();
            d.push_back(x.x);
            d.push_back(x.y);
            d.push_back(x.z);
            break;
        }
        case K_MERGE_REGION:
        {
            SmMergeRegionAttribute* m = (SmMergeRegionAttribute*)a;
            v.push_back((int64_t)m->GetOperation());
            v.push_back((int64_t)m->GetThisRegions().GetSize());
            for (ULONG i = 0; i < m->GetThisRegions().GetSize(); ++i)
                v.push_back(r.obj_index(m->GetThisRegions()[i]));
            v.push_back((int64_t)m->GetThisRegionTypes().GetSize());
            for (ULONG i = 0; i < m->GetThisRegionTypes().GetSize(); ++i)
                v.push_back((int64_t)m->GetThisRegionTypes()[i]);
            v.push_back((int64_t)m->GetOtherRegions().GetSize());
            for (ULONG i = 0; i < m->GetOtherRegions().GetSize(); ++i)
                v.push_back(r.obj_index(m->GetOtherRegions()[i]));
            v.push_back((int64_t)m->GetOtherRegionTypes().GetSize());
            for (ULONG i = 0; i < m->GetOtherRegionTypes().GetSize(); ++i)
                v.push_back((int64_t)m->GetOtherRegionTypes()[i]);
            break;
        }
        case K_TAG:
        {
            SmTagAttribute* t = (SmTagAttribute*)a;
            long f[8] = {t->m_lPrimaryID,        t->m_lPrimaryOrdinal, t->m_lSecondaryIDType, t->m_lSecondaryID,
                         t->m_lSecondaryOrdinal, t->m_lThirdIDType,    t->m_lThirdID,         t->m_lThirdOrdinal};
            for (long x : f)
                v.push_back((int64_t)x);
            break;
        }
        default:
            break;
        }
        ints.push_back((int64_t)v.size());
        ints.insert(ints.end(), v.begin(), v.end());
        ints.push_back((int64_t)d.size());
        dbls.insert(dbls.end(), d.begin(), d.end());
    }
    for (auto& l : lists)
    {
        ints.push_back((int64_t)l.size());
        for (SmAttribute* a : l)
            ints.push_back(index[a]);
    }
    ints.push_back((int64_t)r.all.GetSize());
    for (ULONG i = 0; i < r.all.GetSize(); ++i)
        ints.push_back(i < r.all_handle.size() ? r.all_handle[i] : -1);
    std::istream* in = r.db ? r.db->GetInStreamPtr() : nullptr;
    ints.push_back(in ? (int64_t)in->rdstate() : -1);
    ints.push_back(in ? stream_pos(*in) : -1);
}
} // namespace

// n_objects: attributed objects; ops[7*n_ops]; payload_i / payload_d: creation data
// consumed in order; data/file_type/byte_swap: the stream of OP_READ_INDEXED
// (file_type < 0: no stream).
SC_EXPORT int32_t sc_geom_attribute_run(int64_t n_objects, const int64_t* ops, int64_t n_ops,
                                        const int64_t* payload_i, int64_t n_payload_i, const double* payload_d,
                                        int64_t n_payload_d, const uint8_t* data, int64_t data_len,
                                        int32_t file_type, int32_t byte_swap, int64_t* ints, int64_t ints_cap,
                                        int64_t* ints_len, double* dbls, int64_t dbls_cap, int64_t* dbls_len)
{
    if (n_objects < 0 || n_objects > 64 || n_ops < 0 || n_payload_i < 0 || n_payload_d < 0 || data_len < 0 ||
        file_type > 3)
        return SC_ERR_ARG;
    SC_TRY
    Run r;
    r.ctx = SmApiGetOrCreateContext();
    if (r.ctx == nullptr) return SC_ERR_ARG;
    for (int64_t i = 0; i < n_objects; ++i)
        r.objs.push_back(new (*r.ctx) ShimRegion());
    r.pay_i.assign(payload_i, payload_i + n_payload_i);
    r.pay_d.assign(payload_d, payload_d + n_payload_d);
    if (file_type >= 0)
    {
        r.data.assign((const char*)data, (size_t)data_len);
        r.iss.reset(new std::istringstream(r.data));
        r.db.reset(new SmDatabaseIOStream((SmFileType)file_type, r.iss.get()));
        r.db->SetByteSwap(byte_swap != 0 ? TRUE : FALSE);
    }
    std::vector<int64_t> o_ints;
    std::vector<double> o_dbls;
    for (int64_t k = 0; k < n_ops; ++k)
    {
        const int64_t* a = ops + 7 * k;
        if (a[0] < 0 || a[0] > OP_LAST) return SC_ERR_ARG;
        std::vector<int64_t> out;
        int64_t st = run_op(r, a, out);
        if (st == SC_ERR_ARG) return SC_ERR_ARG;
        o_ints.push_back(st);
        o_ints.push_back((int64_t)out.size());
        o_ints.insert(o_ints.end(), out.begin(), out.end());
    }
    dump(r, o_ints, o_dbls);
    // Objects and surviving attributes are leaked on purpose: deleting a region runs
    // its Notify/ClearAttributes chain, which is not what is being compared.
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
