// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.topology`. Wraps upstream SmTopology (marks, flags,
// user values, Notify, type system, GetTolerance, destructor; source/SMLib/src/SmTopology.cpp,
// source/SMLib/inc/SmTopology.h) and SmTrackTopologyChanges
// (source/SMLib/src/SmTrackTopologyChanges.cpp, source/SMLib/inc/SmTrackTopologyChanges.h).
//
// sc_topology_topology_run runs a script over a pool of bare SmTopology objects (all in
// one fresh SmContext), one empty SmBrep (the tracked B-rep), a stack of
// SmNewMarkAndLock objects and a list of SmTrackTopologyChanges trackers. Each op is
// 8 int64s [op, a, b, c, d, e, f, g]. A user notify callback (optional) records every
// Execute; op OP_SYS_FOREIGN installs or clears a second recorder as the system
// callback (so tracking meets an occupied slot).
//
// Per op output: status, n, n values. Statuses: 0 ok, ST_SKIP (-2) the op refers to a
// destroyed object or tracker, needs a lock when none is held, or would destroy a
// tracker whose callback stays installed (the Rust runner applies the same rules).
// TRACK_LISTS emits the rm, add and chg lists (length then object indices) and the
// notify events (length then action, caller, data1..3 as object indices). SET_KIND
// gives a pool object a stand-in topology type code for the tracker's IsKindOf
// classification; it keeps SmTopology storage, and the state dump asks the base
// class explicitly, so the dump's type answers do not change. Tolerances go to the
// double output.
#include "sc_shim.h"

#include <SmBrep.h>
#include <SmContext.h>
#include <SmTopology.h>
#include <SmTrackTopologyChanges.h>

#include <memory>
#include <vector>

namespace
{
enum Op : int64_t
{
    OP_SET_FLAGS = 0,
    OP_GET_FLAGS = 1,
    OP_SET_UIDX1 = 2,
    OP_SET_UIDX2 = 3,
    OP_SET_UPTR = 4,
    OP_GET_USER = 5,
    OP_MARK = 6,
    OP_GET_MARK = 7,
    OP_IS_MARKED = 8,
    OP_LOCK = 9,
    OP_LOCK_NEW_MARK = 10,
    OP_UNLOCK = 11,
    OP_NOTIFY = 12,
    OP_TOLERANCE = 13,
    OP_CONNECTED = 14,
    OP_TRACK_NEW = 15,
    OP_TRACK_START = 16,
    OP_TRACK_STOP = 17,
    OP_TRACK_IS = 18,
    OP_TRACK_LISTS = 19,
    OP_TRACK_RESET = 20,
    OP_TRACK_DROP = 21,
    OP_SYS_FOREIGN = 22,
    OP_DESTROY = 23,
    OP_SET_KIND = 24,
    OP_LAST = 24,
};

const int64_t ST_SKIP = -2;
const int64_t MAX_OBJECTS = 32;
const int64_t MAX_TRACKERS = 16;

struct ShimTopology : public SmTopology
{
    ShimTopology() : SmTopology() {}
    // Stand-in kind for the change tracker's IsKindOf tests (OP_SET_KIND): the
    // object keeps SmTopology storage, and SmTrackTopologyChanges classifies it by
    // type code alone. The state dump asks the base class explicitly, so its type
    // answers do not change.
    SM_TYPE m_kind = SmTopology_TYPE;
    SM_TYPE GetType() const override { return m_kind; }
    SmBoolean IsKindOf(SM_TYPE t) const override { return t == m_kind || SmTopology::IsKindOf(t); }
    static void* operator new(size_t size, const SmContext& c) { return SmObject::operator new(size, c); }
    static void operator delete(void* p, const SmContext&) { smos_Free(p); }
    static void operator delete(void* p) { smos_Free(p); }
};

// Exposes the tracker's "a callback was installed when tracking started" state.
struct ShimTracker : public SmTrackTopologyChanges
{
    ShimTracker(SmBrep* pBrep, SmBoolean bStartNow) : SmTrackTopologyChanges(pBrep, bStartNow) {}
    bool LastSet() const { return m_pLastNotifyCallback != nullptr; }
};

bool valid_mark_type(int64_t m)
{
    return m == 0 || m == 1 || m == 2 || m == 4 || m == 8 || m == 16 || m == 31;
}

struct Run;

// A notify callback recording (tag, op, caller, caller type, data1, data2, data3).
struct Recorder : public SmNotifyCallback
{
    int64_t m_lTag = 0;
    Run* m_pRun = nullptr;
    SmStatus Execute(SmNotifyOperation eOp, SmObject* pObj, SmObject* p1, SmObject* p2, SmObject* p3) override;
};

struct Run
{
    SmContext* ctx = nullptr;
    SmBrep* brep = nullptr;
    std::vector<ShimTopology*> objs; // nullptr once destroyed
    std::vector<std::unique_ptr<SmNewMarkAndLock>> locks;
    std::vector<ShimTracker*> trackers; // nullptr once dropped
    std::vector<int64_t> rec;
    std::vector<double> dbls;
    Recorder foreign;

