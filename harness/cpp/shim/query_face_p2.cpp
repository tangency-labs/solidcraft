// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.face_p2`: SmFace::PointClassify and SmFace::CurveOnClassify
// (source/SMLib/src/SmFace.cpp) on real SmBrep objects, sm_ClassifyParamOnFaces (a
// file-static function of source/SMLib/src/SmProtoTopology.cpp, VERBATIM copy below in
// namespace sc_face_q2, commit c9c5979, lines 189-256) and SmProtoVertex::ContainsPoint /
// DistanceTo / GetCaptureDistance (source/SMLib/src/SmProtoTopology.cpp,
// source/SMLib/inc/SmProtoTopology.h).
//
// One entry point, sc_query_face_p2_eval(op, path, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/query_face_p2.rs, which validates every
// input. Every B-rep op starts with a B-rep block, then an edit block:
//   `0 rebuild validate_only init_gaps`             SmBrep::ReadFromFile(path) (ASCII .smb)
//   `1 curve t0 t1 validate_only init_gaps`         CreateWireEdgeFromCurve(curve, [t0, t1])
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `3 validate_only init_gaps`                     the wire network of prog_test my_test_topology
//   edits: `n (kind sel value)*` (the entity is `sel` modulo the count of its kind, in
//   export traversal order; skipped when there is none): kind 0 face, 1 edge, 2 vertex:
//   the raw stored zone tolerance (m_sZoneTol3d, SM_USE_OLDTOL) set to SmZoneTol3d(value);
//   kind 3: SmEdgeuse::GetOrCreateUVTrimCurve on every loop edge use of the face (`value`
//   unused), building the UV trim curves a created face or a file without them lacks.
// The B-rep construction, traversal and export (Rd .. build) are copies of the harness
// code of harness/cpp/shim/topology_face_p2.cpp, so the export has that unit's layout and
// traversal order (the Rust side imports it with that unit's importer). They read raw
// members (protected ones through pointer-to-member access), never the functions under
// test. Ops whose upstream path would dereference null are refused by the FFI wrapper.
// Before classifying, the trimmed surface cache of each face surface involved is deleted
// (SmCacheMgr::DeleteObjectsCache): reading a file can leave one, and the port has no
// object caches, so both sides classify through the loops.
#include "sc_analytic_export.h"
#include "sc_exact_curve_export.h"
#include "sc_shim.h"
#include "support/topology_fixture.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmLine.h>
#include <SmLocalSolveNd.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmProtoTopology.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTrimSrfCache.h>
#include <stdexcept>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTopologyIntersector.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>

#include <map>
#include <vector>

