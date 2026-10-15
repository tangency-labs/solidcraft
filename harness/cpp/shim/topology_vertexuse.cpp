// SPDX-License-Identifier: Apache-2.0
// Differential entry point; fixture helpers are shared with topology_traverser.
#include "sc_topology_fixture.h"

SC_EXPORT int32_t sc_topology_vertexuse_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                             double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_brep_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
