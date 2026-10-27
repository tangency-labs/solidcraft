// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.face_p3`: SmFace::CalculateTightBoundingBox,
// SmFace::SectorClassifyCurveOn, sms_FindAndClassifyLoopBisector and (through
// SmFace::CalculateAnInternalPoint) the file-static sm_FaceClassifyUVLine
// (source/SMLib/src/SmFace.cpp) on real SmBrep objects.
//
// One entry point, sc_query_face_p3_eval(op, export_only, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/query_face_p3.rs, which
// validates every input. Every op starts with the B-rep block of unit query.face_p4 (its
// builders: .smb files, CreateFaceFromSurface, MakeFaceWithCurves over B-splines, planes and
// real SmCylinder / SmSphere / SmTorus / SmCone, CreateFaceInRegionFromSurface), then the
// op's case block. With export_only the B-rep graph (unit topology.face_p2's layout) and the
// resolved case (the topology indices and values the Rust side runs on) are exported and
// nothing is run; otherwise the upstream function runs and its results are exported.
//
// The B-rep builders, the traversal and the graph export are those of the query.face_p4
// shim, compiled into this translation unit (its entry point renamed so the two do not
// clash), so both units build and export identical B-reps.
#define sc_query_face_p4_eval sc_query_face_p3_p4_builders_eval
#include "query_face_p4.cpp"
#undef sc_query_face_p4_eval

#include <SmCacheMgr.h>
#include <SmCrvOnSurf.h>


// Not declared in a header; defined (non-static) in SmFace.cpp.
SmStatus sms_FindAndClassifyLoopBisector(ULONG lTgtLoopIndx, SmTArray<SmLoop*>& rLoops,
                                         SmTArray<SmClassifyLoopIO>& rClassifyLoopIOs, SmMarkType eMarkType,
                                         SmFace* pFace, SmSurface* pSurface, SmFaceProps* pFaceProps,
                                         SmContext& rContext, SmCurveClassification* pCrvClassification,
                                         SmCrvOnSurf*& rpCrvOnSurf, SmLine*& rpUVTrimLine,
                                         SmBoolean& rbClassifyCrv_StartOnSeam, SmBoolean& rbClassifyCrv_StopOnSeam,
                                         SmTriState& rebCLassifyCrv_Closed3d);

