// SPDX-License-Identifier: Apache-2.0
// Focused smlib#106(a) comparison driver. Requires SMLib 20b1c01's
// SmBlendSetback.h/.cpp; deliberately not built against the pinned reference.
#include "SmBlendSetback.h"
#include <SmContext.h>
#include <cstdio>
int main() {
 SmContext context;
 const double rr[6][3]={{2,3,4},{1,1.5,2},{2,3,4},{2,3,4},{5,5,5},{2,3,4}};
 const double ss[6][3]={{4.5,4.75,6},{2.25,2.375,3},{4.5,4.75,4.5},{4.25,4.25,4.25},{8,8,8},{4.5,4.75,6}};
 const double expected[6]={1.056,1.056,1.046,1.110,1,0};
 const ULONG expected_solves[6]={35,35,39,34,2,10};
 for(int c=0;c<6;c++) {
  std::vector<SmBlendSetback::Face> faces(3);
  std::vector<SmBlendSetback::Edge> edges(3);
  for(int i=0;i<3;i++) {
   double v[3]={0,0,0}; v[i]=-1;
   faces[i].m_sPoint=SmPoint3d(0,0,0); faces[i].m_sNormal=SmVector3d(v[0],v[1],v[2]);
   v[i]=1; edges[i].m_sAway=SmVector3d(v[0],v[1],v[2]); edges[i].m_dRadius=rr[c][i];
   edges[i].m_alFaces[0]=(i+1)%3; edges[i].m_alFaces[1]=(i+2)%3;
  }
  std::vector<double> setbacks(ss[c],ss[c]+3), most;
  if(c==2) most={20,20,4.5};
  if(c==5) most=setbacks;
  double scale=0; ULONG solves=0,attempts=0; const TCHAR* reason=nullptr;
  auto status=SmBlendSetback::CertifiedScale(context,faces,edges,setbacks,scale,&solves,&reason,most.empty()?nullptr:&most,&attempts);
  printf("case %d status %d scale %.17g attempts %lu solves %lu reason %s\n",c,int(status),scale,(unsigned long)attempts,(unsigned long)solves,reason?reason:"none"); fflush(stdout);
  if(status!=SM_SUCCESS || scale!=expected[c] || solves!=expected_solves[c] || attempts!=(c==4?1:c==5?2:12) || (reason!=nullptr)!=(c==5)) return 1;
 }
}