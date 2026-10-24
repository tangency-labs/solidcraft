// SPDX-License-Identifier: Apache-2.0
// Difftest shim for unit blend.fillet_executive_p2 (SmFilletExecutive part 2):
// the fillet-law constructors, SetFilletParameters (edge pointers, per-edge
// parameters), SetFilletBrepEdgeMap / ProcessSmoothEdgeSequences /
// CollectTangentEdgeSequence on executives with few solvers, and SmTemporaryRadiusChange.
//
// sc_blend_fillet_executive_p2_eval(op, in, n_in, out, cap, out_len); every op writes
// `status` first (0 = SM_SUCCESS, else the SmStatus; -2 = upstream would dereference
// null, the input is refused before the call; -4 n_edges = the box has fewer than 12
// edges, no op runs). Layouts (in -> out):
//   0 LAW_LINEAR   start end has_map min max
//                  -> 0 map_min map_max start end arc_length
//   1 LAW_ELLIPTIC start end outer span start_deriv end_deriv has_map min max
//                  -> 0 map_min map_max has_curve [n p m cpts*4 knots] has_extended
//   2 SET_PARAMS   has_brep P[3] l w h  n e*n  nx x*nx  nr r*nr  nv v*nv
//                  has_laws nl (kind [start end])*nl  approx ang tan xsect cont thumb
//                  -> status initialized piecewise insertion global trimming
//                     classification target_is_brep n_solvers error_code
//   3 EXEC         sub k P[3] l w h  (sub 0: SetFilletBrepEdgeMap with an empty fillet
//                  B-rep, 1: ProcessSmoothEdgeSequences, 2: CollectTangentEdgeSequence;
//                  k constant-radius solvers on the box's first k edges; with two or
//                  more, CreateFilletCorners runs first, as in CreateFilletBrep)
//                  -> status seq_len list_len
//   4 TEMP_RADIUS  vr start end P[3] l w h  (a solver on the box's first edge; the side
//                  edge use is the blend edge use of the next edge sharing a vertex)
//                  -> status [start end during, start end after] (variable radius)
// Box edges are SmBrep::GetEdges order, the order of unit heal.stitch_p2's export.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiPrimitives.h>
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmVertexuse.h>
#include <SmPlane.h>
#include <SmCircle.h>
#include <SmOffsetSurface.h>
#include <cfloat>
#include <SmExtent1d.h>
#include <SmFilletExecutive.h>
#include <SmFilletSolver.h>
#include <SmFilletGeom.h>
#include <SmFilletStandardSolver.h>
#include <SmNurbsCrv.h>
#include <SmTArray.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <vector>

