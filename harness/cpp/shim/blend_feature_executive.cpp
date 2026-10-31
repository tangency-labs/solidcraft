// SPDX-License-Identifier: Apache-2.0
// blend.feature_executive: SmFeatureExecutive (source/SMLib/src/SmFeatureExecutive.cpp) on real
// SmBrep fixtures. The executive is the real SMLib class driven by a small script (features
// added by faces, by a face and loop edges, or by a face and caller-made SmFeatureLoops, then
// DoDefeature / setters / RestoreFeatures); the B-rep construction and the raw graph export
// are the shared ones of `sc_topology_fixture.h` (read only from SMLib). Layout in
// crates/solidcore-ffi/src/units/blend_feature_executive.rs.
#include "sc_topology_fixture.h"
#include <SmFeatureExecutive.h>
#include <SmAttribute.h>
#include <memory>

namespace {

// Read access to SmFeatureLoop's private members for the export only (the standard explicit
// instantiation access rule; nothing is written).
template <typename Tag, typename Tag::type M>
struct Rob {
    friend typename Tag::type get(Tag) { return M; }
};
struct FlBase { typedef SmTArray<SmEdge*> SmFeatureLoop::*type; friend type get(FlBase); };
struct FlFeat { typedef SmTArray<SmEdge*> SmFeatureLoop::*type; friend type get(FlFeat); };
struct FlBPatch { typedef SmBrep* SmFeatureLoop::*type; friend type get(FlBPatch); };
struct FlFPatch { typedef SmBrep* SmFeatureLoop::*type; friend type get(FlFPatch); };
struct FlUseB { typedef SmBoolean SmFeatureLoop::*type; friend type get(FlUseB); };
struct FlUseF { typedef SmBoolean SmFeatureLoop::*type; friend type get(FlUseF); };
struct FlTried { typedef SmBoolean SmFeatureLoop::*type; friend type get(FlTried); };
template struct Rob<FlBase, &SmFeatureLoop::m_sBaseEdges>;
template struct Rob<FlFeat, &SmFeatureLoop::m_sFeatEdges>;
template struct Rob<FlBPatch, &SmFeatureLoop::m_pBasePatch>;
template struct Rob<FlFPatch, &SmFeatureLoop::m_pFeatPatch>;
template struct Rob<FlUseB, &SmFeatureLoop::m_bUseBPatch>;
template struct Rob<FlUseF, &SmFeatureLoop::m_bUseFPatch>;
template struct Rob<FlTried, &SmFeatureLoop::m_bTriedExtSurf>;
struct FtJoined { typedef SmBoolean SmFeature::*type; friend type get(FtJoined); };
template struct Rob<FtJoined, &SmFeature::m_bJoinedPatches>;

SmRebuildBrepType rb(int64_t v) { return (SmRebuildBrepType)v; }

int64_t index_of(const SmTArray<SmEdge*>& all, SmEdge* e) {
    for (ULONG i = 0; i < all.GetSize(); ++i)
        if (all[i] == e) return (int64_t)i;
    return -1;
}

int64_t face_count(SmBrep* b) {
    if (!b) return -1;
    SmTArray<SmFace*> f;
    b->GetFaces(f);
    return (int64_t)f.GetSize();
}

// One feature's state: the public getters, then per loop the rebuild types, extension
// distances, the numbers of its base and feature edges, whether it has patches (the same patch for both: 2), their face counts, and the
// use / tried flags.
void put_feature(std::vector<double>& o, SmFeature* f) {
    o.push_back(flg(f->GetIsPartitioned()));
    o.push_back(flg(f->GetDeatureReady()));
    o.push_back(flg(f->GetBaseRebuilt()));
    o.push_back(flg(f->GetFeatureRebuilt()));
    o.push_back(flg(f->GetBaseWasApplied()));
    o.push_back(flg(f->GetFeatureWasApplied()));
    o.push_back(flg(f->GetCanRestore()));
    SmTArray<SmFace*> faces;
    f->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    o.push_back(f->GetFeatureBrep() ? 1.0 : 0.0);
    SmTArray<SmFeatureLoop*> loops;
    f->GetFeatureLoops(loops);
    o.push_back((double)loops.GetSize());
    for (ULONG i = 0; i < loops.GetSize(); ++i) {
        SmFeatureLoop* l = loops[i];
        o.push_back((double)l->GetBaseRBType());
        o.push_back((double)l->GetFeatureRBType());
        o.push_back(l->GetBaseExtDistance());
        o.push_back(l->GetFeatureExtDistance());
        // counts only: after DoDefeature the stitch may have deleted these edges
        o.push_back((double)(l->*get(FlBase())).GetSize());
        o.push_back((double)(l->*get(FlFeat())).GetSize());
        SmBrep* bp = l->*get(FlBPatch());
        SmBrep* fp = l->*get(FlFPatch());
        o.push_back(bp ? 1.0 : 0.0);
        o.push_back(fp ? (fp == bp ? 2.0 : 1.0) : 0.0);
        o.push_back((double)face_count(bp));
        o.push_back((double)face_count(fp));
        o.push_back(flg(l->*get(FlUseB())));
        o.push_back(flg(l->*get(FlUseF())));
        o.push_back(flg(l->*get(FlTried())));
    }
}

// Evaluates `c` at five parameters of its natural interval.
void put_curve_samples(std::vector<double>& o, SmCurve* c) {
    o.push_back(c ? (double)c->GetType() : -1.0);
    if (!c) return;
    SmExtent1d d = c->GetNaturalInterval();
    o.push_back(d.GetMin());
    o.push_back(d.GetMax());
    for (int i = 0; i < 5; ++i) {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(d.Evaluate((double)i / 4), p);
        o.push_back(s == SM_SUCCESS ? 0.0 : 1.0);
        if (s == SM_SUCCESS) put_vec(o, p);
    }
}

// Evaluates `s` on a 3x3 grid of its natural UV domain.
void put_surface_samples(std::vector<double>& o, SmSurface* s) {
    o.push_back(s ? (double)s->GetType() : -1.0);
    if (!s) return;
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) {
            SmPoint2d uv(d.GetMin().x + (d.GetMax().x - d.GetMin().x) * i / 2.0,
                         d.GetMin().y + (d.GetMax().y - d.GetMin().y) * j / 2.0);
            SmPoint3d p;
            SmStatus st_ = s->EvaluatePoint(uv, p);
            o.push_back(st_ == SM_SUCCESS ? 0.0 : 1.0);
            if (st_ == SM_SUCCESS) put_vec(o, p);
        }
}

