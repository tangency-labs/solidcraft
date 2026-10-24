// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.face_p1`: the SmFace members owned by the unit
// (source/SMLib/src/SmFace.cpp, source/SMLib/inc/SmFace.h) on real SmBrep objects.
//
// One entry point, sc_topology_face_p1_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_face_p1.rs, which
// validates every input. The input is a face selector (index into the traversal-order face
// list, checked by the wrapper), the op's fixed-size arguments, then the B-rep block of
// unit topology.vertexuse (built by the shared sc_topology_fixture.h), whose graph that
// unit's Export op exports for the Rust import. As in the topology.trimming_tools shim,
// exactly exported analytic surfaces keep their dynamic kind; other surfaces and edge
// curves get the plain B-spline copies the Rust import rebuilds, keeping the owner.
//
// Output: `0` (or `-1 status` when the B-rep build failed), then the op record. A slot of
// `-2` marks a call the shim skips because upstream would dereference null; a seam call
// that can reach upstream's crash runs in a forked child, and `-4` marks one whose child
// crashed, `-3` one the shim skipped because it cannot fork (see fp1_trapped).
#include "sc_topology_fixture.h"

#include <SmCurveClass.h>
#include <SmFaceProps.h>

#ifndef _WIN32
#ifdef __linux__
#include <sys/prctl.h>
#endif
#include <sys/wait.h>
#include <unistd.h>

#include <cerrno>
#include <cstdio>
#endif

namespace
{
enum : int32_t
{
    OP_UV_DOMAIN = 1,
    OP_ORIENTED = 2,
    OP_RECT = 3,
    OP_FLIP = 4,
    OP_SET_SURFACE = 5,
    OP_LOOP_ROUND_TRIP = 6,
    OP_REMOVE_UV = 7,
    OP_SWAP_UV = 8,
    OP_SHRINK = 9,
    OP_PROBE = 10,
    OP_MOVE_SEAM = 11,
    OP_SPLIT_AT_SEAM = 12,
    OP_UV_DOMAIN_AFTER_TRIM_SHIFT = 13,
};

const double FP1_NULL_DEREF = -2.0;
// A seam call the shim skips because upstream may crash there and it cannot fork.
const double FP1_NOT_RUN = -3.0;
// A seam call whose child process crashed, hung or sent no whole record (UB-137).
const double FP1_CRASHED = -4.0;

#ifndef _WIN32

bool fp1_write_all(int fd, const void* p, size_t n)
{
    const char* c = (const char*)p;
    while (n > 0)
    {
        ssize_t w = write(fd, c, n);
        if (w <= 0)
            return false;
        c += w;
        n -= (size_t)w;
    }
    return true;
}

bool fp1_read_all(int fd, void* p, size_t n)
{
    char* c = (char*)p;
    while (n > 0)
    {
        ssize_t r = read(fd, c, n);
        if (r <= 0)
            return false;
        c += r;
        n -= (size_t)r;
    }
    return true;
}

#endif

// Runs `fn` (appending its record to `o`) in place, or, when `trap`, in a forked child so
// that upstream's SplitAtSeam crash (plan/upstream-issues.md UB-137) ends the child, not
// the run. The parent appends the child's record, or FP1_CRASHED when the child died on a
// signal, timed out or did not send a whole record. Without fork (Windows), and when the
// pipe or the fork fails, a trapped call is skipped (FP1_NOT_RUN).
template <class F> void fp1_trapped(bool trap, std::vector<double>& o, F fn)
{
    if (!trap)
    {
        fn(o);
        return;
    }
#ifdef _WIN32
    o.push_back(FP1_NOT_RUN);
#else
    int fd[2];
    if (pipe(fd) != 0)
    {
        o.push_back(FP1_NOT_RUN);
        return;
    }
    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0)
    {
        close(fd[0]);
        close(fd[1]);
        o.push_back(FP1_NOT_RUN);
        return;
    }
    if (pid == 0)
    {
        close(fd[0]);
#ifdef __linux__
        // (no core dump of an expected crash: a piped core_pattern ignores RLIMIT_CORE)
        prctl(PR_SET_DUMPABLE, 0);
#endif
        alarm(60);
        int code = 1;
        try
        {
            std::vector<double> c;
            fn(c);
            int64_t n = (int64_t)c.size();
            if (fp1_write_all(fd[1], &n, sizeof n) && fp1_write_all(fd[1], c.data(), c.size() * sizeof(double)))
                code = 0;
        }
        catch (...)
        {
        }
        _exit(code);
    }
    close(fd[1]);
    std::vector<double> c;
    int64_t n = -1;
    bool got = fp1_read_all(fd[0], &n, sizeof n) && n >= 0 && n < (1 << 20);
    if (got)
    {
        c.resize((size_t)n);
        got = fp1_read_all(fd[0], c.data(), c.size() * sizeof(double));
    }
    close(fd[0]);
    int ws = 0;
    pid_t w;
    do
        w = waitpid(pid, &ws, 0);
    while (w < 0 && errno == EINTR);
    if (got && w == pid && WIFEXITED(ws) && WEXITSTATUS(ws) == 0)
        o.insert(o.end(), c.begin(), c.end());
    else
        o.push_back(FP1_CRASHED);
#endif
}

