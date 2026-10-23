// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.curve_class_p5`, B-rep cases: sm_InsertSolutions with
// an SmEdge target and SmCurveClassification::IntersectVertices (source/SMLib/src/
// SmCurveClass.cpp) on real SmBrep objects. sm_InsertSolutions is the verbatim copy in
// intersect_curve_class_p5.cpp, reached through sc_cc_p5_insert_solutions.
//
// One entry point, sc_intersect_curve_class_p5_topo(op, path, in, n_in, out, cap,
// out_len). Layouts are documented in crates/solidcore-ffi/src/units/
// intersect_curve_class_p5.rs, which validates every input. Every op starts with a B-rep
// block of unit `topology.edgeuse_p1` (built by sc_edgeuse_graph.h), then
//   edits: `n (kind sel value)*`, the entity `sel` modulo the count of its kind in export
//     order: kind 0 face, 1 edge, 2 vertex: the raw stored zone tolerance set to
//     SmZoneTol3d(value); 3: SmEdgeuse::GetOrCreateUVTrimCurve on every loop edge use of
//     the face; 4, 5, 6: the raw vertex point's x, y or z moved by value.
//   mates: `n (e1 e2)*`: when n > 0 the block is built a second time (with the same
//     edits) and an SmTopologyIntersector(ctx, b1, b2, 0, 0) relates edge e1 of the
//     first to edge e2 of the second (Relate, in order).
// The edits and Relate read and write raw members or call only native constructors and
// Relate; the export (sc_edgeuse_graph.h) reads raw members.
#include "sc_edgeuse_graph.h"

#include <SmCurveClass.h>
#include <SmPointClass.h>
#include <SmTol.h>
#include <SmTopologyIntersector.h>

#include <memory>

SmStatus sc_cc_p5_insert_solutions(SmCurveClassification* pCurveClassification, SmSolutionArray& rSolutions,
                                   SmObject* pObj, SmXSectTol3d* pOptXSectTol3d);

namespace
{
enum Op : int32_t
{
    OP_EXPORT = 0,
    OP_EDGE_INSERT = 1,
    OP_VERTICES = 2,
};

struct FaceTolPeek : SmFace
{
    static SmZoneTol3d SmFace::*tol() { return &FaceTolPeek::m_sZoneTol3d; }
};
struct EdgeTolPeek : SmEdge
{
    static SmZoneTol3d SmEdge::*tol() { return &EdgeTolPeek::m_sZoneTol3d; }
};
struct VertexPeek : SmVertex
{
    static SmZoneTol3d SmVertex::*tol() { return &VertexPeek::m_sZoneTol3d; }
    static SmPoint3d SmVertex::*point() { return &VertexPeek::m_vPoint; }
};

// The edit block (see the file header).
void apply_edits(const double* edits, Graph& g)
{
    Rd rd{edits, 0};
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t kind = rd.idx();
        int64_t sel = rd.idx();
        double value = rd.num();
        size_t n_kind = kind == 1 ? g.edges.size() : (kind == 0 || kind == 3) ? g.faces.size() : g.vertices.size();
        if (n_kind == 0)
            continue;
        size_t at = (size_t)sel % n_kind;
        if (kind == 3)
        {
            SmFace* f = g.faces[at];
            for (SmEdgeuse* eu : g.edgeuses)
                if (eu->IsLoopEdgeuse() && eu->GetFace() == f)
                {
                    SmBSplineCurve* c = nullptr;
                    eu->GetOrCreateUVTrimCurve(c);
                }
        }
        else if (kind == 0)
            g.faces[at]->*FaceTolPeek::tol() = SmZoneTol3d(value);
        else if (kind == 1)
            g.edges[at]->*EdgeTolPeek::tol() = SmZoneTol3d(value);
        else if (kind == 2)
            g.vertices[at]->*VertexPeek::tol() = SmZoneTol3d(value);
        else
        {
            SmPoint3d& p = g.vertices[at]->*VertexPeek::point();
            (kind == 4 ? p.x : kind == 5 ? p.y : p.z) += value;
        }
    }
}

