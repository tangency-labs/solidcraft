// SPDX-License-Identifier: Apache-2.0
// Independent native executive inputs matching fork_smlib104_setback_corner.rs.
// Args: dx dy dz radius-x radius-y radius-z setback [mode [ease]].
// setback=-1 uses the rule. mode=0 picks the origin, 1 the opposite corner,
// 2 subtracts the upper octant and picks its concave corner (no requested setback),
// 3 picks (dx, 0, dz), the unequal-rounds dependency regression.
// Use the API's context throughout: topology marks belong to that context.
#include "StdAfx.h"
#include <SmApiPrimitives.h>
#include <SmApiGeneral.h>
#include <SmApiQueries.h>
#include <SmApiBrep.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmVertex.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmFilletExecutive.h>
#include <SmFilletCorner.h>
#include <cstdio>
#include <cstdlib>
#include <cmath>
int main(int argc,char **argv) {
 setvbuf(stdout,nullptr,_IONBF,0); if(argc<8||argc>10) return 2;
 double dx=atof(argv[1]),dy=atof(argv[2]),dz=atof(argv[3]);
 double rs[3]={atof(argv[4]),atof(argv[5]),atof(argv[6])};
 int mode=argc>8?atoi(argv[8]):0; double ease=argc>9?atof(argv[9]):0; double sb=atof(argv[7]); // -1 default; >=0 asked at origin
 const SmContext &ctx=*SmApiGetOrCreateContext(); SmBrep *b=nullptr;
 if(SmApiCreateBox(SmPoint3d(0,0,0),dx,dy,dz,b)!=SM_SUCCESS) return 3;
 if(mode==2){SmBrep*cut=nullptr;SmApiCreateBox(SmPoint3d(dx/2,dy/2,dz/2),dx/2,dy/2,dz/2,cut);SmBrep* result=nullptr;if(SmApiBooleanDifference(b,cut,result)!=SM_SUCCESS)return 5;b=result;} SmTArray<SmEdge*> all,edges; b->GetEdges(all);
 for(int axis=0;axis<3;++axis) {
  double best=1e100; SmEdge* chosen=nullptr;
  for(ULONG j=0;j<all.GetSize();++j) {
   SmVector3d p; SmExtent1d iv=all[j]->GetInterval();
   all[j]->GetCurve()->Evaluate((iv.GetMin()+iv.GetMax())/2,0,FALSE,&p);
   double dims[3]={dx,dy,dz}; double target[3]={0,0,0}; for(int a=0;a<3;++a) target[a]= mode==0 ? 0 : mode==1 ? dims[a] : mode==3 ? (a==1?0:dims[a]) : dims[a]/2; target[axis]=(mode==2?0.75:0.5)*dims[axis];
   double d=(p[0]-target[0])*(p[0]-target[0])+(p[1]-target[1])*(p[1]-target[1])+(p[2]-target[2])*(p[2]-target[2]);
   if(d<best){best=d;chosen=all[j];}
  }
  if(best>1e-10) return 4; edges.Add(chosen);
 }
 SmTArray<SmFilletSurfaceGeneratorType> xs;
 SmTArray<SmFilletSolverType> kinds; SmTArray<double> radii;
 for(int i=0;i<3;++i){xs.Add(SM_FSG_CIRCULAR);kinds.Add(SM_FS_CONST_RADIUS);radii.Add(rs[i]);}
 SmFilletExecutive exec(ctx,b);
 SmStatus st=exec.SetFilletParameters(b,edges,xs,kinds,radii,nullptr,0,0,0,0,1,1.0);
 SmVertex* origin=nullptr; SmTArray<SmVertex*> verts;b->GetVertices(verts);
 for(ULONG i=0;i<verts.GetSize();++i){auto p=verts[i]->GetPoint();if(p[0]==(mode==1?dx:0)&&p[1]==(mode==1?dy:0)&&p[2]==(mode==1?dz:0))origin=verts[i];}
 if(sb>=0){exec.AddSetBackVertex(origin);exec.SetSetBackFill(origin,sb,ease,FALSE);}
 if(st==SM_SUCCESS)st=exec.CreateFilletCorners();
 if(sb>=0){auto& cs=exec.GetFilletCorners();for(ULONG i=0;i<cs.GetSize();++i)if(cs[i]->GetFilletedVertex()==origin){cs[i]->SetSetBackDist(sb);cs[i]->SetEaseDist(ease);}}
 if(st==SM_SUCCESS)st=exec.DoFilleting();
 printf("status %d code %d fit %.17g comment %s\n",int(st),int(exec.GetFilletErrorInfo()->GetErrorCode()),exec.GetFitSetBack(),exec.GetFilletErrorInfo()->m_cComment);
 if(st==SM_SUCCESS){
  b->StitchAndOrient(); double volume=0; auto vs=SmApiBrepComputeVolume(b,1e-12,volume);
  printf("volume_status %d volume %.17g faces %lu edges %lu vertices %lu\n",int(vs),volume,(unsigned long)b->GetNumFaces(),(unsigned long)b->GetNumEdges(),(unsigned long)b->GetNumVertices());
  verts.RemoveAll();b->GetVertices(verts);
  for(ULONG i=0;i<verts.GetSize();++i){auto p=verts[i]->GetPoint();printf("v %.17g %.17g %.17g\n",p[0],p[1],p[2]);}
 }
}
