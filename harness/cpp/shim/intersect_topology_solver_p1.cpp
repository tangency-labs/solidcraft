// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.topology_solver_p1`: SmTopologySolver
// (source/SMLib/src/SmTopologySolver.cpp, source/SMLib/inc/SmTopologySolver.h): the
// constructor (operation category and projection flag, read through a derived class),
// MayContainAnswer, GetBestAnswerSoFar, the SolveIt of the local solvers owned by the unit
// (SmTopoLocalSolver, SmTLSPoint, SmTLSPointVertex, SmTLSVertexVertex, SmTLSPointCurve,
// SmTLSPointEdge, SmTLSVertexEdge, SmTLSCurve, SmTLSCurveEdge / SmTLSCurveCurve,
// SmTLSEdgeEdge) and SolveTrees / SolveTwoTrees. Every member called is public; the
// protected members a test sets (m_vInterval, m_pCurveOwner) or reads
// (m_eOperationCategory, m_bProjectedOperation) are reached through derived classes.
//
// Objects are free-standing: SmVertex (point and zone tolerance set directly), curves
// (SmBSplineCurve, SmHermiteCurve, SmLine, SmEllipse, SmCircle) and SmEdge (curve and
// interval set directly, the curve's owner set to the edge so SmCurve::GetEdge finds it).
// Vertices and edges are never deleted (topology destructors notify owners); curves are
// deleted after their owners are cleared.
//
// One entry point, sc_intersect_topology_solver_p1_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/intersect_topology_solver_p1.rs,
// which validates every input. `out` starts with the op status (0 = SM_SUCCESS, else the
// SmStatus value).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmSolutionArray.h>
#include <SmTopologySolver.h>
#include <SmTree.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <type_traits>
#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_topology_solver_p1.rs).
enum : int32_t
{
    OP_CTOR = 0,
    OP_MAY = 1,
    OP_LOCAL = 2,
    OP_TREES = 3,
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmExtent3d box()
    {
        SmVector3d a = vec();
        SmVector3d b = vec();
        return SmExtent3d(a, b);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

gw_CURVE* read_nurb(Rd& rd)
{
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
    return cur;
}

// A curve block `kind dim ...` (as unit curve.curve_p5): 0 SmBSplineCurve `oob n p m
// cpts(x y z w) knots`; 1 SmHermiteCurve `P1 D1 P2 D2`; 2 bounded SmLine `P V d0 d1 scale
// io`; 3 SmEllipse `C X Y d0 d1 rx ry io`; 4 SmCircle `C X Y d0 d1 r io`.
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 2)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    if (kind == 3)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        bool io = rd.flag();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL,
                                   io ? TRUE : FALSE);
    }
    if (kind == 4)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        bool io = rd.flag();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, dim, &ctx, NULL, io ? TRUE : FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

struct ScVertex : public SmVertex
{
    ScVertex(const SmPoint3d& p, double tol) : SmVertex()
    {
        m_vPoint = p;
        m_sZoneTol3d = SM_CAST_ZONETOL3D(tol);
    }
};

struct ScEdge : public SmEdge
{
    ScEdge(SmCurve* c, const SmExtent1d& ivl, bool has_tol, double tol) : SmEdge()
    {
        m_pCurve = c;
        m_vInterval = ivl;
        // an explicit edge tolerance (else SmEdge's default 0.0), which
        // SmTol::GetZoneTol3d of the edge's curve reads (its curve cache's approximation
        // tolerance)
        if (has_tol)
            m_sZoneTol3d = SM_CAST_ZONETOL3D(tol);
    }
};

// The topology solver with its protected state readable.
struct ScTS : public SmTopologySolver
{
    ScTS(SmSolverOperationType op, SmSolutionRequestedType req, double tol, double best,
         const SmVector3d* vecs, SmSolutionArray& sols)
        : SmTopologySolver(op, req, tol, best, vecs, sols)
    {
    }
    int category() const { return (int)m_eOperationCategory; }
    bool projected() const { return m_bProjectedOperation != FALSE; }
    double best_sq() const { return m_dBestAnswerSoFarSq; }
};

