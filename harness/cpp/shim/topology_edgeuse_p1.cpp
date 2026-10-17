// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.edgeuse_p1`: the SmEdgeuse and SmFaceuse members owned
// by the unit (source/SMLib/src/SmEdgeuse.cpp, source/SMLib/inc/SmEdgeuse.h,
// source/SMLib/src/SmFaceuse.cpp, source/SMLib/inc/SmFaceuse.h) on real SmBrep objects,
// plus the file-static helpers of SmEdgeuse.cpp.
//
// Static functions and file-local classes of SmEdgeuse.cpp cannot be linked against, so
// sm_CalcDistAndDev, sm_AreSeamCurves and SmFindStepOffENFO are VERBATIM copies of
// upstream SmEdgeuse.cpp (commit c9c5979) inside namespace sc_eu_p1 (line ranges noted at
// each block; the SM_DEBUG_CODE blocks are dropped, as in the reference build).
//
// One entry point, sc_topology_edgeuse_p1_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_edgeuse_p1.rs, which
// validates every input. Every B-rep op starts with a B-rep block:
//   `0 rebuild validate_only init_gaps`         SmBrep::ReadFromFile(path) (ASCII .smb)
//   `1 curve t0 t1 validate_only init_gaps`     CreateWireEdgeFromCurve(curve, [t0, t1])
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `3 validate_only init_gaps`                 the wire network of prog_test my_test_topology
// `init_gaps` computes the cached vertex/face, edge/face and edge/CCW-edge gaps of every
// loop vertexuse/edgeuse (C++ only) before the op, so their reset is observable.
// A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (SmBSplineSurface) or
// `1 O[3] N[3]` (SmPlane(O, N)); a curve block is `dim n p m cpts(x y z w) knots`
// (SmBSplineCurve).
//
// The B-rep graph is exported in traversal order (each pointer mapped to its first-seen
// index per kind; -1 is null), reading raw members (protected ones through
// pointer-to-member access), never the functions under test. The B-rep construction and
// export helpers are shared with unit `topology.edgeuse_p2` (sc_edgeuse_graph.h).
#include "sc_shim.h"
#include <algorithm>
#include "support/topology_fixture.h"
#include <memory>

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmLocalSolveNd.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>

#include <map>
#include <vector>

#include "sc_edgeuse_graph.h"

