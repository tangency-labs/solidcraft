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
// pointer-to-member access), never the functions under test.
#include "sc_shim.h"

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

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmBoolean bflag() { return flag() ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double u = num();
        double v = num();
        return SmPoint2d(u, v);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = rd.num();
            sur->net->Pw[i][j].y = rd.num();
            sur->net->Pw[i][j].z = rd.num();
            sur->net->Pw[i][j].w = rd.num();
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = rd.num();
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = rd.num();
    SmBSplineSurface* b = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    b->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    b->SetBSplineSurfaceForm(form);
    return b;
}

SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = rd.num();
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(cur, dim, FALSE);
}

// ---- raw member access (pointer-to-member through a derived class) ----------------------

struct EuPeek : SmEdgeuse
{
    static SmTopology* SmEdgeuse::*sorlu() { return &EuPeek::m_pSorLU; }
    static SmBSplineCurve* SmEdgeuse::*uv() { return &EuPeek::m_pUVTrimCurve; }
    static SmDropCurveFail* SmEdgeuse::*fail() { return &EuPeek::m_pDropCurveFail; }
    static SmEdgeFaceGap SmEdgeuse::*face_gap() { return &EuPeek::m_sMaxEdgeFaceGap3d; }
    static SmEdgeEdgeGap SmEdgeuse::*ccw_gap() { return &EuPeek::m_sEdgeCCWEdgeGap3d; }
    static SmVertexuse* SmEdgeuse::*vu() { return &EuPeek::m_pVU; }
};
struct FuPeek : SmFaceuse
{
    static SmFace* SmFaceuse::*face() { return &FuPeek::m_pF; }
    static SmFaceuse* SmFaceuse::*mate() { return &FuPeek::m_pFUMate; }
};
struct LuPeek : SmLoopuse
{
    static SmLoop* SmLoopuse::*loop() { return &LuPeek::m_pL; }
    static SmLoopuse* SmLoopuse::*mate() { return &LuPeek::m_pLUMate; }
    static SM_TYPE SmLoopuse::*type() { return &LuPeek::m_tLoopuseType; }
    static SmTopology* SmLoopuse::*start() { return &LuPeek::m_pEUorVU; }
};
struct VuPeek : SmVertexuse
{
    static SmTopology* SmVertexuse::*owner() { return &VuPeek::m_pSorLUorEU; }
    static SmVertexFaceGap SmVertexuse::*face_gap() { return &VuPeek::m_sVertexFaceGap3d; }
};
struct ShPeek : SmShell
{
    static SM_TYPE SmShell::*type() { return &ShPeek::m_tShellType; }
};

// ---- B-rep construction -------------------------------------------------------------------

struct Built
{
    SmBrep* brep = nullptr;
    ~Built() { delete brep; }
};