struct Fp1EdgePeek : SmEdge
{
    static SmCurve* SmEdge::*curve() { return &Fp1EdgePeek::m_pCurve; }
};
struct Fp1LoopPeek : SmLoop
{
    static SmLoopuse* SmLoop::*lu() { return &Fp1LoopPeek::m_pLU; }
};

// Match the Rust import: keep exactly exported analytic surfaces and give other
// surfaces and edge curves their plain B-spline copies (and lines, planes).
// Replaced originals are leaked: they may still be referenced as owners' geometry.
void fp1_normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*FPeek::srf();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        // Preserve every surface the shared export imports as its exact analytic kind.
        std::vector<double> head;
        if (sc_analytic_export::put_exact_head(head, s, the_context()))
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb = new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        bool owned = s->GetOwner() == f;
        f->*FPeek::srf() = nb;
        if (owned)
        {
            s->SetOwner(NULL);
            nb->SetOwner(f);
        }
    }
    for (SmEdge* e : g.edges)
    {
        SmCurve* c = e->*Fp1EdgePeek::curve();
        if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE) || c->GetType() == SmBSplineCurve_TYPE ||
            c->GetType() == SmLine_TYPE)
            continue;
        SmBSplineCurve* b = (SmBSplineCurve*)c;
        gw_CURVE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineCurve* nc = new (the_context()) SmBSplineCurve(sm_AllocateAndCopyNurbCurve(nurb), b->GetDim(), FALSE);
        bool owned = c->GetOwner() == e;
        e->*Fp1EdgePeek::curve() = nc;
        if (owned)
        {
            c->SetOwner(NULL);
            nc->SetOwner(e);
        }
    }
}

// A curve as samples: `dim t0 t1 (x y z)*5` at t0 + i/4 (t1 - t0); `-1` for none.
void fp1_put_curve(std::vector<double>& o, const SmCurve* c)
{
    if (c == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back((double)c->GetDim());
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 4; i++)
    {
        double t = i == 4 ? ivl.GetMax() : ivl.GetMin() + (double)i / 4.0 * (ivl.GetMax() - ivl.GetMin());
        SmPoint3d p(0, 0, 0);
        SmStatus s = c->EvaluatePoint(t, p);
        if (s != SM_SUCCESS)
            p = SmPoint3d(0, 0, 0);
        put_vec(o, p);
    }
}

