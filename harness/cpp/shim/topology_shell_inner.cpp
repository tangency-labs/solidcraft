// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.shell`: SmShell::IsInnerShellOf (source/SMLib/src/
// SmShell.cpp) on every pair of shells of a B-rep with internal voids.
//
// One entry point, sc_topology_shell_inner_shell_of(in, n_in, out, cap, out_len). The input
// is `n_steps (op kind x y z a b c)*n_steps`: step 0 makes the body (`op` ignored), each
// later step builds a primitive and combines it into the body with
// SmApiBoolean(body, primitive, op) (`op` an SmBooleanOperationType). `kind` 1 is
// SmApiCreateBox(origin (x, y, z), a, b, c), 2 SmApiCreateSphere(center (x, y, z), a).
// A failing constructor or Boolean is SC_ERR_ARG (validated inputs avoid them).
// Output: `n_regions (n_shells)*n_regions` (SmBrep::GetRegions order, each region's shell
// list order), then for every shell i in that order `IsInnerShellOf(NULL)` followed by
// `IsInnerShellOf(shell j)` for every other shell j in order (the SmBoolean, 2 = UNSURE).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiPrimitives.h>
#include <SmBrep.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmTArray.h>
#include <SmVector3d.h>

#include <vector>

namespace
{
struct Bad
{
};

SmBrep* primitive(const double* s)
{
    SmBrep* r = NULL;
    SmPoint3d o(s[2], s[3], s[4]);
    SmApiStatus stat = SM_ERR;
    if ((int)s[1] == 1)
        stat = SmApiCreateBox(o, s[5], s[6], s[7], r);
    else if ((int)s[1] == 2)
        stat = SmApiCreateSphere(o, s[5], r);
    if (stat != SM_SUCCESS || r == NULL)
    {
        delete r;
        throw Bad();
    }
    return r;
}
} // namespace

SC_EXPORT int32_t sc_topology_shell_inner_shell_of(const double* in, int64_t n_in, double* out, int64_t cap,
                                                   int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    int64_t n = (int64_t)in[0];
    if (n < 1 || n_in != 1 + 8 * n)
        return SC_ERR_ARG;
    SC_TRY
    SmBrep* body = NULL;
    try
    {
        body = primitive(in + 1);
        for (int64_t i = 1; i < n; i++)
        {
            const double* s = in + 1 + 8 * i;
            SmBrep* p = primitive(s);
            SmBrep* r = NULL;
            SmApiStatus stat = SmApiBoolean(body, p, (SmBooleanOperationType)(int32_t)s[0], r);
            if (stat != SM_SUCCESS || r != body)
            {
                delete p;
                throw Bad();
            }
        }
    }
    catch (Bad&)
    {
        delete body;
        return SC_ERR_ARG;
    }
    std::vector<double> o;
    std::vector<SmShell*> shells;
    SmTArray<SmRegion*> regs;
    body->GetRegions(regs);
    o.push_back((double)regs.GetSize());
    for (ULONG r = 0; r < regs.GetSize(); r++)
    {
        SmTArray<SmShell*> sh;
        regs[r]->GetShells(sh);
        o.push_back((double)sh.GetSize());
        for (ULONG s = 0; s < sh.GetSize(); s++)
            shells.push_back(sh[s]);
    }
    for (size_t i = 0; i < shells.size(); i++)
    {
        o.push_back((double)shells[i]->IsInnerShellOf(NULL));
        for (size_t j = 0; j < shells.size(); j++)
            if (j != i)
                o.push_back((double)shells[i]->IsInnerShellOf(shells[j]));
    }
    delete body;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
