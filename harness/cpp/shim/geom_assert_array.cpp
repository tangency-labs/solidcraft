// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.assert_array`.
// Wraps upstream SmAssertReport / SmAssertArray / SM_GetAssertLabelList
// (source/SMLib/inc/SmAssertArray.h, source/SMLib/src/SmAssertArray.cpp).
//
// Object pointers are synthesised from integer keys (0 = NULL) and reported back
// as integers; upstream only stores and compares them, never dereferences them
// (SM_DEBUG_CODE is not defined in the reference build).
#include "sc_shim.h"

#include <SmTypes.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmAssertArray.h>

#include <cstring>
#include <string>
#include <vector>

namespace
{
// Integer fields of a report spec (keep in sync with crates/solidcore-ffi/src/units/geom_assert_array.rs).
enum SpecI : int
{
    SI_REPORTER = 0,
    SI_TEST_INDEX = 1,
    SI_TEST_CLASS = 2,
    SI_OWNER = 3,
    SI_OWNER_TYPE = 4,
    SI_OTHER = 5,
    SI_OTHER_TYPE = 6,
    SI_LIST_INDEX = 7,
    SPEC_I_LEN = 8,
};
// Double fields of a report spec.
enum SpecD : int
{
    SD_TOL = 0,
    SD_VALUE = 1,
    SPEC_D_LEN = 2,
};
// Integer fields of a report (output).
const int REPORT_I_LEN = 10;
// Double fields of a report (output).
const int REPORT_D_LEN = 7;

const void* key_ptr(int64_t k)
{
    return reinterpret_cast<const void*>(static_cast<uintptr_t>(k));
}

int64_t ptr_key(const void* p)
{
    return static_cast<int64_t>(reinterpret_cast<uintptr_t>(p));
}

SmAssertReport* make_report(const int64_t* si, const double* sd, const char* owner_ts,
                            const char* other_ts, const char* msg)
{
    return new SmAssertReport(key_ptr(si[SI_REPORTER]), (ULONG)si[SI_TEST_INDEX],
                              (SM_TYPE)si[SI_TEST_CLASS], key_ptr(si[SI_OWNER]),
                              (SM_TYPE)si[SI_OWNER_TYPE], owner_ts, key_ptr(si[SI_OTHER]),
                              (SM_TYPE)si[SI_OTHER_TYPE], other_ts, SmTol3d(sd[SD_TOL]),
                              sd[SD_VALUE], (ULONG)si[SI_LIST_INDEX], msg);
}

void report_ints(const SmAssertReport& r, int64_t* o)
{
    o[0] = r.m_bOK ? 1 : 0;
    o[1] = ptr_key(r.m_pReporter);
    o[2] = (int64_t)r.m_lReportingType;
    o[3] = (int64_t)r.m_lListIndex;
    o[4] = (int64_t)r.m_lTestIndex;
    o[5] = (int64_t)r.m_eAssertType;
    o[6] = ptr_key(r.GetOwner());
    o[7] = (int64_t)r.GetOwnerType();
    o[8] = ptr_key(r.GetOther());
    o[9] = (int64_t)r.GetOtherType();
}

// Optional string argument: len < 0 = NULL.
const char* opt_str(const char* s, int64_t len, std::string& store)
{
    if (len < 0)
        return nullptr;
    store.assign(s, (size_t)len);
    return store.c_str();
}

// Appends string s (NULL -> length -1) to buf; records its length in lens[i].
void put_str(const char* s, std::string& buf, int64_t* lens, int i)
{
    if (!s)
    {
        lens[i] = -1;
        return;
    }
    lens[i] = (int64_t)std::strlen(s);
    buf.append(s);
}
} // namespace

