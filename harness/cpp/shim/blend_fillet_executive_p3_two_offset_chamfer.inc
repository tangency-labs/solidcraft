// SPDX-License-Identifier: Apache-2.0
// tangency-labs/solidcore#880: an unequal (two-offset) chamfer of the rim a flat cut
// leaves on a placed cylinder, through upstream v1.0.1's SmConstantRadiusFS with
// pdOptSecondRadius. Included after blend_fillet_executive_p3_end_to_end.inc, whose
// fillet_real_solid record helpers it uses.
#include <SmFilletStandardSolver.h>
#include <SmFilletSolver.h>
#include <SmAxis2Placement.h>
#include <SmApiCurves.h>
#include <algorithm>
#include <utility>
#include <vector>

namespace fillet_two_offset_chamfer {
// Wire: cylinder radius, height (0-1); placement origin, X axis, Y axis (2-10); cutting
// box origin, size (11-16); rim z (17); derive (18: 1 setbacks, 0 offsets); a, b (19-20);
// up (21-23, read only when deriving); body (24: 0 SmApiCreateCylinder, 1 the linear sweep
// of two half-circle arcs, SolidCraft's extruded cylinder); the first arc's start angle in
// degrees (25, read for body 1); which matching rim edge (26: 0 the first, 1 the second);
// all (27: 1 chamfers every matching rim edge in one executive, each by the given offsets;
// needs offsets, derive 0, and rim 0).
constexpr int64_t WIRE=28;
// Setup outcomes recorded in place of the chamfer record.
constexpr double NO_BODY=-1.0, NO_RIM=-2.0, NO_BLEND_EDGEUSE=-3.0, NO_OFFSETS=-4.0;

// The cylinder of radius w[0] and height w[1] at the origin along +Z: SmApiCreateCylinder
// (body 0), or SmApiCreateLinearSweep (+Z, w[1], capped) of the two SmApiCreateArc half
// circles w[25] to w[25] + 180 and w[25] - 180 to w[25] degrees (body 1; the angles must
// lie within [-360, 360]). Null on failure.
SmBrep* cylinder_at_origin(const double* w) {
    SmBrep* cylinder=nullptr;
    if (w[24] == 0.0) {
        if (SmApiCreateCylinder(SmVector3d(0,0,0),w[0],w[1],cylinder) != SM_SUCCESS || !cylinder) {
            delete cylinder; return nullptr;
        }
        return cylinder;
    }
    SmBSplineCurve* a=nullptr;
    SmBSplineCurve* b=nullptr;
    const SmPoint3d o(0,0,0);
    if (SmApiCreateArc(o,w[0],w[25],w[25]+180.0,a) != SM_SUCCESS || !a
        || SmApiCreateArc(o,w[0],w[25]-180.0,w[25],b) != SM_SUCCESS || !b) {
        delete a; delete b; return nullptr;
    }
    SmTArray<SmCurve*> curves;
    curves.Add(a);
    curves.Add(b);
    // the curves are copied, not consumed
    const SmApiStatus s=SmApiCreateLinearSweep(curves,SmVector3d(0,0,1),w[1],TRUE,cylinder);
    delete a; delete b;
    if (s != SM_SUCCESS || !cylinder) { delete cylinder; return nullptr; }
    return cylinder;
}

// The cylinder (cylinder_at_origin), Transform by the placement (no scale, tolerances
// unscaled), minus SmApiCreateBox. Null on failure.
SmBrep* build(const double* w) {
    SmBrep* cylinder=cylinder_at_origin(w);
    if (!cylinder) return nullptr;
    const SmAxis2Placement placement(SmPoint3d(w[2],w[3],w[4]),SmVector3d(w[5],w[6],w[7]),
        SmVector3d(w[8],w[9],w[10]));
    if (cylinder->Transform(placement,nullptr,FALSE) != SM_SUCCESS) { delete cylinder; return nullptr; }
    SmBrep* box=nullptr;
    if (SmApiCreateBox(SmVector3d(w[11],w[12],w[13]),w[14],w[15],w[16],box) != SM_SUCCESS || !box) {
        delete cylinder; delete box; return nullptr;
    }
    SmBrep* res=nullptr;
    if (SmApiBooleanDifference(cylinder,box,res) != SM_SUCCESS || !res) {
        // Both operands remain allocated on failure.
        if (res && res != cylinder && res != box) delete res;
        delete cylinder; delete box; return nullptr;
    }
    // On success pBrep1 is the result and pBrep2 is consumed.
    if (res != cylinder) delete cylinder;
    return res;
}

// The GetEdges edge with a curve whose middle point lies within 1e-6 of z, the nth such
// (0 the first).
SmEdge* rim(SmBrep* body, double z, int64_t nth, ULONG& index) {
    SmTArray<SmEdge*> edges; body->GetEdges(edges);
    for (ULONG i=0; i<edges.GetSize(); ++i) {
        SmCurve* curve=edges[i]->GetCurve();
        if (!curve) continue;
        SmPoint3d p;
        if (curve->EvaluatePoint(edges[i]->GetInterval().Evaluate(0.5),p) != SM_SUCCESS) continue;
        if (std::fabs(p.z-z) < 1e-6 && nth-- == 0) { index=i; return edges[i]; }
    }
    return nullptr;
}

inline double dot(const SmVector3d& a, const SmVector3d& b) { return a.x*b.x+a.y*b.y+a.z*b.z; }

// SolidCraft's sides_offsets: setback s_up on the face whose face use normal leans most
// along `up`, s_other on the other; b0, b1 the binormals into the faces at the edge's
// middle, c = b0.b1, sn = sqrt(1 - c^2), r0 = (s1 - s0 c) / sn on eu's face and
// r1 = (s0 - s1 c) / sn on its radial's. False when an evaluation fails, the faces are
// tangent (sn <= 1e-6) or an offset is not positive.
bool sides_offsets(SmEdge* edge, SmEdgeuse* u0, double s_up, double s_other, const SmVector3d& up,
    double& r0, double& r1) {
    SmEdgeuse* u1=u0->GetRadial();
    if (!u1) return false;
    const double t=edge->GetInterval().Evaluate(0.5);
    SmPoint3d p0, p1; SmVector3d b0, b1, n0, n1;
    if (u0->EvaluateBinormal(t,FALSE,p0,b0,nullptr,&n0) != SM_SUCCESS) return false;
    if (u1->EvaluateBinormal(t,FALSE,p1,b1,nullptr,&n1) != SM_SUCCESS) return false;
    const double c=std::min(std::max(dot(b0,b1),-1.0),1.0);
    const double sn=std::sqrt(std::max(1.0-c*c,0.0));
    if (!(sn > 1e-6)) return false;
    double s0=s_up, s1=s_other;
    if (std::fabs(dot(n1,up)) > std::fabs(dot(n0,up))) std::swap(s0,s1);
    r0=(s1-s0*c)/sn;
    r1=(s0-s1*c)/sn;
    return r0 > 0.0 && r1 > 0.0;
}

// SetFilletParameters' per-edge set-up by hand, with the second radius it does not pass:
// a linear cross section, SmConstantRadiusFS(4 x edge tolerance, 20 and 10 degrees, r0,
// eu, &r1), loaded and then given the generator; corners created and every one a bevel;
// DoFilleting (no global merge: the executive's default). Returns the first failing
// status. The executive (and its fillet-edge attribute) is gone before the caller's
// StitchAndOrient, as the Rust session is.
// One solver per edge (with `all`, every rim edge), in order, each loaded and then given
// the one generator.
SmStatus chamfer(SmBrep* body, const std::vector<std::pair<SmEdge*,SmEdgeuse*>>& edges,
    double r0, double r1) {
    const SmContext* ctx=SmApiGetOrCreateContext();
    SmFilletExecutive exec(*ctx,body);
    SmFilletSurfaceGenerator* fsg=new (*ctx) SmLinearCrossSectionFSG;
    exec.AddFilletSurfaceGenerator(fsg);
    static constexpr double dTolFactor=4.0;
    for (const auto& [edge,eu] : edges) {
        double second=r1;
        std::unique_ptr<SmConstantRadiusFS> solver(new (*ctx) SmConstantRadiusFS(*ctx,
            dTolFactor*edge->GetTolerance(),SM_DEG2RAD(20.0),SM_DEG2RAD(10.0),r0,eu,&second));
        // A solver LoadFilletSolver refuses is not the executive's.
        SER(exec.LoadFilletSolver(solver.get()));
        SmConstantRadiusFS* loaded=solver.release();
        loaded->SetFilletSurfaceGenerator(fsg);
    }
    SER(exec.CreateFilletCorners());
    SmTArray<SmFilletCorner*>& corners=exec.GetFilletCorners();
    for (ULONG i=0; i<corners.GetSize(); ++i) corners[i]->SetBevel(TRUE);
    return exec.DoFilleting();
}
}

