// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `boolean.merge_p2`: the SmMerge B-rep constructor, MergeEdge,
// MakeFaceuse, MergeAddedFace and MergeAddedTopology (source/SMLib/src/SmMerge.cpp) on
// real SmBrep fixtures. The protected members are reached through a derived accessor.
// Layouts are documented, and every input validated, in
// crates/solidcore-ffi/src/units/boolean_merge_p2.rs; the checks here only keep a
// malformed call from reading past its arrays.
#include "sc_topology_fixture.h"
#include "sc_merge_fixture.h"
#include <SmMerge.h>
#include <memory>

namespace {
struct P2Access : SmMerge {
    P2Access(const SmContext& c, SmBrep* a, SmBrep* b, double tol, double ang, SmMergeOptions* o)
        : SmMerge(c, a, b, tol, ang, o) {}
    using SmMerge::MakeFaceuse;
    using SmMerge::MergeEdge;
    SmStatus relate(SmObject* a, SmObject* b) { return m_vTI.Relate(a, b); }
    void state(std::vector<double>& v) {
        v.push_back(m_vTI.GetThisApproxTol3d());
        v.push_back(m_vTI.GetThisAngTolRad());
        for (SmBoolean f : {m_bCookieCutter, m_bImprinting, m_bBooleanPostProcess,
                            m_bImprintAndClassifyFaces, m_bKeepOtherBrep})
            v.push_back(f ? 1.0 : 0.0);
        v.push_back((double)m_lTagID);
    }
};

// A bounded reader over the request integers.
struct Ints {
    const int64_t* p;
    int64_t n;
    int64_t at = 0;
    bool ok = true;
    int64_t next() {
        if (at >= n) { ok = false; return -1; }
        return p[at++];
    }
};

template <class T>
T* item(std::vector<T*>& v, int64_t i) {
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : nullptr;
}

std::unique_ptr<SmMergeOptions> options_of(int64_t mask) {
    if (mask < 0) return nullptr;
    return std::make_unique<SmMergeOptions>(
        (mask & 1) ? TRUE : FALSE, (mask & 2) ? TRUE : FALSE, (mask & 4) ? TRUE : FALSE,
        (mask & 8) ? TRUE : FALSE, (mask & 16) ? TRUE : FALSE);
}

// Relates the other operand's edges, vertices and faces to their primary mates
// (-1: none), in that order, as the fixture's intersector state.
bool relate_all(P2Access& m, Ints& in, Graph& ga, Graph& gb) {
    auto kind = [&](auto& others, auto& primaries) {
        int64_t n = in.next();
        if (!in.ok || n != (int64_t)others.size()) return false;
        for (int64_t j = 0; j < n; ++j) {
            int64_t i = in.next();
            if (!in.ok || i < -1 || i >= (int64_t)primaries.size()) return false;
            if (i >= 0 && m.relate(primaries[(size_t)i], others[(size_t)j]) != SM_SUCCESS) return false;
        }
        return true;
    };
    return kind(gb.edges, ga.edges) && kind(gb.vertices, ga.vertices) && kind(gb.faces, ga.faces);
}

int32_t finish(std::vector<int64_t>& ids, std::vector<double>& values, double* out, int64_t cap,
               int64_t* out_len) {
    std::vector<double> result{(double)ids.size()};
    for (auto id : ids) result.push_back((double)id);
    result.insert(result.end(), values.begin(), values.end());
    return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, out_len);
}

// The B-rep's graph after the op, snapshotted with a fresh (unused) mark.
void after(SmBrep* b, SmMarkType mt, std::vector<int64_t>& ids, std::vector<double>& values) {
    snapshot(b, mt, ids, values);
}
} // namespace

