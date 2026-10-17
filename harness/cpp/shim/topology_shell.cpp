// SPDX-License-Identifier: Apache-2.0
// topology.shell: real native shell/region queries on shared B-rep fixtures.
// No native pointer crosses the C ABI; identities are traversal-order indices.
#include "sc_shim.h"
#include "support/topology_fixture.h"
#include "StdAfx.h"
#include <SmBrep.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmFaceuse.h>
#include <SmPointClass.h>
#include <cmath>
#include <SmTArray.h>
#include <SmAttribute.h>
#include <limits>
#include <algorithm>

namespace {
template<class T> double identity(const std::vector<T*>& values, T* p) {
    if (!p) return -1.0;
    auto found = std::find(values.begin(), values.end(), p);
    if (found == values.end()) throw 1;
    return (double)(found - values.begin());
}
void text(std::vector<double>& out,const TCHAR* value) {
    std::vector<double> chars;
    for (const TCHAR* c=value;c && *c;++c) chars.push_back((double)*c);
    out.push_back((double)chars.size());
    out.insert(out.end(),chars.begin(),chars.end());
}
// Mark-counter advance of a context over one call: per mark type (MARK, MARK2, MARK3,
// MARKIO, MARKASSERT) the change of the current mark, then whether it is still
// locked. The shim's context is shared by all cases, so only deltas compare.
struct MarkSnap {
    const SmContext* ctx;
    ULONG before[5];
    explicit MarkSnap(const SmContext* c) : ctx(c) {
        for (int i = 0; i < 5; ++i) before[i] = c->GetCurrentMark(kind(i));
    }
    static SmMarkType kind(int i) {
        const SmMarkType kinds[5] = {SM_MT_MARK, SM_MT_MARK2, SM_MT_MARK3, SM_MT_MARKIO, SM_MT_MARKASSERT};
        return kinds[i];
    }
    void put(std::vector<double>& out) const {
        for (int i = 0; i < 5; ++i)
            out.push_back((double)(ctx->GetCurrentMark(kind(i)) - before[i]));
        for (int i = 0; i < 5; ++i)
            out.push_back(ctx->IsMarkLocked(kind(i)) ? 1.0 : 0.0);
    }
};
template<class T> void list(std::vector<double>& out, const SmTArray<T*>& values,
                            const std::vector<T*>& identities) {
    out.push_back((double)values.GetSize());
    for (ULONG i = 0; i < values.GetSize(); ++i)
        out.push_back(identity(identities, values[i]));
}
}

SC_EXPORT int32_t sc_topology_shell_query(const char* path, const double* input,
    int64_t count, int32_t set_void, int32_t void_state,
    double* output, int64_t cap, int64_t* output_len) {
    if (!output_len || cap < 0 || (cap && !output) || (set_void != 0 && set_void != 1))
        return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path, input, count,
        [&](const sc_topology_fixture::View& fixture) -> int32_t {
            std::vector<double> out;
            out.push_back((double)fixture.shells.size());
            out.push_back((double)fixture.regions.size());
            for (SmShell* shell : fixture.shells) {
                SmTArray<SmFaceuse*> faceuses;
                shell->GetFaceuses(faceuses);
                // GetMatedShells dereferences both pointers unconditionally.
                for (ULONG i = 0; i < faceuses.GetSize(); ++i)
                    if (!faceuses[i]->GetMate() || !faceuses[i]->GetMate()->GetShell())
                        return SC_ERR_ARG;
                SmTArray<SmShell*> mates;
                const MarkSnap marks(shell->GetContext());
                if (shell->GetMatedShells(mates) != SM_SUCCESS) return SC_ERR_ARG;
                out.push_back((double)shell->GetType());
                out.push_back((double)shell->IsVertexShell());
                out.push_back((double)shell->IsWireShell());
                out.push_back((double)shell->IsFaceuseShell());
                out.push_back((double)shell->IsUnknownShell());
                out.push_back((double)shell->IsClosed());
                out.push_back(identity(fixture.regions, shell->GetRegion()));
                out.push_back(shell->GetBrep() == &fixture.brep ? 1.0 : 0.0);
                out.push_back(identity(fixture.faceuses, shell->GetFirstFaceuse()));
                out.push_back(identity(fixture.edges, shell->GetWireEdge()));
                out.push_back(identity(fixture.vertices, shell->GetVertex()));
                for (SM_TYPE type : {16013L, 16012L, 16000L, 16999L, -1L, 16010L, 16001L, 12060L, 11001L})
                    out.push_back((double)shell->IsKindOf(type));
                text(out,shell->GetTypeString());
                text(out,shell->GetClassString());
                list(out, faceuses, fixture.faceuses);
                list(out, mates, fixture.shells);
                marks.put(out);
            }
            for (SmRegion* region : fixture.regions) {
                if (set_void) region->SetIsVoid(void_state);
                out.push_back((double)region->GetType());
                out.push_back((double)region->IsVoid());
                out.push_back((double)region->IsInfiniteRegion());
                out.push_back(region->GetBrep() == &fixture.brep ? 1.0 : 0.0);
                for (SM_TYPE type : {16013L, 16012L, 16000L, 16999L, -1L, 16010L, 16001L, 12060L, 11001L})
                    out.push_back((double)region->IsKindOf(type));
                text(out,region->GetTypeString());
                text(out,region->GetClassString());
                SmTArray<SmShell*> shells;
                region->GetShells(shells);
                list(out, shells, fixture.shells);
            }
            return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, output_len);
        });
    SC_CATCH
}

