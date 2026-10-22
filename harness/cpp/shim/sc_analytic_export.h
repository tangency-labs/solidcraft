// SPDX-License-Identifier: Apache-2.0
// Exact export of analytic face surfaces for the shared B-rep graph exports
// (sc_edgeuse_graph.h, sc_topology_fixture.h): the face surface kinds the Rust import can
// rebuild as their own `SurfaceKind` travel as their upstream database record (binary,
// SM_CURRENT_DATABASE_VERSION), which the importer reads with the ported `ReadFromDB`.
// `SmSurfOfRevolution`, `SmCone`, `SmCylinder`, `SmSphere`, `SmTorus` and
// `SmSurfOfExtrusion` qualify (the kinds with a `SurfaceKind` the port's
// `Surface::read_from_db` returns).
//
// Face surface block (layout in crates/solidcore-ffi/src/units/topology_edgeuse_p1.rs):
//   `2 type version surf_oob gen_oob nbytes byte[nbytes]` then the header's regular
//   surface block (the B-spline part), which the importer reads as the fallback.
// The record is only exported when it is lossless for the C++ object: the surface, its
// generator and (of a revolution) its polar curve hold their NURBs (WriteToDB would otherwise create them,
// changing the object the C++ side runs), a C++ ReadFromDB of the bytes succeeds and
// writes back the same bytes, the polar curve survives the round trip, and the polar
// curve's out-of-bounds flag is the reader's default. `m_bOutOfBoundsEnabled` of the
// surface and the generator is not in the record; it is exported beside it.
#ifndef SC_ANALYTIC_EXPORT_H
#define SC_ANALYTIC_EXPORT_H

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmConfig.h>
#include <SmContext.h>
#include <SmCone.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmSphere.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurfOfRevolution.h>
#include <SmTorus.h>
#include <SmSurface.h>

#include <sstream>
#include <string>
#include <vector>

namespace sc_analytic_export
{
struct SorPeek : SmSurfOfRevolution
{
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &SorPeek::m_vPolarConverter; }
};

// The binary record of `s` (WriteToDB), or false.
inline bool record_of(const SmSurface* s, std::string& bytes)
{
    std::ostringstream oss;
    SmDatabaseIOStream db(SM_BINARY, &oss);
    if (s->WriteToDB(db, SM_CURRENT_DATABASE_VERSION) != SM_SUCCESS)
        return false;
    bytes = oss.str();
    return true;
}

inline bool oob_of(const SmBSplineCurve* c) { return c != nullptr && c->IsOutOfBoundsEnabled() != FALSE; }

// The surface types exported exactly (`SmSurfOfRevolution` or a subclass of it:
// `SmCone`/`SmCylinder`, `SmSphere`, `SmTorus`; and
// `SmSurfOfExtrusion`).
inline bool exact_type(SM_TYPE t)
{
    return t == SmSurfOfRevolution_TYPE || t == SmCone_TYPE || t == SmCylinder_TYPE ||
           t == SmSphere_TYPE || t == SmTorus_TYPE || t == SmSurfOfExtrusion_TYPE;
}

// `put_exact_head` of a surface of extrusion: no polar curve; the surface and its
// generator must hold their NURBs (WriteToDB would otherwise create them).
inline bool put_extrusion_head(std::vector<double>& o, const SmSurfOfExtrusion* e, const SmContext& ctx)
{
    const SmBSplineCurve* gen = e->GetGenCurve();
    if (e->GetGwNurbPointer() == nullptr || gen == nullptr || gen->GetGwNurbPointer() == nullptr)
        return false;
    std::string bytes;
    if (!record_of(e, bytes))
        return false;
    // the C++ reader must restore the same object: same record written back
    bool ok = false;
    {
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(SM_BINARY, &iss);
        SmSurface* back = nullptr;
        if (SmSurface::ReadFromDB(SmSurfOfExtrusion_TYPE, db, ctx, back, SM_CURRENT_DATABASE_VERSION) ==
                SM_SUCCESS &&
            back != nullptr && back->GetType() == SmSurfOfExtrusion_TYPE)
        {
            std::string again;
            ok = record_of(back, again) && again == bytes;
        }
        delete back;
    }
    if (!ok)
        return false;
    o.push_back(2.0);
    o.push_back((double)SmSurfOfExtrusion_TYPE);
    o.push_back((double)SM_CURRENT_DATABASE_VERSION);
    o.push_back(e->IsOutOfBoundsEnabled() != FALSE ? 1.0 : 0.0);
    o.push_back(oob_of(gen) ? 1.0 : 0.0);
    o.push_back((double)bytes.size());
    for (char ch : bytes)
        o.push_back((double)(unsigned char)ch);
    return true;
}

// Appends the exact block head (`2 type version surf_oob gen_oob nbytes bytes`) of `s`
// when it can be exported exactly; returns whether it did (the caller then appends the
// regular block either way).
inline bool put_exact_head(std::vector<double>& o, const SmSurface* s, const SmContext& ctx)
{
    if (s == nullptr || !exact_type(s->GetType()))
        return false;
    const SM_TYPE type = s->GetType();
    if (type == SmSurfOfExtrusion_TYPE)
        return put_extrusion_head(o, (const SmSurfOfExtrusion*)s, ctx);
    const SmSurfOfRevolution* r = (const SmSurfOfRevolution*)s;
    const SmBSplineCurve* gen = r->GetGenCurve();
    const SmPolarConversion& polar = r->*SorPeek::polar();
    const SmEllipse* pc = polar.GetCurve();
    // WriteToDB creates a missing NURB (GetOrCreateGwNurbPointer): never mutate the object
    if (r->GetGwNurbPointer() == nullptr || gen == nullptr || gen->GetGwNurbPointer() == nullptr ||
        (pc != nullptr && pc->GetGwNurbPointer() == nullptr))
        return false;
    std::string bytes;
    if (!record_of(r, bytes))
        return false;
    // the C++ reader must restore the same object: same record written back, the polar
    // curve kept (a borrowed one is not written) with the reader's out-of-bounds default
    bool ok = false;
    {
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(SM_BINARY, &iss);
        SmSurface* back = nullptr;
        if (SmSurface::ReadFromDB(type, db, ctx, back, SM_CURRENT_DATABASE_VERSION) == SM_SUCCESS &&
            back != nullptr && back->GetType() == type)
        {
            const SmSurfOfRevolution* rb = (const SmSurfOfRevolution*)back;
            std::string again;
            const SmEllipse* pcb = (rb->*SorPeek::polar()).GetCurve();
            ok = record_of(rb, again) && again == bytes && (pc == nullptr) == (pcb == nullptr) &&
                 (pc == nullptr || oob_of(pc) == oob_of(pcb));
        }
        delete back;
    }
    if (!ok)
        return false;
    o.push_back(2.0);
    o.push_back((double)type);
    o.push_back((double)SM_CURRENT_DATABASE_VERSION);
    o.push_back(r->IsOutOfBoundsEnabled() != FALSE ? 1.0 : 0.0);
    o.push_back(oob_of(gen) ? 1.0 : 0.0);
    o.push_back((double)bytes.size());
    for (char ch : bytes)
        o.push_back((double)(unsigned char)ch);
    return true;
}
} // namespace sc_analytic_export

#endif // SC_ANALYTIC_EXPORT_H