// The wire network of prog_test my_test_topology (smTopoTest.cpp, its shell vertex, wire
// edge and edge topology steps).
SmStatus build_wire_network(const SmContext& ctx, SmBrep* pBrep)
{
    SmShell* pNewShell;
    SmVertex* pNewVertex;
    SER(pBrep->MakeShellVertex(pBrep->GetInfiniteRegion(), SmPoint3d(0, 0, 0), pNewShell, pNewVertex));
    SmVertex* pNewVertex2;
    SER(pBrep->MakeShellVertex(pBrep->GetInfiniteRegion(), SmPoint3d(1, 1, 1), pNewShell, pNewVertex2));

    SmEdge* pNewEdge;
    SmVertex* pNewVertex3;
    SmBSplineCurve* pLine = NULL;
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex->GetPoint(), SmPoint3d(1, 0, 0), pLine));
    SER(pBrep->MakeWireEdgeVertex(pBrep->GetInfiniteRegion(), pNewVertex, pLine, pLine->GetNaturalInterval(),
                                  SM_OT_SAME, SmPoint3d(1, 0, 0), pNewEdge, pNewVertex3));

    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex3->GetPoint(), pNewVertex2->GetPoint(), pLine));
    SER(pBrep->MakeWireEdge(pBrep->GetInfiniteRegion(), pNewVertex3, pNewVertex2, pLine, pLine->GetNaturalInterval(),
                            SM_OT_SAME, pNewEdge));

    SER(pBrep->MakeEdgeTopology(pBrep->GetInfiniteRegion(), pNewVertex2, pNewVertex, pNewEdge));
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex2->GetPoint(), pNewVertex->GetPoint(), pLine));
    pNewEdge->SetCurve(pLine, TRUE);
    pLine->SetOwner(pNewEdge);
    pNewEdge->SetInterval(pLine->GetNaturalInterval());
    pNewEdge->SetOrientation(SM_OT_SAME);

    SmVertex* pNewVertex4;
    SER(pBrep->MakeShellVertexTopology(pBrep->GetInfiniteRegion(), pNewShell, pNewVertex4));
    pNewVertex4->SetPoint(SmPoint3d(0, 1, 0));

    SER(pBrep->MakeEdgeTopology(pBrep->GetInfiniteRegion(), pNewVertex2, pNewVertex4, pNewEdge));
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex2->GetPoint(), pNewVertex4->GetPoint(), pLine));
    pNewEdge->SetCurve(pLine, TRUE);
    pLine->SetOwner(pNewEdge);
    pNewEdge->SetInterval(pLine->GetNaturalInterval());
    pNewEdge->SetOrientation(SM_OT_SAME);

    SER(pBrep->MakeEdgeTopology(pBrep->GetInfiniteRegion(), pNewVertex4, pNewVertex, pNewEdge));
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex4->GetPoint(), pNewVertex->GetPoint(), pLine));
    pNewEdge->SetCurve(pLine, TRUE);
    pLine->SetOwner(pNewEdge);
    pNewEdge->SetInterval(pLine->GetNaturalInterval());
    pNewEdge->SetOrientation(SM_OT_SAME);
    return SM_SUCCESS;
}

// ---- traversal-order registry ----------------------------------------------------------

struct Graph
{
    std::vector<SmRegion*> regions;
    std::vector<SmShell*> shells;
    std::vector<SmFaceuse*> faceuses;
    std::vector<SmFace*> faces;
    std::vector<SmLoopuse*> loopuses;
    std::vector<SmLoop*> loops;
    std::vector<SmEdgeuse*> edgeuses;
    std::vector<SmEdge*> edges;
    std::vector<SmVertexuse*> vertexuses;
    std::vector<SmVertex*> vertices;
    std::map<const void*, int64_t> index[10];

    template <class T> int64_t add(int k, std::vector<T*>& v, T* p)
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        if (it != index[k].end())
            return it->second;
        int64_t i = (int64_t)v.size();
        index[k][(const void*)p] = i;
        v.push_back(p);
        return i;
    }
    template <class T> int64_t find(int k, const T* p) const
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        return it == index[k].end() ? -3 : it->second;
    }

    int64_t region(SmRegion* p) { return add(0, regions, p); }
    int64_t shell(SmShell* p) { return add(1, shells, p); }
    int64_t faceuse(SmFaceuse* p) { return add(2, faceuses, p); }
    int64_t face(SmFace* p) { return add(3, faces, p); }
    int64_t loopuse(SmLoopuse* p) { return add(4, loopuses, p); }
    int64_t loop(SmLoop* p) { return add(5, loops, p); }
    int64_t edgeuse(SmEdgeuse* p) { return add(6, edgeuses, p); }
    int64_t edge(SmEdge* p) { return add(7, edges, p); }
    int64_t vertexuse(SmVertexuse* p) { return add(8, vertexuses, p); }
    int64_t vertex(SmVertex* p) { return add(9, vertices, p); }

    // A topology pointer of the given type code, registered.
    int64_t typed(SM_TYPE t, SmTopology* p)
    {
        if (p == nullptr)
            return -1;
        switch (t)
        {
        case SmShell_TYPE: return shell((SmShell*)p);
        case SmLoopuse_TYPE: return loopuse((SmLoopuse*)p);
        case SmEdgeuse_TYPE: return edgeuse((SmEdgeuse*)p);
        case SmVertexuse_TYPE: return vertexuse((SmVertexuse*)p);
        case SmFaceuse_TYPE: return faceuse((SmFaceuse*)p);
        default: return -4;
        }
    }
};