SC_EXPORT int32_t sc_topology_shell_wire_edges(const char* path, const double* input,
    int64_t count, double* output, int64_t cap, int64_t* output_len) {
    if (!output_len || cap < 0 || (cap && !output)) return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path, input, count,
        [&](const sc_topology_fixture::View& fixture) -> int32_t {
            std::vector<double> out;
            out.push_back((double)fixture.shells.size());
            for (SmShell* shell : fixture.shells) {
                if (shell->IsWireShell() && !shell->GetWireEdge()) return SC_ERR_ARG;
                SmTArray<SmEdge*> wires;
                const MarkSnap marks(shell->GetContext());
                const SmStatus status = shell->GetWireEdges(wires);
                out.push_back(status == SM_SUCCESS ? 0.0 : (double)status);
                list(out, wires, fixture.edges);
                marks.put(out);
            }
            return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, output_len);
        });
    SC_CATCH
}
// Raw mutation snapshots: independent of shell operations being compared.
#include <SmContext.h>
#include <SmRegion.h>
#include <SmEdgeuse.h>
#include <SmVertexuse.h>
#include <memory>
#include <functional>

namespace transfer_detail {
struct NodePeek : SmTopology {
    static SmOwningTopology* SmTopology::*owner() { return &NodePeek::m_pListOwner; }
    static SmTopology* SmTopology::*next() { return &NodePeek::m_pNext; }
    static SmTopology* SmTopology::*last() { return &NodePeek::m_pLast; }
};
struct OwnerPeek : SmOwningTopology {
    static SmTopology* SmOwningTopology::*head() { return &OwnerPeek::m_pList; }
    static ULONG SmOwningTopology::*size() { return &OwnerPeek::m_lListSize; }
};
struct ShellPeek : SmShell {
    static SM_TYPE SmShell::*type() { return &ShellPeek::m_tShellType; }
};
struct EuPeek : SmEdgeuse {
    static SmTopology* SmEdgeuse::*owner() { return &EuPeek::m_pSorLU; }
};
struct VuPeek : SmVertexuse {
    static SmTopology* SmVertexuse::*owner() { return &VuPeek::m_pSorLUorEU; }
};
// Expose the real SmShell constructor, with its real context and region owner.
struct EmptyShell : SmShell { EmptyShell() : SmShell() {} };

struct Restore {
    std::vector<std::function<void()>> actions;
    Restore(const sc_topology_fixture::View& view, const std::vector<SmShell*>& shells) {
        for (SmShell* p : shells) {
            const auto type=p->*ShellPeek::type();
            const auto head=p->*OwnerPeek::head();
            const auto size=p->*OwnerPeek::size();
            actions.push_back([p,type,head,size] {
                p->*ShellPeek::type()=type;
                p->*OwnerPeek::head()=head;
                p->*OwnerPeek::size()=size;
            });
        }
        for (SmFaceuse* p : view.faceuses) {
            const auto owner=p->*NodePeek::owner();
            const auto next=p->*NodePeek::next();
            const auto last=p->*NodePeek::last();
            actions.push_back([p,owner,next,last] {
                p->*NodePeek::owner()=owner; p->*NodePeek::next()=next; p->*NodePeek::last()=last;
            });
        }
        for (SmEdgeuse* p : view.edgeuses) {
            const auto owner=p->*EuPeek::owner();
            actions.push_back([p,owner] { p->*EuPeek::owner()=owner; });
        }
        for (SmVertexuse* p : view.vertexuses) {
            const auto owner=p->*VuPeek::owner();
            actions.push_back([p,owner] { p->*VuPeek::owner()=owner; });
        }
    }
    ~Restore() { for (const auto& action : actions) action(); }
};

void snapshot(std::vector<double>& out, const sc_topology_fixture::View& view,
              const std::vector<SmShell*>& shells) {
    out.insert(out.end(), {(double)shells.size(), (double)view.faceuses.size(),
        (double)view.edgeuses.size(), (double)view.vertexuses.size(), (double)view.regions.size()});
    for (SmShell* p : shells) {
        SmTopology* head=p->*OwnerPeek::head();
        const SM_TYPE kind=head ? head->GetType() : 0;
        double id=-1;
        if (kind==SmFaceuse_TYPE) id=identity(view.faceuses,(SmFaceuse*)head);
        else if (kind==SmEdgeuse_TYPE) id=identity(view.edgeuses,(SmEdgeuse*)head);
        else if (kind==SmVertexuse_TYPE) id=identity(view.vertexuses,(SmVertexuse*)head);
        else if (head) throw 1;
        out.insert(out.end(), {(double)(p->*ShellPeek::type()), (double)(p->*OwnerPeek::size()),
            identity(view.regions,(SmRegion*)(p->*NodePeek::owner())), (double)kind, id});
    }
    for (SmFaceuse* p : view.faceuses) {
        out.insert(out.end(), {identity(shells,(SmShell*)(p->*NodePeek::owner())),
            identity(view.faceuses,(SmFaceuse*)(p->*NodePeek::next())),
            identity(view.faceuses,(SmFaceuse*)(p->*NodePeek::last()))});
    }
    for (SmEdgeuse* p : view.edgeuses) {
        const SM_TYPE kind=p->GetEdgeuseType();
        SmTopology* owner=p->*EuPeek::owner();
        const double id=kind==SmShell_TYPE ? identity(shells,(SmShell*)owner)
            : kind==SmLoopuse_TYPE ? identity(view.loopuses,(SmLoopuse*)owner) : -1;
        out.insert(out.end(), {(double)kind,id});
    }
    for (SmVertexuse* p : view.vertexuses) {
        const SM_TYPE kind=p->GetVertexuseType();
        SmTopology* owner=p->*VuPeek::owner();
        const double id=kind==SmShell_TYPE ? identity(shells,(SmShell*)owner)
            : kind==SmLoopuse_TYPE ? identity(view.loopuses,(SmLoopuse*)owner)
            : kind==SmEdgeuse_TYPE ? identity(view.edgeuses,(SmEdgeuse*)owner) : -1;
        out.insert(out.end(), {(double)kind,id});
    }
    for (SmRegion* p : view.regions) {
        SmTopology* head=p->*OwnerPeek::head();
        out.insert(out.end(), {(double)(p->*OwnerPeek::size()), identity(shells,(SmShell*)head)});
        std::vector<double> members;
        SmTopology* node=head;
        while (node) {
            if (members.size()>=100000 || node->GetType()!=SmShell_TYPE) throw 1;
            members.push_back(identity(shells,(SmShell*)node));
            node=node->*NodePeek::next();
            if (node==head) break;
        }
        out.push_back((double)members.size());
        out.insert(out.end(),members.begin(),members.end());
    }
}
}