// Constructs one SmAssertReport, optionally calls SetValue / SetOwnerParam, and
// returns its fields.
//   spec_i[8], spec_d[2]          : see SpecI / SpecD
//   owner_ts / other_ts / msg     : strings with their lengths (-1 = NULL)
//   set_value, value              : when set_value != 0, call SetValue(value)
//   set_owner_param, param[2]     : when set_owner_param != 0, call SetOwnerParam(param)
//   out_i[10]                     : ok, reporter, reporting type, list index, test index,
//                                   assert type, owner, owner type, other, other type
//   out_d[7]                      : tol3d, value, owner param (2), other param (2), dist3d
//   str_lens[5]                   : lengths of name, message, message-for-file, owner type
//                                   string, other type string (-1 = NULL), whose bytes are
//                                   concatenated into out_s (capacity cap, *out_len bytes)
// The caller guarantees spec_i[SI_TEST_INDEX] is inside the label list.
SC_EXPORT int32_t sc_geom_assert_array_report(const int64_t* spec_i, const double* spec_d,
                                              const char* owner_ts, int64_t owner_ts_len,
                                              const char* other_ts, int64_t other_ts_len,
                                              const char* msg, int64_t msg_len, int32_t set_value,
                                              double value, int32_t set_owner_param,
                                              const double* param, int64_t* out_i, double* out_d,
                                              int64_t* str_lens, char* out_s, int64_t cap,
                                              int64_t* out_len)
{
    SC_TRY
    std::string s_owner, s_other, s_msg;
    const char* p_owner = opt_str(owner_ts, owner_ts_len, s_owner);
    const char* p_other = opt_str(other_ts, other_ts_len, s_other);
    const char* p_msg = opt_str(msg, msg_len, s_msg);
    SmAssertReport* r = make_report(spec_i, spec_d, p_owner, p_other, p_msg);
    if (set_value)
        r->SetValue(value);
    if (set_owner_param)
        r->SetOwnerParam(SmPoint2d(param[0], param[1]));
    report_ints(*r, out_i);
    out_d[0] = (double)r->GetTol3d();
    out_d[1] = r->GetValue();
    out_d[2] = r->GetOwnerParam().x;
    out_d[3] = r->GetOwnerParam().y;
    out_d[4] = r->GetOtherParam().x;
    out_d[5] = r->GetOtherParam().y;
    out_d[6] = r->GetDist3d();
    std::string buf;
    put_str(r->m_pName, buf, str_lens, 0);
    put_str(r->m_pMessage, buf, str_lens, 1);
    put_str(r->m_pMessageForFile, buf, str_lens, 2);
    put_str(r->m_sOwnerTypeString, buf, str_lens, 3);
    put_str(r->m_sOtherTypeString, buf, str_lens, 4);
    SmAssertReport::Delete(r);
    return sc_copy_out(buf.data(), (int64_t)buf.size(), out_s, cap, out_len);
    SC_CATCH
}

// Runs a sequence of operations on one SmAssertArray.
//   kinds[n]          : 0 = AddUniqueDeleteDuplicate(new report), 1 = SetLastAdd(flags[k]),
//                       2 = Contains(report)
//   flags[n]          : SetLastAdd argument (kind 1)
//   spec_i[n*8], spec_d[n*2] : report specs (kinds 0 and 2; strings are NULL)
//   results[n]        : AddUniqueDeleteDuplicate / Contains return (0/1), 0 for kind 1
//   last_add[n]       : GetLastAdd() after the operation
//   out_elems         : the final array, 10 ints per report (see report_ints);
//                       capacity cap ints, *out_len ints written
SC_EXPORT int32_t sc_geom_assert_array_sequence(int64_t n, const int32_t* kinds,
                                                const int32_t* flags, const int64_t* spec_i,
                                                const double* spec_d, int64_t* results,
                                                int64_t* last_add, int64_t* out_elems,
                                                int64_t cap, int64_t* out_len)
{
    if (n < 0)
        return SC_ERR_ARG;
    SC_TRY
    SmAssertArray arr;
    for (int64_t k = 0; k < n; ++k)
    {
        const int64_t* si = spec_i + k * SPEC_I_LEN;
        const double* sd = spec_d + k * SPEC_D_LEN;
        results[k] = 0;
        switch (kinds[k])
        {
        case 0: results[k] = arr.AddUniqueDeleteDuplicate(make_report(si, sd, NULL, NULL, NULL)) ? 1 : 0; break;
        case 1: arr.SetLastAdd(flags[k] ? TRUE : FALSE); break;
        case 2:
        {
            SmAssertReport* r = make_report(si, sd, NULL, NULL, NULL);
            results[k] = arr.Contains(*r) ? 1 : 0;
            SmAssertReport::Delete(r);
            break;
        }
        default: return SC_ERR_ARG;
        }
        last_add[k] = (int64_t)arr.GetLastAdd();
    }
    std::vector<int64_t> elems;
    for (ULONG i = 0; i < arr.GetSize(); ++i)
    {
        int64_t o[REPORT_I_LEN];
        report_ints(*arr[i], o);
        elems.insert(elems.end(), o, o + REPORT_I_LEN);
    }
    return sc_copy_out(elems.data(), (int64_t)elems.size(), out_elems, cap, out_len);
    SC_CATCH
}
