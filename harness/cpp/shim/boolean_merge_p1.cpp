// SPDX-License-Identifier: Apache-2.0
// Compile renamed upstream classes to expose the file-local helpers as well as
// real SmMerge members. No algorithm is reimplemented in this shim.
#include "sc_topology_fixture.h"
#include <SmBrepData.h>
#include <SmBrepCache.h>
#include <SmCacheMgrBrep.h>
#include <SmRelation.h>
#include <SmAttribute.h>
#include <SmPrimitiveCreation.h>
#include <SmTrimmingTools.h>
#include <SmTopologyTraverser.h>
#include <SmTree.h>
#include <SmAssertArray.h>
#define SmMerge sc_mp1_Merge
#define SmMergeOptions sc_mp1_Options
#define SmTemporaryBrepTransform sc_mp1_Transform
#include "../../../upstream/source/SMLib/src/SmMerge.cpp"
#undef SmMerge
#undef SmMergeOptions
#undef SmTemporaryBrepTransform

namespace {
struct Recorder : SmNotifyCallback {
    std::vector<std::pair<const void*, int>> identities;
    std::vector<double> events;
    int identity(const void* p) const {
        if (!p) return -1;
        for (auto item : identities) if (item.first == p) return item.second;
        return -99;
    }
    SmStatus Execute(SmNotifyOperation op, SmObject* caller, SmObject* a, SmObject* b, SmObject* c) override {
        if (op == SM_NO_COPY || op == SM_NO_REG_PROPAGATION) {
            events.insert(events.end(), {(double)op, (double)identity(caller), (double)identity(a), (double)identity(b), (double)identity(c)});
            return SM_ERR; // The caller must ignore callback status.
        }
        return SM_SUCCESS;
    }
    void append(std::vector<double>& out) const {
        out.push_back(events.size()/5);
        out.insert(out.end(),events.begin(),events.end());
    }
};
struct CallbackGuard {
    SmContext& context;
    SmNotifyCallback* previous;
    CallbackGuard(Recorder& recorder) : context(const_cast<SmContext&>(the_context())), previous(context.GetUserNotifyCallback()) {
        context.SetUserNotifyCallback(&recorder);
    }
    ~CallbackGuard() { context.SetUserNotifyCallback(previous); }
};
struct Probe : sc_mp1_Merge {
    Probe() : sc_mp1_Merge(the_context()) {}
    void operands(SmBrep* a, SmBrep* b) { m_vTI.SetPrimaryBrep(a); m_vTI.SetOtherBrep(b); }
    void flags(std::vector<double>& v) {
        v.insert(v.end(), {flg(m_bCookieCutter),flg(m_bImprinting),flg(m_bBooleanPostProcess),
                          flg(m_bImprintAndClassifyFaces),flg(m_bKeepOtherBrep),(double)m_lTagID});
    }
    void lists(Graph& g, int mode, bool check_all) {
        if (mode == 0) return;
        for (SmEdge* e : g.edges) m_sIntersectionEdges.Add(e);
        for (SmVertex* v : g.vertices) m_sIntersectionVertices.Add(v);
        if (mode == 2 && !check_all) {
            // Stale entries: ignored without dereference if no live mate exists.
            m_sIntersectionEdges.Add((SmEdge*)nullptr);
            m_sIntersectionVertices.Add((SmVertex*)nullptr);
        }
    }
};
void options(std::vector<double>& v, sc_mp1_Options& o) {
    v.insert(v.end(), {flg(o.GetCookieCutter()),flg(o.GetImprinting()),flg(o.GetBooleanPostProcess()),
                      flg(o.GetImprintAndClassifyFaces()),flg(o.GetKeepOtherBrep())});
}
}