SC_EXPORT int32_t sc_topology_shell_transfer(const char* path, const double* input,
    int64_t count, int32_t extra_empty, int64_t source, int64_t recipient,
    int32_t has_candidates, const int64_t* candidates, int64_t candidate_count,
    int32_t split_faceuse, const int64_t* pre, int64_t pre_count,
    double* output, int64_t cap, int64_t* output_len) {
    if (!output_len || cap<0 || (cap && !output) || extra_empty<0 || extra_empty>64
        || source<0 || recipient<0 || (has_candidates!=0 && has_candidates!=1)
        || candidate_count<0 || candidate_count>64 || (candidate_count && !candidates)
        || pre_count<0 || pre_count>8 || (pre_count && !pre)
        || (!has_candidates && candidate_count) || (split_faceuse!=0 && split_faceuse!=1)) return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path,input,count,
        [&](const sc_topology_fixture::View& view)->int32_t {
            std::vector<SmShell*> shells=view.shells;
            SmRegion* region=view.brep.GetInfiniteRegion();
            if (!region || !view.brep.GetContext()) return SC_ERR_ARG;
            for (int32_t i=0;i<extra_empty;++i) {
                std::unique_ptr<transfer_detail::EmptyShell> shell(
                    new (&view.brep) transfer_detail::EmptyShell());
                shells.push_back(shell.get());
                if (region->PostInsert(shell.get())!=SM_SUCCESS) return SC_ERR_ARG;
                shell.release();
            }
            if (split_faceuse) {
                if (view.shells.empty() || extra_empty<1) return SC_ERR_ARG;
                SmShell* original=view.shells.front();
                SmShell* extra=shells[view.shells.size()];
                SmTopology* head=original->GetList();
                if (!original->IsFaceuseShell() || original->GetSize()<2 || !head
                    || head->GetType()!=SmFaceuse_TYPE) return SC_ERR_ARG;
                // Fixture construction uses the already-existing base list
                // operations, never TransferChildrenTo under test.
                if (original->Remove(head)!=SM_SUCCESS) return SC_ERR_ARG;
                extra->*transfer_detail::ShellPeek::type()=SmFaceuse_TYPE;
                if (extra->PostInsert(head)!=SM_SUCCESS) return SC_ERR_ARG;
            }
            if (source>=(int64_t)shells.size() || recipient>=(int64_t)shells.size()) return SC_ERR_ARG;
            // GetWireEdges (reached when no candidate list is given, and by the final
            // per-shell snapshot) dereferences the first wire edge of a wire shell;
            // SmShell::GetWireEdge() is NULL for a wire shell without list head.
            const auto crashes_wire_edges=[](SmShell* shell) {
                return shell->IsWireShell() && !shell->GetWireEdge();
            };
            // Candidate edges are dereferenced (GetPrimaryEdgeuse()->m_pSorLU) only on
            // the branches that reach the ownership filter.
            const auto reads_candidates_of=[](SmShell* from,SmShell* to) {
                return !(to->GetSize()==0 && from->IsVertexShell())
                    && !(to->GetSize()!=0 && (to->IsVertexShell() || from->IsVertexShell()));
            };
            // Steps run before the compared one (no candidate list): their statuses
            // lead the snapshot. State is restored for deletion after the whole run.
            transfer_detail::Restore restore(view,shells);
            std::vector<double> pre_status;
            for (int64_t i=0;i<pre_count;++i) {
                if (pre[2*i]<0 || pre[2*i+1]<0 || pre[2*i]>=(int64_t)shells.size()
                    || pre[2*i+1]>=(int64_t)shells.size()) return SC_ERR_ARG;
                SmShell* a=shells[(size_t)pre[2*i]];
                SmShell* b=shells[(size_t)pre[2*i+1]];
                // Conservative: no native fixture reaches such a shell (see above).
                for (SmShell* other : shells)
                    if (crashes_wire_edges(other)) return SC_ERR_ARG;
                const SmStatus st=a->TransferChildrenTo(b,nullptr);
                pre_status.push_back(st==SM_SUCCESS ? 0.0 : (double)st);
            }
            SmShell* from=shells[(size_t)source];
            SmShell* to=shells[(size_t)recipient];
            const bool reads_candidates=reads_candidates_of(from,to);
            SmTArray<SmEdge*> wires;
            for (int64_t i=0;i<candidate_count;++i) {
                const auto index=candidates[i];
                if (index < -1 || index >= (int64_t)view.edges.size()) return SC_ERR_ARG;
                SmEdge* edge=index==-1 ? nullptr : view.edges[(size_t)index];
                if (reads_candidates && (!edge || !edge->GetPrimaryEdgeuse())) return SC_ERR_ARG;
                wires.Add(edge);
            }
            // The post-transfer snapshot calls GetWireEdges on every shell: `from` ends
            // Unknown; a shell other than from/to is unchanged.
            for (SmShell* other : shells)
                if (other!=from && other!=to && crashes_wire_edges(other)) return SC_ERR_ARG;
            if (!has_candidates && reads_candidates && crashes_wire_edges(from)) return SC_ERR_ARG;
            // Snapshot the result before restoring canonical links for deletion.
            // Self-transfer is compared, including its detached face-use cycle.
            const MarkSnap transfer_marks(from->GetContext());
            const auto status=from->TransferChildrenTo(to,has_candidates ? &wires : nullptr);
            std::vector<double> out{status==SM_SUCCESS ? 0.0 : (double)status};
            transfer_marks.put(out);
            transfer_detail::snapshot(out,view,shells);
            // Exercise both connected-wire traversal and global ownership
            // filtering on the actual post-transfer topology.
            for (SmShell* shell : shells) {
                SmTArray<SmEdge*> after;
                const MarkSnap marks(shell->GetContext());
                const auto query_status=shell->GetWireEdges(after);
                out.push_back(query_status==SM_SUCCESS ? 0.0 : (double)query_status);
                list(out,after,view.edges);
                marks.put(out);
            }
            out.push_back((double)pre_status.size());
            out.insert(out.end(),pre_status.begin(),pre_status.end());
            return sc_copy_out(out.data(),(int64_t)out.size(),output,cap,output_len);
        });
    SC_CATCH
}

