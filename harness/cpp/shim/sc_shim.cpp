// SPDX-License-Identifier: Apache-2.0
// Shared native-call serialization; never define a per-translation-unit mutex.
#include "sc_shim.h"

std::recursive_mutex& sc_native_mutex()
{
    static std::recursive_mutex mutex;
    return mutex;
}
