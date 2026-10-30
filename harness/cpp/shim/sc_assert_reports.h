// SPDX-License-Identifier: Apache-2.0
// Shared difftest output of an SmAssertArray (keep in sync with
// crates/difftest/src/util/assert_reports.rs): ok, count, then per report the reporting
// type, list index, test index, assert type, ok flag, reporter / owner / other
// identities, owner and other types, tolerance, value, 3D distance and the label
// texts, then GetLastAdd. Identities are 0 for NULL, 1 for the checked root object and
// 2, 3, .. for other objects in order of first appearance (pointers are not
// comparable across implementations; their identity pattern is).
#pragma once

#include <SmAssertArray.h>

#include <vector>

namespace sc
{
inline void assert_text(std::vector<double>& out, const TCHAR* text)
{
    std::vector<double> s;
    for (const TCHAR* p = text; p && *p; p++)
        s.push_back((double)*p);
    out.push_back((double)s.size());
    out.insert(out.end(), s.begin(), s.end());
}

inline double assert_id(std::vector<const void*>& seen, const void* p, const void* root)
{
    if (p == NULL)
        return 0.0;
    if (p == root)
        return 1.0;
    for (size_t i = 0; i < seen.size(); i++)
        if (seen[i] == p)
            return (double)(i + 2);
    seen.push_back(p);
    return (double)(seen.size() + 1);
}

inline void put_assert_reports(std::vector<double>& v, SmBoolean ok, SmAssertArray& reports, const void* root)
{
    std::vector<const void*> seen;
    v.push_back(ok ? 1.0 : 0.0);
    v.push_back((double)reports.GetSize());
    for (ULONG i = 0; i < reports.GetSize(); i++)
    {
        SmAssertReport* r = reports[i];
        v.insert(v.end(), {(double)r->m_lReportingType, (double)r->m_lListIndex, (double)r->m_lTestIndex,
                           (double)r->m_eAssertType, r->m_bOK ? 1.0 : 0.0});
        v.push_back(assert_id(seen, r->m_pReporter, root));
        v.push_back(assert_id(seen, r->m_pOwner, root));
        v.push_back(assert_id(seen, r->m_pOther, root));
        v.insert(v.end(), {(double)r->m_lOwnerType, (double)r->m_lOtherType, (double)r->m_sTol3d, r->m_dValue,
                           r->m_dDist3d});
        assert_text(v, r->m_pName);
        assert_text(v, r->m_pMessage);
        assert_text(v, r->m_pMessageForFile);
        assert_text(v, r->m_sOwnerTypeString);
        assert_text(v, r->m_sOtherTypeString);
    }
    v.push_back(reports.GetLastAdd() ? 1.0 : 0.0);
}
} // namespace sc
