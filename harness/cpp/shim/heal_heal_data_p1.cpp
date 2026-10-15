// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.heal_data_p1`: SmApiHealBrep
// (source/SM_API/src/SmApiHeal.cpp). The SmHealData members of the unit (ReSet,
// MoveToNotYetProblems, HealBrep, Cache_FaceProps_Stage2/3, Fix_BadSheets,
// Make_UVTrimCurves, Fix_InfiniteRegion) run as steps 23..31 of the heal scripts of
// sc_heal_heal_data_p2_eval (harness/cpp/shim/heal_heal_data_p2.cpp), which builds the
// healer on a real B-rep.
//
// sc_heal_heal_data_p1_api(kind, dx, dy, dz, out, cap, out_len): kind 0 calls
// SmApiHealBrep(NULL); kind 1 calls it on SmApiCreateBox(origin, dx, dy, dz) (as
// SM_API_test TestSmHealBrep). Output: `create_status heal_status` (0 = SM_SUCCESS;
// heal_status is 0 when the box could not be made).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiGeneral.h>
#include <SmApiHeal.h>
#include <SmApiPrimitives.h>
#include <SmBrep.h>
#include <SmVector3d.h>

#include <vector>

SC_EXPORT int32_t sc_heal_heal_data_p1_api(int32_t kind, double dx, double dy, double dz, double* out, int64_t cap,
                                           int64_t* out_len)
{
    if (out_len == nullptr || kind < 0 || kind > 1)
        return SC_ERR_ARG;
    SC_TRY
    SmApiGetOrCreateContext();
    std::vector<double> o;
    if (kind == 0)
    {
        o.push_back(0.0);
        o.push_back((double)SmApiHealBrep(NULL));
    }
    else
    {
        SmBrep* box = NULL;
        SmApiStatus cs = SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), dx, dy, dz, box);
        o.push_back((double)cs);
        if (cs == SM_SUCCESS && box != NULL)
            o.push_back((double)SmApiHealBrep(box));
        else
            o.push_back(0.0);
        delete box;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