// A local solver of class T with its protected members settable.
template <class T> struct ScLS : public T
{
    ScLS(SmTopologySolver& ts, bool swap) : T(ts, swap ? TRUE : FALSE, NULL) {}
    void set_interval(const SmExtent1d& ivl)
    {
        if constexpr (std::is_base_of<SmTLSPointCurve, T>::value || std::is_base_of<SmTLSCurve, T>::value)
            this->m_vInterval = ivl;
    }
    void set_owner(SmObject* o)
    {
        if constexpr (std::is_base_of<SmTLSPointCurve, T>::value)
            this->m_pCurveOwner = o;
    }
};

// The solver header `op req tol best has_at at nvec V*nvec`.
struct Header
{
    SmSolverOperationType op;
    SmSolutionRequestedType req;
    double tol;
    double best;
    bool has_at;
    double at;
    SmVector3d vecs[4];
    int64_t nvec;
};

Header read_header(Rd& rd)
{
    Header h;
    h.op = (SmSolverOperationType)rd.idx();
    h.req = (SmSolutionRequestedType)rd.idx();
    h.tol = rd.num();
    h.best = rd.num();
    h.has_at = rd.flag();
    h.at = rd.num();
    h.nvec = rd.idx();
    for (int64_t i = 0; i < h.nvec; i++)
        h.vecs[i] = rd.vec();
    return h;
}

// The object table `n (kind data)*`: 0 vertex `P tol`, 1 curve block, 2 edge `c a b has_tol tol`.
struct Objects
{
    std::vector<SmObject*> all;
    std::vector<SmCurve*> curves;
    // An object of the call's context, which topology objects are allocated through.
    SmCurve* anchor = NULL;

    SmObject* at(int64_t i) const { return i < 0 ? NULL : all[(size_t)i]; }

    // Index of `p` in the table; -1 for NULL, -2 for anything else.
    double id(const SmObject* p) const
    {
        if (p == NULL)
            return -1.0;
        for (size_t i = 0; i < all.size(); i++)
            if (all[i] == p)
                return (double)i;
        return -2.0;
    }

    void release()
    {
        for (SmCurve* c : curves)
        {
            c->SetOwner(NULL);
            delete c;
        }
        curves.clear();
        delete anchor;
        anchor = NULL;
    }
};

void read_objects(Rd& rd, const SmContext& ctx, Objects& objs)
{
    objs.anchor = new (ctx) SmLine(SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 0.0, 0.0), 3, &ctx);
    SmObject* anchor = objs.anchor;
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t kind = rd.idx();
        if (kind == 0)
        {
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            objs.all.push_back(new (anchor) ScVertex(p, tol));
        }
        else if (kind == 1)
        {
            SmCurve* c = read_curve(rd, ctx);
            objs.curves.push_back(c);
            objs.all.push_back(c);
        }
        else
        {
            SmCurve* c = (SmCurve*)objs.at(rd.idx());
            double a = rd.num();
            double b = rd.num();
            bool has_tol = rd.flag();
            double tol = rd.num();
            ScEdge* e = new (anchor) ScEdge(c, SmExtent1d(a, b), has_tol, tol);
            c->SetOwner(e);
            objs.all.push_back(e);
        }
    }
}

// The local solver record `kind swap P pv has_ivl a b owner curve cedge`.
struct SolverSpec
{
    int64_t kind;
    bool swap;
    SmPoint3d point;
    int64_t pv;
    bool has_ivl;
    double a, b;
    int64_t owner;
    int64_t curve;
    int64_t cedge;
};