// Checked member getters after arbitrary metadata changes. Use integer ABI so
// every SM_TYPE bit is preserved rather than passing large type codes via double.
#include <limits>
#include <array>

SC_EXPORT int32_t sc_topology_shell_type_members(const char* path, const double* input,
    int64_t count, int64_t shell_index, int64_t type_code, int64_t* output, int64_t cap) {
    if (!output || cap<11 || shell_index < -1
        || type_code < std::numeric_limits<SM_TYPE>::min()
        || type_code > std::numeric_limits<SM_TYPE>::max()) return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path,input,count,
        [&](const sc_topology_fixture::View& view)->int32_t {
            SmShell* shell=nullptr;
            if (shell_index==-1) {
                SmRegion* region=view.brep.GetInfiniteRegion();
                if (!region || !view.brep.GetContext()) return SC_ERR_ARG;
                std::unique_ptr<transfer_detail::EmptyShell> added(
                    new (&view.brep) transfer_detail::EmptyShell());
                if (region->PostInsert(added.get())!=SM_SUCCESS) return SC_ERR_ARG;
                shell=added.release();
            } else {
                if (shell_index>=(int64_t)view.shells.size()) return SC_ERR_ARG;
                shell=view.shells[(size_t)shell_index];
            }
            const SM_TYPE original=shell->*transfer_detail::ShellPeek::type();
            struct RestoreType {
                SmShell* shell; SM_TYPE original;
                ~RestoreType() { shell->*transfer_detail::ShellPeek::type()=original; }
            } restore{shell,original};
            shell->SetShellType((SM_TYPE)type_code);
            SmTopology* head=shell->GetList();
            const SM_TYPE head_kind=head ? head->GetType() : 0;
            int64_t head_id=-1;
            if (head_kind==SmFaceuse_TYPE) head_id=(int64_t)identity(view.faceuses,(SmFaceuse*)head);
            else if (head_kind==SmEdgeuse_TYPE) head_id=(int64_t)identity(view.edgeuses,(SmEdgeuse*)head);
            else if (head_kind==SmVertexuse_TYPE) head_id=(int64_t)identity(view.vertexuses,(SmVertexuse*)head);
            else if (head) return SC_ERR_ARG;
            const std::array<int64_t,11> values{{
                (int64_t)(shell->*transfer_detail::ShellPeek::type()),
                (int64_t)shell->IsVertexShell(),(int64_t)shell->IsWireShell(),
                (int64_t)shell->IsFaceuseShell(),(int64_t)shell->IsUnknownShell(),
                (int64_t)identity(view.faceuses,shell->GetFirstFaceuse()),
                (int64_t)identity(view.edges,shell->GetWireEdge()),
                (int64_t)identity(view.vertices,shell->GetVertex()),
                (int64_t)head_kind,head_id,(int64_t)shell->GetSize()
            }};
            for (size_t i=0;i<values.size();++i) output[i]=values[i];
            return SC_OK;
        });
    SC_CATCH
}

