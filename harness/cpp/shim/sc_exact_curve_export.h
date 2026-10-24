// SPDX-License-Identifier: Apache-2.0
// Lossless native ellipse/circle transport shared by the B-rep graph exporters.
// Keeps the stored NURB, STEP/NURB mapping, flags and concrete kind. The old
// B-spline block follows the binary record as an independent integrity check.
#pragma once

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmConfig.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cstring>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace sc_exact_curve_export
{
// Operation results retain the old line/B-spline layout. Ellipse results also
// compare their analytic data and flags, without requiring a Rust database writer.
struct CurvePeek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*knots() { return &CurvePeek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &CurvePeek::m_eBSplineCurveForm; }
};
struct EllipsePeek : SmEllipse
{
    static SmPolarConversion SmEllipse::*polar() { return &EllipsePeek::m_vPolarConverter; }
};

inline std::string polar_record(const SmEllipse* e)
{
    std::ostringstream stream;
    SmDatabaseIOStream db(SM_BINARY, &stream);
    if ((e->*EllipsePeek::polar()).WriteToDB(db, SM_CURRENT_DATABASE_VERSION) != SM_SUCCESS)
        throw std::runtime_error("ellipse polar table write failed");
    return stream.str();
}

template <class PutBase>
void put_curve(std::vector<double>& o, const SmCurve* c, PutBase put_curve_base)
{
    put_curve_base(o, c);
    if (c == nullptr || (c->GetType() != SmEllipse_TYPE && c->GetType() != SmCircle_TYPE))
        return;
    const SmEllipse* e = (const SmEllipse*)c;
    const SmAxis2Placement& pos = e->GetPosition();
    for (const SmVector3d& v : {pos.GetOrigin(), pos.GetXAxis(), pos.GetYAxis()})
        o.insert(o.end(), {v.x, v.y, v.z});
    SmExtent1d domain = e->GetSTEPInterval();
    o.push_back(domain.GetMin());
    o.push_back(domain.GetMax());
    o.push_back(e->GetXRadius());
    o.push_back(e->GetYRadius());
    o.push_back(e->GetInsideOut() ? 1.0 : 0.0);
    o.push_back(e->IsOutOfBoundsEnabled() ? 1.0 : 0.0);
    o.push_back((double)(e->*CurvePeek::knots()));
    o.push_back((double)(e->*CurvePeek::form()));
}

inline bool same_curve_bits(const std::vector<double>& a, const std::vector<double>& b)
{
    return a.size() == b.size() &&
           std::memcmp(a.data(), b.data(), a.size() * sizeof(double)) == 0;
}

inline bool curve_record(const SmCurve* curve, std::string& bytes)
{
    std::ostringstream stream;
    SmDatabaseIOStream db(SM_BINARY, &stream);
    if (curve->WriteToDB(db, SM_CURRENT_DATABASE_VERSION) != SM_SUCCESS)
        return false;
    bytes = stream.str();
    return true;
}

// Private graph transport: -3 type version dim oob nbytes bytes, then the
// independent old B-spline block. No lazy NURB construction is allowed here.
template <class PutBase>
void put_export_curve(std::vector<double>& out, const SmCurve* c, const SmContext& ctx, PutBase put_curve_base)
{
    if (c == nullptr || (c->GetType() != SmEllipse_TYPE && c->GetType() != SmCircle_TYPE))
    {
        put_curve_base(out, c);
        return;
    }
    const SmEllipse* e = (const SmEllipse*)c;
    if (e->GetDim() != 3 || e->GetGwNurbPointer() == nullptr)
    {
        put_curve_base(out, c);
        return;
    }
    std::vector<double> before, after;
    put_curve(before, c, put_curve_base);
    const SmObject* owner = c->GetOwner();
    const gw_CURVE* nurb = e->GetGwNurbPointer();
    const std::string polar = polar_record(e);
    std::string bytes;
    if (!curve_record(c, bytes))
        throw std::runtime_error("ellipse database write failed");
    put_curve(after, c, put_curve_base);
    if (!same_curve_bits(before, after) || c->GetOwner() != owner ||
        e->GetGwNurbPointer() != nurb || polar_record(e) != polar)
        throw std::runtime_error("ellipse database write mutated its source");
    SmCurve* back = nullptr;
    std::istringstream stream(bytes);
    SmDatabaseIOStream db(SM_BINARY, &stream);
    bool exact = false;
    if (SmCurve::ReadFromDB(c->GetType(), db, e->GetDim(), ctx, back,
                          SM_CURRENT_DATABASE_VERSION) == SM_SUCCESS &&
        back != nullptr && back->GetType() == c->GetType() &&
        stream.peek() == std::char_traits<char>::eof())
    {
        std::unique_ptr<SmCurve> held(back);
        ((SmBSplineCurve*)back)->SetOutOfBoundsEnabled(e->IsOutOfBoundsEnabled());
        std::vector<double> restored;
        put_curve(restored, back, put_curve_base);
        std::string again;
        exact = same_curve_bits(before, restored) && curve_record(back, again) && again == bytes;
        back = nullptr;
    }
    delete back;
    if (exact)
    {
        out.push_back(-3.0);
        out.push_back((double)c->GetType());
        out.push_back((double)SM_CURRENT_DATABASE_VERSION);
        out.push_back((double)e->GetDim());
        out.push_back(e->IsOutOfBoundsEnabled() ? 1.0 : 0.0);
        out.push_back((double)bytes.size());
        for (unsigned char byte : bytes)
            out.push_back((double)byte);
    }
    put_curve_base(out, c);
}

} // namespace sc_exact_curve_export