SolverSpec read_spec(Rd& rd)
{
    SolverSpec s;
    s.kind = rd.idx();
    s.swap = rd.flag();
    s.point = rd.vec();
    s.pv = rd.idx();
    s.has_ivl = rd.flag();
    s.a = rd.num();
    s.b = rd.num();
    s.owner = rd.idx();
    s.curve = rd.idx();
    s.cedge = rd.idx();
    return s;
}

template <class T> void setup_point(ScLS<T>& ls, const SolverSpec& s, const Objects& objs)
{
    ls.SetPoint(s.point);
    if (s.pv >= 0)
        ls.SetPointVertex((SmVertex*)objs.at(s.pv));
    if (s.has_ivl)
        ls.set_interval(SmExtent1d(s.a, s.b));
    if (s.owner >= 0)
        ls.set_owner(objs.at(s.owner));
}

template <class T> void setup_curve(ScLS<T>& ls, const SolverSpec& s, const Objects& objs)
{
    SmExtent1d ivl;
    if (s.has_ivl)
        ivl = SmExtent1d(s.a, s.b);
    ls.SetCurve((const SmCurve*)objs.at(s.curve), ivl);
    if (s.cedge >= 0)
        ls.SetCurveEdge((SmEdge*)objs.at(s.cedge));
}

// Runs `body` on a local solver of the spec's class.
template <class F> void with_solver(SmTopologySolver& ts, const SolverSpec& s, const Objects& objs, F body)
{
    switch (s.kind)
    {
    case 0: { ScLS<SmTopoLocalSolver> ls(ts, s.swap); body(ls); break; }
    case 1: { ScLS<SmTLSPoint> ls(ts, s.swap); setup_point(ls, s, objs); body(ls); break; }
    case 2: { ScLS<SmTLSPointVertex> ls(ts, s.swap); setup_point(ls, s, objs); body(ls); break; }
    case 3: { ScLS<SmTLSVertexVertex> ls(ts, s.swap); setup_point(ls, s, objs); body(ls); break; }
    case 4: { ScLS<SmTLSPointCurve> ls(ts, s.swap); setup_point(ls, s, objs); body(ls); break; }
    case 5: { ScLS<SmTLSPointEdge> ls(ts, s.swap); setup_point(ls, s, objs); body(ls); break; }
    case 6: { ScLS<SmTLSVertexEdge> ls(ts, s.swap); setup_point(ls, s, objs); body(ls); break; }
    case 7: { ScLS<SmTLSCurve> ls(ts, s.swap); setup_curve(ls, s, objs); body(ls); break; }
    case 8: { ScLS<SmTLSCurveEdge> ls(ts, s.swap); setup_curve(ls, s, objs); body(ls); break; }
    case 10: { ScLS<SmTLSCurveVertex> ls(ts, s.swap); setup_curve(ls, s, objs); body(ls); break; }
    default: { ScLS<SmTLSEdgeEdge> ls(ts, s.swap); setup_curve(ls, s, objs); body(ls); break; }
    }
}

// A solution `type nvar nobj o0 o1 value s[nvar] [evalue e[nvar]]`.
void put_sol(std::vector<double>& o, const SmSolution& s, const Objects& objs)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_lNumObjects > 0 ? objs.id(s.m_apObjects[0]) : -1.0);
    o.push_back(s.m_lNumObjects > 1 ? objs.id(s.m_apObjects[1]) : -1.0);
    o.push_back(s.m_vStart.m_dSolutionValue);
    for (ULONG i = 0; i < s.m_lNumVariables && i < 10; i++)
        o.push_back(s.m_vStart.m_adParameters[i]);
    if (s.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
    {
        o.push_back(s.m_vEnd.m_dSolutionValue);
        for (ULONG i = 0; i < s.m_lNumVariables && i < 10; i++)
            o.push_back(s.m_vEnd.m_adParameters[i]);
    }
}

void put_result(std::vector<double>& o, const ScTS& ts, SmSolutionArray& sols, const Objects& objs)
{
    o.push_back(ts.best_sq());
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
        put_sol(o, sols[i], objs);
}