SC_EXPORT int32_t sc_topology_shell_degeneracy(const char* path,const double* input,
    int64_t count,int32_t has_tolerance,double tolerance,double* output,int64_t cap,int64_t* output_len) {
    if (!output_len || cap<0 || (cap && !output) || (has_tolerance!=0 && has_tolerance!=1)) return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path,input,count,[&](const sc_topology_fixture::View& view)->int32_t {
        std::vector<double> out{(double)view.shells.size(),(double)view.regions.size()};
        for (SmShell* shell:view.shells)
            out.push_back((double)(has_tolerance ? shell->IsDegenerate(tolerance) : shell->IsDegenerate()));
        for (SmRegion* region:view.regions)
            out.push_back((double)(has_tolerance ? region->IsDegenerate(tolerance) : region->IsDegenerate()));
        return sc_copy_out(out.data(),(int64_t)out.size(),output,cap,output_len);
    });
    SC_CATCH
}

SC_EXPORT int32_t sc_topology_shell_ray(const char* path,const double* input,int64_t count,
    const double* start,const double* direction,double tolerance,int32_t has_radius,double radius,
    double* output,int64_t cap,int64_t* output_len) {
    if (!start || !direction || !output_len || cap<0 || (cap && !output) ||
        (has_radius!=0 && has_radius!=1)) return SC_ERR_ARG;
    bool finite = std::isfinite(tolerance) && (!has_radius || std::isfinite(radius));
    for (int i=0;i<3;++i)
        finite = finite && std::isfinite(start[i]) && std::isfinite(direction[i]);
    SC_TRY
    return sc_topology_fixture::apply(path,input,count,[&](const sc_topology_fixture::View& view)->int32_t {
        std::vector<double> out;
        SmPoint3d point(start[0],start[1],start[2]);
        SmVector3d vector(direction[0],direction[1],direction[2]);
        for (SmShell* shell:view.shells) {
            SmTArray<SmFaceuse*> fus; shell->GetFaceuses(fus);
            bool boundary=false;
            for (ULONG i=0;i<fus.GetSize();++i) {
                if (!fus[i]->GetFace() || !fus[i]->GetMate()) return SC_ERR_ARG;
                if (fus[i]->GetMate()->GetShell()!=shell) boundary=true;
            }
            // Non-finite values only matter once boundary faces reach the solver;
            // an empty shape never uses them beyond the status checks compared here.
            if (boundary && !finite) return SC_ERR_ARG;
            SmRegion* region=view.regions.empty() ? nullptr : view.regions.front();
            SmPointClassification pc(SmZoneTol3d(0.125),shell->GetContext());
            pc.SetClassObject(SM_PC_POINT,nullptr);
            pc.SetGap3d(123.0); pc.SetPreSnapParam(456.0);
            const MarkSnap marks(shell->GetContext());
            SmStatus status=shell->RayFire(point,vector,SmZoneTol3d(tolerance),region,pc,
                                           has_radius ? &radius : nullptr);
            out.push_back(status==SM_SUCCESS ? 0.0 : (double)status);
            out.push_back(identity(view.regions,region));
            out.push_back((double)pc.GetPointClass());
            out.push_back(pc.GetObject()==nullptr ? 1.0 : 0.0);
            out.push_back(pc.GetTolerance());
            out.push_back(pc.GetGap3d());
            out.push_back(pc.GetPreSnapParam());
            marks.put(out);
        }
        return sc_copy_out(out.data(),(int64_t)out.size(),output,cap,output_len);
    });
    SC_CATCH
}