// Members of an owner's circular list, from its head (bounded by a generous count).
std::vector<SmTopology*> list_of(SmTopology* head)
{
    std::vector<SmTopology*> v;
    SmTopology* p = head;
    while (p != nullptr && v.size() < 100000)
    {
        v.push_back(p);
        p = p->GetNext();
        if (p == head)
            break;
    }
    return v;
}

// Walks everything reachable from the B-rep's regions, registering in first-seen order.
void walk(SmBrep* brep, Graph& g)
{
    SmTArray<SmRegion*> regs;
    brep->GetRegions(regs);
    for (ULONG i = 0; i < regs.GetSize(); i++)
        g.region(regs[i]);
    size_t r = 0, s = 0, fu = 0, lu = 0, eu = 0, vu = 0, l = 0, e = 0, f = 0;
    bool more = true;
    while (more)
    {
        more = false;
        for (; r < g.regions.size(); r++, more = true)
            for (SmTopology* t : list_of(g.regions[r]->GetList()))
                g.shell((SmShell*)t);
        for (; s < g.shells.size(); s++, more = true)
        {
            SmShell* sh = g.shells[s];
            SM_TYPE ty = sh->*ShPeek::type();
            if (ty == SmFaceuse_TYPE)
                for (SmTopology* t : list_of(sh->GetList()))
                    g.faceuse((SmFaceuse*)t);
            else
                g.typed(ty, sh->GetList());
        }
        for (; fu < g.faceuses.size(); fu++, more = true)
        {
            SmFaceuse* p = g.faceuses[fu];
            g.face(p->*FuPeek::face());
            g.faceuse(p->*FuPeek::mate());
            for (SmTopology* t : list_of(p->GetList()))
                g.loopuse((SmLoopuse*)t);
        }
        for (; lu < g.loopuses.size(); lu++, more = true)
        {
            SmLoopuse* p = g.loopuses[lu];
            g.loop(p->*LuPeek::loop());
            g.loopuse(p->*LuPeek::mate());
            SmTopology* start = p->*LuPeek::start();
            g.typed(p->*LuPeek::type(), start);
            // the CCW walk of an edge loop
            if (p->*LuPeek::type() == SmEdgeuse_TYPE)
            {
                SmEdgeuse* q = (SmEdgeuse*)start;
                for (int k = 0; q != nullptr && k < 100000; k++)
                {
                    g.edgeuse(q);
                    q = q->GetCCWEdgeuse();
                    if (q == (SmEdgeuse*)start)
                        break;
                }
            }
        }
        for (; eu < g.edgeuses.size(); eu++, more = true)
        {
            SmEdgeuse* p = g.edgeuses[eu];
            g.edge((SmEdge*)p->GetOwner());
            g.typed(p->GetEdgeuseType(), p->*EuPeek::sorlu());
            g.edgeuse(p->GetCWEdgeuse());
            g.edgeuse(p->GetCCWEdgeuse());
            g.vertexuse(p->*EuPeek::vu());
            g.edgeuse((SmEdgeuse*)p->GetNext());
            g.edgeuse((SmEdgeuse*)p->GetLast());
        }
        for (; e < g.edges.size(); e++, more = true)
            for (SmTopology* t : list_of(g.edges[e]->GetList()))
                g.edgeuse((SmEdgeuse*)t);
        for (; vu < g.vertexuses.size(); vu++, more = true)
        {
            SmVertexuse* p = g.vertexuses[vu];
            g.vertex((SmVertex*)p->GetOwner());
            g.typed(p->GetVertexuseType(), p->*VuPeek::owner());
        }
        for (; l < g.loops.size(); l++, more = true)
            g.loopuse(g.loops[l]->GetLoopuse());
        for (; f < g.faces.size(); f++)
            ;
    }
}