namespace sc_eu_p1
{
// clang-format off
// ---- verbatim: SmEdgeuse.cpp lines 1065-1113: sm_CalcDistAndDev
static void sm_CalcDistAndDev( const SmPoint3d  &rPt,   // in: Point that was dropped
                               const SmSolution & rSol, // in: Point-drop solution
                               double & rDist,          // out: Dist from surface
                               double * pDev )          // out: Dist from normal
{
  if ( pDev == NULL )
  {
      rDist = rSol.m_vStart.m_dSolutionValue;
      return;
  }

  // pDev is not Null.

  rDist = -1.0;
  *pDev = -1.0;

  SmSurface *pSrf = SM_CAST_PTR( SmSurface, rSol.m_apObjects[0] );
  if ( pSrf == NULL ) { return; }

  // Find distance (along surface normal) and deviation (in normal plane).
  SmPoint2d sUV( rSol.m_vStart.m_adParameters[0], rSol.m_vStart.m_adParameters[1] );
  SmPoint3d sPt;
  SmVector3d sSu, sSv;
  pSrf->Evaluate1stDerivatives( sUV, TRUE, TRUE, sPt, sSu, sSv );

  SmVector3d sNorm( sSu * sSv );
  double dMag = sNorm.Length();
  if ( dMag > SM_EFF_ZERO )
    { sNorm /= dMag; }
  else
    {
      pSrf->EvaluateNormal( sUV, TRUE, TRUE, sNorm );
      if ( sNorm.Length() < 0.5 )
        { return; }
    }

  SmVector3d sDiff( rPt - sPt );
  double dDot = sDiff.Dot( sNorm );
  rDist = smos_Fabs( dDot );

  double d3dDist = sDiff.Length();

  // Pythagorean:
  *pDev = ( rDist >= d3dDist ) ? 0.0    // (Shouldn't happen, just a check for Sqrt.)
                               : smos_Sqrt( d3dDist*d3dDist - rDist*rDist );

  return;

} // end static sm_CalcDistAndDev

// ---- verbatim: SmEdgeuse.cpp lines 1122-1160: sm_AreSeamCurves
static SmBoolean sm_AreSeamCurves
 ( const SmEdgeuse                   * cpEdgeuse,         // NotUsed: in :
   const SmTArray< SmBSplineCurve* > & crUVCurves,        // in :
   const SmExtent1d                  & sEdgeDomain,       // in :
         SmBoolean                     bDropCurveCalled)  // in :
{
  SM_REF1(cpEdgeuse) ;
  if ( crUVCurves.GetSize() != 2 )
    { return FALSE; }

  if ( bDropCurveCalled )
    { return TRUE; }  // DropCurve() will never return two curves except for seams.

  // Check curve domains.
  SmCurve *pUVCurve1 = (SmCurve*)crUVCurves[0];
  SmCurve *pUVCurve2 = (SmCurve*)crUVCurves[1];
  SmExtent1d sDom1 = pUVCurve1->GetNaturalInterval();
  SmExtent1d sDom2 = pUVCurve2->GetNaturalInterval();
  if ( sDom1 == sDom2 ) // This would be a very tight agreement.
    {
      // The check vs. the Edge domain can have a little slop?  Say 1%.
      double dTol = sEdgeDomain.GetLength() / 100.0;
      if ( sDom1.AreEqual( sEdgeDomain, dTol ) )
        { return TRUE; }
    }

  return FALSE;

} // end sm_AreSeamCurves

// ---- verbatim (SM_DEBUG_CODE dropped): SmEdgeuse.cpp lines 3215-3328: SmFindStepOffENFO
class SmFindStepOffENFO : public SmEvalNFunctionsObject
{
protected:
    double m_dStepOffDist;
    const SmPoint3d  & m_crPoint;
    const SmVector3d & m_crPlaneNormal;
    const SmSurface  & m_crSurface;
public:
    SmFindStepOffENFO(double dStepOffDist,
                      const SmPoint3d & crPoint,
                      const SmVector3d & crPlaneNormal,
                      const SmSurface & crSurface)
      : m_dStepOffDist(dStepOffDist),
        m_crPoint(crPoint),
        m_crPlaneNormal(crPlaneNormal),
        m_crSurface(crSurface) {}

    virtual ~SmFindStepOffENFO() {}

    virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                              SmTArray<double>       & rF,              // out: F of Ax=F function values,
                              SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                              SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                        //      TRUE = converged (F members are within tolerance of 0.0
                                                                        //      FALSE= Not Used or Not Converged
} ; // end class SmFindStepOffENFO

SmStatus SmFindStepOffENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: TRUE =
                                           //      FALSE=

{
    SM_ASSERT(crX.GetSize() == 2);
    SM_ASSERT(rF.GetSize() == 2);
    if (pOptJacobian) {
        SM_ASSERT(pOptJacobian->GetNumRows() == 2);
        SM_ASSERT(pOptJacobian->GetNumColumns() == 2);
    }

    rbFoundAnswer = FALSE;
    SmPoint3d sPoint;
    SmVector3d sDU, sDV;
    SmPoint2d sUV(crX[0],crX[1]);
    SER(m_crSurface.Evaluate1stDerivatives(sUV,TRUE,TRUE,sPoint,sDU,sDV));

    double sDULenSq = sDU.LengthSquared();
    double sDVLenSq = sDV.LengthSquared();
    // if we have a zero length tangent then we must fail
    if (sDULenSq < SM_EFF_ZERO_SQ || sDVLenSq < SM_EFF_ZERO_SQ) {
        return SM_ERR;
    }

    SmVector3d sVecSMinusP = sPoint - m_crPoint;

    double dPlaneD = - m_crPlaneNormal.Dot(m_crPoint);

    if (pOptJacobian) {
        (*pOptJacobian)[0][0] = 2.0 * sDU.Dot(sVecSMinusP);
        (*pOptJacobian)[0][1] = 2.0 * sDV.Dot(sVecSMinusP);
        (*pOptJacobian)[1][0] = sDU.Dot(m_crPlaneNormal);
        (*pOptJacobian)[1][1] = sDV.Dot(m_crPlaneNormal);
    }

    // Compute function values
    rF[0] = sVecSMinusP.Dot(sVecSMinusP) - m_dStepOffDist*m_dStepOffDist;
    rF[1] = sPoint.Dot(m_crPlaneNormal) + dPlaneD;

    return SM_SUCCESS;

} // end SmFindStepOffENFO::Evaluate
// clang-format on
} // namespace sc_eu_p1

