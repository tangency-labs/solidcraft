// Two actual independent solids and actual mutually linked classification records.
static int32_t sc_p6_peer_run(int32_t mode,double* out,int64_t cap,int64_t* len) {
 SC_TRY
 if(mode<0||mode>3)return SC_ERR_ARG;
 std::unique_ptr<SmBrep> bodies[2];SmEdge* edges[2];SmVertex* starts[2];SmVertex* ends[2];SmCurveClassification cc[2];
 for(int i=0;i<2;++i){
  SmBrep* b=nullptr;if(SmApiCreateBox(SmVector3d(i*1e-7,0,0),1,1,1,b)!=SM_SUCCESS||!b)return SC_ERR_ARG;
  bodies[i].reset(b);b->m_bEditingEnabled=TRUE;SmTArray<SmEdge*> es;b->GetEdges(es);if(es.GetSize()==0)return SC_ERR_ARG;
  edges[i]=es[0];edges[i]->GetVertices(starts[i],ends[i]);if(!starts[i]||!ends[i]||!edges[i]->GetCurve())return SC_ERR_ARG;
  SmCurve* copied=nullptr;if(edges[i]->GetCurve()->Copy(*b->GetContext(),copied)!=SM_SUCCESS||!copied)return SC_ERR_ARG;
  cc[i].SetCurve(copied,TRUE);SmTol::SetZoneTol3d(&cc[i],SmZoneTol3d(1e-5));cc[i].SetSize(1);
  auto& v=cc[i][0];v.m_vInterval=edges[i]->GetInterval();
  v.m_vStart.SetClassObject(SM_PC_VERTEX,starts[i]);v.m_vStart.SetTParam(v.m_vInterval.GetMin());
  v.m_vMid.SetClassObject(SM_PC_EDGE,edges[i]);v.m_vMid.SetTParam(v.m_vInterval.Evaluate(.5));
  v.m_vEnd.SetClassObject(SM_PC_VERTEX,ends[i]);v.m_vEnd.SetTParam(v.m_vInterval.GetMax());
 }
 const SmBoolean doing=mode!=3;
 cc[0].SetFilletData(mode==0?&cc[1]:nullptr,doing);cc[1].SetFilletData(mode==0?&cc[0]:nullptr,doing);
 auto snapshot=[&](std::vector<double>& values)->bool {
  for(int i=0;i<2;++i){
   for(auto* v:{starts[i],ends[i]}){const auto p=v->GetPoint();values.insert(values.end(),{p.x,p.y,p.z});}
   auto* c=edges[i]->GetCurve();if(!c)return false;auto d=c->GetNaturalInterval(),raw=edges[i]->GetInterval();
   values.insert(values.end(),{double(c->GetType()),raw.GetMin(),raw.GetMax(),d.GetMin(),d.GetMax()});
   for(double t:{0.,.5,1.}){SmPoint3d p;if(c->EvaluatePoint(raw.Evaluate(t),p)!=SM_SUCCESS)return false;values.insert(values.end(),{p.x,p.y,p.z});}
  }return true;
 };
 std::vector<double> before;if(!snapshot(before))return SC_ERR_ARG;
 SmTArray<SmFace*> f[2];SmTArray<SmEdge*> e[2];SmTArray<SmVertex*> v[2];
 auto merge=[&](){return cc[0].MergeClassifications(cc[1],FALSE,&f[0],&f[1],&e[0],&e[1],&v[0],&v[1]);};
 SmStatus status;
 if(mode==1||mode==3){
  struct PeerScope {
   SmCurveClassification& a;SmCurveClassification& b;SmCurveClassification* old_a;SmCurveClassification* old_b;SmBoolean doing_a;SmBoolean doing_b;
   PeerScope(SmCurveClassification& first,SmCurveClassification& second):a(first),b(second),old_a(a.GetOtherClassification()),old_b(b.GetOtherClassification()),doing_a(a.GetDoingFillets()),doing_b(b.GetDoingFillets()){a.SetFilletData(&b,doing_a);b.SetFilletData(&a,doing_b);}
   ~PeerScope(){a.SetFilletData(old_a,doing_a);b.SetFilletData(old_b,doing_b);}
  } scope(cc[0],cc[1]);
  status=merge();
 }else status=merge();
 std::vector<double> result{status_code(status),double(f[0].GetSize()),double(f[1].GetSize()),double(e[0].GetSize()),double(e[1].GetSize()),double(v[0].GetSize()),double(v[1].GetSize())};
 result.insert(result.end(),before.begin(),before.end());if(!snapshot(result))return SC_ERR_ARG;
 if(mode!=0)result.insert(result.end(),{double(cc[0].GetOtherClassification()!=nullptr),double(cc[0].GetDoingFillets()),double(cc[1].GetOtherClassification()!=nullptr),double(cc[1].GetDoingFillets())});
 return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
 SC_CATCH
}

SC_EXPORT int32_t sc_intersect_curve_class_p6_peer(double* out,int64_t cap,int64_t* len){return sc_p6_peer_run(0,out,cap,len);}
SC_EXPORT int32_t sc_intersect_curve_class_p6_peer_scoped(int32_t mode,double* out,int64_t cap,int64_t* len){if(mode<1||mode>3)return SC_ERR_ARG;return sc_p6_peer_run(mode,out,cap,len);}
