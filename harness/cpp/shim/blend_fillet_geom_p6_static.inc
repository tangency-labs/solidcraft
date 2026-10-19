// SPDX-License-Identifier: Apache-2.0
// Included at the end of blend_fillet_geom_p1.cpp, the translation unit that compiles upstream
// SmFilletGeom.cpp: wrappers of its file-statics for the shim of unit `blend.fillet_geom_p6`
// (declarations and rationale in blend_fillet_geom_p6.h). Not compiled on its own.
#include "blend_fillet_geom_p6.h"

SmStatus sc_p6_join_two_fillet_verts_by_curves(SmFilletGeom* pFilletGeom, SmVertex* pStartFV, SmVertex* pEndFV,
                                               SmTArray<SmFilletEdgeuse*>& rSideEUs, double dUVTol,
                                               SmTArray<SmCurve*>& r3DCurves, SmTArray<SmBSplineCurve*>& rUVCurves,
                                               SmTArray<SmOrientType>& rCurveOrients, SmBoolean& rbMadeConnection)
{
    return JoinTwoFilletVertsByCurves(pFilletGeom, pStartFV, pEndFV, rSideEUs, dUVTol, r3DCurves, rUVCurves,
                                      rCurveOrients, rbMadeConnection, 0);
}

SmStatus sc_p6_move_tsp(SmTArray<SmTsectPnt*>& rTsectPnts, ULONG lFrom, ULONG lTo)
{
    return sm_MoveTSP(rTsectPnts, lFrom, lTo);
}
