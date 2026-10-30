// SPDX-License-Identifier: Apache-2.0
// Unit intersect.topology_solver_p2: real SmTopologySolver::LocalSolve, reached
// through SolveTrees / SolveTwoTrees. Reuse the p1 object-construction protocol
// and reference driver; its tree operation invokes the pinned C++ LocalSolve.
#include "sc_shim.h"

extern "C" int32_t sc_intersect_topology_solver_p1_eval(int32_t op,
    const double* input, int64_t n_in, double* out, int64_t cap, int64_t* out_len);

SC_EXPORT int32_t sc_intersect_topology_solver_p2_trees(const double* input,
    int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    return sc_intersect_topology_solver_p1_eval(3, input, n_in, out, cap, out_len);
}

// Fixture preflight: unlike a managed node, an external point node is invisible
// to SmTree::GetObjectList, which walks the node manager rather than the root.
#include "StdAfx.h"
#include <SmTree.h>
#include <SmContext.h>
SC_EXPORT int32_t sc_intersect_topology_solver_p2_point_storage(int64_t* counts)
{
    SC_TRY
    SmContext context;
    SmTreeNode external_node;
    SmObjectList external_list, managed_list;
    SmTree external(SmPoint3d(1,2,3), 0.0, &external_node, &external_list);
    SmTree managed(SmPoint3d(1,2,3), 0.0, NULL, &managed_list);
    SmTArray<SmObjectList*> objects;
    SmStatus status = external.GetObjectList(objects);
    if (status != SM_SUCCESS) return SC_ERR_ARG;
    counts[0] = (int64_t)objects.GetSize();
    status = managed.GetObjectList(objects);
    if (status != SM_SUCCESS) return SC_ERR_ARG;
    counts[1] = (int64_t)objects.GetSize();
    return 0;
    SC_CATCH
}

SC_EXPORT int32_t sc_intersect_topology_solver_p2_local(const double* input,
    int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    return sc_intersect_topology_solver_p1_eval(2, input, n_in, out, cap, out_len);
}

// Part-2 static entrypoints on real shell-vertex B-reps and empty shapes.
#include "sc_analytic_surface.h"
#include <SmBrep.h>
#include <SmPlane.h>
#include <SmShell.h>
#include <SmVertex.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmShape.h>
#include <SmBSplineCurve.h>
#include <SmTopologySolver.h>
#include <SmSolutionArray.h>
#include <nurbs.h>
#include <vector>

