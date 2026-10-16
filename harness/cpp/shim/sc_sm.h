// SPDX-License-Identifier: Apache-2.0
// Output/input helpers over SM_API value types (SmVector2d/3d, SmExtent2d, SmStatus).
//
// Copied verbatim from the per-shim copies they replace (only `inline` and the
// `sc` namespace were added), so behaviour is unchanged. A shim that needs a
// different variant keeps its own copy and does not include this header.
// Shims pull a helper in with `using sc::<name>;` after their includes.
#ifndef SC_SM_H
#define SC_SM_H

#include <cstdint>
#include <vector>

#include <SmExtent2d.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

namespace sc
{

// An SmStatus as an output value: 0 on SM_SUCCESS, else the status.
inline double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

// A bool as an output value: 1 or 0.
inline double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

// Appends x y z.
inline void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// Appends x y.
inline void put_uv(std::vector<double>& o, const SmVector2d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
}

// Appends min.x min.y max.x max.y.
inline void put_dom(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

// Sequential reader over the input record (validated on the Rust side).
struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

} // namespace sc

#endif // SC_SM_H