SC_EXPORT int32_t sc_topology_shell_region_order(const char* path,const double* input,
    int64_t count,int32_t prefix_empty,int32_t gather_faces,double* output,int64_t cap,int64_t* output_len) {
    if (!output_len || cap<0 || (cap && !output) || prefix_empty<0 || prefix_empty>8 || (gather_faces!=0 && gather_faces!=1)) return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path,input,count,[&](const sc_topology_fixture::View& view)->int32_t {
        std::vector<SmShell*> shells=view.shells;
        if(gather_faces && !view.regions.empty()) {
            SmRegion* target=view.regions.front();
            for(size_t i=1;i<view.regions.size();++i) {
                SmRegion* source=view.regions[i];SmTArray<SmShell*> members;
                source->GetShells(members);
                for(ULONG j=0;j<members.GetSize();++j) if(members[j]->IsFaceuseShell()) {
                    if(source->Remove(members[j])!=SM_SUCCESS || target->PostInsert(members[j])!=SM_SUCCESS) return SC_ERR_ARG;
                }
            }
        }
        std::vector<double> out{(double)view.regions.size()};
        for(SmRegion* region:view.regions) {
            SmShell* first=nullptr;
            for(int32_t i=0;i<prefix_empty;++i) {
                std::unique_ptr<transfer_detail::EmptyShell> extra(new (&view.brep) transfer_detail::EmptyShell());
                if(region->PostInsert(extra.get())!=SM_SUCCESS) return SC_ERR_ARG;
                if(!first) first=extra.get();
                shells.push_back(extra.get());
                extra.release(); // real region owns the new shell until B-rep destruction
            }
            if(first) region->*transfer_detail::OwnerPeek::head()=first;
            const SmStatus status=region->OrderShells();
            out.push_back(status==SM_SUCCESS ? 0.0 : (double)status);
            SmTArray<SmShell*> members;region->GetShells(members);
            list(out,members,shells);
        }
        return sc_copy_out(out.data(),(int64_t)out.size(),output,cap,output_len);
    });
    SC_CATCH
}