namespace
{
// The surface of face `which` (0 or 1) of the face entries: the default plane (z = 0 or 1,
// domain (0,0,1,1)) when `srf` is NULL or its block is `0`, else the analytic surface of
// its block (sc_analytic_surface.h) on its GetNaturalUVDomain(). `at` walks `srf`; NULL
// for a malformed block.
SmSurface* face_surface(const SmContext& context, int which, const double* srf, int64_t n_srf,
                        int64_t& at, SmExtent2d& domain)
{
    domain = SmExtent2d(0, 0, 1, 1);
    int64_t kind = 0;
    if (srf != NULL)
    {
        if (at >= n_srf)
            return NULL;
        kind = (int64_t)srf[at++];
    }
    if (kind == 0)
        return new (context) SmPlane(SmPoint3d(0.0, 0.0, (double)which), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                                     SmVector2d(1, 1), SmExtent2d(0, 0, 1, 1), &context);
    int64_t len = sc_analytic::length(kind);
    if (len < 0 || at + len > n_srf)
        return NULL;
    SmSurface* s = sc_analytic::build(context, kind, srf + at);
    at += len;
    if (s != NULL)
        domain = s->GetNaturalUVDomain();
    return s;
}

int32_t run_static(const double* in, int64_t n_in, const double* srf, int64_t n_srf, double* out,
                   int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmContext context;
    SmBrep first(SM_USE_DEFAULT, SM_USE_DEFAULT, &context);
    SmBrep second(SM_USE_DEFAULT, SM_USE_DEFAULT, &context);
    const int entry=(int)in[0], n1=(int)in[5], n2=(int)in[6];
    // face surfaces are only built by the face entries
    if (srf != NULL && entry < 5)
        return SC_ERR_ARG;
    const SmSolverOperationType op=(SmSolverOperationType)(int)in[1];
    const SmSolutionRequestedType req=(SmSolutionRequestedType)(int)in[2];
    const double tol=in[3], best=in[4];
    const SmPoint3d point(in[7],in[8],in[9]);
    std::vector<SmVertex*> vertices;
    for (int i=0;i<n1+n2;++i) {
        SmShell* shell=NULL; SmVertex* vertex=NULL;
        const int at=10+3*i;
        SmBrep& brep = i<n1 ? first : second;
        if (brep.MakeShellVertex(NULL,SmPoint3d(in[at],in[at+1],in[at+2]),shell,vertex)!=SM_SUCCESS || !vertex)
            return SC_ERR_ARG;
        vertices.push_back(vertex);
    }
    gw_CPOINT cp[2]={{-1,0,0,NL_NOW},{1,0,0,NL_NOW}};
    double knots[4]={0,0,1,1};
    SmBSplineCurve* curve=new(context) SmBSplineCurve(3,1,3,1,cp,knots);
    SmTArray<SmBrep*> bs; SmTArray<SmFace*> fs;
    SmTArray<SmEdge*> es; SmTArray<SmVertex*> vs;
    SmShape shape(bs,fs,es,vs);
    const SmVector3d vectors[2]={SmVector3d(0,0,1),SmVector3d(1,0,0)};
    SmSolutionArray solutions;
    SmFace* face0=NULL; SmFace* face1=NULL;
    bool curve_consumed=false;
    const bool swap=in[n_in-2]!=0;
    const SmShape* limit=in[n_in-1]!=0?NULL:&shape;
    if(entry>=5) {
        int64_t at=0;
        SmExtent2d dom0, dom1;
        SmSurface* plane0=face_surface(context,0,srf,n_srf,at,dom0);
        if(!plane0) {delete curve;return SC_ERR_ARG;}
        SmSurface* plane1=face_surface(context,1,srf,n_srf,at,dom1);
        if(!plane1 || at!=n_srf) {delete plane0;delete plane1;delete curve;return SC_ERR_ARG;}
        if(first.CreateFaceFromSurface(plane0,dom0,face0)!=SM_SUCCESS || !face0) {delete curve;return SC_ERR_ARG;}
        if(second.CreateFaceFromSurface(plane1,dom1,face1)!=SM_SUCCESS || !face1) {delete curve;return SC_ERR_ARG;}
        if(entry==7) {
            SmEdge* edge=NULL;
            curve_consumed=true;
            if(first.CreateWireEdgeFromCurve(curve,SmExtent1d(0,1),edge)!=SM_SUCCESS || !edge) return SC_ERR_ARG;
        }
    }
    SmStatus status=SM_ERR_INVALID_INPUT;
    switch(entry) {
    case 0: status=SmTopologySolver::BrepPointSolve(&first,point,op,req,tol,best,vectors,solutions);break;
    case 1: status=SmTopologySolver::BrepCurveSolve(&first,*curve,SmExtent1d(0,1),op,req,tol,best,vectors,solutions);break;
    case 2: status=SmTopologySolver::BrepBrepSolve(&first,&second,op,req,tol,best,vectors,solutions);break;
    case 3: status=SmTopologySolver::ShapePointSolve(&shape,point,op,req,tol,best,vectors,solutions);break;
    case 4: status=SmTopologySolver::ShapeCurveSolve(&shape,*curve,SmExtent1d(0,1),op,req,tol,best,vectors,solutions);break;
    case 5: {
        SmTopologySolver ts(op,req,tol,best,vectors,solutions);
        SmTLSVertexFace local(ts,swap?TRUE:FALSE,limit);
        status=swap?local.SolveIt(face0->GetSurface(),vertices[0]):local.SolveIt(vertices[0],face0->GetSurface());break;
    }
    case 6: {
        SmTopologySolver ts(op,req,tol,best,vectors,solutions);
        SmTLSCurveFace local(ts,swap?TRUE:FALSE,limit);local.SetCurve(curve,SmExtent1d(0,1));
        status=swap?local.SolveIt(face0->GetSurface(),curve):local.SolveIt(curve,face0->GetSurface());break;
    }
    case 7: {
        SmTopologySolver ts(op,req,tol,best,vectors,solutions);
        SmTLSEdgeFace local(ts,swap?TRUE:FALSE,limit);
        status=swap?local.SolveIt(face0->GetSurface(),curve):local.SolveIt(curve,face0->GetSurface());break;
    }
    case 8: {
        SmTopologySolver ts(op,req,tol,best,vectors,solutions);
        SmTLSFaceFace local(ts,swap?TRUE:FALSE,limit);
        status=swap?local.SolveIt(face1->GetSurface(),face0->GetSurface()):local.SolveIt(face0->GetSurface(),face1->GetSurface());break;
    }
    }
    std::vector<double> values;
    values.push_back(status==SM_SUCCESS ? 0.0 : (double)status);
    // Verify the fixture tolerance independently of the port's adapter.
    values.push_back(first.GetTolerance());
    values.push_back(second.GetTolerance());
    for (auto* vertex:vertices) values.push_back(vertex->GetTolerance());
    values.push_back((double)solutions.GetSize());
    auto id=[&](const SmObject* object) -> double {
        if (!object) return -1.0;
        if (object==curve) return 999.0;
        for (int i=0;i<n1+n2;++i) if (object==vertices[(size_t)i]) return (double)i;
        return -2.0;
    };
    for (ULONG i=0;i<solutions.GetSize();++i) {
        const SmSolution& solution=solutions[i];
        values.push_back((double)solution.m_eSolutionType);
        values.push_back((double)solution.m_lNumVariables);
        values.push_back((double)solution.m_lNumObjects);
        values.push_back(id(solution.m_apObjects[0]));values.push_back(id(solution.m_apObjects[1]));
        values.push_back(solution.m_vStart.m_dSolutionValue);
        for (int p=0;p<solution.m_lNumVariables;++p) values.push_back(solution.m_vStart[p]);
        if (solution.m_eSolutionType==SM_ST_RANGE_OF_VALUES) {
            values.push_back(solution.m_vEnd.m_dSolutionValue);
            for (int p=0;p<solution.m_lNumVariables;++p) values.push_back(solution.m_vEnd[p]);
        }
        // Query/local trees can be temporary in the native static API. Compare
        // presence without dereferencing or serializing any pointer addresses.
        // SmSolution has five node slots, distinct from the global solver's
        // maximum of three simultaneously traversed trees. Serialize all slots.
        for (auto* node : solution.m_apNodes)
            values.push_back(node?1.0:0.0);
    }
    if(!curve_consumed) delete curve;
    return sc_copy_out(values.data(),(int64_t)values.size(),out,cap,out_len);
    SC_CATCH
}
} // namespace