// ---- VERBATIM: source/SMLib/src/SmProtoTopology.cpp lines 189-256 (sm_ClassifyParamOnFaces)
namespace sc_face_q2
{
static int sm_ClassifyParamOnFaces    // rtn: Pt Classify value: 1 = On pFace1, 2 = On pFace2, 0 = not classifable (could be on both Faces)
 (const SmPoint2d  & sFaceUVParam,    // in : Target UVParam to classify
  SmZoneTol3d        sMySrcZoneTol3d, // in : obj ZoneTol3d assoc with UVParam, usually not this face
                                      //      if none, use:SmTol::GetZoneTol3d(BREP_CONTEXT_OR_NULL)
  SmFace           * pFace1,          // in : 1st TgtFace to check for pt containment
  const SmExtent2d & sDomain1,        // in : TgtFace1 UV Domain
  SmFace           * pFace2,          // in : 2nd TgtFace to check for pt containment
  const SmExtent2d & sDomain2)        // in : TgtFace2 UV Domain
{
  // quick BBox containment check
  SmBoolean bInFace1 = sDomain1.ContainsPoint2d( sFaceUVParam, SM_EFF_ZERO) ; //cbiTol: need uv-space tol.
  SmBoolean bInFace2 = sDomain2.ContainsPoint2d( sFaceUVParam, SM_EFF_ZERO) ;

  // low work case - TgtUVParam is in only one TgtFace Domain BBox
  if     (bInFace1==TRUE  && bInFace2==FALSE) { return 1 ; }
  else if(bInFace1==FALSE && bInFace2==TRUE ) { return 2 ; }

  // arrive here when UVPoint has to be classified against the Faces' boundaries

  // ZoneTol3d. When input ZoneTol3d is zero - set ZoneTol3d = Max(pFace1->ZoneTol3d, pFace2->ZoneTol3d)
  SmZoneTol3d sSrcZoneTol3d = ( sMySrcZoneTol3d > SM_EFF_ZERO ) ? sMySrcZoneTol3d
                                                                : SmTol::GetZoneTol3d_ForXSectResult(pFace1, pFace2);

  // classify sFaceUVParam against pFace1
  SmPointClassification ePC1(sSrcZoneTol3d, pFace1->GetContext()) ;
  pFace1->PointClassify(sFaceUVParam,  // in : UVPoint to Classify 
                        sSrcZoneTol3d, // NotUsed: in : Obj ZoneTol3d assoc with UVPoint, not this face  
                                       //               (if none, use: SmTol::GetZoneTol3d(Brep_Context_Or_NULL))
                        TRUE,          // NotUsed: in : TRUE = return point face->Vertex/Edge intersections when found 
                                       //               FALSE= return loop-containment classifications. 
                        TRUE,          // NotUsed: in : no longer used 
                        ePC1) ;        // out: Classification of the point. 
                                       //      when InFace     : m_ePointClass = SM_PC_FACE 
                                       //           OutOfFace  : m_ePointClass = SM_PC_UNKNOWN 
                                       //           OnFaceBndry: m_ePointClass = SM_PC_EDGE or SM_PC_VERTEX 

  // when UVParam classifies to pFace1 - return 1                                        
  SmPointClassificationType ePC1Type = ePC1.GetPointClass();
  if(ePC1Type == SM_PC_FACE)
    { return 1 ; }

  // classify sFaceUVParam against pFace2
  SmPointClassification ePC2( sSrcZoneTol3d, pFace2->GetContext()) ;
  pFace2->PointClassify(sFaceUVParam,  // in : UVPoint to Classify 
                        sSrcZoneTol3d, // NotUsed: in : Obj ZoneTol3d assoc with UVPoint, not this face  
                                       //               (if none, use: SmTol::GetZoneTol3d(Brep_Context_Or_NULL))
                        TRUE,          // NotUsed: in : TRUE = return point face->Vertex/Edge intersections when found 
                                       //               FALSE= return loop-containment classifications. 
                        TRUE,          // NotUsed: in : no longer used 
                        ePC2) ;        // out: Classification of the point. 
                                       //      when InFace     : m_ePointClass = SM_PC_FACE 
                                       //           OutOfFace  : m_ePointClass = SM_PC_UNKNOWN 
                                       //           OnFaceBndry: m_ePointClass = SM_PC_EDGE or SM_PC_VERTEX 

  // when UVParam classifies to pFace2 - return 2                                        
  SmPointClassificationType ePC2Type = ePC2.GetPointClass();
  if(ePC2Type == SM_PC_FACE)
    { return 2 ; }

  // when UVParam is on both FaceBndrys its probably on the XSectCrve between the faces - no action required
  if(   ePC1Type == SM_PC_EDGE 
     && ePC2Type == SM_PC_EDGE)
    { return 0 ; }

  // arrive here when the UVPoint did not classify cleanly to one face or the other or to the Boundary between them - GWC: probably should have an err return value - for now 0 means no actions
  return 0; // Problem? Breakpoint here.

} // end sm_ClassifyParamOnFaces
} // namespace sc_face_q2

