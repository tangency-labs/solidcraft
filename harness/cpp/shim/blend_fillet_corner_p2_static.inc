// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_corner_p2`, its file-static helper of upstream
// source/SMLib/src/SmFilletCorner.cpp: sm_FindConcaveEdgeuses. Included at the end of
// blend_fillet_corner_p1.cpp, the only translation unit that compiles SmFilletCorner.cpp
// (static functions cannot be linked against); it reuses that file's B-rep helpers (build,
// walk, Graph, req, REFUSED).
//
// One entry point, sc_blend_fillet_corner_p2_find_concave(path, in, n_in, out, cap,
// out_len). Layout in crates/solidcore-ffi/src/units/blend_fillet_corner_p2.rs, which
// validates every input: topology.edgeuse_p1's B-rep block, then `v n eu*n` (graph indices,
// n >= 1). Output: `-1 status` when the B-rep could not be built, else `0` then `-2`
// (refused: a pair the loop reaches has an edge use without radial, vertex use or edge,
// which sm_CheckFaceComplimentaryCornerConcavity dereferences) or `count eu*count`.

namespace
{
void run_find_concave(Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst = SM_SUCCESS;
    SmBrep* brep = build(rd, path, bst);
    if (brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    Built owner;
    owner.brep = brep;
    Graph g;
    walk(brep, g);
    o.push_back(0.0);

    SmVertex* v = req(g.vertices, rd.idx());
    int64_t n = rd.idx();
    SmTArray<SmEdgeuse*> all;
    for (int64_t i = 0; i < n; i++)
        all.Add(req(g.edgeuses, rd.idx()));

    // Every edge use the loop's concavity tests dereference, pair by pair as the loop
    // reaches them (it stops at the first concave pair): both test edge uses and their
    // radials (vertex use, edge).
    auto usable = [](SmEdgeuse* p) { return p->GetVertexuse() != nullptr && p->GetEdge() != nullptr; };
    auto pair_ok = [&](SmEdgeuse* a, SmEdgeuse* b) {
        return a->GetRadial() != nullptr && b->GetRadial() != nullptr && usable(a) && usable(b) &&
               usable(a->GetRadial()) && usable(b->GetRadial());
    };
    SmEdgeuse* prev = all[all.GetSize() - 1];
    for (ULONG i = 0; i < all.GetSize(); i++)
    {
        SmEdgeuse* curr = all[i];
        if (!pair_ok(prev, curr))
        {
            o.push_back(REFUSED);
            return;
        }
        if (sm_CheckFaceComplimentaryCornerConcavity(v, prev, curr, NULL, 0) == TRUE)
            break;
        prev = curr;
    }

    SmTArray<SmEdgeuse*> concave;
    sm_FindConcaveEdgeuses(v, &all, concave, 0);
    o.push_back((double)concave.GetSize());
    for (ULONG i = 0; i < concave.GetSize(); i++)
        o.push_back((double)g.find(6, concave[i]));
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_corner_p2_find_concave(const char* path, const double* in, int64_t n_in,
                                                         double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_find_concave(rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