// Both B-reps (the second only with mates), their graphs and the intersector.
struct Model
{
    Built b1, b2;
    Graph g1, g2;
    SmTopologyIntersector* ti = nullptr;
    ~Model() { delete ti; }
};

// Builds the model; false (with `-1 status` written) when a B-rep cannot be built. The
// reader is left after the mate block.
bool build_model(Rd& rd, const char* path, Model& m, std::vector<double>& o)
{
    int64_t block_at = rd.at;
    SmStatus bst;
    m.b1.brep = build(rd, path, bst);
    if (m.b1.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return false;
    }
    walk(m.b1.brep, m.g1);
    int64_t edits_at = rd.at;
    int64_t n_edits = rd.idx();
    rd.at += 3 * n_edits;
    apply_edits(rd.in + edits_at, m.g1);
    int64_t n_mates = rd.idx();
    if (n_mates > 0)
    {
        Rd again{rd.in, block_at};
        m.b2.brep = build(again, path, bst);
        if (m.b2.brep == nullptr)
        {
            o.push_back(-1.0);
            o.push_back(st(bst));
            return false;
        }
        walk(m.b2.brep, m.g2);
        apply_edits(rd.in + edits_at, m.g2);
        m.ti = new SmTopologyIntersector(the_context(), m.b1.brep, m.b2.brep, 0.0, 0.0);
    }
    for (int64_t i = 0; i < n_mates; i++)
    {
        SmEdge* e1 = m.g1.edges[(size_t)rd.idx()];
        SmEdge* e2 = m.g2.edges[(size_t)rd.idx()];
        if (m.ti->Relate(e1, e2) != SM_SUCCESS)
        {
            o.push_back(-2.0);
            return false;
        }
    }
    o.push_back(0.0);
    return true;
}

// `kind brep index` of a classification object: 0 none; 1 face, 2 edge, 3 vertex (with
// the B-rep, 0 or 1, and the export index); 4 the classification curve, 5 surface 1,
// 6 surface 2; -3 anything else.
void put_object(std::vector<double>& o, Model& m, const SmObject* p, const SmCurve* curve, const SmSurface* s1,
                const SmSurface* s2)
{
    auto topo = [&](double kind, int k, const void* q) {
        int64_t i = m.g1.find(k, q);
        double b = 0.0;
        if (i < 0 && m.b2.brep != nullptr)
        {
            i = m.g2.find(k, q);
            b = 1.0;
        }
        o.insert(o.end(), {kind, b, (double)i});
    };
    if (p == nullptr)
        o.insert(o.end(), {0.0, -1.0, -1.0});
    else if (p == (const SmObject*)curve)
        o.insert(o.end(), {4.0, -1.0, -1.0});
    else if (s1 != nullptr && p == (const SmObject*)s1)
        o.insert(o.end(), {5.0, -1.0, -1.0});
    else if (s2 != nullptr && p == (const SmObject*)s2)
        o.insert(o.end(), {6.0, -1.0, -1.0});
    else if (p->IsKindOf(SmFace_TYPE))
        topo(1.0, 3, p);
    else if (p->IsKindOf(SmEdge_TYPE))
        topo(2.0, 7, p);
    else if (p->IsKindOf(SmVertex_TYPE))
        topo(3.0, 9, p);
    else
        o.insert(o.end(), {-3.0, -1.0, -1.0});
}

void put_pc(std::vector<double>& o, Model& m, const SmPointClassification& pc, const SmCurve* curve,
            const SmSurface* s1, const SmSurface* s2)
{
    o.push_back((double)pc.GetPointClass());
    put_object(o, m, pc.GetObject(), curve, s1, s2);
    o.push_back(pc.GetDeviation());
    o.push_back(pc.GetTParam());
    SmPoint2d uv = pc.GetUVParam();
    SmPoint3d uvw = pc.GetUVWParam(), sss = pc.GetSSSPoint();
    o.insert(o.end(), {uv.x, uv.y, uvw.x, uvw.y, uvw.z, flg(pc.AreParametersSet()), flg(pc.GetSSSPointFlag()), sss.x,
                       sss.y, sss.z, pc.GetPreSnapParam(), (double)SmTol::GetSrcZoneTol3d(&pc)});
}