namespace
{
enum Op : int32_t
{
    OP_EXPORT = 0,
    OP_POINT_CLASSIFY = 1,
    OP_CLASSIFY_PARAM = 2,
    OP_CURVE_ON_CLASSIFY = 3,
    OP_PROTO_VERTEX = 20,
};

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
    static SmOrientType SmLoopuse::*orient() { return &LuPeek::m_eOrientation; }
};
struct VuPeek : SmVertexuse
{
    static SmTopology* SmVertexuse::*owner() { return &VuPeek::m_pSorLUorEU; }
    static SmVertexFaceGap SmVertexuse::*face_gap() { return &VuPeek::m_sVertexFaceGap3d; }
    static SmVertexEdgeGap SmVertexuse::*edge_gap() { return &VuPeek::m_sVertexEdgeGap3d; }
    static SmPoint2d SmVertexuse::*uv() { return &VuPeek::m_sUVPoint; }
};
struct FPeek : SmFace
{
    static SmFaceuse* SmFace::*fu() { return &FPeek::m_pFU; }
    static SmExtent2d SmFace::*dom() { return &FPeek::m_vUVDomain; }
    static SmBoolean SmFace::*rect() { return &FPeek::m_bRectangularTrim; }
    static SmSurface* SmFace::*srf() { return &FPeek::m_pSurface; }
    static SmZoneTol3d SmFace::*tol() { return &FPeek::m_sZoneTol3d; }
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
    // SmBrep::GetEdges order is observable (SmShell::GetWireEdges, shell wire ownership), and a
    // wire owned by a face shell may be unreachable from faces/vertex lists: register it first.
    SmTArray<SmEdge*> all_edges;
    brep->GetEdges(all_edges);
    for (ULONG i = 0; i < all_edges.GetSize(); ++i)
        g.edge(all_edges[i]);
    size_t r = 0, s = 0, fu = 0, lu = 0, eu = 0, vu = 0, l = 0, e = 0, f = 0, v = 0;
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
        // the vertex use lists (a list may hold uses no owner chain reaches)
        for (; v < g.vertices.size(); v++, more = true)
            for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                g.vertexuse((SmVertexuse*)t);
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
    if (s->GetType() == SmPlane_TYPE)
    {
        // `1 O[3] X[3] Y[3] scale[2] anal_domain(u0 v0 u1 v1)`
        const SmPlane* pl = (const SmPlane*)s;
        const SmAxis2Placement& pos = pl->GetPosition();
        o.push_back(1.0);
        put_vec(o, pos.GetOrigin());
        put_vec(o, pos.GetXAxis());
        put_vec(o, pos.GetYAxis());
        SmVector2d sc = pl->GetUVScale();
        o.push_back(sc.x);
        o.push_back(sc.y);
        SmExtent2d d = pl->GetSTEPUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        return;
    }
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    gw_SURFACE* sur = b->GetGwNurbPointer();
    o.push_back(0.0);
    // the surface's own type: an SmBSplineSurface subclass other than SmPlane (a
    // revolution, torus, ...) is exported as its B-spline part only
    o.push_back((double)s->GetType());
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
    {
        SmSurface* s = p->*FPeek::srf();
        sc_analytic_export::put_exact_head(o, s, the_context());
        put_surface(o, s);
        o.push_back((double)g.find(2, p->*FPeek::fu()));
        SmExtent2d d = p->*FPeek::dom();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)(p->*FPeek::rect()));
        o.push_back((double)(p->*FPeek::tol()));
        o.push_back(s && s->GetOwner() == (SmObject*)p ? 1.0 : 0.0);
    }
    for (SmLoopuse* p : g.loopuses)
    {
        o.push_back((double)g.find(2, p->GetOwner()));
        o.push_back((double)g.find(5, p->*LuPeek::loop()));
        o.push_back((double)g.find(4, p->*LuPeek::mate()));
        SM_TYPE ty = p->*LuPeek::type();
        o.push_back((double)ty);
        SmTopology* start = p->*LuPeek::start();
        o.push_back((double)(ty == SmEdgeuse_TYPE ? g.find(6, start) : ty == SmVertexuse_TYPE ? g.find(8, start) : -1));
        o.push_back((double)(p->*LuPeek::orient()));
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
        sc_exact_curve_export::put_export_curve(o, p->GetCurve(), the_context(), put_curve);
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
        o.push_back(flg((p->*VuPeek::edge_gap()).IsInit()));
        SmPoint2d uv = p->*VuPeek::uv();
        o.push_back(uv.x);
        o.push_back(uv.y);
        SmPoint2d fuv = (p->*VuPeek::face_gap()).GetFaceUV();
        o.push_back(fuv.x);
        o.push_back(fuv.y);
        o.push_back((p->*VuPeek::face_gap()).GetLength());
    }
    for (SmVertex* p : g.vertices)
    {
        put_vec(o, p->GetPoint());
        o.push_back((double)p->GetTolerance());
    }
    // Version 3 shell/region state (infinite region, void flags, wire/vertex shell heads, vertex-use
    // lists, face-use links), read from the native records by the shared exporter.
    sc_topology_fixture::append_shell_state(
        {*brep, g.regions, g.shells, g.faceuses, g.faces, g.loopuses, g.loops, g.edgeuses, g.edges,
         g.vertexuses, g.vertices},
        o);
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
        {
            if (p->IsLoopVertexuse() || p->IsEdgeVertexuse())
                if (p->GetFaceuse() && p->GetFaceuse()->GetFace() && p->GetFaceuse()->GetFace()->GetSurface())
                    p->GetVertexFaceGap();
            if (p->HasVertexEdgeGap())
                p->GetVertexEdgeGap();
        }
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