namespace
{
enum Op : int32_t
{
    OP_EXPORT = 0,
    OP_EU_QUERY = 1,
    OP_CONNECTED = 2,
    OP_FU_QUERY = 3,
    OP_COLLECT = 4,
    OP_UV_LOOP_GAP = 5,
    OP_SET_UV = 6,
    OP_GAPS = 7,
    OP_STEPOFF = 8,
    OP_DISTDEV = 9,
    OP_SEAM = 10,
    OP_SET_ORIENT = 11,
};

// SetUVTrimCurve argument kinds.
const int64_t UV_NULL = 0;     // SetUVTrimCurve(NULL, ...)
const int64_t UV_CURRENT = 1;  // SetUVTrimCurve(GetUVTrimCurve(), ...)
const int64_t UV_NEW = 2;      // SetUVTrimCurve(new curve, ...)
const int64_t UV_DELETE = 3;   // DeleteUVTrimCurve()
const int64_t UV_MEMBER = 4;   // SetMemberUVTrimCurve(new curve, ...)

// Status of an op the shim refuses because upstream would dereference null.
const double ST_NULL_DEREF = -2.0;

const SM_TYPE KIND_TYPES[] = {SmEdgeuse_TYPE,  SmFaceuse_TYPE, SmTopology_TYPE, SmOwningTopology_TYPE,
                              SmAObject_TYPE,  SmObject_TYPE,  SmShell_TYPE,    SmLoopuse_TYPE,
                              SmUnknown_TYPE, 0};

void run_brep_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        // consume the op arguments as the Rust side does: none are read past the block
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    switch (op)
    {
    case OP_EXPORT:
    {
        export_graph(b.brep, g, o);
        break;
    }
    case OP_EU_QUERY:
    {
        SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
        int64_t corner_v = rd.idx();
        SmVertex* cv = corner_v >= 0 && corner_v < (int64_t)g.vertices.size() ? g.vertices[(size_t)corner_v] : nullptr;
        o.push_back((double)g.find(7, p->GetEdge()));
        o.push_back((double)p->GetOrientation());
        o.push_back((double)g.find(6, p->GetCWEdgeuse()));
        o.push_back((double)g.find(6, p->GetCCWEdgeuse()));
        o.push_back(flg(p->HasEdgeFaceGap()));
        o.push_back(flg(p->HasEdgeCCWEdgeGap()));
        o.push_back(flg(p->IsMaxEdgeFaceGapInit()));
        o.push_back(flg(p->IsLoopEdgeuse()));
        o.push_back(flg(p->IsShellEdgeuse()));
        o.push_back((double)g.find(8, p->GetVertexuse()));
        o.push_back((double)g.find(4, p->GetLoopuse()));
        o.push_back((double)g.find(2, p->GetFaceuse()));
        o.push_back((double)g.find(3, p->GetFace()));
        o.push_back((double)g.find(1, p->GetShell()));
        o.push_back(p->GetBrep() == b.brep ? 1.0 : p->GetBrep() == nullptr ? 0.0 : -3.0);
        o.push_back((double)p->GetTolerance());
        // GetMate/GetRadial/IsStrut need the edge's primary edgeuse
        bool prim = p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
        o.push_back(prim ? (double)g.find(6, p->GetMate()) : ST_NULL_DEREF);
        o.push_back(prim ? (double)g.find(6, p->GetRadial()) : ST_NULL_DEREF);
        o.push_back(prim ? flg(p->IsStrut()) : ST_NULL_DEREF);
        SmEdgeuse* owner = (SmEdgeuse*)0x1;
        SmBSplineCurve* uv = prim ? p->GetUVTrimCurve(&owner) : nullptr;
        o.push_back(prim ? (uv ? 1.0 : 0.0) : ST_NULL_DEREF);
        o.push_back(prim ? (double)g.find(6, owner) : ST_NULL_DEREF);
        owner = (SmEdgeuse*)0x1;
        SmDropCurveFail* fail = prim ? p->GetDropCurveFail(&owner) : nullptr;
        o.push_back(prim ? (fail ? 1.0 : 0.0) : ST_NULL_DEREF);
        o.push_back(prim ? (double)g.find(6, owner) : ST_NULL_DEREF);
        o.push_back(p->GetUVTrimCurvePointer() ? 1.0 : 0.0);
        // GetCornerMateEdgeuse dereferences m_pVU and the mate's m_pVU for loop edgeuses
        if (p->IsLoopEdgeuse())
        {
            bool ok = p->GetVertexuse() != nullptr && prim;
            if (ok && p->GetVertexuse()->GetVertex() != cv)
                ok = p->GetMate() != nullptr && p->GetMate()->GetVertexuse() != nullptr;
            o.push_back(ok ? (double)g.find(6, p->GetCornerMateEdgeuse(cv)) : ST_NULL_DEREF);
        }
        else
            o.push_back((double)g.find(6, p->GetCornerMateEdgeuse(cv)));
        o.push_back((double)p->GetType());
        for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
            o.push_back(flg(p->IsKindOf(*t)));
        break;
    }
    case OP_CONNECTED:
    {
        SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        SmTopology* t = target_of(g, ty, i);
        o.push_back(flg(p->IsConnectedTo(t)));
        break;
    }
    case OP_FU_QUERY:
    {
        SmFaceuse* p = g.faceuses[(size_t)rd.idx()];
        bool tgt_only = rd.flag();
        bool has_loops = rd.flag();
        int64_t nl = rd.idx();
        SmTArray<SmLoop*> loops;
        for (int64_t k = 0; k < nl; k++)
            loops.Add(g.loops[(size_t)rd.idx()]);
        o.push_back((double)g.find(1, p->GetShell()));
        o.push_back((double)g.find(0, p->GetRegion()));
        o.push_back((double)g.find(3, p->GetFace()));
        o.push_back((double)g.find(2, p->GetMate()));
        o.push_back((double)p->GetOrientation());
        o.push_back((double)p->GetType());
        for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
            o.push_back(flg(p->IsKindOf(*t)));
        SmTArray<SmLoopuse*> lus;
        p->GetLoopuses(lus, tgt_only ? TRUE : FALSE, has_loops ? &loops : NULL);
        std::vector<int64_t> v;
        for (ULONG k = 0; k < lus.GetSize(); k++)
            v.push_back(g.find(4, lus[k]));
        put_list(o, v);
        SmTArray<SmEdgeuse*> eus;
        p->GetEdgeuses(eus, tgt_only ? TRUE : FALSE, has_loops ? &loops : NULL);
        v.clear();
        for (ULONG k = 0; k < eus.GetSize(); k++)
            v.push_back(g.find(6, eus[k]));
        put_list(o, v);
        for (SmBoolean skip : {FALSE, TRUE})
        {
            SmTArray<SmFaceuse*> fus;
            p->GetNeighbors(fus, skip);
            v.clear();
            for (ULONG k = 0; k < fus.GetSize(); k++)
                v.push_back(g.find(2, fus[k]));
            put_list(o, v);
        }
        break;
    }
    case OP_COLLECT:
    {
        SmFaceuse* p = g.faceuses[(size_t)rd.idx()];
        bool has_thr = rd.flag();
        double thr = rd.num();
        SmTArray<SmFaceuse*> fus;
        SmStatus r = p->CollectAdjacentFaceuses(fus, has_thr ? &thr : NULL);
        o.push_back(st(r));
        std::vector<int64_t> v;
        for (ULONG k = 0; k < fus.GetSize(); k++)
            v.push_back(g.find(2, fus[k]));
        put_list(o, v);
        break;
    }
    case OP_UV_LOOP_GAP:
    {
        SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
        o.push_back(p->GetStartPointUVLoopGap());
        break;
    }
    case OP_SET_UV:
    {
        const SmContext& ctx = the_context();
        SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
        int64_t kind = rd.idx();
        SmBSplineCurve* nc = nullptr;
        if (kind == UV_NEW || kind == UV_MEMBER)
            nc = read_curve(rd, ctx);
        double dmax = rd.num();
        SmBoolean del = rd.bflag();
        bool has_fail = rd.flag();
        int64_t fail_status = rd.idx();
        SmDropCurveFail fail;
        fail.m_sFailStatus = (SmStatus)fail_status;
        SmEdgeuse* mate = p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse())
                              ? p->GetMate()
                              : nullptr;
        bool prim = p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
        if (!prim)
        {
            delete nc;
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBSplineCurve* old = p->GetUVTrimCurve();
        if (kind == UV_MEMBER)
        {
            if (p->GetOrientation() != SM_OT_SAME && mate == nullptr)
            {
                delete nc;
                o.push_back(ST_NULL_DEREF);
                break;
            }
            // the member set does no memory management: the old curve is the shim's
            SmEdgeuse* own = p->GetOrientation() == SM_OT_SAME ? p : mate;
            SmBSplineCurve* prev = own->*EuPeek::uv();
            SmDropCurveFail* prev_fail = own->*EuPeek::fail();
            SmDropCurveFail* nf = has_fail ? new SmDropCurveFail(fail) : nullptr;
            p->SetMemberUVTrimCurve(nc, nf);
            if (prev != nc)
                delete prev;
            if (nf && prev_fail != nf)
                delete prev_fail;
        }
        else if (kind == UV_DELETE)
            p->DeleteUVTrimCurve();
        else
        {
            SmBSplineCurve* arg = kind == UV_NULL ? nullptr : kind == UV_CURRENT ? old : nc;
            // SetUVTrimCurve increases the edge tolerance unless validating only; it
            // dereferences GetBrep() and both vertexuses.
            if (arg != nullptr && arg != old && p->GetBrep() == nullptr)
            {
                delete nc;
                o.push_back(ST_NULL_DEREF);
                break;
            }
            p->SetUVTrimCurve(arg, dmax, del, has_fail ? &fail : NULL);
            if (!del && old != nullptr && old != arg)
                delete old; // detached and handed back to the caller
        }
        o.push_back(0.0);
        put_eu_state(o, g, p);
        put_eu_state(o, g, mate);
        o.push_back(p->GetEdge() ? (double)p->GetEdge()->GetTolerance() : -1.0);
        o.push_back(has_fail && fail.m_pBrep ? 1.0 : 0.0);
        break;
    }
    case OP_GAPS:
    {
        SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
        int64_t which = rd.idx();
        SmBoolean flag = rd.bflag();
        bool want_out = rd.flag();
        // review: compare the values of gaps the op (re)computes
        bool fg0 = (p->*EuPeek::face_gap()).IsInit();
        bool cg0 = (p->*EuPeek::ccw_gap()).IsInit();
        std::vector<double> vals;
        if (which == 0)
            o.push_back(p->GetMaxEdgeFaceGap(flag) ? 1.0 : 0.0);
        else if (which == 1)
            o.push_back(p->GetEdgeCCWEdgeGap(flag) ? 1.0 : 0.0);
        else if (which == 2)
        {
            SmEdgeFaceGap gap;
            o.push_back(st(p->CalcMaxEdgeFaceGap(want_out ? &gap : NULL, flag)));
            o.push_back(flg(gap.IsInit()));
            if (want_out && gap.IsInit())
            {
                SmPoint2d uv = gap.GetFaceUV();
                vals = {gap.m_dGap3d, gap.GetEdgeT(), uv.x, uv.y};
            }
        }
        else
        {
            SmEdgeEdgeGap gap;
            o.push_back(st(p->CalcEdgeCCWEdgeGap(want_out ? &gap : NULL, flag)));
            o.push_back(flg(gap.IsInit()));
            if (want_out && gap.IsInit())
                vals = {gap.m_dGap3d, gap.GetEdgeT(0), gap.GetEdgeT(1)};
        }
        bool fcomp = p->IsLoopEdgeuse() && ((which == 0 && (flag || !fg0)) || (which == 2 && flag));
        bool ccomp = p->IsLoopEdgeuse() && ((which == 1 && (flag || !cg0)) || (which == 3 && flag));
        if (fcomp && (p->*EuPeek::face_gap()).IsInit())
        {
            const SmEdgeFaceGap& fg = p->*EuPeek::face_gap();
            SmPoint2d uv = fg.GetFaceUV();
            vals.insert(vals.end(), {fg.m_dGap3d, fg.GetEdgeT(), uv.x, uv.y});
        }
        if (ccomp && (p->*EuPeek::ccw_gap()).IsInit())
        {
            const SmEdgeEdgeGap& cg = p->*EuPeek::ccw_gap();
            vals.insert(vals.end(), {cg.m_dGap3d, cg.GetEdgeT(0), cg.GetEdgeT(1)});
        }
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
        bool prim = p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
        SmEdgeuse* mate = prim ? p->GetMate() : nullptr;
        o.push_back(mate ? flg((mate->*EuPeek::face_gap()).IsInit()) : -1.0);
        o.insert(o.end(), vals.begin(), vals.end());
        break;
    }
    case OP_SET_ORIENT:
    {
        SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
        p->SetOrientation((SmOrientType)rd.idx());
        o.push_back((double)p->GetOrientation());
        bool prim = p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
        o.push_back(prim ? (double)g.find(6, p->GetMate()) : ST_NULL_DEREF);
        o.push_back(prim ? (double)g.find(6, p->GetRadial()) : ST_NULL_DEREF);
        break;
    }
    default:
        break;
    }
}