SC_EXPORT int32_t sc_topology_shell_region_filter(const char* path,const double* input,
    int64_t count,uint64_t attribute_id,int32_t enabled,int32_t pattern,
    double* output,int64_t cap,int64_t* output_len) {
    if (!output_len || cap<0 || (cap && !output) || (enabled!=0 && enabled!=1)
        || pattern<0 || pattern>2 || attribute_id>std::numeric_limits<ULONG>::max()) return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path,input,count,[&](const sc_topology_fixture::View& view)->int32_t {
        ULONG id=(ULONG)attribute_id;
        for(size_t i=0;i<view.shells.size();++i) {
            // These construction-only fixtures start without this attribute.
            // Fail setup rather than invent state for an unexpected collision.
            if(view.shells[i]->FindAttribute(id)) return SC_ERR_ARG;
            if(pattern==2 || (pattern==1 && i%2==0)) {
                std::unique_ptr<SmLongAttribute> tag(new (*view.brep.GetContext()) SmLongAttribute(id,123,SM_AB_COPY));
                view.shells[i]->AddAttribute(tag.get());
                tag.release();
            }
        }
        std::vector<double> out{(double)view.regions.size()};
        for(SmRegion* region:view.regions) {
            SmTArray<SmShell*> members;
            region->GetShells(members,enabled ? &id : nullptr);
            list(out,members,view.shells);
        }
        return sc_copy_out(out.data(),(int64_t)out.size(),output,cap,output_len);
    });
    SC_CATCH
}