    int64_t Index(const void* p) const
    {
        if (p == nullptr) return -1;
        for (size_t i = 0; i < objs.size(); ++i)
            if (objs[i] != nullptr && (const void*)static_cast<SmObject*>(objs[i]) == p) return (int64_t)i;
        if (p == (const void*)static_cast<SmObject*>(brep)) return MAX_OBJECTS;
        return -2;
    }

    // -1: null; >= 0: a live object; SC_ERR_ARG out of range; ST_SKIP destroyed.
    int64_t obj_ref(int64_t o, bool allow_null) const
    {
        if (o == -1 && allow_null) return -1;
        if (o < 0 || o >= (int64_t)objs.size()) return SC_ERR_ARG;
        return objs[(size_t)o] ? o : ST_SKIP;
    }
};

SmStatus Recorder::Execute(SmNotifyOperation eOp, SmObject* pObj, SmObject* p1, SmObject* p2, SmObject* p3)
{
    m_pRun->rec.push_back(m_lTag);
    m_pRun->rec.push_back((int64_t)eOp);
    m_pRun->rec.push_back(m_pRun->Index(pObj));
    m_pRun->rec.push_back(pObj ? (int64_t)pObj->GetType() : 0);
    m_pRun->rec.push_back(m_pRun->Index(p1));
    m_pRun->rec.push_back(m_pRun->Index(p2));
    m_pRun->rec.push_back(m_pRun->Index(p3));
    return SM_ERR; // ignored by Notify
}

#define REF(var, o, allow_null)                                                                    \
    int64_t var = r.obj_ref((o), (allow_null));                                                    \
    if (var == SC_ERR_ARG || var == ST_SKIP) return var;

int64_t run_op(Run& r, const int64_t* a, std::vector<int64_t>& out)
{
    switch (a[0])
    {
    case OP_SET_FLAGS:
    {
        REF(o, a[1], false);
        r.objs[(size_t)o]->SetAllFlags((ULONG)a[2]);
        return 0;
    }
    case OP_GET_FLAGS:
    {
        REF(o, a[1], false);
        out.push_back((int64_t)r.objs[(size_t)o]->GetAllFlags());
        return 0;
    }
    case OP_SET_UIDX1:
    case OP_SET_UIDX2:
    {
        REF(o, a[1], false);
        if (a[0] == OP_SET_UIDX1)
            r.objs[(size_t)o]->SetUserIndex1((ULONG)a[2]);
        else
            r.objs[(size_t)o]->SetUserIndex2((ULONG)a[2]);
        return 0;
    }
    case OP_SET_UPTR:
    {
        REF(o, a[1], false);
        if (a[2] < -1 || a[2] >= (int64_t)r.objs.size()) return SC_ERR_ARG;
        // the pointer is only stored and compared, never dereferenced
        r.objs[(size_t)o]->SetUserPtr1(a[2] < 0 ? nullptr : (void*)&r.objs[(size_t)a[2]]);
        return 0;
    }
    case OP_GET_USER:
    {
        REF(o, a[1], false);
        ShimTopology* p = r.objs[(size_t)o];
        out.push_back((int64_t)p->GetUserIndex1());
        out.push_back((int64_t)p->GetUserIndex2());
        void* ptr = p->GetUserPtr1();
        int64_t idx = -1;
        if (ptr != nullptr)
        {
            idx = -2;
            for (size_t i = 0; i < r.objs.size(); ++i)
                if (ptr == (void*)&r.objs[i]) idx = (int64_t)i;
        }
        out.push_back(idx);
        return 0;
    }
    case OP_MARK:
    case OP_GET_MARK:
    case OP_IS_MARKED:
    {
        REF(o, a[1], false);
        if (!valid_mark_type(a[2])) return SC_ERR_ARG;
        SmMarkType m = (SmMarkType)a[2];
        ShimTopology* p = r.objs[(size_t)o];
        if (a[0] == OP_MARK)
            p->Mark(m);
        else if (a[0] == OP_GET_MARK)
            out.push_back((int64_t)p->GetMark(m));
        else
            out.push_back(p->IsMarked(m) ? 1 : 0);
        return 0;
    }
    case OP_LOCK:
        if (a[1] < 0 || a[1] > 31) return SC_ERR_ARG;
        r.locks.emplace_back(new SmNewMarkAndLock(r.ctx, (SmBitArray)a[1]));
        out.push_back((int64_t)r.locks.back()->GetMarkType());
        return 0;
    case OP_LOCK_NEW_MARK:
        if (r.locks.empty()) return ST_SKIP;
        r.locks.back()->NewMark();
        return 0;
    case OP_UNLOCK:
        if (r.locks.empty()) return ST_SKIP;
        r.locks.pop_back();
        return 0;
    case OP_NOTIFY:
    {
        if (a[1] < 0 || a[1] > (int64_t)SM_NO_UNKNOWN) return SC_ERR_ARG;
        REF(t, a[2], false);
        REF(d1, a[3], true);
        REF(d2, a[4], true);
        REF(d3, a[5], true);
        SmObject* p1 = d1 < 0 ? nullptr : r.objs[(size_t)d1];
        SmObject* p2 = d2 < 0 ? nullptr : r.objs[(size_t)d2];
        SmObject* p3 = d3 < 0 ? nullptr : r.objs[(size_t)d3];
        r.rec.clear();
        r.objs[(size_t)t]->Notify((SmNotifyOperation)a[1], p1, p2, p3);
        out.push_back((int64_t)(r.rec.size() / 7));
        out.insert(out.end(), r.rec.begin(), r.rec.end());
        return 0;
    }
    case OP_TOLERANCE:
    {
        REF(o, a[1], false);
        r.dbls.push_back((double)r.objs[(size_t)o]->GetTolerance());
        return 0;
    }
    case OP_CONNECTED:
    {
        REF(o, a[1], false);
        REF(p, a[2], true);
        const SmTopology* tgt = p < 0 ? nullptr : r.objs[(size_t)p];
        out.push_back(r.objs[(size_t)o]->SmTopology::IsConnectedTo(tgt) ? 1 : 0);
        return 0;
    }
    case OP_TRACK_NEW:
    {
        if ((int64_t)r.trackers.size() >= MAX_TRACKERS) return ST_SKIP;
        r.trackers.push_back(new ShimTracker(a[2] ? r.brep : nullptr, a[1] ? TRUE : FALSE));
        out.push_back((int64_t)r.trackers.size() - 1);
        return 0;
    }
    case OP_TRACK_START:
    case OP_TRACK_STOP:
    case OP_TRACK_IS:
    case OP_TRACK_LISTS:
    case OP_TRACK_RESET:
    case OP_TRACK_DROP:
    {
        if (a[1] < 0 || a[1] >= (int64_t)r.trackers.size()) return SC_ERR_ARG;
        ShimTracker* t = r.trackers[(size_t)a[1]];
        if (t == nullptr) return ST_SKIP;
        switch (a[0])
        {
        case OP_TRACK_START: t->StartTracking(); break;
        case OP_TRACK_STOP: t->StopTracking(); break;
        case OP_TRACK_IS: out.push_back(t->IsTracking() ? 1 : 0); break;
        case OP_TRACK_LISTS:
        {
            const auto put_list = [&out, &r](SmTArray<SmTopology*>* list) {
                out.push_back((int64_t)list->GetSize());
                for (ULONG i = 0; i < list->GetSize(); ++i) out.push_back(r.Index((*list)[i]));
            };
            put_list(t->GetRmList());
            put_list(t->GetAddList());
            put_list(t->GetChgList());
            SmTArray<SmNotifyEvent>* events = t->GetNotifyEvents();
            out.push_back((int64_t)events->GetSize());
            for (ULONG i = 0; i < events->GetSize(); ++i)
            {
                const SmNotifyEvent& e = (*events)[i];
                out.push_back((int64_t)e.m_eAction);
                out.push_back(r.Index(e.m_pCaller));
                out.push_back(r.Index(e.m_pData1));
                out.push_back(r.Index(e.m_pData2));
                out.push_back(r.Index(e.m_pData3));
            }
            break;
        }
        case OP_TRACK_RESET: t->ReSetArrays(); break;
        default:
            // A tracker whose destructor would leave its callback installed (it
            // started while a callback was already set, its own included) leaves a
            // dangling SysNotifyCallback: undefined behaviour on the next Notify.
            if (t->IsTracking() && t->LastSet()) return ST_SKIP;
            delete t;
            r.trackers[(size_t)a[1]] = nullptr;
            break;
        }
        return 0;
    }
    case OP_SYS_FOREIGN:
        r.ctx->SetSysNotifyCallback(a[1] ? &r.foreign : nullptr);
        return 0;
    case OP_DESTROY:
    {
        REF(o, a[1], false);
        delete r.objs[(size_t)o];
        r.objs[(size_t)o] = nullptr;
        return 0;
    }
    case OP_SET_KIND:
    {
        REF(o, a[1], false);
        SM_TYPE t = (SM_TYPE)a[2];
        if (t != SmTopology_TYPE && t != SmFace_TYPE && t != SmEdge_TYPE && t != SmVertex_TYPE &&
            t != SmEdgeuse_TYPE)
            return SC_ERR_ARG;
        r.objs[(size_t)o]->m_kind = t;
        return 0;
    }
    default:
        return SC_ERR_ARG;
    }
}

void push_str(std::vector<int64_t>& v, const TCHAR* s)
{
    std::vector<int64_t> b;
    for (const TCHAR* c = s; *c; ++c) b.push_back((int64_t)*c);
    v.push_back((int64_t)b.size());
    v.insert(v.end(), b.begin(), b.end());
}
} // namespace