struct EdgePeek : SmEdge
{
    static SmZoneTol3d SmEdge::*tol() { return &EdgePeek::m_sZoneTol3d; }
};
struct VertexPeek : SmVertex
{
    static SmZoneTol3d SmVertex::*tol() { return &VertexPeek::m_sZoneTol3d; }
};

// The edit block (after the B-rep block): selectors (non-negative, validated by the FFI
// wrapper) are taken modulo the entity count; an edit of an absent kind is skipped.
void apply_edits(Rd& rd, Graph& g)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t kind = rd.idx();
        int64_t sel = rd.idx();
        double value = rd.num();
        size_t n_kind = kind == 1 ? g.edges.size() : kind == 2 ? g.vertices.size() : g.faces.size();
        if (n_kind == 0)
            continue;
        size_t at = (size_t)sel % n_kind;
        if (kind == 3)
        {
            // build the missing UV trim curves of the face's edge uses
            SmFace* f = g.faces[at];
            for (SmEdgeuse* eu : g.edgeuses)
                if (eu->IsLoopEdgeuse() && eu->GetFace() == f)
                {
                    SmBSplineCurve* c = nullptr;
                    eu->GetOrCreateUVTrimCurve(c);
                }
        }
        else if (kind == 0)
            g.faces[at]->*FPeek::tol() = SmZoneTol3d(value);
        else if (kind == 1)
            g.edges[at]->*EdgePeek::tol() = SmZoneTol3d(value);
        else
            g.vertices[at]->*VertexPeek::tol() = SmZoneTol3d(value);
    }
}

// Clear incidental caches left by reading files, before applying the fixture's mode.
struct SurfaceCachePeek : SmSurfaceCache
{
    static SmTree* SmSurfaceCache::*tree() { return &SurfaceCachePeek::m_pTree; }
};

void put_cache_state(std::vector<double>& o, SmSurface* surface)
{
    auto* c = (SmSurfaceCache*)SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, surface);
    if (!c) { o.insert(o.end(), {0, 0, 0, 0, 0}); return; }
    o.insert(o.end(), {1, flg(c->m_bHaveTSurfaceCache), (double)c->m_bFaceContainmentDone,
                      flg(c->m_bFaceWasModified), c->*SurfaceCachePeek::tree() ? 1.0 : 0.0});
}

void drop_trim_cache(SmFace* f)
{
    SmSurface* srf = f->GetSurface();
    if (srf != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, srf) != nullptr)
        SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF, srf);
}

