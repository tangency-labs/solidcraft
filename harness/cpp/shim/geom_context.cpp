// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.context`. Wraps upstream SmContext, SmNewMarkAndLock
// (source/SMLib/inc/SmContext.h, source/SMLib/src/SmContext.cpp), the SmContext
// overloads of SmTol (source/SMLib/src/SmTol.cpp), SmMassPropertyBehavior
// (source/SMLib/inc/SmTopoTypes.h) and SmApiCreateContext / SmApiGetOrCreateContext
// (source/SM_API/src/SmApiGeneral.cpp).
#include "sc_shim.h"
#include <SmApiGeneral.h>
#include <SmContext.h>
#include <SmTol.h>
#include <SmTopoTypes.h>

#include <cstring>
#include <vector>

namespace
{

// Exposes SmContext's protected tolerance members and accessors.
struct ScCtx : public SmContext
{
    ScCtx(double dModel, double dRatio) : SmContext(dModel, dRatio) {}
    using SmContext::GetThisModelSizeEstimate;
    using SmContext::SetThisLargeSmallSizeRatio;
    using SmContext::SetThisModelSizeEstimate;
    double RawModel() const { return m_dThisModelSizeEstimate; }
    double RawRatio() const { return m_dThisLargeSmallSizeRatio; }
    double RawZone() const { return m_sThisZoneTol3d; }
};

// A concrete notify callback (only its address is observed).
struct ScNotify : public SmNotifyCallback
{
    SmStatus Execute(SmNotifyOperation, SmObject*, SmObject*, SmObject*, SmObject*) override
    {
        return 0;
    }
};

// Sets the system tolerance statics, restoring the defaults on scope exit.
struct ScSystemTol
{
    ScSystemTol(double dModelSize, double dRatio)
    {
        SmTol::SetModelSizeEstimate(dModelSize);
        SmTol::SetLargeSmallSizeRatio(dRatio);
    }
    ~ScSystemTol()
    {
        SmTol::SetModelSizeEstimate(SM_USE_DEFAULT);
        SmTol::SetLargeSmallSizeRatio(SM_USE_DEFAULT);
    }
};

enum
{
    OP_CUR_MARK = 0,
    OP_IS_LOCKED = 1,
    OP_LOCK_NEW = 2,
    OP_LOCK_PAIR = 3,
    OP_LOCK_DEFAULT = 4,
    OP_LOCK_NEWMARK = 5,
    OP_LOCK_SETCTX = 6,
    OP_LOCK_SETCTX_PAIR = 7,
    OP_LOCK_DROP = 8,
    OP_LOCK_INFO = 9,
    OP_DOING_BOOL = 10,
    OP_GET_MODEL = 11,
    OP_SET_MODEL = 12,
    OP_SET_RATIO = 13,
    OP_TOL_CTX = 14,
    OP_CALC_EDGE = 15,
    OP_CALC_VERTEX = 16,
    OP_CALLBACK = 17,
    OP_MASS = 18,
    OP_API = 19,
};

const int64_t kMarkTypes[7] = {SM_MT_NOMARK, SM_MT_MARK,       SM_MT_MARK2,   SM_MT_MARK3,
                               SM_MT_MARKIO, SM_MT_MARKASSERT, SM_MT_ALLMARKS};

bool valid_mark(int64_t m)
{
    for (int64_t v : kMarkTypes)
        if (v == m)
            return true;
    return false;
}

int64_t f_bits(double d)
{
    int64_t i;
    static_assert(sizeof(i) == sizeof(d), "f64 size");
    memcpy(&i, &d, sizeof(d));
    return i;
}

} // namespace

