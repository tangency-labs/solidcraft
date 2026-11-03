// SPDX-License-Identifier: Apache-2.0
// Preparatory smlib#106(b) count/equivalence gate. Requires SMLib 3843ddb's
// SmBlendSetback.h/.cpp; deliberately not built against the pinned reference.
#include "SmBlendSetback.h"
#include <SmContext.h>
#include <cstdio>
int main() {
 SmContext context;
 const double rr[6][3]={{2,3,4},{1,1.5,2},{2,3,4},{2,3,4},{5,5,5},{2,3,4}};
 const double ss[6][3]={{4.5,4.75,6},{2.25,2.375,3},{4.5,4.75,4.5},{4.25,4.25,4.25},{8,8,8},{4.5,4.75,6}};
 const double expected[6]={1.056,1.056,1.046,1.110,1,0};
 const ULONG expected_solves[6]={22,26,21,30,3,2};
 const unsigned long long expected_rows[6]={3594502,4569089,4542244,3257414,7044,33152};
 const unsigned long long expected_peak[6]={10000,10000,10679,11422,1950,3931};
 const unsigned long long expected_steps[6]={6537,8089,8194,7150,22,1324};
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
  SmBlendSetback::Work work; double scale=0; ULONG solves=0,attempts=0; const TCHAR* reason=nullptr;
  auto status=SmBlendSetback::CertifiedScale(context,faces,edges,setbacks,scale,&solves,&reason,most.empty()?nullptr:&most,&attempts,&work);
  printf("case %d status %d scale %.17g attempts %lu solves %lu reason %s\n",c,int(status),scale,(unsigned long)attempts,(unsigned long)solves,reason?reason:"none"); fflush(stdout);
  printf("work attempts %lu solves %lu rows %llu peak %llu steps %llu\n", (unsigned long)work.m_lAttempts,(unsigned long)work.m_lSolves,work.m_lRowsBuilt,work.m_lPeakRows,work.m_lQPSteps); fflush(stdout);
  if(work.m_lAttempts!=attempts || work.m_lSolves!=solves || solves!=expected_solves[c] || work.m_lRowsBuilt!=expected_rows[c] || work.m_lPeakRows!=expected_peak[c] || work.m_lQPSteps!=expected_steps[c]) return 2;
  if(status!=SM_SUCCESS || scale!=expected[c] || attempts!=(c==4?1:c==5?2:12) || (reason!=nullptr)!=(c==5)) return 1;
 }
}