// A tree `0 P size` (one node with an object list holding NULL) or `1 box n (obj box)*`
// (a spatial tree).
SmTree* read_tree(Rd& rd, const SmContext& ctx, const Objects& objs, SmTreeNode& /*node*/,
                  SmObjectList& list)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        SmPoint3d p = rd.vec();
        double size = rd.num();
        // Rust Tree::from_point stores its node in the arena. An externally
        // supplied node is NOT registered in SmTree's node manager, so
        // GetObjectList (and hence SolveTwoTrees) would see an empty tree.
        // Use the managed-node constructor on both sides of this protocol.
        return new (ctx) SmTree(p, size, NULL, &list);
    }
    SmExtent3d box = rd.box();
    SmTree* t = new (ctx) SmTree(box);
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        SmObject* obj = objs.at(rd.idx());
        SmExtent3d b = rd.box();
        t->AddToSpatialTree(b, obj);
    }
    return t;
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Header h = read_header(rd);
    SmSolutionArray sols;
    ScTS ts(h.op, h.req, h.tol, h.best, h.nvec > 0 ? h.vecs : NULL, sols);
    if (h.has_at)
        ts.SetAtDistance(h.at);
    switch (op)
    {
    case OP_CTOR:
    {
        o.push_back(0.0);
        o.push_back((double)ts.category());
        o.push_back(ts.projected() ? 1.0 : 0.0);
        double best = 0.0;
        double* pbest = NULL;
        ts.GetBestAnswerSoFar(best, pbest);
        o.push_back(pbest != NULL ? 1.0 : 0.0);
        o.push_back(best);
        break;
    }
    case OP_MAY:
    {
        int64_t n = rd.idx();
        o.push_back(0.0);
        for (int64_t i = 0; i < n; i++)
        {
            SmExtent3d b1 = rd.box();
            SmExtent3d b2 = rd.box();
            o.push_back(ts.MayContainAnswer(b1, b2) ? 1.0 : 0.0);
        }
        break;
    }
    case OP_LOCAL:
    {
        Objects objs;
        read_objects(rd, ctx, objs);
        SolverSpec spec = read_spec(rd);
        int64_t ncall = rd.idx();
        std::vector<int64_t> calls;
        for (int64_t i = 0; i < 2 * ncall; i++)
            calls.push_back(rd.idx());
        SmStatus s = SM_SUCCESS;
        int64_t done = 0;
        with_solver(ts, spec, objs, [&](SmTopoLocalSolver& ls) {
            for (int64_t i = 0; i < ncall && s == SM_SUCCESS; i++)
            {
                s = ls.SolveIt(objs.at(calls[(size_t)(2 * i)]), objs.at(calls[(size_t)(2 * i + 1)]));
                done++;
            }
        });
        o.push_back(st(s));
        o.push_back((double)done);
        put_result(o, ts, sols, objs);
        objs.release();
        break;
    }
    case OP_TREES:
    {
        Objects objs;
        read_objects(rd, ctx, objs);
        SolverSpec spec = read_spec(rd);
        SmTreeNode nodes[2];
        SmObjectList lists[2];
        SmTree* t1 = read_tree(rd, ctx, objs, nodes[0], lists[0]);
        SmTree* t2 = read_tree(rd, ctx, objs, nodes[1], lists[1]);
        bool two = rd.flag();
        SmStatus s = SM_SUCCESS;
        with_solver(ts, spec, objs, [&](SmTopoLocalSolver& ls) {
            s = two ? ts.SolveTwoTrees(*t1, *t2, ls) : ts.SolveTrees(*t1, *t2, ls);
        });
        o.push_back(st(s));
        o.push_back(1.0);
        put_result(o, ts, sols, objs);
        delete t1;
        delete t2;
        objs.release();
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_intersect_topology_solver_p1_eval(int32_t op, const double* in, int64_t n_in,
                                                        double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, ctx, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