namespace qf3
{
enum Kind : int32_t
{
    K_TIGHT = 0,
    K_BISECTOR = 1,
    K_SECTOR = 2,
    K_INTERNAL = 3,
};

void put_pt(std::vector<double>& o, const SmPoint3d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

// The point at fractions (a, b) of the face's UV domain.
SmPoint2d face_frac(const SmFace* f, double a, double b)
{
    SmExtent2d d = f->GetUVDomain();
    return SmPoint2d(d.GetMin().x + a * (d.GetMax().x - d.GetMin().x), d.GetMin().y + b * (d.GetMax().y - d.GetMin().y));
}

// ---- bisector -------------------------------------------------------------------------------

// The bisector case: SetProps_Stage3's props and closed loops (unit query.face_p4's
// props_mode 1), the target index, the marked loops (mask over the loop list) and the
// containment of the marked loops' records.
struct Bisector
{
    SmFace* face = nullptr;
    ULONG face_index = 0;
    SmFaceProps* props = nullptr;
    SmTArray<SmLoop*> loops;
    ULONG target = 0;
    ULONG mask = 0;
    SmContainmentType containment = SM_CMT_UNKNOWN;
    bool rejected = false;
    ~Bisector() { delete props; }
};

// `target_sel mask containment` after the face selector.
void bisector_setup(Rd& rd, Graph& g, int64_t face_sel, Bisector& c)
{
    int64_t target_sel = rd.idx();
    c.mask = (ULONG)rd.idx();
    c.containment = (SmContainmentType)rd.idx();
    if (g.faces.empty())
    {
        c.rejected = true;
        return;
    }
    c.face_index = (ULONG)(face_sel % (int64_t)g.faces.size());
    c.face = g.faces[c.face_index];
    c.props = new SmFaceProps(c.face, c.face_index, c.face->GetContext());
    c.props->SetProps_Stage2(c.face, c.face_index, SM_HO_ALL);
    stage3_prefix(*c.props, c.loops);
    if (c.loops.GetSize() == 0 || c.face->GetSurface() == nullptr)
    {
        c.rejected = true;
        return;
    }
    c.target = (ULONG)(target_sel % (int64_t)c.loops.GetSize());
    // the pole points are read at their four slots
    if (c.props->m_lPoles != SM_SS_NONE && c.props->m_sPolePoints.GetSize() < 4)
        c.rejected = true;
}

void bisector_export(std::vector<double>& o, Graph& g, const Bisector& c)
{
    o.push_back((double)c.face_index);
    o.push_back((double)c.target);
    o.push_back((double)c.loops.GetSize());
    for (ULONG i = 0; i < c.loops.GetSize(); i++)
        o.push_back((double)g.find(5, c.loops[i]));
    o.push_back((double)c.mask);
    o.push_back((double)c.containment);
    const SmFaceProps& p = *c.props;
    o.push_back((double)p.m_bClosedU);
    o.push_back((double)p.m_bClosedV);
    o.push_back((double)p.m_lPoles);
    o.push_back((double)p.m_sPolePoints.GetSize());
    for (ULONG i = 0; i < p.m_sPolePoints.GetSize(); i++)
        put_pt(o, p.m_sPolePoints[i]);
    o.push_back((double)p.m_eBadMissingSeam);
    o.push_back((double)p.m_sZoneTol3d);
}

void put_curve_class(std::vector<double>& o, Graph& g, const SmCurveClassification& cc)
{
    o.push_back((double)cc.GetSize());
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        const SmCurveInterval& iv = cc.GetAt(i);
        o.push_back(iv.m_vInterval.GetMin());
        o.push_back(iv.m_vInterval.GetMax());
        const SmPointClassification* pcs[3] = {&iv.m_vStart, &iv.m_vMid, &iv.m_vEnd};
        for (const SmPointClassification* pc : pcs)
        {
            o.push_back((double)pc->GetPointClass());
            o.push_back(pc->GetPointClass() == SM_PC_VERTEX ? (double)topo_index(g, pc->GetObject()) : -1.0);
        }
    }
}

void bisector_run(std::vector<double>& o, Graph& g, Bisector& c)
{
    SmNewMarkAndLock sLoopMark(c.face->GetContext());
    SmMarkType eMarkType = sLoopMark.GetMarkType();
    SmTArray<SmClassifyLoopIO> ios;
    ios.SetSize(c.loops.GetSize());
    for (ULONG i = 0; i < c.loops.GetSize(); i++)
        if (i < 32 && (c.mask & (1UL << i)))
        {
            c.loops[i]->Mark(eMarkType);
            ios[i].m_eContainmentType = c.containment;
        }
    SmContext sLocalContext;
    SmCurveClassification scratch;
    SmCrvOnSurf* pCrvOnSurf = nullptr;
    SmLine* pUVTrimLine = nullptr;
    SmBoolean bStart = 7, bStop = 7;
    SmTriState eClosed((ULONG)9);
    SmStatus s = sms_FindAndClassifyLoopBisector(c.target, c.loops, ios, eMarkType, c.face, c.face->GetSurface(),
                                                 c.props, sLocalContext, &scratch, pCrvOnSurf, pUVTrimLine, bStart,
                                                 bStop, eClosed);
    o.push_back(st(s));
    o.push_back((double)bStart);
    o.push_back((double)bStop);
    o.push_back((double)eClosed.m_Val);
    o.push_back(pCrvOnSurf ? 1.0 : 0.0);
    if (pUVTrimLine)
    {
        SmExtent1d ivl = pUVTrimLine->GetNaturalInterval();
        SmPoint3d a, b;
        pUVTrimLine->EvaluatePoint(ivl.GetMin(), a);
        pUVTrimLine->EvaluatePoint(ivl.GetMax(), b);
        o.push_back(a.x);
        o.push_back(a.y);
        o.push_back(b.x);
        o.push_back(b.y);
    }
    put_curve_class(o, g, scratch);
    // the classification does not own the curves; the caller deletes them
    scratch.ReSet();
    delete pCrvOnSurf;
    delete pUVTrimLine;
}

// ---- sector -----------------------------------------------------------------------------------

// The resolved SectorClassifyCurveOn arguments.
struct Sector
{
    SmFace* face = nullptr;
    ULONG face_index = 0;
    SmEdgeuse* eu1 = nullptr;
    SmEdgeuse* eu2 = nullptr;
    double p1 = 0.0, p2 = 0.0, tol1 = 0.0, tol2 = 0.0;
    SmOrientType o1 = SM_OT_SAME, o2 = SM_OT_OPPOSITE;
    ULONG dim = 3;
    SmPoint3d start, end;
    SmOrientType curve_orient = SM_OT_SAME;
    double curve_tol = 0.0;
    bool singular = false;
    SmPoint2d sing_uv;
    bool uv_ok = true;
    bool rejected = false;
};

// `loop_sel eu_sel mode frac curve_kind ta tb curve_tol edge_tol singular sa sb uv_ok` after
// the face selector (see the FFI wrapper).
void sector_setup(Rd& rd, Graph& g, int64_t face_sel, Sector& c)
{
    int64_t loop_sel = rd.idx();
    int64_t eu_sel = rd.idx();
    int64_t mode = rd.idx();
    double frac = rd.num();
    int64_t curve_kind = rd.idx();
    double ta = rd.num(), tb = rd.num();
    c.curve_tol = rd.num();
    double edge_tol = rd.num();
    c.singular = rd.flag();
    double sa = rd.num(), sb = rd.num();
    c.uv_ok = rd.flag();
    if (g.faces.empty())
    {
        c.rejected = true;
        return;
    }
    c.face_index = (ULONG)(face_sel % (int64_t)g.faces.size());
    c.face = g.faces[c.face_index];
    SmSurface* srf = c.face->GetSurface();
    SmTArray<SmLoop*> loops;
    c.face->GetLoops(loops);
    if (srf == nullptr || loops.GetSize() == 0)
    {
        c.rejected = true;
        return;
    }
    SmLoop* l = loops[(ULONG)(loop_sel % (int64_t)loops.GetSize())];
    SmTArray<SmEdgeuse*> eus;
    if (l->GetLoopuse() == nullptr)
    {
        c.rejected = true;
        return;
    }
    l->GetLoopuse()->GetEdgeuses(eus);
    if (eus.GetSize() == 0)
    {
        c.rejected = true;
        return;
    }
    SmEdgeuse* eu = eus[(ULONG)(eu_sel % (int64_t)eus.GetSize())];
    SmEdge* e = eu->GetEdge();
    if (e == nullptr || e->GetCurve() == nullptr)
    {
        c.rejected = true;
        return;
    }
    SmExtent1d ivl = e->GetInterval();
    SmPoint3d cross;
    if (mode == 0)
    {
        // crossing an edge's interior: both edge uses the same
        c.eu1 = c.eu2 = eu;
        c.p1 = c.p2 = ivl.Evaluate(frac);
        c.o1 = SM_OT_SAME;
        c.o2 = SM_OT_OPPOSITE;
        e->GetCurve()->EvaluatePoint(c.p1, cross);
    }
    else
    {
        // at the start vertex of eu: eu (SAME) and its clockwise neighbour (OPPOSITE), or
        // eu's radial edge use (mode 2)
        c.eu1 = eu;
        c.p1 = eu->GetOrientation() == SM_OT_SAME ? ivl.GetMin() : ivl.GetMax();
        c.o1 = SM_OT_SAME;
        c.eu2 = mode == 1 ? eu->GetCWEdgeuse() : eu->GetRadial();
        if (c.eu2 == nullptr || c.eu2->GetEdge() == nullptr || c.eu2->GetEdge()->GetCurve() == nullptr)
        {
            c.rejected = true;
            return;
        }
        SmExtent1d ivl2 = c.eu2->GetEdge()->GetInterval();
        c.p2 = c.eu2->GetOrientation() == SM_OT_SAME ? ivl2.GetMax() : ivl2.GetMin();
        c.o2 = SM_OT_OPPOSITE;
        e->GetCurve()->EvaluatePoint(c.p1, cross);
    }
    c.tol1 = edge_tol < 0.0 ? (double)SmTol::GetZoneTol3d(e) : edge_tol;
    c.tol2 = edge_tol < 0.0 ? (double)SmTol::GetZoneTol3d(c.eu2->GetEdge()) : edge_tol;
    // the curve: a line from the crossing to the face point at fractions (ta, tb) (kind 0),
    // the reverse line classified at its max end (kind 1), or the UV line between their
    // parameters (kind 2)
    SmPoint2d tuv = face_frac(c.face, ta, tb);
    SmPoint3d target;
    if (curve_kind == 2)
    {
        SmBoolean ok = FALSE, multi = FALSE;
        SmPoint2d cuv;
        double gap = 0.0;
        if (srf->DropPoint(cross, srf->GetNaturalUVDomain(), NULL, ok, cuv, gap, multi) != SM_SUCCESS || !ok)
        {
            c.rejected = true;
            return;
        }
        c.dim = 2;
        c.start.Set(cuv.x, cuv.y, 0.0);
        c.end.Set(tuv.x, tuv.y, 0.0);
    }
    else
    {
        if (srf->EvaluatePoint(tuv, target) != SM_SUCCESS)
        {
            c.rejected = true;
            return;
        }
        c.dim = 3;
        c.start = curve_kind == 1 ? target : cross;
        c.end = curve_kind == 1 ? cross : target;
        c.curve_orient = curve_kind == 1 ? SM_OT_OPPOSITE : SM_OT_SAME;
    }
    if (c.start.DistanceBetween(c.end) < 1e-9)
    {
        c.rejected = true;
        return;
    }
    c.sing_uv = face_frac(c.face, sa, sb);
}

void sector_export(std::vector<double>& o, Graph& g, const Sector& c)
{
    o.push_back((double)c.face_index);
    o.push_back((double)g.find(6, c.eu1));
    o.push_back((double)g.find(6, c.eu2));
    o.push_back(c.p1);
    o.push_back(c.p2);
    o.push_back(c.tol1);
    o.push_back(c.tol2);
    o.push_back((double)c.o1);
    o.push_back((double)c.o2);
    o.push_back((double)c.dim);
    put_pt(o, c.start);
    put_pt(o, c.end);
    o.push_back((double)c.curve_orient);
    o.push_back(c.curve_tol);
    o.push_back(flg(c.singular ? TRUE : FALSE));
    o.push_back(c.sing_uv.x);
    o.push_back(c.sing_uv.y);
    o.push_back(flg(c.uv_ok ? TRUE : FALSE));
}

void sector_run(std::vector<double>& o, Sector& c)
{
    SmLine line(c.start, c.end, c.dim, c.face->GetContext());
    SmExtent1d ivl = line.GetNaturalInterval();
    SmBoolean inside = 7, problem = 7;
    SmClassifyProblem eProblem = (SmClassifyProblem)99;
    SmStatus s = c.face->SectorClassifyCurveOn(c.eu1, c.p1, SmZoneTol3d(c.tol1), c.o1, c.eu2, c.p2,
                                               SmZoneTol3d(c.tol2), c.o2, line, ivl, c.curve_orient,
                                               SmZoneTol3d(c.curve_tol), c.singular ? TRUE : FALSE, c.sing_uv,
                                               inside, problem, eProblem, c.uv_ok ? TRUE : FALSE);
    o.push_back(st(s));
    o.push_back((double)inside);
    o.push_back((double)problem);
    o.push_back((double)eProblem);
}

// Drop the trimmed surface cache that reading the file left on the face's surface: the healer
// that SmBrep::ReadFromFile runs (Fix_InfiniteRegion -> RayFire -> GlobalLineIntersect)
// builds it while Brep editing is enabled, so it stays untrimmed (no UV trim curves, no
// containment) and later queries reuse it. The Rust side imports the exported B-rep without
// caches, so both sides start from no cache (as query.face_p1/p2 do).
void drop_trim_cache(SmFace* f)
{
    SmSurface* srf = f->GetSurface();
    if (srf != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, srf) != nullptr)
        SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF, srf);
}

