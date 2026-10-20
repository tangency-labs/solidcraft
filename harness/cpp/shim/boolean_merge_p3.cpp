// SPDX-License-Identifier: Apache-2.0
// Real SmMerge entry points on independently constructed B-reps. Stable input
// identities and full raw graph snapshots are exported only AFTER each call.
#include "sc_topology_fixture.h"
#include "sc_sm.h"
#include <SmMerge.h>
#include <SmApiPrimitives.h>
#include <SmApiGeneral.h>
#include <cmath>
#include <algorithm>
#include <set>
#include <stdexcept>

namespace {
struct Bad : std::runtime_error { Bad() : std::runtime_error("invalid merge program") {} };
struct Reader {
    const double* in; int64_t n, at=0;
    double number() { if(at>=n) throw Bad(); return in[at++]; }
    int64_t integer(int64_t lo, int64_t hi) {
        double v=number();
        if(!std::isfinite(v) || v<lo || v>hi || v!=std::floor(v)) throw Bad();
        return static_cast<int64_t>(v);
    }
};
struct MergeAccess : SmMerge {
    MergeAccess(const SmContext& c, SmBrep* a, SmBrep* b, SmMergeOptions* options)
        : SmMerge(c,a,b,0.0,0.0,options) {}
    bool manifold() const { return m_bManifoldBoolean!=FALSE; }
    ULONG lamina() { return m_vTI.m_lIntersectLaminaEdges; }
};
// Family 6: the context-only SmMerge with explicitly set members, as the
// boolean_merge_p4 shim's Access, for the Rust adapter's adopted-engine path.
// It holds at most one operand, so ManifoldBoolean returns at its NER checks.
struct EngineAccess : SmMerge {
    EngineAccess(const SmContext& c, SmBrep* a, SmBrep* b, SmMergeOptions* options) : SmMerge(c) {
        m_vTI.SetPrimaryBrep(a); m_vTI.SetOtherBrep(b);
        m_vTI.SetThisApproxTol3d(1e-9); m_vTI.SetThisAngTolRad(0.3);
        m_bManifoldBoolean = TRUE;
        SetMergeOptions(options);
    }
    ~EngineAccess() { m_vTI.SetPrimaryBrep(nullptr); m_vTI.SetOtherBrep(nullptr); }
    bool manifold() const { return m_bManifoldBoolean!=FALSE; }
    ULONG lamina() { return m_vTI.m_lIntersectLaminaEdges; }
};
void put(SmBrep* b, const std::vector<SmBrep*>& originals, std::vector<double>& out) {
    auto it=std::find(originals.begin(),originals.end(),b);
    out.push_back(it==originals.end() ? -1.0 : static_cast<double>(it-originals.begin()));
    if(b) { Graph graph; walk(b,graph,true); std::vector<double> raw;
        export_graph(b,graph,raw); out.push_back(raw.size()); out.insert(out.end(),raw.begin(),raw.end()); }
    else out.push_back(0);
}
}
SC_EXPORT int32_t sc_boolean_merge_p3_eval(const double* input, int64_t n,
    double* out, int64_t cap, int64_t* len) {
    if(!input || n<0 || !out || !len || cap<0) return SC_ERR_ARG;
    SC_NATIVE_LOCK;
    try {
        Reader rd{input,n};
        int family=rd.integer(0,6); int operation=rd.integer(-64,64);
        int mask=rd.integer(0,31); bool have_options=rd.integer(0,1)!=0;
        int na=rd.integer(0,16), nb=rd.integer(0,16);
        if(na+nb>16 || ((family==3 || family==4) && (na!=1 || nb!=1))) throw Bad();
        std::vector<SmPoint3d> points;
        std::vector<std::vector<double>> sizes;
        for(int i=0;i<na+nb;++i) {
            int kind=rd.integer(0,1);
            double x=rd.number(), y=rd.number(), z=rd.number();
            if(!std::isfinite(x)||!std::isfinite(y)||!std::isfinite(z)) throw Bad();
            points.emplace_back(x,y,z);
            std::vector<double> dimensions;
            if(kind==1) for(int j=0;j<3;++j) { double d=rd.number(); if(!std::isfinite(d)||d<=0) throw Bad(); dimensions.push_back(d); }
            sizes.push_back(dimensions);
        }
        // UB-190: boxes have multiple surfaces and reuse the other operand
        // after imprinting deletes it. Shell vertices have no surface loop.
        if(family==4 && !sizes[na].empty() && have_options && (mask&2) && !(mask&16)) throw Bad();
        int nt=rd.integer(0,128); SmTArray<long> tree;
        std::set<int> used;
        std::vector<bool> stack; bool active=true;
        for(int i=0;i<nt;++i) {
            long node=rd.integer(-64,64);
            if(active) {
                if(node>=0) {
                    if(node>=na) throw Bad();
                    stack.push_back(used.insert(node).second);
                } else if(stack.size()<2) active=false;
                else {
                    bool other=stack.back(); stack.pop_back();
                    bool primary=stack.back(); stack.pop_back();
                    if(!primary || !other) throw Bad();
                    if(node < -6) active=false; else stack.push_back(true);
                }
            }
            tree.Add(node);
        }
        if(rd.at!=rd.n || (family==0 && nb!=0) || (family!=0 && nt!=0)
            || ((family==3 || family==4) && (operation<0 || operation>10))
            || (family==5 && (na!=1 || nb!=0))
            || (family==6 && (na>1 || nb>1 || na+nb>1 || operation<0 || operation>10))) throw Bad();
        // Failed consuming calls can leave live objects whose ownership is not
        // observable. Their context must outlive the call, like the API fixture
        // context used by SmApiCreateBox; a stack context leaves dangling caches.
        SmContext& context = *SmApiGetOrCreateContext();
        std::vector<SmBrep*> originals;
        for(size_t i=0;i<points.size();++i) {
            auto point=points[i];
            SmBrep* b=nullptr;
            SmStatus made=SM_ERR;
            if(sizes[i].empty()) {
                b=new(context) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,&context);
                SmShell* shell=nullptr; SmVertex* vertex=nullptr;
                made=b->MakeShellVertex(b->GetInfiniteRegion(),point,shell,vertex);
            } else {
                made=SmApiCreateBox(point,sizes[i][0],sizes[i][1],sizes[i][2],b);
            }
            if(made!=SM_SUCCESS || !b) {
                delete b; for(auto* original:originals) delete original; throw Bad();
            }
            originals.push_back(b);
        }
        SmMergeOptions options((mask&1)!=0,(mask&2)!=0,(mask&4)!=0,(mask&8)!=0,(mask&16)!=0);
        auto* opt=have_options ? &options : nullptr;
        SmTArray<SmBrep*> a,b,result;
        SmTArray<SmSurface*> empty_surfaces, result_surfaces;
        for(int i=0;i<na;++i) a.Add(originals[i]);
        for(int i=0;i<nb;++i) b.Add(originals[na+i]);
        SmBrep* single=nullptr;
        SmStatus status=SM_ERR;
        std::vector<double> values;
        if(family==5) { Graph graph; walk(originals[0],graph,true); export_graph(originals[0],graph,values);
            delete originals[0]; *len=values.size(); if(*len>cap) return SC_ERR_BUFFER;
            std::copy(values.begin(),values.end(),out); return SC_OK; }
        if(family==6) {
            {
                EngineAccess merge(context,na ? originals[0] : nullptr,nb ? originals[0] : nullptr,opt);
                status=merge.NonManifoldBoolean(static_cast<SmBooleanOperationType>(operation),single);
                values.insert(values.end(),{static_cast<double>(sc::st(status)),
                    static_cast<double>(merge.manifold()),static_cast<double>(merge.lamina()),single ? 1.0 : 0.0});
            }
            for(auto* original:originals) delete original;
            *len=static_cast<int64_t>(values.size()); if(*len>cap) return SC_ERR_BUFFER;
            std::copy(values.begin(),values.end(),out); return SC_OK;
        }
        if(family==0) status=SmMerge::BooleanTrees(a,tree,opt);
        else if(family==1) status=SmMerge::merge_breps(a,static_cast<ULONG>(operation),single,opt);
        else if(family==2) status=SmMerge::BooleanLists(&context,a,empty_surfaces,b,empty_surfaces,
            operation,result,result_surfaces,opt);
        else {
            MergeAccess merge(context,originals[0],originals[1],opt);
            if(family==3) status=merge.NonManifoldBoolean(static_cast<SmBooleanOperationType>(operation),single);
            else status=merge.PiecewiseMerge(nullptr,(mask&1)!=0,(mask&2)!=0,single);
            values.push_back(merge.manifold()); values.push_back(merge.lamina());
        }
        values.insert(values.begin(),sc::st(status));
        if(status==SM_SUCCESS) {
            if(family==0) { values.push_back(a.GetSize()); for(ULONG i=0;i<a.GetSize();++i) put(a[i],originals,values); }
            else if(family==1 || family>=3) { values.push_back(single!=nullptr); if(single) put(single,originals,values); }
            else { values.push_back(result.GetSize()); for(ULONG i=0;i<result.GetSize();++i) put(result[i],originals,values); values.push_back(result_surfaces.GetSize()); }
        }
        // Follow only outputs re-walked after the operation. Never dereference an
        // original operand pointer after a binary call may have deleted it.
        // A failed consuming binary call exposes no complete live-object set;
        // retain those unknown objects until the guard-probe child exits.
        bool tree_no_binary=family==0;
        size_t depth=0;
        for(ULONG i=0;i<tree.GetSize() && tree_no_binary;++i) {
            if(tree[i]>=0) ++depth;
            else if(depth>=2) tree_no_binary=false;
            else break;
        }
        // BooleanLists retains every original and returns independent copies.
        // Release those copies first, while any source attribute users are live.
        if(family==2) {
            std::set<SmBrep*> copied;
            for(ULONG i=0;i<result.GetSize();++i) if(result[i]) copied.insert(result[i]);
            for(auto* live:copied) delete live;
            for(ULONG i=0;i<result_surfaces.GetSize();++i) delete result_surfaces[i];
        }
        std::set<SmBrep*> cleanup;
        if(tree_no_binary || family==2 || (family==1 && (na<2 || operation<0 || operation>3))) {
            cleanup.insert(originals.begin(),originals.end());
        }
        if(status==SM_SUCCESS) {
            if(family==0) for(ULONG i=0;i<a.GetSize();++i) {if(a[i]) cleanup.insert(a[i]);}
            else if(family!=2 && single) cleanup.insert(single);
        }
        for(auto* live:cleanup) delete live;
        *len=static_cast<int64_t>(values.size());
        if(*len>cap) return SC_ERR_BUFFER;
        std::copy(values.begin(),values.end(),out);
        return SC_OK;
    } catch(...) { return SC_ERR_ARG; }
}
