// SPDX-License-Identifier: Apache-2.0
// Wrappers of two file-statics of upstream source/SMLib/src/SmFilletGeom.cpp for the difftest
// shim of unit `blend.fillet_geom_p6` (blend_fillet_geom_p6.cpp). Static functions cannot be
// linked against and SmFilletGeom.cpp can only be compiled into one translation unit (that of
// blend_fillet_geom_p1.cpp), so the wrappers are defined there, in
// blend_fillet_geom_p6_static.inc.
#pragma once

#include "StdAfx.h"
#include <SmFilletGeom.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>

// JoinTwoFilletVertsByCurves(pFilletGeom, pStartFV, pEndFV, rSideEUs, dUVTol, r3DCurves,
// rUVCurves, rCurveOrients, rbMadeConnection, 0).
SmStatus sc_p6_join_two_fillet_verts_by_curves(SmFilletGeom* pFilletGeom, SmVertex* pStartFV, SmVertex* pEndFV,
                                               SmTArray<SmFilletEdgeuse*>& rSideEUs, double dUVTol,
                                               SmTArray<SmCurve*>& r3DCurves, SmTArray<SmBSplineCurve*>& rUVCurves,
                                               SmTArray<SmOrientType>& rCurveOrients, SmBoolean& rbMadeConnection);

// sm_MoveTSP(rTsectPnts, lFrom, lTo).
SmStatus sc_p6_move_tsp(SmTArray<SmTsectPnt*>& rTsectPnts, ULONG lFrom, ULONG lTo);