// Runs the script. Inputs: n_objs (1..=32) pool size, user_cb (record the user
// callback), kind_ts (type codes asked of IsKindOf), ops (8 int64s each).
// ints: per op [status, n, values...]; then per object (alive flag, then when alive:
// flags, marks for SM_MT_MARK, MARK2, MARK3, MARKIO, MARKASSERT, user idx1, idx2, ptr
// index, owner/next/last index); context current marks for 0,1,2,4,8,16,31 and locks
// for 1,2,4,8,16; whether the system callback is the foreign recorder; GetType, then
// IsKindOf of each kind_ts, type string, class string. dbls: tolerances in op order.
SC_EXPORT int32_t sc_topology_topology_run(int64_t n_objs, int32_t user_cb, const int64_t* kind_ts,
                                           int64_t n_kind_ts, const int64_t* ops, int64_t n_ops, int64_t* ints,
                                           int64_t ints_cap, int64_t* ints_len, double* dbls, int64_t dbls_cap,
                                           int64_t* dbls_len)
{
    if (n_objs < 1 || n_objs > MAX_OBJECTS || n_ops < 0 || n_kind_ts < 0) return SC_ERR_ARG;
    for (int64_t k = 0; k < n_ops; ++k)
        if (ops[8 * k] < 0 || ops[8 * k] > OP_LAST) return SC_ERR_ARG;
    SC_TRY
    Run r;
    r.ctx = new SmContext();
    r.brep = new (*r.ctx) SmBrep();
    for (int64_t i = 0; i < n_objs; ++i) r.objs.push_back(new (*r.ctx) ShimTopology());
    Recorder sUser;
    sUser.m_lTag = 1;
    sUser.m_pRun = &r;
    r.foreign.m_lTag = 2;
    r.foreign.m_pRun = &r;
    r.ctx->SetUserNotifyCallback(user_cb ? &sUser : nullptr);
    std::vector<int64_t> o_ints;
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
    if (status == SC_OK)
    {
        for (ShimTopology* p : r.objs)
        {
            o_ints.push_back(p ? 1 : 0);
            if (!p) continue;
            o_ints.push_back((int64_t)p->GetAllFlags());
            for (int64_t m : {1, 2, 4, 8, 16}) o_ints.push_back((int64_t)p->GetMark((SmMarkType)m));
            o_ints.push_back((int64_t)p->GetUserIndex1());
            o_ints.push_back((int64_t)p->GetUserIndex2());
            void* ptr = p->GetUserPtr1();
            int64_t idx = -1;
            if (ptr != nullptr)
            {
                idx = -2;
                for (size_t i = 0; i < r.objs.size(); ++i)
                    if (ptr == (void*)&r.objs[i]) idx = (int64_t)i;
            }
            o_ints.push_back(idx);
            o_ints.push_back(r.Index(p->GetOwner()));
            o_ints.push_back(r.Index(p->GetNext()));
            o_ints.push_back(r.Index(p->GetLast()));
        }
        for (int64_t m : {0, 1, 2, 4, 8, 16, 31}) o_ints.push_back((int64_t)r.ctx->GetCurrentMark((SmMarkType)m));
        for (int64_t m : {1, 2, 4, 8, 16}) o_ints.push_back(r.ctx->IsMarkLocked((SmMarkType)m) ? 1 : 0);
        o_ints.push_back(r.ctx->GetSysNotifyCallback() == &r.foreign ? 1 : 0);
        ShimTopology* p0 = nullptr;
        for (ShimTopology* p : r.objs)
            if (p && !p0) p0 = p;
        if (p0)
        {
            o_ints.push_back((int64_t)p0->SmTopology::GetType());
            for (int64_t i = 0; i < n_kind_ts; ++i)
                o_ints.push_back(p0->SmTopology::IsKindOf((SM_TYPE)kind_ts[i]) ? 1 : 0);
            push_str(o_ints, p0->SmTopology::GetTypeString());
            push_str(o_ints, p0->SmTopology::GetClassString());
        }
    }
    // Clean up: trackers (their destructors stop tracking), locks, callbacks. Objects,
    // the B-rep and the context are leaked on purpose (their destructors are not compared).
    for (ShimTracker* t : r.trackers) delete t;
    r.locks.clear();
    r.ctx->SetUserNotifyCallback(nullptr);
    r.ctx->SetSysNotifyCallback(nullptr);
    if (status != SC_OK) return status;
    *ints_len = (int64_t)o_ints.size();
    *dbls_len = (int64_t)r.dbls.size();
    if (*ints_len > ints_cap || *dbls_len > dbls_cap) return SC_ERR_BUFFER;
    for (size_t i = 0; i < o_ints.size(); ++i) ints[i] = o_ints[i];
    for (size_t i = 0; i < r.dbls.size(); ++i) dbls[i] = r.dbls[i];
    return SC_OK;
    SC_CATCH
}