namespace
{
enum : int32_t
{
    OP_LAW_LINEAR = 0,
    OP_LAW_ELLIPTIC = 1,
    OP_SET_PARAMS = 2,
    OP_EXEC = 3,
    OP_TEMP_RADIUS = 4,
    OP_LOAD_SOLVER = 5,
};

const double ST_NULL_DEREF = -2.0;
// The box has fewer than 12 edges (SmApiCreateBox at a coordinate scale where its
// sides collapse): outside the harness domain, reported as `-4 n_edges`.
const double ST_BOX_EDGES = -4.0;
const int64_t BOX_EDGES = 12;

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool ok = true;
    double num()
    {
        if (at >= n)
        {
            ok = false;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    bool flag() { return num() != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

// `has [n p m cpts knots]` of a B-spline curve's NURB.
void put_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    const gw_CURVE* cur = c ? c->GetGwNurbPointer() : NULL;
    o.push_back(flg(cur != NULL));
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

// The executive members the ops read or set up.
struct Executive : SmFilletExecutive
{
    Executive(const SmContext& context, SmBrep* target) : SmFilletExecutive(context, target)
    {
        // UB-185: the constructor leaves m_bIsInitialized uninitialized.
        m_bIsInitialized = FALSE;
    }
    SmBoolean initialized() const { return m_bIsInitialized; }
    SmBoolean piecewise() const { return m_bDoPiecewiseMerge; }
    SmBoolean insertion() const { return m_bDoTopologyInsertion; }
    SmBoolean global() const { return m_bDoGlobalMerge; }
    SmBoolean trimming() const { return m_bDoTrimming; }
    SmBoolean classification() const { return m_bDoClassification; }
    SmBrep* target() const { return m_pTargetBrep; }
    void set_fillet(SmBrep* b) { m_pFilletBrep = b; }
    ~Executive()
    {
        // The upstream destructor does not own the fillet B-rep.
        delete m_pFilletBrep;
        m_pFilletBrep = NULL;
    }
};

SmBrep* make_box(Rd& rd)
{
    SmVector3d p = rd.vec();
    double l = rd.num(), w = rd.num(), h = rd.num();
    if (!rd.ok)
        return NULL;
    SmBrep* box = NULL;
    if (SmApiCreateBox(p, l, w, h, box) != SM_SUCCESS)
    {
        delete box;
        return NULL;
    }
    return box;
}

// Writes `-4 n_edges` and returns true when `box` does not have 12 edges.
bool short_box(SmBrep* box, std::vector<double>& o)
{
    SmTArray<SmEdge*> all;
    box->GetEdges(all);
    if ((int64_t)all.GetSize() == BOX_EDGES)
        return false;
    o.push_back(ST_BOX_EDGES);
    o.push_back((double)all.GetSize());
    return true;
}

// The box's first k edges as constant-radius solvers (linear cross section, radius 1).
SmStatus load_solvers(Executive& ex, SmBrep* box, int64_t k)
{
    SmTArray<SmEdge*> all;
    box->GetEdges(all);
    SmTArray<SmEdge*> edges;
    for (int64_t i = 0; i < k && i < (int64_t)all.GetSize(); i++)
        edges.Add(all[(ULONG)i]);
    SmTArray<SmFilletSurfaceGeneratorType> xs;
    xs.Add(SM_FSG_LINEAR);
    SmTArray<SmFilletSolverType> rt;
    rt.Add(SM_FS_CONST_RADIUS);
    SmTArray<double> rv;
    rv.Add(1.0);
    return ex.SetFilletParameters(box, edges, xs, rt, rv, NULL, 0.0, 0.0, 0.0, 0.0, 1, 1.0);
}

void op_law_linear(Rd& rd, std::vector<double>& o)
{
    double start = rd.num(), end = rd.num();
    bool has_map = rd.flag();
    double mn = rd.num(), mx = rd.num();
    SmExtent1d map(mn, mx);
    SmLinearFilletLaw law(start, end, has_map ? &map : NULL);
    o.push_back(0.0);
    o.push_back(law.m_vEdgeMap.GetMin());
    o.push_back(law.m_vEdgeMap.GetMax());
    o.push_back(law.GetStartRadius());
    o.push_back(law.GetEndRadius());
    o.push_back(law.m_dEdgeArcLength);
}

void op_law_elliptic(Rd& rd, std::vector<double>& o)
{
    double start = rd.num(), end = rd.num(), outer = rd.num(), span = rd.num();
    double sd = rd.num(), ed = rd.num();
    bool has_map = rd.flag();
    double mn = rd.num(), mx = rd.num();
    SmContext ctx;
    SmExtent1d map(mn, mx);
    SmBSplineFilletLaw* law =
        new (ctx) SmBSplineFilletLaw(ctx, start, end, outer, span, sd, ed, has_map ? &map : NULL);
    o.push_back(0.0);
    o.push_back(law->m_vEdgeMap.GetMin());
    o.push_back(law->m_vEdgeMap.GetMax());
    put_curve(o, law->GetLawCurve());
    o.push_back(flg(law->GetExtendedCurve() != NULL));
    delete law;
}

void op_set_params(Rd& rd, std::vector<double>& o)
{
    bool has_brep = rd.flag();
    SmBrep* box = make_box(rd);
    if (!box)
    {
        o.push_back((double)SM_ERR);
        return;
    }
    if (short_box(box, o))
    {
        delete box;
        return;
    }
    SmTArray<SmEdge*> all;
    box->GetEdges(all);
    SmTArray<SmEdge*> edges;
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t e = rd.idx();
        if (e >= 0 && e < (int64_t)all.GetSize())
            edges.Add(all[(ULONG)e]);
    }
    SmTArray<SmFilletSurfaceGeneratorType> xs;
    int64_t nx = rd.idx();
    for (int64_t i = 0; i < nx; i++)
        xs.Add((SmFilletSurfaceGeneratorType)rd.idx());
    SmTArray<SmFilletSolverType> rt;
    int64_t nr = rd.idx();
    for (int64_t i = 0; i < nr; i++)
        rt.Add((SmFilletSolverType)rd.idx());
    SmTArray<double> rv;
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; i++)
        rv.Add(rd.num());
    bool has_laws = rd.flag();
    SmTArray<SmFilletLaw*> laws;
    std::vector<SmFilletLaw*> owned;
    int64_t nl = rd.idx();
    for (int64_t i = 0; i < nl; i++)
    {
        int64_t kind = rd.idx();
        if (kind == 1)
        {
            double a = rd.num(), b = rd.num();
            SmFilletLaw* law = new (*box->GetContext()) SmLinearFilletLaw(a, b);
            owned.push_back(law);
            laws.Add(law);
        }
        else
            laws.Add(NULL);
    }
    double approx = rd.num(), ang = rd.num(), tan = rd.num(), xsect = rd.num();
    ULONG cont = (ULONG)rd.idx();
    double thumb = rd.num();

    // Upstream dereferences a null B-rep (once the list sizes pass, with any edge) and
    // a null law list (at the first variable-radius edge it reaches). Refuse exactly
    // those inputs, following SetFilletParameters' order of early returns; the runner
    // reaches the same points (or defers before them).
    bool null_deref = false;
    ULONG ne = edges.GetSize();
    bool sizes_ok = (xs.GetSize() == 1 || xs.GetSize() == ne) &&
                    (rt.GetSize() == 1 || rt.GetSize() == ne) &&
                    (rv.GetSize() == 1 || rv.GetSize() == ne);
    auto fsg_ok = [](SmFilletSurfaceGeneratorType t) {
        return t == SM_FSG_LINEAR || t == SM_FSG_CIRCULAR || t == SM_FSG_BLEND_CURVE;
    };
    if (ne > 0 && sizes_ok)
    {
        if (!has_brep)
            null_deref = true;
        else if (!has_laws && (xs.GetSize() != 1 || fsg_ok(xs[0])))
        {
            for (ULONG i = 0; i < ne; i++)
            {
                if (edges[i]->GetBlendEdgeuse() == NULL)
                    break;
                SmFilletSolverType t = rt.GetSize() == 1 ? rt[0] : rt[i];
                if (t == SM_FS_VARIABLE_RADIUS)
                {
                    null_deref = true;
                    break;
                }
                if (t != SM_FS_CONST_RADIUS && t != SM_FS_CONST_DIST)
                    break;
                if (xs.GetSize() != 1 && !fsg_ok(xs[i]))
                    break;
            }
        }
    }
    if (null_deref)
    {
        o.push_back(ST_NULL_DEREF);
        for (SmFilletLaw* l : owned)
            delete l;
        delete box;
        return;
    }

    std::vector<double> res;
    {
        Executive ex(*box->GetContext(), NULL);
        SmStatus s = ex.SetFilletParameters(has_brep ? box : NULL, edges, xs, rt, rv,
                                            has_laws ? &laws : NULL, approx, ang, tan, xsect,
                                            cont, thumb);
        res.push_back(st(s));
        res.push_back(flg(ex.initialized() != FALSE));
        res.push_back(flg(ex.piecewise() != FALSE));
        res.push_back(flg(ex.insertion() != FALSE));
        res.push_back(flg(ex.global() != FALSE));
        res.push_back(flg(ex.trimming() != FALSE));
        res.push_back(flg(ex.classification() != FALSE));
        res.push_back(flg(ex.target() == box));
        res.push_back((double)ex.GetFilletSolvers().GetSize());
        res.push_back((double)ex.GetFilletErrorInfo()->GetErrorCode());
    }
    o.insert(o.end(), res.begin(), res.end());
    for (SmFilletLaw* l : owned)
        delete l;
    delete box;
}

void op_exec(Rd& rd, std::vector<double>& o)
{
    int64_t sub = rd.idx();
    int64_t k = rd.idx();
    SmBrep* box = make_box(rd);
    if (!box)
    {
        o.push_back((double)SM_ERR);
        return;
    }
    if (short_box(box, o))
    {
        delete box;
        return;
    }
    {
        Executive ex(*box->GetContext(), box);
        SmStatus s = SM_SUCCESS;
        if (k > 0)
            s = load_solvers(ex, box, k);
        // ProcessSmoothEdgeSequences runs in CreateFilletBrep after CreateFilletCorners:
        // GetFilletCornerOfVertex reads m_vVertexCornerMap, which only that fills.
        if (s == SM_SUCCESS && sub > 0 && k > 1)
            s = ex.CreateFilletCorners();
        if (s != SM_SUCCESS)
        {
            o.push_back(st(s));
        }
        else if (sub == 0)
        {
            ex.set_fillet(new (*box->GetContext()) SmBrep());
            o.push_back(st(ex.SetFilletBrepEdgeMap()));
            o.push_back(0.0);
            o.push_back((double)ex.GetFilletSolvers().GetSize());
        }
        else if (sub == 1)
        {
            o.push_back(st(ex.ProcessSmoothEdgeSequences()));
            o.push_back(0.0);
            o.push_back((double)ex.GetFilletSolvers().GetSize());
        }
        else
        {
            SmTArray<SmFilletSolver*> list(ex.GetFilletSolvers());
            SmTArray<SmFilletSolver*> seq;
            o.push_back(st(ex.CollectTangentEdgeSequence(list, seq)));
            o.push_back((double)seq.GetSize());
            o.push_back((double)list.GetSize());
        }
    }
    delete box;
}

void op_temp_radius(Rd& rd, std::vector<double>& o)
{
    bool vr = rd.flag();
    double a = rd.num(), b = rd.num();
    SmBrep* box = make_box(rd);
    if (!box)
    {
        o.push_back((double)SM_ERR);
        return;
    }
    if (short_box(box, o))
    {
        delete box;
        return;
    }
    SmLinearFilletLaw* law = new (*box->GetContext()) SmLinearFilletLaw(a, b);
    {
        Executive ex(*box->GetContext(), box);
        SmTArray<SmEdge*> all;
        box->GetEdges(all);
        SmTArray<SmEdge*> edges;
        edges.Add(all[0]);
        SmTArray<SmFilletSurfaceGeneratorType> xs;
        xs.Add(SM_FSG_LINEAR);
        SmTArray<SmFilletSolverType> rt;
        rt.Add(vr ? SM_FS_VARIABLE_RADIUS : SM_FS_CONST_RADIUS);
        SmTArray<double> rv;
        rv.Add(1.0);
        SmTArray<SmFilletLaw*> laws;
        laws.Add(law);
        SmStatus s =
            ex.SetFilletParameters(box, edges, xs, rt, rv, &laws, 0.0, 0.0, 0.0, 0.0, 1, 1.0);
        SmEdgeuse* side = NULL;
        SmVertex* v0 = all[0]->GetStartVertex();
        for (ULONG i = 1; i < all.GetSize() && side == NULL && s == SM_SUCCESS; i++)
        {
            SmVertex* s0 = all[i]->GetStartVertex();
            if (s0 == v0 || all[i]->GetOtherVertex(s0) == v0)
                side = all[i]->GetBlendEdgeuse();
        }
        if (s == SM_SUCCESS && side == NULL)
            s = SM_ERR;
        o.push_back(st(s));
        if (s == SM_SUCCESS && vr)
        {
            SmFilletSolver* fs = ex.GetFilletSolvers()[0];
            {
                SmTemporaryRadiusChange change(fs, 0, side);
                o.push_back(law->GetStartRadius());
                o.push_back(law->GetEndRadius());
            }
            o.push_back(law->GetStartRadius());
            o.push_back(law->GetEndRadius());
        }
        else if (s == SM_SUCCESS)
        {
            SmFilletSolver* fs = ex.GetFilletSolvers()[0];
            SmTemporaryRadiusChange change(fs, 0, side);
        }
    }
    delete law;
    delete box;
}
struct LoaderSolver : SmConstantRadiusFS
{
    using SmConstantRadiusFS::SmConstantRadiusFS;
    const SmTArray<double>& g1_knots() const { return m_vG1Knots; }
    void attach_wire(SmEdgeuse* a,SmEdgeuse* b) { m_pEdgeuses[0]=a;m_pEdgeuses[1]=b; }
};

// Native loader fixtures retain real constant-radius solvers and real box topology.
// Only the fixture owns the solvers: repeated loads retain the same solver more than once.
struct LoaderExecutive : Executive
{
    using Executive::Executive;
    SmStatus load(SmFilletSolver* solver) { return LoadFilletSolver(solver); }
    bool has_vertices() const { return m_vFilletedVertices.GetSize()!=0; }
    void clear_first_pending()
    {
        if (m_vFilletedVertices.GetSize() == 0) return;
        SmVertex* vertex = m_vFilletedVertices[0];
        delete (SmTArray<SmEdgeuse*>*)m_vVertexCornerMap.At(vertex);
        m_vVertexCornerMap.Insert(vertex, NULL);
    }
    ~LoaderExecutive()
    {
        m_vFilletSolvers.ReSet(); // fixture owns each actual solver exactly once
        for (ULONG i=0;i<m_vFilletedVertices.GetSize();i++)
            delete (SmTArray<SmEdgeuse*>*)m_vVertexCornerMap.At(m_vFilletedVertices[i]);
    }
    void put_loader(std::vector<double>& out, const std::vector<LoaderSolver*>& solvers,
                    const SmTArray<SmEdge*>& edges)
    {
        auto solver_label = [&](SmFilletSolver* target) {
            for (size_t i=0;i<solvers.size();i++) if (solvers[i]==target) return (double)i+1;
            return 0.0;
        };
        auto edgeuse_label = [&](SmEdgeuse* target) {
            for (ULONG i=0;i<edges.GetSize();i++) {
                SmTArray<SmEdgeuse*> uses; edges[i]->GetEdgeuses(uses);
                for (ULONG j=0;j<uses.GetSize();j++) if (uses[j]==target) return 1.0+4.0*i+j;
            }
            return 0.0;
        };
        out.push_back((double)m_vFilletSolvers.GetSize());
        for (ULONG i=0;i<m_vFilletSolvers.GetSize();i++) out.push_back(solver_label(m_vFilletSolvers[i]));
        out.push_back((double)m_vFilletedVertices.GetSize());
        for (ULONG i=0;i<m_vFilletedVertices.GetSize();i++) {
            SmVertex* vertex=m_vFilletedVertices[i];
            out.push_back(vertex ? 1.0 : 0.0);
            if (vertex) { SmPoint3d p=vertex->GetPoint(); out.push_back(p.x);out.push_back(p.y);out.push_back(p.z); }
            SmTArray<SmEdgeuse*>* pending=(SmTArray<SmEdgeuse*>*)m_vVertexCornerMap.At(vertex);
            out.push_back(pending ? 1.0 : 0.0);
            if (pending) {
                out.push_back((double)pending->GetSize());
                for (ULONG j=0;j<pending->GetSize();j++) out.push_back(edgeuse_label((*pending)[j]));
            }
        }
        for (ULONG i=0;i<edges.GetSize();i++) {
            SmTArray<SmEdgeuse*> uses; edges[i]->GetEdgeuses(uses);
            for (ULONG j=0;j<uses.GetSize();j++) out.push_back(solver_label(GetFilletSolverOfEdgeuse(uses[j])));
        }
        out.push_back(GetPseudoBrep()!=NULL ? 1.0 : 0.0);
    }
};

void op_loader(Rd& rd, std::vector<double>& out)
{
    double scale=rd.num(); bool g1=rd.flag();
    SmBrep* box=NULL;
    if (SmApiCreateBox(SmVector3d(0,0,0),scale,2*scale,3*scale,box)!=SM_SUCCESS || !box) {
        out.push_back((double)SM_ERR); return;
    }
    if (short_box(box,out)) { delete box; return; }
    SmTArray<SmEdge*> edges; box->GetEdges(edges);
    // Keep the actual edge geometry unchanged while introducing unequal speeds at
    // its interior knot: the curve is G1, but its parameter derivative is not C1.
    if (g1) for (ULONG i=0;i<edges.GetSize();i++) {
        SmEdge* edge=edges[i]; const SmExtent1d domain=edge->GetInterval();
        SmPoint3d a,b; edge->GetCurve()->EvaluatePoint(domain.GetMin(),a);
        edge->GetCurve()->EvaluatePoint(domain.GetMax(),b);
        SmPoint3d middle=a+(b-a)*0.25;
        gw_CURVE* curve=sm_AllocateNurbCurve(2,1,4);
        SmPoint3d points[3]={a,middle,b};
        for (int j=0;j<3;j++) { curve->pol->Pw[j].x=points[j].x;curve->pol->Pw[j].y=points[j].y;curve->pol->Pw[j].z=points[j].z;curve->pol->Pw[j].w=-DBL_MAX; }
        double t0=domain.GetMin(),t1=domain.GetMax();
        double knots[5]={t0,t0,(t0+t1)*0.5,t1,t1};
        for(int j=0;j<5;j++) curve->knt->U[j]=knots[j];
        edge->Replace3DCurve(new (*box->GetContext()) SmBSplineCurve(curve,3,FALSE));
    }
    int64_t count=rd.idx();
    std::vector<LoaderSolver*> solvers;
    std::vector<SmSurface*> detached_surfaces;
    SmPlane plane0(SmPoint3d(0,0,0),SmVector3d(0,0,1),box->GetContext());
    SmPlane plane1(SmPoint3d(0,0,0),SmVector3d(0,1,0),box->GetContext());
    SmEdge* closed_edge=NULL;
    for(int64_t i=0;i<count;i++) {
        int64_t edge=rd.idx(),mask=rd.idx(); bool radial=rd.flag(),geometry_surfaces=rd.flag();
        LoaderSolver* solver;
        if(edge<12) {
            SmEdgeuse* eu=edges[(ULONG)edge]->GetBlendEdgeuse();
            if(radial && eu) eu=eu->GetRadial();
            solver=new (*box->GetContext()) LoaderSolver(*box->GetContext(),1e-6,0.1,0.01,0.125*scale,eu);
        } else {
            solver=new (*box->GetContext()) LoaderSolver(*box->GetContext(),1e-6,0.1,0.01,0.125*scale,0.125*scale,plane0,plane1,TRUE,TRUE);
            if(edge==13) {
                if(!closed_edge) {
                    SmCircle* circle=new (*box->GetContext()) SmCircle(SmPoint3d(5*scale,0,0),SmVector3d(1,0,0),SmVector3d(0,1,0),SmExtent1d(0,360),0.5*scale,3);
                    if(box->CreateWireEdgeFromCurve(circle,circle->GetNaturalInterval(),closed_edge)!=SM_SUCCESS || !closed_edge) { rd.ok=false;delete solver;break; }
                    box->GetEdges(edges);
                }
                SmTArray<SmEdgeuse*> uses;closed_edge->GetEdgeuses(uses);
                if(uses.GetSize()!=2) {rd.ok=false;delete solver;break;}
                solver->attach_wire(uses[0],uses[1]);
            }
        }
        for(ULONG rail=0;rail<2;rail++) if(!(mask&(1<<rail))) {
            detached_surfaces.push_back(solver->GetSurface(rail));
            solver->SetSurface(rail,NULL);
        }
        if(geometry_surfaces) { SmTArray<SmFilletGeom*> geoms;solver->GetFilletGeoms(geoms);solver->SetFilletGeomForSurfaces(geoms[0]); }
        solvers.push_back(solver);
    }
    if(!rd.ok) {
        for(auto* solver:solvers) delete solver;
        for(auto* surface:detached_surfaces) delete surface;
        delete box;return;
    }
    {
        LoaderExecutive executive(*box->GetContext(),box);
        out.push_back(0.0);
        // Compare actual edge domains and geometry before loader mutations.
        out.push_back((double)edges.GetSize());
        for(ULONG i=0;i<edges.GetSize();i++) {
            SmExtent1d domain=edges[i]->GetInterval();
            out.push_back(domain.GetMin());out.push_back(domain.GetMax());
            for(double fraction : {0.0,0.5,1.0}) {
                double t=domain.GetMin()+(domain.GetMax()-domain.GetMin())*fraction;
                SmPoint3d point;
                out.push_back(st(edges[i]->GetCurve()->EvaluatePoint(t,point)));
                out.push_back(point.x);out.push_back(point.y);out.push_back(point.z);
            }
        }
        // Every new comparison includes actual offset geometry, not just statuses.
        for(auto* solver:solvers) for(ULONG rail=0;rail<2;rail++) {
            SmSurface* surface=solver->GetSurface(rail); out.push_back(surface ? 1.0 : 0.0);
            if(surface) {
                SmPoint2d uv=surface->GetNaturalUVDomain().Evaluate(0.5,0.5); SmPoint3d p;
                out.push_back(st(surface->EvaluatePoint(uv,p)));out.push_back(p.x);out.push_back(p.y);out.push_back(p.z);
            }
        }
        int64_t loads=rd.idx();
        std::vector<int64_t> which;
        for(int64_t i=0;i<loads;i++) { if(rd.idx()!=0) rd.ok=false; which.push_back(rd.idx()); }
        int64_t clear_after=rd.idx();
        for(int64_t i=0;i<loads;i++) {
            LoaderExecutive* current=&executive;
            LoaderSolver* target=solvers[(size_t)which[(size_t)i]];
            // With no rails and an existing vertex list the native loop reads
            // uninitialized pVerts. Guard that actual state before calling it.
            if(target->GetSurface(0) && target->GetSurface(1) && !target->GetEdgeuse(0) && !target->GetEdgeuse(1) && current->has_vertices()) { rd.ok=false;break; }
            out.push_back(st(current->load(target)));
            if(clear_after==i+1) current->clear_first_pending();
            current->put_loader(out,solvers,edges);
            for(auto* solver:solvers) {
                SmFilletExecutive* owner=solver->GetFilletExecutive();
                out.push_back(owner ? 1.0 : 0.0);
                SmFilletBrep* pseudo=owner ? owner->GetPseudoBrep() : NULL;
                out.push_back(owner && pseudo==current->GetPseudoBrep() ? 1.0 : 0.0);
                out.push_back((double)solver->g1_knots().GetSize());
                for(ULONG j=0;j<solver->g1_knots().GetSize();j++) out.push_back(solver->g1_knots()[j]);
            }
        }
    }
    for(auto* solver:solvers) delete solver;
    for(auto* surface:detached_surfaces) delete surface;
    delete box;
}

} // namespace

SC_EXPORT int32_t sc_blend_fillet_executive_p2_eval(int32_t op, const double* in, int64_t n_in,
                                                    double* out, int64_t cap, int64_t* out_len)
{
    if (!in && n_in > 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    switch (op)
    {
    case OP_LAW_LINEAR:
        op_law_linear(rd, o);
        break;
    case OP_LAW_ELLIPTIC:
        op_law_elliptic(rd, o);
        break;
    case OP_SET_PARAMS:
        op_set_params(rd, o);
        break;
    case OP_EXEC:
        op_exec(rd, o);
        break;
    case OP_LOAD_SOLVER:
        op_loader(rd,o);
        break;
    case OP_TEMP_RADIUS:
        op_temp_radius(rd, o);
        break;
    default:
        return SC_ERR_ARG;
    }
    if (!rd.ok)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
