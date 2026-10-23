// SPDX-License-Identifier: Apache-2.0
// Geometry/trim record mirrored by crates/difftest/src/util/primitive.rs.
#pragma once
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmSurface.h>
#include <SmShell.h>
#include <SmPointClass.h>
#include <SmVertex.h>
#include <vector>

namespace primitive_record {
inline void point(std::vector<double>& out, const SmPoint3d& p) {
    out.insert(out.end(), {p.x, p.y, p.z});
}
inline void sample(std::vector<double>& out, SmStatus status, const SmPoint3d& p) {
    out.push_back(status == SM_SUCCESS ? 0.0 : (double)status);
    if(status == SM_SUCCESS) point(out, p);
}
inline void append(std::vector<double>& out, SmBrep* brep) {
    SmTArray<SmVertex*> vertices; brep->GetVertices(vertices);
    for(ULONG i=0; i<vertices.GetSize(); ++i) {
        point(out, vertices[i]->GetPoint()); out.push_back(vertices[i]->GetTolerance());
    }
    SmTArray<SmEdge*> edges; brep->GetEdges(edges);
    for(ULONG i=0; i<edges.GetSize(); ++i) {
        SmEdge* edge=edges[i]; const auto interval=edge->GetInterval();
        out.insert(out.end(), {edge->GetTolerance(), interval.GetMin(), interval.GetMax()});
        SmCurve* curve=edge->GetCurve(); out.push_back(curve ? 1.0 : 0.0);
        if(curve) for(double fraction : {0.17, 0.37, 0.83}) {
            SmPoint3d p; auto status=curve->EvaluatePoint(interval.Evaluate(fraction), p);
            sample(out, status, p);
        }
        SmTArray<SmEdgeuse*> uses; edge->GetEdgeuses(uses); ULONG trims=0;
        for(ULONG j=0; j<uses.GetSize(); ++j) trims += uses[j]->GetUVTrimCurvePointer() != nullptr;
        out.push_back((double)uses.GetSize()); out.push_back((double)trims);
    }
    SmTArray<SmFace*> faces; brep->GetFaces(faces);
    for(ULONG i=0; i<faces.GetSize(); ++i) {
        SmFace* face=faces[i]; const auto domain=face->GetUVDomain();
        out.insert(out.end(), {face->GetTolerance(), domain.GetMin().x, domain.GetMin().y,
            domain.GetMax().x, domain.GetMax().y});
        SmSurface* surface=face->GetSurface(); out.push_back(surface ? 1.0 : 0.0);
        if(surface) for(auto uv : {SmPoint2d(0.17,0.37), SmPoint2d(0.37,0.61), SmPoint2d(0.83,0.23)}) {
            SmPoint3d p; auto status=surface->EvaluatePoint(domain.Evaluate(uv.x,uv.y), p);
            sample(out, status, p);
        }
    }
}
inline void pyramid_rays(std::vector<double>& out, SmBrep* brep) {
    SmExtent3d bounds; SmTArray<SmVertex*> vertices; brep->GetVertices(vertices);
    for(ULONG i=0; i<vertices.GetSize(); ++i) bounds.AddPoint3d(vertices[i]->GetPoint());
    const auto size=bounds.GetMax()-bounds.GetMin();
    const auto mid=(bounds.GetMin()+bounds.GetMax())*0.5;
    SmPoint3d starts[3] = {bounds.GetMax()+size*1000.0,
        SmPoint3d(mid.x,mid.y,bounds.GetMin().z+size.z*0.25),
        SmPoint3d(mid.x,mid.y,bounds.GetMax().z+size.z)};
    SmVector3d directions[3] = {SmVector3d(1,0,0),SmVector3d(1,0,0),SmVector3d(0,0,-1)};
    SmTArray<SmShell*> shells; brep->GetShells(shells);
    if(shells.GetSize()==0) return;
    for(int i=0; i<3; ++i) {
        SmRegion* region=nullptr; SmPointClassification pc(1e-5,brep->GetContext());
        auto status=shells[0]->RayFire(starts[i],directions[i],1e-5,region,pc);
        ULONG trims=0; SmTArray<SmEdge*> edges; brep->GetEdges(edges);
        for(ULONG j=0; j<edges.GetSize(); ++j) {
            SmTArray<SmEdgeuse*> uses; edges[j]->GetEdgeuses(uses);
            for(ULONG m=0; m<uses.GetSize(); ++m) trims += uses[m]->GetUVTrimCurvePointer()!=nullptr;
        }
        out.insert(out.end(), {status==SM_SUCCESS?0.0:(double)status,region?1.0:0.0,
            (double)pc.GetPointClass(),(double)trims});
    }
}
}