void put_cc(std::vector<double>& o, Model& m, const SmCurveClassification& cc, const SmCurve* curve,
            const SmSurface* s1, const SmSurface* s2)
{
    o.push_back((double)cc.GetSize());
    o.push_back(cc.GetInterval().GetMin());
    o.push_back(cc.GetInterval().GetMax());
    for (ULONG j = 0; j < cc.GetSize(); j++)
    {
        const SmCurveInterval& ivl = cc[j];
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_pc(o, m, ivl.m_vStart, curve, s1, s2);
        put_pc(o, m, ivl.m_vMid, curve, s1, s2);
        put_pc(o, m, ivl.m_vEnd, curve, s1, s2);
    }
}

// Every vertex tolerance, then per edge use whether it has a UV trim curve, of each B-rep.
void put_topology_state(std::vector<double>& o, Model& m)
{
    for (Graph* g : {&m.g1, &m.g2})
    {
        if (g == &m.g2 && m.b2.brep == nullptr)
            break;
        for (SmVertex* v : g->vertices)
            o.push_back((double)v->GetTolerance());
        for (SmEdgeuse* eu : g->edgeuses)
            o.push_back(eu->*EuPeek::uv() != nullptr ? 1.0 : 0.0);
    }
}

void run_edge_insert(Rd& rd, Model& m, std::vector<double>& o)
{
    // `e ok_set ignore tol P[3] Q[3] opt_tol has_srf [surface surface] ns (type t0 t1 t2 t3 g0 g1)*`
    const SmContext& ctx = the_context();
    SmEdge* edge = m.g1.edges[(size_t)rd.idx()];
    SmBoolean ok_set = rd.bflag();
    SmBoolean ignore = rd.bflag();
    double tol = rd.num();
    SmPoint3d p = rd.vec();
    SmPoint3d q = rd.vec();
    double opt_tol = rd.num();
    bool has_srf = rd.flag();
    std::unique_ptr<SmSurface> s1, s2;
    if (has_srf)
    {
        s1.reset(read_surface(rd, the_context()));
        s2.reset(read_surface(rd, the_context()));
    }
    // the native code dereferences the edge curve and both end vertices
    if (edge->GetCurve() == nullptr || edge->GetStartVertex() == nullptr ||
        edge->GetOtherVertex(edge->GetStartVertex()) == nullptr)
    {
        o.push_back(-3.0);
        return;
    }
    SmBSplineCurve* raw_line = nullptr;
    SmStatus cs = SmBSplineCurve::CreateLineSegment(ctx, 3, p, q, raw_line);
    std::unique_ptr<SmBSplineCurve> line(raw_line);
    if (cs != SM_SUCCESS || line == nullptr)
    {
        o.push_back(-4.0);
        o.push_back(st(cs));
        return;
    }
    SmCurveClassification cc(line.get(), line->GetNaturalInterval(), NULL, SmZoneTol3d(tol));
    cc.SetIgnoreTangency(ignore);
    cc.SetOKToSetTolerance(ok_set);
    if (has_srf)
        cc.SetIntersectData(s1.get(), s2.get(), NULL, TRUE, NULL);
    int64_t ns = rd.idx();
    SmSolutionArray solutions;
    for (int64_t j = 0; j < ns; j++)
    {
        SmSolution sol;
        sol.m_eSolutionType = (SmSolutionType)rd.idx();
        sol.m_lNumObjects = 2;
        sol.m_lNumVariables = 2;
        sol.m_apObjects[0] = line.get();
        sol.m_apObjects[1] = edge;
        sol.m_vStart[0] = rd.num();
        sol.m_vStart[1] = rd.num();
        sol.m_vEnd[0] = rd.num();
        sol.m_vEnd[1] = rd.num();
        sol.m_vStart.m_dSolutionValue = rd.num();
        sol.m_vEnd.m_dSolutionValue = rd.num();
        solutions.Add(sol);
    }
    SmXSectTol3d xt(opt_tol);
    SmStatus s = sc_cc_p5_insert_solutions(&cc, solutions, edge, opt_tol < 0 ? NULL : &xt);
    o.push_back(st(s));
    put_cc(o, m, cc, line.get(), s1.get(), s2.get());
    for (ULONG j = 0; j < solutions.GetSize(); j++)
    {
        SmSolution& sol = solutions[j];
        o.insert(o.end(), {(double)sol.m_eSolutionType, sol.m_vStart[0], sol.m_vStart[1], sol.m_vEnd[0],
                           sol.m_vEnd[1], sol.m_vStart.m_dSolutionValue, sol.m_vEnd.m_dSolutionValue});
    }
    put_topology_state(o, m);
}