// `kind index` of a classification object: 1 face, 2 edge, 3 vertex (export traversal
// index), `0 -1` for none, `-3 -1` for anything else.
void put_object(std::vector<double>& o, Graph& g, const SmObject* p)
{
    if (p == nullptr)
    {
        o.push_back(0.0);
        o.push_back(-1.0);
        return;
    }
    if (p->IsKindOf(SmFace_TYPE))
    {
        o.push_back(1.0);
        o.push_back((double)g.find(3, (const SmFace*)p));
    }
    else if (p->IsKindOf(SmEdge_TYPE))
    {
        o.push_back(2.0);
        o.push_back((double)g.find(7, (const SmEdge*)p));
    }
    else if (p->IsKindOf(SmVertex_TYPE))
    {
        o.push_back(3.0);
        o.push_back((double)g.find(9, (const SmVertex*)p));
    }
    else
    {
        o.push_back(-3.0);
        o.push_back(-1.0);
    }
}

// `class kind index gap src_tol presnap tparam u v param_set`
void put_pc(std::vector<double>& o, Graph& g, const SmPointClassification& pc)
{
    o.push_back((double)pc.GetPointClass());
    put_object(o, g, pc.GetObject());
    o.push_back(pc.GetGap3d());
    o.push_back((double)pc.GetTolerance());
    o.push_back(pc.GetPreSnapParam());
    o.push_back(pc.GetTParam());
    SmPoint2d uv = pc.GetUVParam();
    o.push_back(uv.x);
    o.push_back(uv.y);
    o.push_back(flg(pc.AreParametersSet()));
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
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    apply_edits(rd, g);
    if (op == OP_EXPORT)
    {
        export_graph(b.brep, g, o);
        return;
    }
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_POINT_CLASSIFY:
    {
        // `f u v src_tol do_bnd use_uv tgt_only has_loops n loop* pc_tol uv_ok`
        SmFace* f = g.faces[(size_t)rd.idx()];
        SmPoint2d uv = rd.uv();
        double src_tol = rd.num();
        SmBoolean do_bnd = rd.bflag();
        SmBoolean use_uv = rd.bflag();
        SmBoolean tgt_only = rd.bflag();
        bool has_loops = rd.flag();
        int64_t nl = rd.idx();
        SmTArray<SmLoop*> loops;
        for (int64_t k = 0; k < nl; k++)
            loops.Add(g.loops[(size_t)rd.idx()]);
        double pc_tol = rd.num();
        SmBoolean uv_ok = rd.bflag();
        int64_t cache_mode = rd.idx(), repeats = rd.idx();
        drop_trim_cache(f);
        SmSurface* srf = f->GetSurface();
        const bool traced = cache_mode != 0 || repeats != 1;
        if (cache_mode != 0)
        {
            if (!srf) throw std::runtime_error("cached classification without surface");
            SmSurfaceCache* cache = smsurf_GetSurfaceCache(srf);
            if (!cache || !cache->IsKindOf(SmTrimSrfCache_TYPE) || !cache->GetTree())
                throw std::runtime_error("trim cache construction failed");
            if (cache_mode == 2) f->Notify(SM_NO_PRE_EDIT, f, NULL, NULL);
        }
        for (int64_t query = 0; query < repeats; ++query)
        {
            if (traced) put_cache_state(o, srf);
            bool has_tsc = srf != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, srf) != nullptr;
            SmPointClassification pc(SmZoneTol3d(pc_tol), &ctx);
            SmStatus s = f->PointClassify(uv, SmZoneTol3d(src_tol), do_bnd, use_uv, pc, tgt_only,
                                          has_loops ? &loops : nullptr, uv_ok);
            o.push_back(st(s));
            o.push_back(has_tsc ? 1.0 : 0.0);
            put_pc(o, g, pc);
            if (traced) put_cache_state(o, srf);
        }
        break;
    }
    case OP_CLASSIFY_PARAM:
    {
        // `u v tol f1 dom1[4] f2 dom2[4]`
        SmPoint2d uv = rd.uv();
        double tol = rd.num();
        SmFace* f1 = g.faces[(size_t)rd.idx()];
        SmPoint2d a1 = rd.uv(), b1 = rd.uv();
        SmFace* f2 = g.faces[(size_t)rd.idx()];
        SmPoint2d a2 = rd.uv(), b2 = rd.uv();
        drop_trim_cache(f1);
        drop_trim_cache(f2);
        int r = sc_face_q2::sm_ClassifyParamOnFaces(uv, SmZoneTol3d(tol), f1, SmExtent2d(a1, b1), f2,
                                                   SmExtent2d(a2, b2));
        o.push_back((double)r);
        break;
    }
    case OP_CURVE_ON_CLASSIFY:
    {
        // `f dim P[3] Q[3] tol do_pc big_gaps check3d uv_ok`: the classified curve is the
        // line segment P -> Q (SmBSplineCurve::CreateLineSegment, dimension `dim`; a 2D
        // segment in the face's parameter space ignores z)
        SmFace* f = g.faces[(size_t)rd.idx()];
        drop_trim_cache(f);
        ULONG dim = (ULONG)rd.idx();
        SmPoint3d p = rd.vec();
        SmPoint3d q = rd.vec();
        double tol = rd.num();
        SmBoolean do_pc = rd.bflag();
        SmBoolean big = rd.bflag();
        SmBoolean check3d = rd.bflag();
        SmBoolean uv_ok = rd.bflag();
        SmBSplineCurve* line = nullptr;
        SmStatus cs = SmBSplineCurve::CreateLineSegment(ctx, dim, p, q, line);
        if (cs != SM_SUCCESS || line == nullptr)
        {
            delete line;
            o.push_back(-3.0);
            o.push_back(st(cs));
            break;
        }
        {
            SmCurveClassification cc(line, line->GetNaturalInterval(), NULL, SmZoneTol3d(tol));
            SmBoolean modified = FALSE;
            SmStatus s = f->CurveOnClassify(do_pc, cc, big, check3d, &modified, uv_ok);
            o.push_back(st(s));
            o.push_back(flg(modified));
            o.push_back((double)cc.GetSize());
            for (ULONG i = 0; i < cc.GetSize(); i++)
            {
                SmCurveInterval& iv = cc[i];
                o.push_back(iv.m_vInterval.GetMin());
                o.push_back(iv.m_vInterval.GetMax());
                put_pc(o, g, iv.m_vStart);
                put_pc(o, g, iv.m_vMid);
                put_pc(o, g, iv.m_vEnd);
            }
        }
        delete line;
        break;
    }
    default:
        break;
    }
}