void run_geom_op(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_STEPOFF:
    {
        SmSurface* s = read_surface(rd, ctx);
        double dist = rd.num();
        SmPoint3d pt = rd.vec();
        SmVector3d nrm = rd.vec();
        SmTArray<double> x, f;
        x.Add(rd.num());
        x.Add(rd.num());
        f.Add(SM_UNDEF_DOUBLE);
        f.Add(SM_UNDEF_DOUBLE);
        bool jac = rd.flag();
        SmMatrix m(2, 2);
        m[0][0] = m[0][1] = m[1][0] = m[1][1] = SM_UNDEF_DOUBLE;
        SmBoolean found = UNSURE;
        sc_eu_p1::SmFindStepOffENFO e(dist, pt, nrm, *s);
        SmStatus r = e.Evaluate(x, f, jac ? &m : NULL, found);
        o.push_back(st(r));
        o.push_back((double)found);
        o.push_back(f[0]);
        o.push_back(f[1]);
        o.push_back(m[0][0]);
        o.push_back(m[0][1]);
        o.push_back(m[1][0]);
        o.push_back(m[1][1]);
        delete s;
        break;
    }
    case OP_DISTDEV:
    {
        SmSurface* s = read_surface(rd, ctx);
        SmPoint3d pt = rd.vec();
        SmSolution sol;
        sol.m_vStart.m_adParameters[0] = rd.num();
        sol.m_vStart.m_adParameters[1] = rd.num();
        sol.m_vStart.m_dSolutionValue = rd.num();
        int64_t obj = rd.idx(); // 0 null, 1 the surface, 2 a curve
        SmBSplineCurve* c = nullptr;
        if (obj == 1)
            sol.m_apObjects[0] = s;
        else if (obj == 2)
        {
            SmPoint3d a(0, 0, 0), b2(1, 0, 0);
            SmBSplineCurve::CreateLineSegment(ctx, 3, a, b2, c);
            sol.m_apObjects[0] = c;
        }
        bool want_dev = rd.flag();
        double dist = SM_UNDEF_DOUBLE, dev = SM_UNDEF_DOUBLE;
        sc_eu_p1::sm_CalcDistAndDev(pt, sol, dist, want_dev ? &dev : NULL);
        o.push_back(dist);
        o.push_back(dev);
        delete c;
        delete s;
        break;
    }
    case OP_SEAM:
    {
        int64_t n = rd.idx();
        SmTArray<SmBSplineCurve*> curves;
        for (int64_t k = 0; k < n; k++)
            curves.Add(read_curve(rd, ctx));
        double a = rd.num(), b = rd.num();
        SmExtent1d dom;
        dom.SetMinMax(a, b);
        SmBoolean called = rd.bflag();
        o.push_back(flg(sc_eu_p1::sm_AreSeamCurves(NULL, curves, dom, called)));
        for (ULONG k = 0; k < curves.GetSize(); k++)
            delete curves[k];
        break;
    }
    default:
        break;
    }
}
} // namespace