void run_vertices(Rd& rd, Model& m, std::vector<double>& o)
{
    // `f big uv_ok tol cc_face curve has_srf [surface surface] has_uv1 [curve] has_uv2 [curve] nv v*`
    const SmContext& ctx = the_context();
    SmFace* face = m.g1.faces[(size_t)rd.idx()];
    SmBoolean big = rd.bflag();
    SmBoolean uv_ok = rd.bflag();
    double tol = rd.num();
    bool cc_face = rd.flag();
    std::unique_ptr<SmBSplineCurve> curve(read_curve(rd, ctx));
    bool has_srf = rd.flag();
    std::unique_ptr<SmSurface> s1, s2;
    if (has_srf)
    {
        s1.reset(read_surface(rd, the_context()));
        s2.reset(read_surface(rd, the_context()));
    }
    std::unique_ptr<SmBSplineCurve> uv1, uv2;
    if (rd.flag())
        uv1.reset(read_curve(rd, ctx));
    if (rd.flag())
        uv2.reset(read_curve(rd, ctx));
    int64_t nv = rd.idx();
    SmTArray<SmVertex*> vertices;
    for (int64_t i = 0; i < nv; i++)
        vertices.Add(m.g1.vertices[(size_t)rd.idx()]);
    SmSurface* surface = face->GetSurface();
    // the native code dereferences the face surface and its owner, and with large gaps the
    // vertex use of each vertex on the face
    if (surface == nullptr || surface->GetOwner() == nullptr)
    {
        o.push_back(-3.0);
        return;
    }
    const SmFace* gap_face = cc_face ? face : surface->GetOwner()->IsKindOf(SmFace_TYPE) ? (SmFace*)surface->GetOwner() : NULL;
    if (big && gap_face != nullptr)
        for (ULONG i = 0; i < vertices.GetSize(); i++)
            if (vertices[i]->GetVertexuseOfFace(gap_face) == nullptr)
            {
                o.push_back(-3.0);
                return;
            }
    SmCurveClassification cc(curve.get(), curve->GetNaturalInterval(), uv1.get(), SmZoneTol3d(tol));
    if (has_srf)
        cc.SetIntersectData(s1.get(), s2.get(), uv2.get(), TRUE, m.ti);
    if (cc_face)
        cc.SetFace(face);
    SmStatus s = cc.IntersectVertices(vertices, *surface, big, uv_ok);
    o.push_back(st(s));
    o.push_back(flg(cc.GetIsCurveCurrent()));
    put_curve(o, cc.GetCurve());
    put_cc(o, m, cc, curve.get(), s1.get(), s2.get());
    put_topology_state(o, m);
}
} // namespace

SC_EXPORT int32_t sc_intersect_curve_class_p5_topo(int32_t op, const char* path, const double* in, int64_t n_in,
                                                    double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL || op < OP_EXPORT || op > OP_VERTICES)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    Model m;
    if (build_model(rd, path, m, o))
    {
        if (op == OP_EXPORT)
        {
            // `which`: the B-rep to export (1 only with mates)
            int64_t which = rd.idx();
            if (which == 1 && m.b2.brep == nullptr)
                return SC_ERR_ARG;
            export_graph(which == 1 ? m.b2.brep : m.b1.brep, which == 1 ? m.g2 : m.g1, o);
        }
        else if (op == OP_EDGE_INSERT)
            run_edge_insert(rd, m, o);
        else
            run_vertices(rd, m, o);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
