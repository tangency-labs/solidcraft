// SPDX-License-Identifier: Apache-2.0
// Unit-local observations for rv_next_clipped_step. Never included in the pinned
// reference. Virtual overrides only copy arguments/results and delegate once.
// No supplemental geometry evaluations or reconstruction are performed.

namespace
{
void probe_event(std::vector<double>& events, int tag, const std::vector<double>& values)
{
    events.push_back((double)tag);
    events.push_back((double)values.size());
    events.insert(events.end(), values.begin(), values.end());
}

struct ProbeCra : IxCra
{
    using IxCra::IxCra;
    std::vector<double> events;

    SmStatus PointOnPlaneSolve(const SmPoint3d& origin, const SmVector3d& normal,
                               const SmExtent2d& d0, const SmExtent2d& d1,
                               const SmVector2d& uv0, const SmVector2d& uv1,
                               SmBoolean& found, SmTsectPnt& point) override
    {
        std::vector<double> before;
        put_vec(before, origin);
        put_vec(before, normal);
        put_uv(before, uv0);
        put_uv(before, uv1);
        before.push_back(GetSurface(0)->GetOffsetDistance());
        before.push_back(GetSurface(1)->GetOffsetDistance());
        probe_event(events, 1, before);
        SmStatus result = SmConstantRadiusAssistedFS::PointOnPlaneSolve(
            origin, normal, d0, d1, uv0, uv1, found, point);
        std::vector<double> after{st(result), flg(found)};
        put_point(after, point);
        probe_event(events, 2, after);
        return result;
    }
};

struct ProbePeek : Peek
{
    ProbeCra& solver;
    ProbePeek(const SmSurface& a, const SmExtent2d& da, const SmSurface& b,
              const SmExtent2d& db, ProbeCra& fs) : Peek(a, da, b, db, fs), solver(fs) {}

    SmStatus ComputePointValues(SmPoint2d uv[2], SmTsectPnt& point,
                                SmTsectPnt* previous, double* step) override
    {
        // In the unchanged fixture this is the first call after StepSolve. Its
        // non-virtual implementation has already returned, including rdStep.
        if (previous == NULL || step == NULL)
            throw Bad();
        std::vector<double> before{*step};
        put_point(before, point);
        probe_event(solver.events, 3, before);
        SmStatus result = SmFilletIntersector::ComputePointValues(uv, point, previous, step);
        std::vector<double> after{st(result)};
        put_point(after, point);
        probe_event(solver.events, 4, after);
        return result;
    }
};

} // namespace

// Same validated program as the original entry point. This companion owns fresh
// geometry and still calls run_op -> genuine ComputeNextPoint, not a copied body.
SC_EXPORT int32_t sc_blend_fillet_intersector_probe_next(const double* in, int64_t n_in,
                                                       double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 2) return SC_ERR_ARG;
    SC_NATIVE_LOCK;
    try
    {
        SmContext ctx;
        Rd rd{in, n_in, 0};
        SmSurface* a = read_surface(rd, ctx);
        bool same = rd.at < rd.n && rd.in[rd.at] == 2.0;
        if (same) ++rd.at;
        SmSurface* b = same ? a : read_surface(rd, ctx);
        SmExtent2d d0 = rd.dom4(), d1 = rd.dom4();
        double r0 = rd.num(), r1 = rd.num();
        bool o0 = rd.flag(), o1 = rd.flag();
        double ftol = rd.num(), fang = rd.num(), ftan = rd.num();
        double ang = rd.num(), tol = rd.num();
        bool clipped = rd.flag();
        double dir = rd.num();
        if (clipped) throw Bad();
        Pts pts;
        int64_t np = rd.idx(0, 16);
        for (int64_t i = 0; i < np; ++i)
        {
            SmTsectPnt* point = new SmTsectPnt();
            pts.v.push_back(point);
            read_point(rd, *point);
        }
        SmBrep* box = NULL;
        if (SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), 1.0, 1.0, 1.0, box) != SM_SUCCESS || box == NULL)
            throw Bad();
        std::vector<double> result;
        {
            SmTArray<SmEdge*> edges;
            box->GetEdges(edges);
            if (edges.GetSize() == 0 || edges[0]->GetPrimaryEdgeuse() == NULL) throw Bad();
            ProbeCra fs(ctx, ftol, fang, ftan, r0, r1, edges[0]->GetPrimaryEdgeuse());
            SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(r0*(o0 ? 1.0 : -1.0), *a);
            SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(r1*(o1 ? 1.0 : -1.0), *b);
            s0->SetAllowExtension(TRUE); s1->SetAllowExtension(TRUE);
            fs.setup(s0, s1, r0, r1, o0 ? 1.0 : -1.0, o1 ? 1.0 : -1.0);
            ProbePeek si(*a, d0, *b, d1, fs);
            si.set(ang, tol, false, dir);
            if (rd.idx(0, OP_LAST) != OP_NEXT) throw Bad();
            std::vector<double> original{(double)OP_NEXT};
            run_op(OP_NEXT, rd, pts, si, ctx, original);
            si.put_state(original, fs);
            original.push_back((double)pts.v.size());
            for (SmTsectPnt* point : pts.v) put_point(original, *point);
            result.push_back((double)original.size());
            result.insert(result.end(), original.begin(), original.end());
            result.insert(result.end(), fs.events.begin(), fs.events.end());
            for (SmTsectPnt* point : pts.v) point->ReSet();
        }
        delete box;
        if (!same) delete b;
        delete a;
        return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, out_len);
    }
    catch (const Bad&) { return SC_ERR_ARG; }
    catch (...) { return SC_ERR_EXCEPTION; }
}