// A curve block: `-1` (none), `-2` (not an SmBSplineCurve), else `type dim has_nurb
// [n p m cpts(x y z w) knots]` and, for an SmLine, `P[3] V[3] a0 a1 scale inside_out`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE))
    {
        o.push_back(c == nullptr ? -1.0 : -2.0);
        return;
    }
    const SmBSplineCurve* b = (const SmBSplineCurve*)c;
    gw_CURVE* nurb = b->GetGwNurbPointer();
    o.push_back((double)c->GetType());
    o.push_back((double)b->GetDim());
    o.push_back(nurb ? 1.0 : 0.0);
    if (nurb)
    {
        o.push_back((double)nurb->pol->n);
        o.push_back((double)nurb->p);
        o.push_back((double)nurb->knt->m);
        for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
        {
            o.push_back(nurb->pol->Pw[k].x);
            o.push_back(nurb->pol->Pw[k].y);
            o.push_back(nurb->pol->Pw[k].z);
            o.push_back(nurb->pol->Pw[k].w);
        }
        for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
            o.push_back(nurb->knt->U[k]);
    }
    if (c->GetType() == SmLine_TYPE)
    {
        const SmLine* l = (const SmLine*)c;
        put_vec(o, l->GetLinePoint());
        put_vec(o, l->GetLineVector());
        SmExtent1d d = l->GetSTEPInterval();
        o.push_back(d.GetMin());
        o.push_back(d.GetMax());
        o.push_back(l->GetLineScale());
        o.push_back(flg(l->GetInsideOut()));
    }
}

void put_surface(std::vector<double>& o, const SmSurface* s)
{
    if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE))
    {
        o.push_back(s == nullptr ? -1.0 : -2.0);
        return;
    }
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    gw_SURFACE* sur = b->GetGwNurbPointer();
    o.push_back(0.0);
    o.push_back(flg(((SmBSplineSurface*)b)->GetOutOfBoundsEnabled()));
    o.push_back((double)b->GetBSplineSurfaceForm());
    o.push_back((double)sur->net->n);
    o.push_back((double)sur->net->m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)sur->knu->m);
    o.push_back((double)sur->knv->m);
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

void put_list(std::vector<double>& o, const std::vector<int64_t>& v)
{
    o.push_back((double)v.size());
    for (int64_t x : v)
        o.push_back((double)x);
}