// Every edge's curve and its number of edge uses, then every face's surface, in list order
// (`topology.brep_mirror`'s record without UV trim curves).
void put_geometry(std::vector<double>& o, SmBrep* b) {
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    o.push_back((double)edges.GetSize());
    for (ULONG i = 0; i < edges.GetSize(); ++i) {
        put_curve_samples(o, edges[i]->GetCurve());
        SmTArray<SmEdgeuse*> uses;
        edges[i]->GetEdgeuses(uses);
        o.push_back((double)uses.GetSize());
    }
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    for (ULONG i = 0; i < faces.GetSize(); ++i) put_surface_samples(o, faces[i]->GetSurface());
}

// `len geometry... graph_len graph...` of `b`.
void put_brep(std::vector<double>& o, SmBrep* b) {
    std::vector<double> g;
    put_geometry(g, b);
    o.push_back((double)g.size());
    o.insert(o.end(), g.begin(), g.end());
    std::vector<double> graph = {0.0};  // the import's leading status slot
    Graph walked;
    walk(b, walked, true);
    export_graph(b, walked, graph);
    o.push_back((double)graph.size());
    o.insert(o.end(), graph.begin(), graph.end());
}

// Whether DoDefeature may extend a surface that has no extension: ExtSurfaceRebuild then
// dereferences the NULL `CreateExtendedSurface` result (an analytic surface without the
// override, e.g. a cylinder). For each loop side rebuilt by ExtendSurface or FromOtherBrep
// with no patch yet and no failed try, the source faces along the loop whose outer loop
// holds a loop edge are extended by the loop's distance (estimated as upstream when
// negative). Before the partition the source faces are taken from the base B-rep: the
// feature's own faces for the feature, the others for the base (the copies share their
// geometry). This may refuse a script where the patch of the other side would be reused.
bool null_extension(const SmContext& ctx, SmFeature* f) {
    SmTArray<SmFace*> ffaces;
    f->GetFaces(ffaces);
    bool part = f->GetIsPartitioned() != FALSE;
    SmTArray<SmFeatureLoop*> loops;
    f->GetFeatureLoops(loops);
    for (ULONG li = 0; li < loops.GetSize(); ++li) {
        SmFeatureLoop* l = loops[li];
        if (l->*get(FlTried())) continue;
        for (int side = 0; side < 2; ++side) {
            bool base = side == 0;
            SmRebuildBrepType t = base ? l->GetBaseRBType() : l->GetFeatureRBType();
            if (t != SM_RB_EXTEND_SURFACE && t != SM_RB_FROM_OTHER_BREP) continue;
            if (base ? l->*get(FlBPatch()) : l->*get(FlFPatch())) continue;
            bool base_src = base ^ (t == SM_RB_FROM_OTHER_BREP);
            double d = base_src ? l->GetBaseExtDistance() : l->GetFeatureExtDistance();
            const SmTArray<SmEdge*>& es =
                (base_src || !part) ? l->*get(FlBase()) : l->*get(FlFeat());
            if (d < 0.0) {
                SmExtent3d box;
                for (ULONG i = 0; i < es.GetSize(); ++i) {
                    SmExtent3d one;
                    es[i]->CalculateBoundingBox(&one);
                    box.Union(one, box);
                }
                d = 2. * box.GetMaxLength();
            }
            SmTArray<SmFace*> fs;
            for (ULONG i = 0; i < es.GetSize(); ++i) {
                SmTArray<SmFace*> lf;
                es[i]->GetFaces(lf);
                fs.AppendUnique(lf);
            }
            for (ULONG i = 0; i < fs.GetSize(); ++i) {
                SmFace* face = fs[i];
                if (!part && (ffaces.IsIn(face) != FALSE) == base_src) continue;
                SmTArray<SmEdge*> outer, common;
                face->GetEdges(outer, NULL, TRUE);
                outer.FindCommonElements(es, common);
                if (common.GetSize() == 0) continue;
                SmSurface* ns = NULL;
                face->GetSurface()->CreateExtendedSurface(ctx, d, SM_CT_G1, ns);
                if (!ns) return true;
                delete ns;
            }
        }
    }
    return false;
}
// Whether DoDefeature would read a loop edge that is no longer in its B-rep: the
// StitchAndOrient after an applied patch may delete loop edges. A later BuildPatches reads
// the loop edges of every loop it builds a patch for (BuildPatch's source edges, and
// JoinPatches' merged loops), and with joined patches reused for the feature the feature
// edges of every loop (ValidatePatch). Only the pointers are compared.
bool stale_edges(SmFeatureExecutive& ex, SmBrep* b) {
    SmTArray<SmFeature*> fs;
    ex.GetFeatures(fs);
    SmTArray<SmEdge*> be;
    b->GetEdges(be);
    for (ULONG i = 0; i < fs.GetSize(); ++i) {
        SmFeature* f = fs[i];
        if (!f->GetIsPartitioned()) continue;
        SmBrep* fb = f->GetFeatureBrep();
        SmTArray<SmEdge*> fe;
        if (fb) fb->GetEdges(fe);
        SmTArray<SmFeatureLoop*> loops;
        f->GetFeatureLoops(loops);
        bool base_build = false, feat_build = false, b_for_f = true;
        bool base_stale = false, feat_stale = false;
        for (ULONG li = 0; li < loops.GetSize(); ++li) {
            SmFeatureLoop* l = loops[li];
            if (!(l->*get(FlBPatch()))) base_build = true;
            if (!(l->*get(FlFPatch()))) feat_build = true;
            if (l->GetFeatureRBType() != SM_RB_FROM_OTHER_BREP) b_for_f = false;
            const SmTArray<SmEdge*>& lb = l->*get(FlBase());
            const SmTArray<SmEdge*>& lf = l->*get(FlFeat());
            for (ULONG j = 0; j < lb.GetSize(); ++j)
                if (!be.IsIn(lb[j])) base_stale = true;
            for (ULONG j = 0; j < lf.GetSize(); ++j)
                if (!fe.IsIn(lf[j])) feat_stale = true;
        }
        bool validate_joined = (f->*get(FtJoined())) && b_for_f;
        if (base_build && (base_stale || feat_stale)) return true;
        if (validate_joined && feat_stale) return true;
        if (!validate_joined && feat_build && (base_stale || feat_stale)) return true;
    }
    return false;
}
// The numbers of faces and edges of `b` that carry an attribute of id `id`.
void put_attribute_counts(std::vector<double>& o, SmBrep* b, ULONG id) {
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    b->GetFaces(faces);
    b->GetEdges(edges);
    double nf = 0, ne = 0;
    for (ULONG i = 0; i < faces.GetSize(); ++i) if (faces[i]->FindAttribute(id)) nf += 1;
    for (ULONG i = 0; i < edges.GetSize(); ++i) if (edges[i]->FindAttribute(id)) ne += 1;
    o.push_back(nf);
    o.push_back(ne);
}
// Number of features whose RestoreFeature would succeed.
int64_t restorable(SmFeatureExecutive& ex) {
    SmTArray<SmFeature*> fs;
    ex.GetFeatures(fs);
    int64_t n = 0;
    for (ULONG i = 0; i < fs.GetSize(); ++i)
        if (fs[i]->GetCanRestore()) ++n;
    return n;
}

}  // namespace