// op 0 constructor, 1 MergeEdge, 2 MakeFaceuse, 3 MergeAddedTopology, 4 MergeAddedFace.
// params = [dApproxTol3d, dAngleTolRadians]; ints = [options mask (-1: NULL), ...op].
SC_EXPORT int32_t sc_boolean_merge_p2_eval(
    int32_t op, const char* path_a, const double* a, int64_t na,
    const char* path_b, const double* b, int64_t nb,
    const double* params, int64_t np, const int64_t* ints, int64_t ni,
    double* out, int64_t cap, int64_t* out_len) {
    if (!path_a || !a || na < 1 || !params || np != 2 || !ints || ni < 1 || !out_len) return SC_ERR_ARG;
    if (op < 0 || op > 4) return SC_ERR_ARG;
    SC_TRY
    const SmContext& ctx = the_context();
    Ints in{ints, ni};
    auto opts = options_of(in.next());
    Built aa, bb;
    SmStatus status = SM_SUCCESS;
    aa.brep = operand(path_a, a, status);
    if (!aa.brep || status != SM_SUCCESS) return SC_ERR_ARG;
    if (nb > 0) {
        if (!path_b || !b) return SC_ERR_ARG;
        bb.brep = operand(path_b, b, status);
        if (!bb.brep || status != SM_SUCCESS) return SC_ERR_ARG;
    }
    // A computed tolerance dereferences the other B-rep.
    if (!bb.brep && params[0] < SM_EFF_ZERO) return SC_ERR_ARG;
    Graph ga; native_graph(aa.brep, ga);
    Graph gb;
    if (bb.brep) native_graph(bb.brep, gb);

    P2Access merge(ctx, aa.brep, bb.brep, params[0], params[1], opts.get());
    std::vector<int64_t> ids;
    std::vector<double> values;
    SmNewMarkAndLock lock(&ctx, 1);
    auto mt = lock.GetMarkType();

    if (op == 0) {
        if (in.at != ni) return SC_ERR_ARG;
        ids.push_back(0);
        merge.state(values);
        return finish(ids, values, out, cap, out_len);
    }
    if (op == 1 || op == 2) {
        if (!bb.brep || !relate_all(merge, in, ga, gb)) return SC_ERR_ARG;
        if (op == 1) {
            int64_t eu = in.next(), region = in.next();
            if (!in.ok || in.at != ni || eu < -1 || eu >= (int64_t)gb.edgeuses.size() || region < -1 ||
                region >= (int64_t)ga.regions.size())
                return SC_ERR_ARG;
            SmEdge* made = nullptr;
            status = merge.MergeEdge(item(gb.edgeuses, eu), item(ga.regions, region), made);
            Graph g; native_graph(aa.brep, g);
            ids.push_back(status == SM_SUCCESS ? 0 : (int64_t)status);
            ids.push_back(made ? g.find(7, made) : -1);
        } else {
            int64_t fu = in.next(), region = in.next(), want = in.next();
            if (!in.ok || in.at != ni || fu < 0 || fu >= (int64_t)gb.faceuses.size() || region < -1 ||
                region >= (int64_t)ga.regions.size() || (want != 0 && want != 1))
                return SC_ERR_ARG;
            SmRegion *old_region = nullptr, *new_region = nullptr;
            SmTArray<SmFace*> made;
            status = merge.MakeFaceuse(gb.faceuses[(size_t)fu], old_region, new_region,
                                       item(ga.regions, region), want ? &made : nullptr);
            Graph g; native_graph(aa.brep, g);
            ids.push_back(status == SM_SUCCESS ? 0 : (int64_t)status);
            ids.push_back(old_region ? g.find(0, old_region) : -1);
            ids.push_back(new_region ? g.find(0, new_region) : -1);
            ids.push_back((int64_t)made.GetSize());
            for (ULONG i = 0; i < made.GetSize(); ++i) ids.push_back(g.find(3, made[i]));
        }
        after(aa.brep, mt, ids, values);
        after(bb.brep, mt, ids, values);
        return finish(ids, values, out, cap, out_len);
    }
    if (bb.brep) return SC_ERR_ARG;
    ULONG stitched = 0;
    double gaps[4] = {0, 0, 0, 0};
    if (op == 4) {
        int64_t f = in.next();
        if (!in.ok || in.at != ni || f < 0 || f >= (int64_t)ga.faces.size()) return SC_ERR_ARG;
        status = merge.MergeAddedFace(ga.faces[(size_t)f], stitched, gaps[0], gaps[1], gaps[2], gaps[3]);
    } else {
        SmTArray<SmFace*> faces;
        SmTArray<SmEdge*> edges;
        SmTArray<SmVertex*> verts;
        auto list = [&](auto& dst, auto& src, bool& given) {
            int64_t has = in.next();
            given = has == 1;
            if (!in.ok || (has != 0 && has != 1)) return false;
            if (!given) return true;
            int64_t n = in.next();
            if (!in.ok || n < 0 || n > ni) return false;
            for (int64_t j = 0; j < n; ++j) {
                auto* x = item(src, in.next());
                if (!in.ok || !x) return false;
                dst.Add(x);
            }
            return true;
        };
        bool hf, he, hv;
        if (!list(faces, ga.faces, hf) || !list(edges, ga.edges, he) || !list(verts, ga.vertices, hv))
            return SC_ERR_ARG;
        int64_t flags[5];
        for (auto& f : flags) {
            f = in.next();
            if (!in.ok || (f != 0 && f != 1)) return SC_ERR_ARG;
        }
        if (in.at != ni) return SC_ERR_ARG;
        status = merge.MergeAddedTopology(hf ? &faces : nullptr, he ? &edges : nullptr, hv ? &verts : nullptr,
                                          stitched, gaps[0], gaps[1], gaps[2], gaps[3],
                                          // Positional, in the definition's order (bDoGluing last).
                                          flags[0] ? TRUE : FALSE, flags[1] ? TRUE : FALSE,
                                          flags[2] ? TRUE : FALSE, flags[3] ? TRUE : FALSE,
                                          flags[4] ? TRUE : FALSE);
    }
    ids.push_back(status == SM_SUCCESS ? 0 : (int64_t)status);
    ids.push_back((int64_t)stitched);
    values.insert(values.end(), gaps, gaps + 4);
    after(aa.brep, mt, ids, values);
    return finish(ids, values, out, cap, out_len);
    SC_CATCH
}