// op: 0 control, 1 cleanup, 2 face coincidence, 3 vertex copy, 4 region attrs,
// 5 face-region lookup, 6 solid-border lookup, 7 scoped translation,
// 8 RadialSectorClassify, 9 FindRadialSector across independent operands.
// a/b/c are operation selectors; fixture bytes are validated by the safe wrapper.
SC_EXPORT int32_t sc_boolean_merge_p1_eval(int32_t op, int32_t a, int32_t b, int32_t c,
    const char* path, const double* in, int64_t n,
    const char* other_path, const double* other_in, int64_t other_n,
    int32_t edit_tolerances, double primary_tolerance, double other_tolerance, double parameter,
    double* out, int64_t cap, int64_t* len) {
    if (op < 0 || op > 9 || a < 0 || a > 31 || b < 0 || b > 31 || c < 0 || c > 31) return SC_ERR_ARG;
    SC_TRY
    std::vector<double> record;
    Probe merge;
    if (op == 0) {
        merge.flags(record);
        sc_mp1_Options o(a&1,a&2,a&4,a&8,a&16);
        sc_mp1_Options copy(o);
        options(record, copy);
        merge.SetMergeOptions(b ? &o : nullptr);
        merge.flags(record);
        merge.SetCookieCutter(c&1); merge.SetImprinting(c&2); merge.SetBooleanPostProcess(c&4);
        merge.SetClassifyFaces(c&8); merge.SetKeepOtherBrep(c&16);
        merge.flags(record);
        return sc_copy_out(record.data(),record.size(),out,cap,len);
    }
    if (in == nullptr || n == 0 || other_in == nullptr || other_n == 0) return SC_ERR_ARG;
    Rd rd{in,0}, rd2{other_in,0};
    Built primary, other;
    SmStatus build_status;
    primary.brep = build(rd,path,build_status);
    if (!primary.brep) return SC_ERR_ARG;
    other.brep = build(rd2,other_path,build_status);
    if (!other.brep) return SC_ERR_ARG;
    if (rd.at != n || rd2.at != other_n) return SC_ERR_ARG;
    Graph g,h; walk(primary.brep,g); walk(other.brep,h);
    merge.operands(primary.brep,other.brep);
    SmStatus status = SM_SUCCESS;
    if (op == 1) {
        merge.lists(g,b,a!=0);
        if (b == 1) {
            for (size_t i=0; i<g.edges.size() && i<h.edges.size(); ++i) merge.GetTopologyIntersector().Relate(g.edges[i],h.edges[i]);
            for (size_t i=0; i<g.vertices.size() && i<h.vertices.size(); ++i) merge.GetTopologyIntersector().Relate(g.vertices[i],h.vertices[i]);
        }
        status = merge.BooleanPostProcess(a ? TRUE : FALSE);
    } else if (op == 2) {
        if (g.faces.empty() || h.faces.empty()) return SC_ERR_ARG;
        if (b) for (size_t i=0; i<g.edges.size() && i<h.edges.size(); ++i)
            if (b != 2 || i != 0) merge.GetTopologyIntersector().Relate(g.edges[i],h.edges[i]);
        SmTArray<SmFace*> aa,bb; SmBoolean changed = TRUE;
        status = merge.FixFoundFaceCoincidence(g.faces[a%g.faces.size()],h.faces[c%h.faces.size()],aa,bb,changed);
        record.push_back(flg(changed)); record.push_back(aa.GetSize());
        for (ULONG i=0;i<aa.GetSize();++i) record.push_back(g.find(3,aa[i]));
        record.push_back(bb.GetSize()); for (ULONG i=0;i<bb.GetSize();++i) record.push_back(h.find(3,bb[i]));
    } else if (op == 3) {
        if (g.vertices.empty() || h.vertices.empty() || g.regions.empty()) return SC_ERR_ARG;
        SmVertex* src = a == 0 ? nullptr : h.vertices[c%h.vertices.size()];
        SmVertex* dst = g.vertices[c%g.vertices.size()];
        if (src) {
            src->SetAllFlags(16); dst->SetAllFlags(1);
            src->AddAttribute(new (the_context()) SmLongAttribute(555,29));
            if (b) merge.GetTopologyIntersector().Relate(dst,src);
        }
        // Relate itself normalises tolerances. Override AFTER it, so this tests
        // MergeVertex's independent tolerance update instead of fixture setup.
        if (edit_tolerances) {
            dst->SetTolerance(primary_tolerance, FALSE);
            if (src) src->SetTolerance(other_tolerance, FALSE);
        }
        Recorder recorder; recorder.identities={{src,1},{dst,2},{primary.brep,3},{other.brep,4}};
        CallbackGuard callback(recorder);
        SmVertex* result = nullptr;
        status = merge.MergeVertex(src,a == 2 ? nullptr : g.regions[0],result);
        Graph fresh; walk(primary.brep,fresh);
        record.push_back(fresh.find(9,result));
        if (result) {
            record.push_back(result->GetAllFlags());
            SmLongAttribute* at = (SmLongAttribute*)result->FindAttribute(555);
            record.push_back(at ? at->GetValue() : -1);
        }
        recorder.append(record);
    } else if (op == 4) {
        if (g.regions.empty() || h.regions.empty()) return SC_ERR_ARG;
        SmRegion* r=g.regions[0]; r->SetIsVoid(a ? TRUE : FALSE);
        r->AddAttribute(new (the_context()) SmLongAttribute(555,29));
        if (b) r->AddAttribute(new (the_context()) SmColorAttribute(SM_AI_COLOR,SmVector3d(0.2,0.4,0.6)));
        if (c) r->AddAttribute(new (the_context()) SmMergeRegionAttribute(r,0,h.regions[0],1));
        Recorder recorder; recorder.identities={{r,1}};
        SmMergeRegionAttribute* mr=(SmMergeRegionAttribute*)r->FindAttribute(SM_AI_OTHER_REGION_ID);
        if (mr) { recorder.identities.push_back({&mr->GetThisRegions(),5}); recorder.identities.push_back({&mr->GetOtherRegions(),6}); }
        CallbackGuard callback(recorder);
        status = merge.PropagateOtherSrcRegionAttribs(SM_BO_UNION);
        record.push_back(r->FindAttribute(555) ? 1 : 0);
        record.push_back(r->FindAttribute(SM_AI_COLOR) ? 1 : 0);
        record.push_back(r->FindAttribute(SM_AI_OTHER_REGION_ID) ? 1 : 0);
        recorder.append(record);
    } else if (op == 5) {
        if (h.faceuses.empty()) return SC_ERR_ARG;
        if (b) for (size_t i=0;i<g.edges.size() && i<h.edges.size();++i)
            merge.GetTopologyIntersector().Relate(g.edges[i],h.edges[i]);
        SmRegion* result=sm_FindBrepRegionOfOtherFace(h.faceuses[a%h.faceuses.size()],merge.GetTopologyIntersector());
        record.push_back(g.find(0,result));
    } else if (op == 6) {
        if (g.edges.empty()) return SC_ERR_ARG;
        record.push_back(g.find(1,sm_EdgeBordersSolid(g.edges[a%g.edges.size()])));
    } else if (op == 8 || op == 9) {
        if (g.edgeuses.empty() || h.edgeuses.empty()) return SC_ERR_ARG;
        SmEdgeuse* sec = g.edgeuses[a%g.edgeuses.size()];
        SmEdgeuse* cls = h.edgeuses[b%h.edgeuses.size()];
        SmEdge* edge = sec->GetEdge();
        auto has_surface = [](SmEdgeuse* eu) {
            return eu && eu->IsLoopEdgeuse() && eu->GetFace() && eu->GetFace()->GetSurface();
        };
        if (!edge || !edge->GetCurve() || !cls->GetEdge() || !cls->GetEdge()->GetCurve()) return SC_ERR_ARG;
        if (op == 8) {
            if (!has_surface(sec) || !has_surface(sec->GetRadial()) || !has_surface(cls)) return SC_ERR_ARG;
            ULONG result = 99;
            status = edge->RadialSectorClassify(parameter,sec,cls,result,NULL);
            if (status == SM_SUCCESS) record.push_back(result);
        } else {
            if (!edge->IsWire()) {
                SmTArray<SmEdgeuse*> uses; edge->GetEdgeuses(uses);
                for (ULONG i=0;i<uses.GetSize();++i)
                    if (!has_surface(uses[i]) || !has_surface(uses[i]->GetRadial()) || !has_surface(cls)) return SC_ERR_ARG;
            }
            SmEdgeuse* result=nullptr; SmFaceuse* face=nullptr;
            status = edge->FindRadialSector(cls,NULL,SM_OT_SAME,result,face,NULL);
            record.push_back(g.find(6,result)); record.push_back(g.find(2,face));
        }
    } else {
        { sc_mp1_Transform shift(&merge.GetTopologyIntersector()); }
    }
    std::vector<double> result{st(status),(double)record.size()};
    result.insert(result.end(),record.begin(),record.end());
    Graph after; walk(primary.brep,after); result.push_back(0.0); export_graph(primary.brep,after,result);
    return sc_copy_out(result.data(),result.size(),out,cap,len);
    SC_CATCH
}