// Wire: fillet_two_offset_chamfer::WIRE. Output: a setup code, 0 or one of the negative
// fillet_two_offset_chamfer outcomes (then nothing else); after 0 the rim's GetEdges
// index, r0 and r1, then fillet_real_solid::fillet_and_record's record (the rim's
// before-section, the chamfer status, counts, topology, geometry and mass).
SC_EXPORT int32_t sc_blend_fillet_executive_p3_two_offset_chamfer(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    using namespace fillet_two_offset_chamfer;
    if (!input || n != WIRE) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    for (int i : {0,1,14,15,16,19,20}) if (input[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    if (input[18] != 0.0 && input[18] != 1.0) return SC_ERR_ARG;
    if (input[24] != 0.0 && input[24] != 1.0) return SC_ERR_ARG;
    if (input[26] != 0.0 && input[26] != 1.0) return SC_ERR_ARG;
    if (input[27] != 0.0 && input[27] != 1.0) return SC_ERR_ARG;
    if (input[27] == 1.0 && (input[18] != 0.0 || input[26] != 0.0)) return SC_ERR_ARG;
    SC_TRY
    std::vector<double> values;
    std::unique_ptr<SmBrep> body(build(input));
    if (!body) {
        values.push_back(NO_BODY);
        return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    }
    ULONG index=0;
    SmEdge* edge=rim(body.get(),input[17],(int64_t)input[26],index);
    SmEdgeuse* eu=edge ? edge->GetBlendEdgeuse() : nullptr;
    double r0=input[19], r1=input[20];
    const bool have_offsets=input[18] == 0.0
        || (eu && sides_offsets(edge,eu,input[19],input[20],SmVector3d(input[21],input[22],input[23]),r0,r1));
    const double setup=!edge ? NO_RIM : !eu ? NO_BLEND_EDGEUSE : !have_offsets ? NO_OFFSETS : 0.0;
    // with `all`, every other edge rim() would find after the first, in GetEdges order
    std::vector<std::pair<SmEdge*,SmEdgeuse*>> edges;
    double all_setup=0.0;
    if (setup == 0.0) {
        edges.push_back({edge,eu});
        if (input[27] == 1.0) {
            for (int64_t nth=1;; ++nth) {
                ULONG other_index=0;
                SmEdge* other=rim(body.get(),input[17],nth,other_index);
                if (!other) break;
                SmEdgeuse* other_eu=other->GetBlendEdgeuse();
                if (!other_eu) { all_setup=NO_BLEND_EDGEUSE; break; }
                edges.push_back({other,other_eu});
            }
        }
    }
    values.push_back(setup != 0.0 ? setup : all_setup);
    if (values.back() != 0.0) return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    values.insert(values.end(), {double(index),r0,r1});
    std::vector<double> before;
    const auto interval=edge->GetInterval();
    SmCurve* curve=edge->GetCurve();
    before.insert(before.end(), {double(index),interval.GetMin(),interval.GetMax(),1.0,double(curve->GetType())});
    for (double fraction : {0.17,0.37,0.83}) {
        SmPoint3d p; const auto s=curve->EvaluatePoint(interval.Evaluate(fraction),p);
        primitive_record::sample(before,s,p);
    }
    const SmStatus result=chamfer(body.get(),edges,r0,r1);
    // StitchAndOrient's status is ignored, as SmApiSetBevelCorners ignores it.
    if (result == SM_SUCCESS) body->StitchAndOrient();
    // The rim and its edge use may no longer be live; all result traversals are fresh.
    fillet_real_solid::section(values,before);
    values.push_back(fillet_real_solid::status(result));
    fillet_real_solid::counts(values,body.get());
    std::vector<double> topology_record; fillet_real_solid::topology(topology_record,body.get());
    fillet_real_solid::section(values,topology_record);
    std::vector<double> geometry; primitive_record::append(geometry,body.get());
    fillet_real_solid::section(values,geometry);
    double area,volume,mass; SmPoint3d centroid; SmVector3d inertia,products;
    const auto mass_status=SmApiBrepComputeMassProperties(body.get(),1e-4,1.0,SmPoint3d(0,0,0),area,volume,mass,centroid,inertia,products);
    values.push_back(fillet_real_solid::status(mass_status));
    if (mass_status == SM_SUCCESS) {
        values.insert(values.end(), {area,volume,mass});
        primitive_record::point(values,centroid);
        primitive_record::point(values,inertia);
        primitive_record::point(values,products);
    }
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}