SC_EXPORT int32_t sc_intersect_topology_solver_p2_static(const double* in,
    int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    return run_static(in, n_in, NULL, 0, out, cap, out_len);
}

// The face entries (5..8) with the faces' surfaces given by `srf`: per face `0` (the default
// plane) or an analytic block of sc_analytic_surface.h.
SC_EXPORT int32_t sc_intersect_topology_solver_p2_static_faces(const double* in,
    int64_t n_in, const double* srf, int64_t n_srf, double* out, int64_t cap, int64_t* out_len)
{
    if (srf == NULL || n_srf < 2)
        return SC_ERR_ARG;
    return run_static(in, n_in, srf, n_srf, out, cap, out_len);
}

// Probe the actual selected operand's runtime class without performing the
// undefined SmVertex* -> SmCurve* cast of SmTLSEdgeVertex::SolveIt:1032.
SC_EXPORT int32_t sc_intersect_topology_solver_p2_edge_vertex_operand()
{
    SC_TRY
    SmContext context;
    SmBrep brep(SM_USE_DEFAULT,SM_USE_DEFAULT,&context);
    SmShell* shell=NULL;SmVertex* vertex=NULL;
    if(brep.MakeShellVertex(NULL,SmPoint3d(0,0,0),shell,vertex)!=SM_SUCCESS || !vertex)
        return SC_ERR_ARG;
    return vertex->IsKindOf(SmCurve_TYPE)?1:0;
    SC_CATCH
}

// Exercise native LocalSolve's ignored GetObjectList status in both orders.
SC_EXPORT int32_t sc_intersect_topology_solver_p2_empty_lists(int64_t* results)
{
    SC_TRY
    SmContext context;
    SmTree empty(SmPoint3d(0,0,0),0.0);
    SmObjectList item;
    SmTree populated(SmPoint3d(0,0,0),0.0,NULL,&item);
    for (int swap=0;swap<2;++swap) {
        SmSolutionArray solutions;
        SmTopologySolver solver(SM_SO_INTERSECT,SM_SR_ALL,1e-6,1e30,NULL,solutions);
        SmTopoLocalSolver local(solver,FALSE,NULL);
        SmStatus status = swap ? solver.SolveTrees(populated,empty,local)
                              : solver.SolveTrees(empty,populated,local);
        results[2*swap]=status==SM_SUCCESS ? 0 : (int64_t)status;
        results[2*swap+1]=(int64_t)solutions.GetSize();
    }
    return 0;
    SC_CATCH
}