void run_geom_op(int32_t op, Rd& rd, std::vector<double>& o)
{
    if (op != OP_PROTO_VERTEX)
        return;
    // `approx_tol capture override P[3] tol Q[3]`: a manager over two empty B-reps and one
    // proto vertex at P with tolerance tol; outputs `contains distance pv_capture
    // mgr_capture` for Q.
    const SmContext& ctx = the_context();
    double approx_tol = rd.num();
    double capture = rd.num();
    double override_tol = rd.num();
    SmPoint3d p = rd.vec();
    double tol = rd.num();
    SmPoint3d q = rd.vec();
    SmBrep* b1 = new (ctx) SmBrep();
    SmBrep* b2 = new (ctx) SmBrep();
    SmTopologyIntersector* ti = new SmTopologyIntersector(ctx, b1, b2, approx_tol, 0.0);
    SmProtoTopologyManager* mgr = new (b1) SmProtoTopologyManager(b1, b2, ti, capture, override_tol);
    SmProtoVertex* v = new (mgr) SmProtoVertex(p, tol, mgr);
    o.push_back(flg(v->ContainsPoint(q)));
    o.push_back(v->DistanceTo(q));
    o.push_back(v->GetCaptureDistance());
    o.push_back(mgr->GetCaptureDistance());
    // as the topology.proto_topology_p1 shim, the objects live for the whole process
    // (their destructors reach each other)
}
} // namespace

SC_EXPORT int32_t sc_query_face_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                        int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    if (op < OP_PROTO_VERTEX)
        run_brep_op(op, rd, path, o);
    else
        run_geom_op(op, rd, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