void fp1_put_domain(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

// The orientation state of the B-rep: per face its upward face use, per face use its
// orientation and loop uses, per loop its primary loop use (traversal indices).
void fp1_put_orientation(std::vector<double>& o, Graph& g)
{
    for (SmFace* f : g.faces)
        o.push_back((double)g.find(2, f->*FPeek::fu()));
    for (SmFaceuse* fu : g.faceuses)
    {
        o.push_back((double)fu->GetOrientation());
        std::vector<SmTopology*> lus = list_of(fu->GetList());
        o.push_back((double)lus.size());
        for (SmTopology* lu : lus)
            o.push_back((double)g.find(4, (SmLoopuse*)lu));
    }
    for (SmLoop* l : g.loops)
        o.push_back((double)g.find(4, l->*Fp1LoopPeek::lu()));
}

// Every edge use's own UV trim curve (samples).
void fp1_put_uv_curves(std::vector<double>& o, Graph& g)
{
    for (SmEdgeuse* p : g.edgeuses)
        fp1_put_curve(o, p->*EuPeek::uv());
}

void fp1_put_class(std::vector<double>& o, const SmCurveClassification& c)
{
    o.push_back(flg(c.GetTgtLoopsOnly()));
    o.push_back((double)c.GetSize());
    for (ULONG i = 0; i < c.GetSize(); i++)
        o.push_back((double)c[i].m_vMid.GetPointClass());
}

// The seam ops' inputs: `props crossed missing near_miss closed_u closed_v` (props 0: no
// SmFaceProps, the enums are HasSeamProblem's).
struct SeamIn
{
    bool props;
    SmSurfParamType crossed, missing, near_miss;
    SmBoolean closed_u, closed_v;
};

SeamIn fp1_read_seam(Rd& rd)
{
    SeamIn s;
    s.props = rd.flag();
    s.crossed = (SmSurfParamType)rd.idx();
    s.missing = (SmSurfParamType)rd.idx();
    s.near_miss = (SmSurfParamType)rd.idx();
    s.closed_u = (SmBoolean)rd.idx();
    s.closed_v = (SmBoolean)rd.idx();
    return s;
}

void fp1_put_split(std::vector<double>& o, SmTArray<SmFace*>& fs, SmTArray<SmEdge*>& es,
                   SmTArray<SmVertex*>& vs)
{
    o.push_back((double)fs.GetSize());
    o.push_back((double)es.GetSize());
    o.push_back((double)vs.GetSize());
}

// The number of op arguments after the face selector.
int64_t fp1_nargs(int32_t op)
{
    switch (op)
    {
    case OP_UV_DOMAIN_AFTER_TRIM_SHIFT: return 9;
    case OP_UV_DOMAIN:
    case OP_ORIENTED: return 3;
    case OP_SET_SURFACE:
    case OP_LOOP_ROUND_TRIP: return 1;
    case OP_MOVE_SEAM:
    case OP_SPLIT_AT_SEAM: return 8;
    default: return 0;
    }
}

void fp1_brep_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    int64_t fi = rd.idx();
    std::vector<double> args;
    for (int64_t i = 0; i < fp1_nargs(op); i++)
        args.push_back(rd.num());
    Rd ad{args.data(), 0};
    SmStatus status = SM_SUCCESS;
    Built built;
    built.brep = build(rd, path, status);
    if (built.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(built.brep, g);
    fp1_normalise(g);
    const SmContext& ctx = the_context();
    if (fi < 0 || fi >= (int64_t)g.faces.size())
    {
        o.push_back((double)SC_ERR_ARG);
        return;
    }
    SmFace* f = g.faces[(size_t)fi];
    switch (op)
    {
    case OP_UV_DOMAIN:
    case OP_UV_DOMAIN_AFTER_TRIM_SHIFT:
    {
        SmBoolean limit = ad.bflag();
        bool want_max = ad.flag();
        bool want_2nd = ad.flag();
        if (op == OP_UV_DOMAIN_AFTER_TRIM_SHIFT)
        {
            if (f->GetSurface() == NULL)
            {
                o.push_back(FP1_NULL_DEREF);
                break;
            }
            double tmp;
            SmStatus prep = f->CreateUVTrimCurves(FALSE, NULL, NULL, tmp, tmp, tmp, tmp, tmp, tmp);
            if (prep != SM_SUCCESS)
            {
                o.push_back(st(prep));
                return;
            }
            SmPoint2d lo = ad.uv(), hi = ad.uv();
            f->SetUVDomain(SmExtent2d(lo, hi));
            SmPoint2d delta = ad.uv();
            SmAxis2Placement transform;
            transform.Translate(SmVector3d(delta[0], delta[1], 0.0));
            SmTArray<SmLoopuse*> loops;
            f->GetUpwardLoopuses(loops);
            for (int64_t li = 0; li < loops.GetSize(); ++li)
            {
                SmTArray<SmEdgeuse*> edges;
                loops[li]->GetEdgeuses(edges);
                for (int64_t ei = 0; ei < edges.GetSize(); ++ei)
                {
                    SmCurve* curve = edges[ei]->GetUVTrimCurve();
                    if (curve != NULL)
                    {
                        prep = curve->Transform(transform, NULL);
                        if (prep != SM_SUCCESS)
                        {
                            o.push_back(st(prep));
                            return;
                        }
                    }
                }
            }
        }
        SmExtent2d dom;
        double gap = -7.0, gap2 = -7.0;
        SmStatus r = f->CalculateUVDomainFromUVTrimCurves(dom, limit, want_max ? &gap : NULL,
                                                         want_2nd ? &gap2 : NULL);
        o.push_back(st(r));
        fp1_put_domain(o, dom);
        // (the gaps of a failed call are partial: compared only on success)
        o.push_back(r == SM_SUCCESS && want_max ? gap : 0.0);
        o.push_back(r == SM_SUCCESS && want_2nd ? gap2 : 0.0);
        break;
    }
    case OP_ORIENTED:
    {
        SmBoolean param = ad.bflag();
        SmBoolean fill = ad.bflag();
        int64_t li = ad.idx();
        SmLoop* l = (li >= 0 && li < (int64_t)g.loops.size()) ? g.loops[(size_t)li] : nullptr;
        // (upstream dereferences the upward face use only without a loop to use)
        if ((l == nullptr && f->GetUpwardFaceuse() == nullptr) || (l != nullptr && l->GetLoopuse() == nullptr))
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        SmTArray<ULONG> loops;
        SmTArray<SmBSplineCurve*> curves;
        SmStatus r = f->CreateOrientedTrimBoundary(ctx, param, fill, loops, curves, l);
        o.push_back(st(r));
        o.push_back((double)loops.GetSize());
        for (ULONG i = 0; i < loops.GetSize(); i++)
            o.push_back((double)loops[i]);
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            fp1_put_curve(o, curves[i]);
            delete curves[i];
        }
        break;
    }
    case OP_RECT:
    {
        SmExtent2d dom;
        SmBoolean rect = UNSURE, nat = UNSURE;
        SmStatus r = f->FindIfRectangularTrim(dom, rect, nat);
        o.push_back(st(r));
        fp1_put_domain(o, dom);
        o.push_back((double)rect);
        o.push_back((double)nat);
        break;
    }
    case OP_FLIP:
    {
        o.push_back(st(f->FlipFaceOrientation()));
        fp1_put_orientation(o, g);
        break;
    }
    case OP_SET_SURFACE:
    {
        SmBoolean del = ad.bflag();
        SmSurface* s = f->GetSurface();
        if (s == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        SmSurface* copy = nullptr;
        SmStatus cs = s->Copy(ctx, copy);
        if (cs != SM_SUCCESS || copy == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        f->SetSurface(copy, del);
        o.push_back(0.0);
        fp1_put_domain(o, f->*FPeek::dom());
        o.push_back(flg(f->GetSurface() == copy && copy->GetOwner() == f));
        break;
    }
    case OP_LOOP_ROUND_TRIP:
    {
        int64_t li = ad.idx();
        SmLoop* l = (li >= 0 && li < (int64_t)g.loops.size()) ? g.loops[(size_t)li] : nullptr;
        SmLoopuse* a = l ? l->GetLoopuse() : nullptr;
        SmLoopuse* b = a ? a->GetOtherLoopuse() : nullptr;
        SmFaceuse* fa = a ? a->GetFaceuse() : nullptr;
        SmFaceuse* fb = b ? b->GetFaceuse() : nullptr;
        if (l == nullptr || a == nullptr || b == nullptr || fa == nullptr || fb == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        o.push_back(st(f->RemoveLoop(l)));
        o.push_back(st(f->InsertLoop(l)));
        fp1_put_orientation(o, g);
        break;
    }
    case OP_REMOVE_UV:
    {
        if (f->GetUpwardFaceuse() == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        o.push_back(st(f->RemoveUVTrimCurves()));
        fp1_put_uv_curves(o, g);
        break;
    }
    case OP_SWAP_UV:
    {
        SmSurface* s = f->GetSurface();
        if (s == nullptr || f->GetUpwardFaceuse() == nullptr || f->GetBrep() == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        o.push_back(st(f->SwapUV()));
        fp1_put_domain(o, f->*FPeek::dom());
        fp1_put_orientation(o, g);
        fp1_put_uv_curves(o, g);
        break;
    }
    case OP_SHRINK:
    {
        o.push_back(st(f->ShrinkGeometry()));
        fp1_put_domain(o, f->*FPeek::dom());
        SmSurface* s = f->GetSurface();
        if (s)
            fp1_put_domain(o, s->GetNaturalUVDomain());
        else
            o.push_back(-1.0);
        break;
    }
    case OP_PROBE:
    {
        if (f->GetSurface() == nullptr || f->GetUpwardFaceuse() == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        SmCurveClassification cu, cv;
        SmSurfParamType c = SM_SP_UNKNOWN, m = SM_SP_UNKNOWN, n = SM_SP_UNKNOWN;
        SmBoolean has = f->HasSeamProblem(cu, cv, c, m, n);
        o.push_back(flg(has));
        o.push_back((double)c);
        o.push_back((double)m);
        o.push_back((double)n);
        fp1_put_class(o, cu);
        fp1_put_class(o, cv);
        break;
    }
    case OP_MOVE_SEAM:
    case OP_SPLIT_AT_SEAM:
    {
        SeamIn si = fp1_read_seam(ad);
        SmBoolean center = ad.bflag();
        SmBoolean split = ad.bflag();
        if (f->GetSurface() == nullptr || f->GetUpwardFaceuse() == nullptr)
        {
            o.push_back(FP1_NULL_DEREF);
            break;
        }
        SmCurveClassification cu, cv;
        SmSurfParamType c = SM_SP_UNKNOWN, m = SM_SP_UNKNOWN, n = SM_SP_UNKNOWN;
        f->HasSeamProblem(cu, cv, c, m, n);
        SmFaceProps props(&ctx);
        if (si.props)
        {
            props.m_eBadCrossedSeam = si.crossed;
            props.m_eBadMissingSeam = si.missing;
            props.m_eBadNearMissSeam = si.near_miss;
            props.m_bClosedU = si.closed_u;
            props.m_bClosedV = si.closed_v;
        }
        SmFaceProps* pp = si.props ? &props : NULL;
        bool trap = false;

        // Upstream SplitAtSeam, after a U merge that made new faces, recurses on `this`
        // with the classifications of each new face (not on that face), and
        // SmCurveClassification::MergeInterval then crashes (plan/upstream-issues.md).
        // Trap (fp1_trapped) every call that can reach that branch: a SplitAtSeam asked
        // to merge in both directions on a face periodic in either, and every MoveSeam
        // with split that gets past its early return. No input predicate separates the
        // calls that crash, so the trap runs them all; the runner rejects those whose child
        // crashed (inputs undefined in C++, UB-137), so neither side is compared.
        SmSurface* srf = f->GetSurface();
        SmExtent2d nat = srf->GetNaturalUVDomain();
        // (the face properties' closed flags decide SplitAtSeam's own periodicity test,
        // but the recursion follows the real surface: either makes a direction count)
        bool per_u = (si.props && si.closed_u != FALSE) || srf->IsPeriodic(nat, SM_SP_U) != FALSE;
        bool per_v = (si.props && si.closed_v != FALSE) || srf->IsPeriodic(nat, SM_SP_V) != FALSE;
        if (op == OP_SPLIT_AT_SEAM)
        {
            auto has = [](SmSurfParamType p, SmSurfParamType d) { return p == d || p == SM_SP_BOTH; };
            bool merge_u = has(c, SM_SP_U) || has(m, SM_SP_U);
            bool merge_v = has(c, SM_SP_V) || has(m, SM_SP_V);
            bool no_missing = si.props && (si.missing == SM_SP_NEITHER || si.missing == SM_SP_UNKNOWN);
            // (the U-then-V branch needs both merges and either direction periodic)
            if (!no_missing && (per_u || per_v) && merge_u && merge_v)
                trap = true;
        }
        else if (split)
        {
            // MoveSeam calls SplitAtSeam (with the face properties) on the problem it
            // recomputes after moving
            bool quiet;
            if (si.props)
                quiet = (si.crossed == SM_SP_NEITHER || si.crossed == SM_SP_UNKNOWN) &&
                        (si.missing == SM_SP_NEITHER || si.missing == SM_SP_UNKNOWN);
            else
            {
                auto q = [](SmCurveClassification& k) {
                    return k.GetSize() < 1 || (k.GetSize() == 1 && k[0].m_vMid.GetPointClass() != SM_PC_FACE);
                };
                quiet = q(cu) && q(cv);
            }
            // (MoveSeam recomputes the seam problem before splitting, so any split it
            // makes can reach the U-then-V branch)
            if (!quiet)
                trap = true;
        }
        fp1_trapped(trap, o, [&](std::vector<double>& to) {
            SmTArray<SmFace*> fs;
            SmTArray<SmEdge*> es;
            SmTArray<SmVertex*> vs;
            if (op == OP_MOVE_SEAM)
            {
                SmFixSeamPlanType ru = SmFixSeamPlanType::SM_FSP_UNKNOWN;
                SmFixSeamPlanType rv = SmFixSeamPlanType::SM_FSP_UNKNOWN;
                SmStatus r = f->MoveSeam(ru, rv, cu, cv, c, m, n, center, split, &fs, &es, &vs, pp);
                to.push_back(st(r));
                to.push_back((double)(int)ru);
                to.push_back((double)(int)rv);
            }
            else
            {
                to.push_back(st(f->SplitAtSeam(cu, cv, c, m, n, &fs, &es, &vs, pp)));
            }
            fp1_put_split(to, fs, es, vs);
        });
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_face_p1_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    fp1_brep_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