// Runs a script of SmContext / SmNewMarkAndLock operations.
//   sys_model, sys_ratio : system SmTol::SetModelSizeEstimate / SetLargeSmallSizeRatio
//   ctx_params[2*n_ctx]  : (model size, ratio) of each SmContext constructed
//   ops[6*n_ops]         : per op (op, slot, ctx, ctx2, m, flag); ctx -1 = NULL where allowed
//   xs[4*n_ops]          : per op float operands
//   n_slots              : number of SmNewMarkAndLock slots
// Output: tagged (tag, payload) int64 pairs; tag 0 = integer, tag 1 = double bits.
// Per op: its results, then a snapshot of every context. At the end the live locks
// are destroyed in slot order and a final snapshot is written.
SC_EXPORT int32_t sc_geom_context_script(double sys_model, double sys_ratio, const double* ctx_params,
                                         int64_t n_ctx, const int64_t* ops, const double* xs,
                                         int64_t n_ops, int64_t n_slots, int64_t* out, int64_t cap,
                                         int64_t* out_len)
{
    if (n_ctx < 1 || n_ctx > 8 || n_ops < 0 || n_slots < 0 || n_slots > 16)
        return SC_ERR_ARG;
    // validate everything before touching upstream
    for (int64_t i = 0; i < n_ops; ++i)
    {
        const int64_t* o = ops + 6 * i;
        int64_t op = o[0], s = o[1], c = o[2], c2 = o[3], m = o[4], fl = o[5];
        bool need_slot = op >= OP_LOCK_NEW && op <= OP_LOCK_INFO;
        bool null_ok = op == OP_TOL_CTX || op == OP_CALC_EDGE || op == OP_CALC_VERTEX;
        bool no_ctx = op == OP_LOCK_DEFAULT || op == OP_LOCK_NEWMARK || op == OP_LOCK_DROP ||
                      op == OP_LOCK_INFO || op == OP_MASS || op == OP_API;
        if (op < 0 || op > OP_API)
            return SC_ERR_ARG;
        if (need_slot && (s < 0 || s >= n_slots))
            return SC_ERR_ARG;
        if (!no_ctx && !(c >= 0 && c < n_ctx) && !(null_ok && c == -1))
            return SC_ERR_ARG;
        if ((op == OP_LOCK_PAIR || op == OP_LOCK_SETCTX_PAIR) && (c2 < 0 || c2 >= n_ctx))
            return SC_ERR_ARG;
        if ((op == OP_CUR_MARK || op == OP_IS_LOCKED || op == OP_LOCK_SETCTX) && !valid_mark(m))
            return SC_ERR_ARG;
        if (op == OP_DOING_BOOL && (m < 0 || m > 1))
            return SC_ERR_ARG;
        if (op == OP_TOL_CTX && (m < 0 || m > 4))
            return SC_ERR_ARG;
        if (op == OP_MASS && (m < 0 || fl < 0))
            return SC_ERR_ARG;
    }
    SC_TRY
    ScSystemTol sSys(sys_model, sys_ratio);
    static ScNotify s_Notify;
    std::vector<ScCtx*> vCtx;
    for (int64_t i = 0; i < n_ctx; ++i)
        vCtx.push_back(new ScCtx(ctx_params[2 * i], ctx_params[2 * i + 1]));
    std::vector<SmNewMarkAndLock*> vSlots((size_t)n_slots, nullptr);
    std::vector<int64_t> res;
    auto put_i = [&](int64_t v) {
        res.push_back(0);
        res.push_back(v);
    };
    auto put_f = [&](double v) {
        res.push_back(1);
        res.push_back(f_bits(v));
    };
    auto ctx_index = [&](SmContext* p) -> int64_t {
        for (int64_t i = 0; i < n_ctx; ++i)
            if ((SmContext*)vCtx[(size_t)i] == p)
                return i;
        return -1;
    };
    auto snapshot = [&]() {
        for (ScCtx* p : vCtx)
        {
            for (int64_t m : kMarkTypes)
            {
                put_i((int64_t)p->GetCurrentMark((SmMarkType)m));
                put_i(p->IsMarkLocked((SmMarkType)m) ? 1 : 0);
            }
            put_i(p->GetDoingBoolean() ? 1 : 0);
            put_i(p->GetSysNotifyCallback() ? 1 : 0);
            put_i(p->GetUserNotifyCallback() ? 1 : 0);
            put_f(p->RawModel());
            put_f(p->RawRatio());
            put_f(p->RawZone());
        }
    };
    auto drop_slot = [&](int64_t s) {
        delete vSlots[(size_t)s];
        vSlots[(size_t)s] = nullptr;
    };
    for (int64_t i = 0; i < n_ops; ++i)
    {
        const int64_t* o = ops + 6 * i;
        const double* x = xs + 4 * i;
        int64_t op = o[0], s = o[1], fl = o[5];
        SmMarkType eM = (SmMarkType)o[4];
        ScCtx* pC = o[2] >= 0 && o[2] < n_ctx ? vCtx[(size_t)o[2]] : nullptr;
        ScCtx* pC2 = o[3] >= 0 && o[3] < n_ctx ? vCtx[(size_t)o[3]] : nullptr;
        switch (op)
        {
        case OP_CUR_MARK: put_i((int64_t)pC->GetCurrentMark(eM)); break;
        case OP_IS_LOCKED: put_i(pC->IsMarkLocked(eM) ? 1 : 0); break;
        case OP_LOCK_NEW:
            drop_slot(s);
            vSlots[(size_t)s] = new SmNewMarkAndLock(pC, (SmBitArray)o[4]);
            put_i(vSlots[(size_t)s]->GetMarkType());
            break;
        case OP_LOCK_PAIR:
            drop_slot(s);
            vSlots[(size_t)s] = new SmNewMarkAndLock((SmContext*)pC, (SmContext*)pC2);
            put_i(vSlots[(size_t)s]->GetMarkType1());
            put_i(vSlots[(size_t)s]->GetMarkType2());
            break;
        case OP_LOCK_DEFAULT:
            drop_slot(s);
            vSlots[(size_t)s] = new SmNewMarkAndLock();
            break;
        case OP_LOCK_NEWMARK:
            if (vSlots[(size_t)s] && vSlots[(size_t)s]->GetContext())
            {
                vSlots[(size_t)s]->NewMark();
                put_i(1);
            }
            else
                put_i(-1);
            break;
        case OP_LOCK_SETCTX:
            if (!vSlots[(size_t)s])
                vSlots[(size_t)s] = new SmNewMarkAndLock();
            put_i(vSlots[(size_t)s]->SetContext(pC, eM));
            break;
        case OP_LOCK_SETCTX_PAIR:
            if (!vSlots[(size_t)s])
                vSlots[(size_t)s] = new SmNewMarkAndLock();
            put_i(vSlots[(size_t)s]->SetContext((SmContext*)pC, (SmContext*)pC2));
            break;
        case OP_LOCK_DROP: drop_slot(s); break;
        case OP_LOCK_INFO:
            if (!vSlots[(size_t)s])
            {
                put_i(-2);
                break;
            }
            put_i(vSlots[(size_t)s]->GetMarkType());
            put_i(vSlots[(size_t)s]->GetMarkType1());
            put_i(vSlots[(size_t)s]->GetMarkType2());
            put_i(ctx_index(vSlots[(size_t)s]->GetContext()));
            put_i(ctx_index(vSlots[(size_t)s]->GetContext2()));
            break;
        case OP_DOING_BOOL:
            if (fl)
                pC->GetDoingBooleanRef() = (SmBoolean)o[4];
            else
                pC->SetDoingBoolean((SmBoolean)o[4]);
            put_i(pC->GetDoingBoolean() ? 1 : 0);
            break;
        case OP_GET_MODEL:
            put_f(pC->GetThisModelSizeEstimate());
            put_f(SmTol::GetModelSizeEstimate(*(const SmContext*)pC));
            break;
        case OP_SET_MODEL:
            if (fl)
                put_f(SmTol::SetModelSizeEstimate(*(SmContext*)pC, x[0]));
            else
                put_f(pC->SetThisModelSizeEstimate(x[0]));
            break;
        case OP_SET_RATIO: pC->SetThisLargeSmallSizeRatio(x[0]); break;
        case OP_TOL_CTX:
        {
            const SmContext* cp = pC;
            SmSmallTopoType eS = (SmSmallTopoType)o[4];
            put_f(SmTol::GetZoneTol3d(cp, eS));
            put_f(SmTol::GetApproxTol3d(cp, eS));
            put_f(SmTol::GetXSectTol3d(cp, eS));
            put_f(SmTol::CalcFaceZoneTol3d(cp));
            break;
        }
        case OP_CALC_EDGE:
        {
            SmZoneTol3d sMax(x[2]);
            put_f(SmTol::CalcEdgeZoneTol3d(x[0], x[1], fl ? &sMax : (SmZoneTol3d*)NULL,
                                           (const SmContext*)pC));
            break;
        }
        case OP_CALC_VERTEX:
        {
            SmZoneTol3d sMax(x[2]);
            put_f(SmTol::CalcVertexZoneTol3d(x[0], x[1], fl ? &sMax : (SmZoneTol3d*)NULL,
                                             (const SmContext*)pC));
            break;
        }
        case OP_CALLBACK:
            if (fl)
                pC->SetUserNotifyCallback(o[4] ? &s_Notify : NULL);
            else
                pC->SetSysNotifyCallback(o[4] ? &s_Notify : NULL);
            put_i(pC->GetSysNotifyCallback() ? 1 : 0);
            put_i(pC->GetUserNotifyCallback() ? 1 : 0);
            break;
        case OP_MASS:
        {
            SmMassPropertyBehavior sDef;
            SmMassPropertyBehavior sArg(x[0], x[1], x[2], x[3], (ULONG)o[4], (ULONG)fl);
            const SmMassPropertyBehavior& r = s ? sDef : sArg;
            put_f(r.m_dDensity);
            put_f(r.m_dFaceThickness);
            put_f(r.m_dEdgeCrossSectionRadius);
            put_f(r.m_dVertexSphereRadius);
            put_i((int64_t)r.m_lFaceAreaBehavior);
            put_i((int64_t)r.m_lFaceVolumeBehavior);
            break;
        }
        case OP_API:
        {
            SmApiCreateContext();
            SmContext* p = SmApiGetOrCreateContext();
            put_i(p != NULL ? 1 : 0);
            put_i(p == SmApiGetOrCreateContext() ? 1 : 0);
            break;
        }
        default: break;
        }
        snapshot();
    }
    for (int64_t s = 0; s < n_slots; ++s)
        drop_slot(s);
    snapshot();
    for (ScCtx* p : vCtx)
    {
        p->SetSysNotifyCallback(NULL);
        delete p;
    }
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}
