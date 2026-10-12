// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.periodic_extent1d`.
// Wraps upstream SmPeriodicExtent1d (source/SMLib/inc/SmPeriodicExtent1d.h,
// source/SMLib/src/SmPeriodicExtent1d.cpp).
#include "sc_shim.h"

#include <SmPeriodicExtent1d.h>

#include <cmath>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_periodic_extent1d.rs).
enum ScPe1dOp : int32_t
{
    OP_NEW_FULL = 0,        // SmPeriodicExtent1d(s[0])
    OP_COPY = 1,            // SmPeriodicExtent1d(a) (copy constructor, destructor)
    OP_GET_PERIOD = 2,      // a.GetPeriod()
    OP_SET_FULL_PERIOD = 3, // a.SetFullPeriod(flag)
    OP_IS_EMPTY = 4,        // a.IsEmpty()
    OP_IS_FULL_PERIOD = 5,  // a.IsFullPeriod()
    OP_CROSSES_SEAM = 6,    // a.CrossesSeam()
    OP_MAP_VALUE = 7,       // a.MapValue(s[0])
    OP_NORMALIZE_VALUE = 8, // a.NormalizeValue(s[0])
    OP_COMPLIMENT = 9,      // a.Compliment()
    OP_ADD_INTERVAL = 10,   // a.AddInterval(s[0], s[1], flag)
    OP_CONTAINS_VALUE = 11, // a.ContainsValue(s[0])
    OP_SET_MIN_MAX = 12,    // a.SetMinMax(s[0], s[1], has_period ? &s[2] : NULL, flag)
    OP_IS_CONTAINED_BY = 13, // a.IsContainedBy(b)
    OP_ARE_DISJOINT = 14,   // a.AreDisjoint(b)
    OP_GET_MID = 15,        // a.GetMid()
};

namespace
{
// Gives the shim access to the protected members (upstream tests do the same as friends).
struct Probe : public SmPeriodicExtent1d
{
    // raw state: [min, max, period, full]
    explicit Probe(const double* v) : SmPeriodicExtent1d(1.0)
    {
        m_dMin = v[0];
        m_dMax = v[1];
        m_dPeriod = v[2];
        m_bFullPeriod = (v[3] != 0.0) ? TRUE : FALSE;
    }
    struct Full
    {
    };
    Probe(Full, double dPeriod) : SmPeriodicExtent1d(dPeriod) {}
    explicit Probe(const SmPeriodicExtent1d& o) : SmPeriodicExtent1d(o) {}
    void Get(double* o) const
    {
        o[0] = m_dMin;
        o[1] = m_dMax;
        o[2] = m_dPeriod;
        o[3] = (double)m_bFullPeriod;
    }
};
} // namespace

// Runs one SmPeriodicExtent1d operation.
//   a[4], b[4] : extents as raw members [min, max, period, full (0/1)]
//   s[3]       : scalar operands
//   flag       : SmBoolean operand (bForward / bSetSinglePoint / SetFullPeriod argument)
//   has_period : OP_SET_MIN_MAX passes &s[2] as pOptPeriod when nonzero, else NULL
//   out[5]     : state of a after the operation (4) and the scalar result (booleans 0/1)
//   *op_status : the SmStatus returned by the operation (0 when it returns none)
// Returns SC_OK, SC_ERR_ARG or SC_ERR_EXCEPTION.
SC_EXPORT int32_t sc_geom_periodic_extent1d_op(int32_t op, const double* a, const double* b,
                                               const double* s, int32_t flag, int32_t has_period,
                                               double* out, int32_t* op_status)
{
    SC_TRY
    Probe pa(a);
    Probe pb(b);
    double r = 0.0;
    *op_status = 0;
    switch (op)
    {
    case OP_NEW_FULL:
    {
        Probe c(Probe::Full{}, s[0]);
        c.Get(out);
        out[4] = 0.0;
        return SC_OK;
    }
    case OP_COPY:
    {
        Probe c(static_cast<const SmPeriodicExtent1d&>(pa));
        c.Get(out);
        out[4] = 0.0;
        return SC_OK;
    }
    case OP_GET_PERIOD: r = pa.GetPeriod(); break;
    case OP_SET_FULL_PERIOD: pa.SetFullPeriod(flag ? TRUE : FALSE); break;
    case OP_IS_EMPTY: r = pa.IsEmpty() ? 1.0 : 0.0; break;
    case OP_IS_FULL_PERIOD: r = pa.IsFullPeriod() ? 1.0 : 0.0; break;
    case OP_CROSSES_SEAM: r = pa.CrossesSeam() ? 1.0 : 0.0; break;
    case OP_MAP_VALUE: r = pa.MapValue(s[0]); break;
    case OP_NORMALIZE_VALUE: r = pa.NormalizeValue(s[0]); break;
    case OP_COMPLIMENT: pa.Compliment(); break;
    case OP_ADD_INTERVAL: *op_status = (int32_t)pa.AddInterval(s[0], s[1], flag ? TRUE : FALSE); break;
    case OP_CONTAINS_VALUE: r = pa.ContainsValue(s[0]) ? 1.0 : 0.0; break;
    case OP_SET_MIN_MAX:
    {
        double dPeriod = s[2];
        *op_status = (int32_t)pa.SetMinMax(s[0], s[1], has_period ? &dPeriod : NULL,
                                           flag ? TRUE : FALSE);
        break;
    }
    case OP_IS_CONTAINED_BY: r = pa.IsContainedBy(pb) ? 1.0 : 0.0; break;
    case OP_ARE_DISJOINT: r = pa.AreDisjoint(pb) ? 1.0 : 0.0; break;
    case OP_GET_MID: r = pa.GetMid(); break;
    default: return SC_ERR_ARG;
    }
    pa.Get(out);
    out[4] = r;
    return SC_OK;
    SC_CATCH
}