// ---- dispatch ---------------------------------------------------------------------------------

void run(int32_t kind, bool export_only, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back((double)bst);
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    int64_t face_sel = rd.idx();
    size_t start = 0;
    if (export_only)
    {
        export_graph(b.brep, g, o);
        start = o.size();
    }
    if (kind == K_TIGHT || kind == K_INTERNAL)
    {
        if (g.faces.empty())
        {
            o.push_back(1.0);
        }
        else
        {
            ULONG fi = (ULONG)(face_sel % (int64_t)g.faces.size());
            SmFace* f = g.faces[fi];
            o.push_back(0.0);
            if (export_only)
                o.push_back((double)fi);
            else if (kind == K_TIGHT)
            {
                SmExtent3d box;
                SmStatus s = f->CalculateTightBoundingBox(box);
                o.push_back(st(s));
                put_pt(o, box.GetMin());
                put_pt(o, box.GetMax());
                SmSurface* srf = f->GetSurface();
                o.push_back(srf != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, srf) != nullptr ? 1.0 : 0.0);
            }
            else
            {
                SmPoint3d p(9, 9, 9);
                drop_trim_cache(f);
                SmStatus s = f->CalculateAnInternalPoint(p);
                o.push_back(st(s));
                put_pt(o, p);
            }
        }
    }
    else if (kind == K_BISECTOR)
    {
        Bisector c;
        bisector_setup(rd, g, face_sel, c);
        o.push_back(c.rejected ? 1.0 : 0.0);
        if (!c.rejected)
        {
            if (export_only)
                bisector_export(o, g, c);
            else
                bisector_run(o, g, c);
        }
    }
    else
    {
        Sector c;
        sector_setup(rd, g, face_sel, c);
        o.push_back(c.rejected ? 1.0 : 0.0);
        if (!c.rejected)
        {
            if (export_only)
                sector_export(o, g, c);
            else
                sector_run(o, c);
        }
    }
    if (export_only)
        o.push_back((double)(o.size() - start));
}
} // namespace qf3

SC_EXPORT int32_t sc_query_face_p3_eval(int32_t kind, int32_t export_only, const char* path, const double* in,
                                        int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL || kind < 0 || kind > 3)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    qf3::run(kind, export_only != 0, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