// Builds the fixture B-rep from `input` (the shared B-rep block), then runs `script` on an
// SmFeatureExecutive over it (layout in the FFI module). Output: `record_len record...` then
// the base B-rep (`put_brep`) and, per feature of GetFeatureBreps, `0` (none) or `1`
// `put_brep`. A failed fixture build returns `-999 status`; an index past the fixture's faces
// or edges `-998`; a RestoreFeatures with two or more restorable features `-997` (upstream
// removes the wrong entry after the first and keeps a deleted feature); a DoDefeature
// that would read a loop edge an applied patch's stitch deleted `-996`; a DoDefeature that
// may extend a surface without an extension `-995` (NULL dereference in ExtSurfaceRebuild).
SC_EXPORT int32_t sc_blend_feature_executive_eval(const char* path, const double* input, int64_t n,
        const double* script, int64_t ns, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 1 || !path || !out_len || !script || ns < 2) return SC_ERR_ARG;
    SC_TRY
    Rd rd{input, 0};
    SmStatus status = SM_SUCCESS;
    Built built;
    built.brep = build(rd, path, status);
    if (!built.brep || status != SM_SUCCESS) {
        std::vector<double> o = {-999.0, st(status)};
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmBrep* b = built.brep;
    const SmContext& ctx = the_context();
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    b->GetFaces(faces);
    b->GetEdges(edges);
    std::vector<double> bad = {-998.0};
    Rd sc{script, 0};
    auto face_at = [&](int64_t i) -> SmFace* {
        return (i >= 0 && i < (int64_t)faces.GetSize()) ? faces[(ULONG)i] : nullptr;
    };
    auto edge_at = [&](int64_t i) -> SmEdge* {
        return (i >= 0 && i < (int64_t)edges.GetSize()) ? edges[(ULONG)i] : nullptr;
    };
    std::vector<double> record;
    std::vector<SmFeatureLoop*> made_loops;
    struct LoopsClean {
        std::vector<SmFeatureLoop*>& v;
        ~LoopsClean() { for (auto* l : v) delete l; }
    } loops_clean{made_loops};
    // attributes: kind (0 face, 1 edge) index id behavior -> SmLongAttribute(id, 7, behavior)
    std::vector<ULONG> attr_ids;
    int64_t n_attrs = sc.idx();
    for (int64_t ai = 0; ai < n_attrs; ++ai) {
        int64_t kind = sc.idx();
        int64_t index = sc.idx();
        ULONG id = (ULONG)sc.idx();
        SmAttributeBehaviorType behavior = (SmAttributeBehaviorType)sc.idx();
        SmAObject* obj = kind == 0 ? (SmAObject*)face_at(index) : (SmAObject*)edge_at(index);
        if (!obj) return sc_copy_out(bad.data(), 1, out, cap, out_len);
        obj->AddAttribute(new (ctx) SmLongAttribute(id, 7, behavior));
        attr_ids.push_back(id);
    }
    SmFeatureExecutive ex(ctx, *b);
    int64_t n_features = sc.idx();
    for (int64_t fi = 0; fi < n_features; ++fi) {
        int64_t kind = sc.idx();
        SmRebuildBrepType base_rb = rb(sc.idx());
        SmRebuildBrepType feat_rb = rb(sc.idx());
        SmFeature* made = nullptr;
        if (kind == 0) {
            int64_t nf = sc.idx();
            SmTArray<SmFace*> fs;
            for (int64_t i = 0; i < nf; ++i) {
                SmFace* f = face_at(sc.idx());
                if (!f) return sc_copy_out(bad.data(), 1, out, cap, out_len);
                fs.Add(f);
            }
            made = ex.AddFeature(fs, base_rb, feat_rb);
        } else if (kind == 1) {
            SmFace* f = face_at(sc.idx());
            int64_t ne = sc.idx();
            SmTArray<SmEdge*> es;
            for (int64_t i = 0; i < ne; ++i) {
                SmEdge* e = edge_at(sc.idx());
                if (!e) return sc_copy_out(bad.data(), 1, out, cap, out_len);
                es.Add(e);
            }
            if (!f) return sc_copy_out(bad.data(), 1, out, cap, out_len);
            made = ex.AddFeature(f, es, base_rb, feat_rb);
        } else {
            SmFace* f = face_at(sc.idx());
            int64_t nl = sc.idx();
            SmTArray<SmFeatureLoop*> ls;
            for (int64_t li = 0; li < nl; ++li) {
                SmRebuildBrepType lb = rb(sc.idx());
                SmRebuildBrepType lf = rb(sc.idx());
                int64_t ne = sc.idx();
                SmTArray<SmEdge*> es;
                for (int64_t i = 0; i < ne; ++i) {
                    SmEdge* e = edge_at(sc.idx());
                    if (!e) return sc_copy_out(bad.data(), 1, out, cap, out_len);
                    es.Add(e);
                }
                SmFeatureLoop* l = new SmFeatureLoop(es, lb, lf);
                made_loops.push_back(l);
                ls.Add(l);
            }
            if (!f) return sc_copy_out(bad.data(), 1, out, cap, out_len);
            made = ex.AddFeature(f, ls);
        }
        record.push_back(made ? 1.0 : 0.0);
        // the loops' base edges in their sorted order, as indices of GetEdges
        if (made) {
            SmTArray<SmFeatureLoop*> loops;
            made->GetFeatureLoops(loops);
            record.push_back((double)loops.GetSize());
            for (ULONG i = 0; i < loops.GetSize(); ++i) {
                const SmTArray<SmEdge*>& be = loops[i]->*get(FlBase());
                record.push_back((double)be.GetSize());
                for (ULONG j = 0; j < be.GetSize(); ++j)
                    record.push_back((double)index_of(edges, be[j]));
            }
        }
    }
    // ops: 0 DoDefeature, 1 SetBaseRBType(t), 2 SetFeatureRBType(t), 3 SetBaseExtDist(d),
    // 4 SetFeatureExtDist(d), 5 RestoreFeatures
    int64_t n_ops = sc.idx();
    for (int64_t oi = 0; oi < n_ops; ++oi) {
        int64_t op = sc.idx();
        if (op == 0) {
            if (stale_edges(ex, b)) {
                std::vector<double> ub = {-996.0};
                return sc_copy_out(ub.data(), 1, out, cap, out_len);
            }
            SmTArray<SmFeature*> pre;
            ex.GetFeatures(pre);
            for (ULONG i = 0; i < pre.GetSize(); ++i)
                if (null_extension(ctx, pre[i])) {
                    std::vector<double> ub = {-995.0};
                    return sc_copy_out(ub.data(), 1, out, cap, out_len);
                }
            SmBrep* pb = nullptr;
            SmTArray<SmBrep*> fbs;
            SmStatus s = ex.DoDefeature(pb, &fbs);
            record.push_back(st(s));
            record.push_back(pb == b ? 1.0 : 0.0);
            record.push_back((double)fbs.GetSize());
        } else if (op == 1) {
            ex.SetBaseRBType(rb(sc.idx()));
        } else if (op == 2) {
            ex.SetFeatureRBType(rb(sc.idx()));
        } else if (op == 3) {
            ex.SetBaseExtDist(sc.num());
        } else if (op == 4) {
            ex.SetFeatureExtDist(sc.num());
        } else {
            if (restorable(ex) >= 2) {
                std::vector<double> ub = {-997.0};
                return sc_copy_out(ub.data(), 1, out, cap, out_len);
            }
            record.push_back(st(ex.RestoreFeatures()));
        }
    }
    SmTArray<SmFeature*> fs;
    ex.GetFeatures(fs);
    record.push_back((double)fs.GetSize());
    for (ULONG i = 0; i < fs.GetSize(); ++i) put_feature(record, fs[i]);
    {
        SmTArray<SmBrep*> fbs;
        ex.GetFeatureBreps(fbs);
        for (ULONG id : attr_ids) {
            put_attribute_counts(record, b, id);
            for (ULONG i = 0; i < fbs.GetSize(); ++i) {
                if (fbs[i]) put_attribute_counts(record, fbs[i], id);
                else { record.push_back(-1.0); record.push_back(-1.0); }
            }
        }
    }
    std::vector<double> result = {(double)record.size()};
    result.insert(result.end(), record.begin(), record.end());
    put_brep(result, b);
    SmTArray<SmBrep*> fbs;
    ex.GetFeatureBreps(fbs);
    for (ULONG i = 0; i < fbs.GetSize(); ++i) {
        if (!fbs[i]) {
            result.push_back(0.0);
            continue;
        }
        result.push_back(1.0);
        put_brep(result, fbs[i]);
    }
    // the executive does not delete the feature B-reps
    for (ULONG i = 0; i < fbs.GetSize(); ++i) delete fbs[i];
    return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, out_len);
    SC_CATCH
}
