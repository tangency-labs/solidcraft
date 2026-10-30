// SPDX-License-Identifier: Apache-2.0
// Analytic surfaces built exactly as the ssi_corpus shim's build_surface builds them (real
// SmCylinder, SmCone through CreateCanonical, SmSphere, SmTorus; angles in degrees), from a
// flat block of doubles. The Rust side builds the same surfaces with
// crates/difftest/src/ssi/kernel.rs (rust_surface). Layouts (after the kind):
//   4 cylinder origin[3] x[3] y[3] radius angle0 angle1 height0 height1
//   5 cone     apex[3] x[3] y[3] half_angle_deg height0 height1
//   6 sphere   center[3] x[3] y[3] radius lon0 lon1 lat0 lat1
//   7 torus    center[3] x[3] y[3] R r major0 major1 minor0 minor1
// The values are validated by the callers' FFI wrappers; build() only bounds checks.
#pragma once

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCylinder.h>
#include <SmExtent2d.h>
#include <SmSphere.h>
#include <SmSurface.h>
#include <SmTorus.h>
#include <SmVector3d.h>

#include <cmath>

namespace sc_analytic
{
// Values after the kind code.
inline int64_t length(int64_t kind)
{
    switch (kind)
    {
    case 4:
        return 14;
    case 5:
        return 12;
    case 6:
        return 14;
    case 7:
        return 15;
    default:
        return -1;
    }
}

// The ssi_corpus shim's unit_cross: the unit vector along a x b.
inline SmVector3d unit_cross(const SmVector3d& a, const SmVector3d& b)
{
    SmVector3d c(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
    double l = std::sqrt(c.x * c.x + c.y * c.y + c.z * c.z);
    return SmVector3d(c.x / l, c.y / l, c.z / l);
}

// The surface of kind `kind` (4..7) from `v[0 .. length(kind))`; NULL when it cannot be built.
inline SmSurface* build(const SmContext& ctx, int64_t kind, const double* v)
{
    SmVector3d c(v[0], v[1], v[2]);
    SmVector3d x(v[3], v[4], v[5]);
    SmVector3d y(v[6], v[7], v[8]);
    const double* r = v + 9;
    if (kind == 4)
    {
        SmVector3d z = unit_cross(x, y);
        SmPoint3d o = c + z * r[3];
        return new (ctx) SmCylinder(o, x, y, r[0], r[1], r[2], r[4] - r[3], FALSE, FALSE, FALSE, &ctx);
    }
    if (kind == 5)
    {
        SmVector3d z = unit_cross(x, y);
        double t = std::tan(r[0] * M_PI / 180.0);
        SmPoint3d o = c + z * r[1];
        SmAxis2Placement pl(o, x, y);
        SmCone* cone = NULL;
        if (SmCone::CreateCanonical(ctx, pl, r[1] * t, r[2] * t, r[2] - r[1], cone) != SM_SUCCESS)
            return NULL;
        return cone;
    }
    if (kind == 6)
    {
        SmExtent2d anal(r[1], r[3], r[2], r[4]);
        return new (ctx) SmSphere(c, x, y, anal, r[0], FALSE, FALSE, NULL, &ctx);
    }
    if (kind == 7)
    {
        SmExtent2d anal(r[2], r[4], r[3], r[5]);
        return new (ctx) SmTorus(c, x, y, anal, r[0], r[1], FALSE, FALSE, NULL, &ctx);
    }
    return NULL;
}
} // namespace sc_analytic