// The exported graph; layout in the FFI wrapper (`export` section).
void export_graph(SmBrep* brep, Graph& g, std::vector<double>& o)
{
    o.push_back(flg(brep->m_bValidateOnly));
    for (auto n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                   g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.push_back((double)n);
    for (SmRegion* p : g.regions)
    {
        o.push_back(p->GetOwner() == (SmOwningTopology*)brep ? 1.0 : 0.0);
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(1, t));
        put_list(o, v);
    }
    for (SmShell* p : g.shells)
    {
        o.push_back((double)g.find(0, p->GetOwner()));
        SM_TYPE ty = p->*ShPeek::type();
        o.push_back((double)ty);
        std::vector<int64_t> v;
        if (ty == SmFaceuse_TYPE)
            for (SmTopology* t : list_of(p->GetList()))
                v.push_back(g.find(2, t));
        put_list(o, v);
    }
    for (SmFaceuse* p : g.faceuses)
    {
        o.push_back((double)g.find(1, p->GetOwner()));
        o.push_back((double)g.find(3, p->*FuPeek::face()));
        o.push_back((double)g.find(2, p->*FuPeek::mate()));
        o.push_back((double)p->GetOrientation());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(4, t));
        put_list(o, v);
    }
    for (SmFace* p : g.faces)
        put_surface(o, p->GetSurface());
    for (SmLoopuse* p : g.loopuses)
    {
        o.push_back((double)g.find(2, p->GetOwner()));
        o.push_back((double)g.find(5, p->*LuPeek::loop()));
        o.push_back((double)g.find(4, p->*LuPeek::mate()));
        SM_TYPE ty = p->*LuPeek::type();
        o.push_back((double)ty);
        SmTopology* start = p->*LuPeek::start();
        o.push_back((double)(ty == SmEdgeuse_TYPE ? g.find(6, start) : ty == SmVertexuse_TYPE ? g.find(8, start) : -1));
    }
    for (SmLoop* p : g.loops)
        o.push_back((double)g.find(4, p->GetLoopuse()));
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.push_back((double)g.find(7, p->GetOwner()));
        SM_TYPE ty = p->GetEdgeuseType();
        o.push_back((double)ty);
        SmTopology* sorlu = p->*EuPeek::sorlu();
        o.push_back((double)(ty == SmLoopuse_TYPE ? g.find(4, sorlu) : ty == SmShell_TYPE ? g.find(1, sorlu) : -1));
        o.push_back((double)g.find(6, p->GetCWEdgeuse()));
        o.push_back((double)g.find(6, p->GetCCWEdgeuse()));
        o.push_back((double)g.find(8, p->*EuPeek::vu()));
        o.push_back((double)p->GetOrientation());
        SmBSplineCurve* uv = p->*EuPeek::uv();
        put_curve(o, uv);
        o.push_back(uv ? (double)g.find(6, uv->GetOwner()) : -1.0);
        SmDropCurveFail* fail = p->*EuPeek::fail();
        o.push_back(fail ? 1.0 : 0.0);
        o.push_back(fail ? (double)fail->m_sFailStatus : 0.0);
        o.push_back(fail && fail->m_pBrep ? 1.0 : 0.0);
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
    }
    for (SmEdge* p : g.edges)
    {
        o.push_back((double)p->GetTolerance());
        SmExtent1d ivl = p->GetInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(6, t));
        put_list(o, v);
        put_curve(o, p->GetCurve());
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.push_back((double)g.find(9, p->GetOwner()));
        SM_TYPE ty = p->GetVertexuseType();
        o.push_back((double)ty);
        SmTopology* own = p->*VuPeek::owner();
        o.push_back((double)(ty == SmEdgeuse_TYPE    ? g.find(6, own)
                             : ty == SmLoopuse_TYPE ? g.find(4, own)
                             : ty == SmShell_TYPE   ? g.find(1, own)
                                                    : -1));
        o.push_back(flg((p->*VuPeek::face_gap()).IsInit()));
    }
    for (SmVertex* p : g.vertices)
    {
        put_vec(o, p->GetPoint());
        o.push_back((double)p->GetTolerance());
    }
}

// Builds the B-rep of a B-rep block; nullptr (status in `st`) when construction fails.
SmBrep* build(Rd& rd, const char* path, SmStatus& status)
{
    const SmContext& ctx = the_context();
    int64_t kind = rd.idx();
    SmBrep* brep = new (ctx) SmBrep();
    status = SM_SUCCESS;
    if (kind == 0)
    {
        bool rebuild = rd.flag();
        status = brep->ReadFromFile(ctx, path, SM_ASCII, rebuild ? TRUE : FALSE);
    }
    else if (kind == 1)
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t0 = rd.num(), t1 = rd.num();
        SmEdge* pEdge = nullptr;
        status = brep->CreateWireEdgeFromCurve(c, SmExtent1d(t0, t1), pEdge);
    }
    else if (kind == 2)
    {
        SmSurface* s = read_surface(rd, ctx);
        SmPoint2d a = rd.uv(), b = rd.uv();
        SmFace* pFace = nullptr;
        status = brep->CreateFaceFromSurface(s, SmExtent2d(a, b), pFace);
    }
    else
    {
        status = build_wire_network(ctx, brep);
    }
    brep->m_bValidateOnly = rd.bflag();
    bool init_gaps = rd.flag();
    if (status != SM_SUCCESS)
    {
        delete brep;
        return nullptr;
    }
    if (init_gaps)
    {
        Graph g;
        walk(brep, g);
        for (SmVertexuse* p : g.vertexuses)
            if (p->IsLoopVertexuse() || p->IsEdgeVertexuse())
                if (p->GetFaceuse() && p->GetFaceuse()->GetFace() && p->GetFaceuse()->GetFace()->GetSurface())
                    p->GetVertexFaceGap();
        for (SmEdgeuse* p : g.edgeuses)
            if (p->IsLoopEdgeuse() && p->GetCCWEdgeuse() && p->GetFace() && p->GetFace()->GetSurface() &&
                p->GetEdge() && p->GetEdge()->GetCurve())
            {
                p->GetMaxEdgeFaceGap();
                p->GetEdgeCCWEdgeGap();
            }
    }
    return brep;
}

