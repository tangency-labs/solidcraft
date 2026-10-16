// SPDX-License-Identifier: Apache-2.0
// Unit heal.heal_data_p3. The shared script driver constructs real topology,
// calls SmHealData's native methods (steps 32..40), and serializes the complete
// props/problem-list state in traversal order. See heal_heal_data_p2.cpp.
#include "sc_shim.h"
extern "C" int32_t sc_heal_heal_data_p2_eval(int32_t, const char*, const double*, int64_t,
                                            double*, int64_t, int64_t*);
SC_EXPORT int32_t sc_heal_heal_data_p3_eval(const char* path, const double* input, int64_t count,
                                          double* out, int64_t cap, int64_t* len)
{
    return sc_heal_heal_data_p2_eval(2, path, input, count, out, cap, len);
}