namespace sc_topology_fixture {
void append_shell_state(const View& view,std::vector<double>& output)
{
    const auto identity=[](const auto& values,const void* pointer)->int64_t {
        if (!pointer) return -1;
        auto found=std::find(values.begin(),values.end(),pointer);
        if(found==values.end()) throw 1;
        return (int64_t)(found-values.begin());
    };
    output.push_back(3.0);
    output.push_back((double)identity(view.regions,view.brep.GetInfiniteRegion()));
    for(SmRegion* region:view.regions)
        output.push_back((double)(region->*RegionPeek::is_void()));
    for(SmShell* shell:view.shells) {
        SM_TYPE type=shell->*ShPeek::type();
        output.push_back((double)(type==SmEdgeuse_TYPE ? identity(view.edgeuses,shell->GetList())
            : type==SmVertexuse_TYPE ? identity(view.vertexuses,shell->GetList()) : -1));
    }
    for(SmVertex* vertex:view.vertices) {
        std::vector<int64_t> uses;
        for(SmTopology* use:list_of(vertex->GetList())) uses.push_back(identity(view.vertexuses,use));
        put_list(output,uses);
    }
    for(SmFaceuse* faceuse:view.faceuses) {
        output.push_back((double)identity(view.faceuses,faceuse->GetNext()));
        output.push_back((double)identity(view.faceuses,faceuse->GetLast()));
    }
}

int32_t apply(const char* path, const double* input, int64_t count,
              const std::function<int32_t(const View&)>& operation)
{
    if (!path || !input || count < 1) return SC_ERR_ARG;
    Rd reader{input, 0};
    SmStatus status = SM_SUCCESS;
    std::unique_ptr<SmBrep> brep(build(reader, path, status));
    if (!brep || status != SM_SUCCESS) return SC_ERR_ARG;
    Graph graph;
    walk(brep.get(), graph);
    const View view{*brep, graph.regions, graph.shells, graph.faceuses,
                    graph.faces, graph.loopuses, graph.loops, graph.edgeuses,
                    graph.edges, graph.vertexuses, graph.vertices};
    return operation(view);
}
} // namespace sc_topology_fixture

SC_EXPORT int32_t sc_topology_edgeuse_p1_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                              double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    if (op <= OP_GAPS || op == OP_SET_ORIENT)
        run_brep_op(op, rd, path, o);
    else
        run_geom_op(op, rd, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