SmTopology* target_of(Graph& g, int64_t ty, int64_t i)
{
    auto at = [i](auto& v) -> SmTopology* { return (i >= 0 && i < (int64_t)v.size()) ? (SmTopology*)v[(size_t)i] : nullptr; };
    switch (ty)
    {
    case SmRegion_TYPE: return at(g.regions);
    case SmShell_TYPE: return at(g.shells);
    case SmFaceuse_TYPE: return at(g.faceuses);
    case SmFace_TYPE: return at(g.faces);
    case SmLoopuse_TYPE: return at(g.loopuses);
    case SmLoop_TYPE: return at(g.loops);
    case SmEdgeuse_TYPE: return at(g.edgeuses);
    case SmEdge_TYPE: return at(g.edges);
    case SmVertexuse_TYPE: return at(g.vertexuses);
    case SmVertex_TYPE: return at(g.vertices);
    default: return nullptr;
    }
}

const SM_TYPE KIND_TYPES[] = {SmEdgeuse_TYPE,  SmFaceuse_TYPE, SmTopology_TYPE, SmOwningTopology_TYPE,
                              SmAObject_TYPE,  SmObject_TYPE,  SmShell_TYPE,    SmLoopuse_TYPE,
                              SmUnknown_TYPE, 0};

// A curve's fingerprint: `-1` (none), else `1 t0 t1 P(t0) P(mid) P(t1)`.
void put_curve_id(std::vector<double>& o, const SmBSplineCurve* c)
{
    if (c == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back(1.0);
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (double t : {ivl.GetMin(), ivl.GetMid(), ivl.GetMax()})
    {
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        c->EvaluatePoint(t, p);
        put_vec(o, p);
    }
}

// The state of an edgeuse after SetUVTrimCurve: its uv curve (fingerprint and owner),
// drop curve fail (presence, status, B-rep set) and its vertexuse's vertex/face gap.
void put_eu_state(std::vector<double>& o, Graph& g, SmEdgeuse* p)
{
    if (p == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back(1.0);
    SmBSplineCurve* uv = p->*EuPeek::uv();
    put_curve_id(o, uv);
    o.push_back(uv ? (double)g.find(6, uv->GetOwner()) : -1.0);
    SmDropCurveFail* fail = p->*EuPeek::fail();
    o.push_back(fail ? 1.0 : 0.0);
    o.push_back(fail ? (double)fail->m_sFailStatus : 0.0);
    o.push_back(fail && fail->m_pBrep ? 1.0 : 0.0);
    SmVertexuse* vu = p->*EuPeek::vu();
    o.push_back(vu ? flg((vu->*VuPeek::face_gap()).IsInit()) : -1.0);
}

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
        if (which == 0)
            o.push_back(p->GetMaxEdgeFaceGap(flag) ? 1.0 : 0.0);
        else if (which == 1)
            o.push_back(p->GetEdgeCCWEdgeGap(flag) ? 1.0 : 0.0);
        else if (which == 2)
        {
            SmEdgeFaceGap gap;
            o.push_back(st(p->CalcMaxEdgeFaceGap(want_out ? &gap : NULL, flag)));
            o.push_back(flg(gap.IsInit()));
        }
        else
        {
            SmEdgeEdgeGap gap;
            o.push_back(st(p->CalcEdgeCCWEdgeGap(want_out ? &gap : NULL, flag)));
            o.push_back(flg(gap.IsInit()));
        }
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
        bool prim = p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
        SmEdgeuse* mate = prim ? p->GetMate() : nullptr;
        o.push_back(mate ? flg((mate->*EuPeek::face_gap()).IsInit()) : -1.0);
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
