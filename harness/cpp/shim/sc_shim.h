// SPDX-License-Identifier: Apache-2.0
// Shared helpers for solidcore difftest shims (harness/cpp/shim/*.cpp).
//
// Conventions (see crates/difftest/README.md):
//  * Every exported function is `extern "C"`, named sc_<unit_id_with_underscores>_<fn>.
//  * Only plain C data crosses the boundary: double/int64_t/int32_t scalars,
//    pointers to caller-owned flat arrays plus their lengths.
//  * Return value is an int32_t status: 0 = ok, >0 = upstream error flag/code,
//    SC_ERR_EXCEPTION = a C++ exception escaped, SC_ERR_BUFFER = output buffer too small,
//    SC_ERR_ARG = argument rejected by the shim.
//  * Never let a C++ exception cross the boundary: wrap bodies in SC_TRY / SC_CATCH.
//  * SC_TRY also serializes native calls across ALL translation units. Handwritten
//    try/catch entrypoints must acquire SC_NATIVE_LOCK before touching native state
//    and retain it through every native destructor and catch-path cleanup.
#ifndef SC_SHIM_H
#define SC_SHIM_H

#include <cstdint>
#include <mutex>

#define SC_OK 0
#define SC_ERR_EXCEPTION (-1000)
#define SC_ERR_BUFFER (-1001)
#define SC_ERR_ARG (-1002)

#define SC_EXPORT extern "C"

// Upstream's API context, callbacks and system tolerances are process-global.
// In particular an allocation observer must not see another call's live B-reps.
// One out-of-line mutex serves every shim, including direct fixed-buffer calls;
// a Rust buffer-helper lock or a per-unit mutex would leave other entrypoints out.
// Recursive acquisition permits existing shim-to-shim forwarding on one thread.
std::recursive_mutex& sc_native_mutex();
#define SC_NATIVE_LOCK std::lock_guard<std::recursive_mutex> sc_native_lock(sc_native_mutex())
#define SC_TRY try { SC_NATIVE_LOCK;
#define SC_CATCH                                                                                   \
    }                                                                                              \
    catch (...)                                                                                    \
    {                                                                                              \
        return SC_ERR_EXCEPTION;                                                                   \
    }

// Copies n items to a caller buffer of capacity cap and stores n in *out_len.
// Returns SC_ERR_BUFFER (writing only *out_len) when n > cap; the Rust helper
// solidcore_ffi::call_with_buffer then retries with an exact-size buffer.
template <typename T>
inline int32_t sc_copy_out(const T* src, int64_t n, T* dst, int64_t cap, int64_t* out_len)
{
    *out_len = n;
    if (n > cap)
        return SC_ERR_BUFFER;
    for (int64_t i = 0; i < n; ++i)
        dst[i] = src[i];
    return SC_OK;
}

#endif // SC_SHIM_H
