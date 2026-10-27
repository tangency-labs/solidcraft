// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `boolean.topology_intersector_p2`: the SmTopologyIntersector members
// owned by the unit (source/SMLib/src/SmTopologyIntersector.cpp,
// source/SMLib/inc/SmTopologyIntersector.h) on real SmBrep objects.
//
// Part 2 runs on the same intersector object and two-B-rep script as part 1, so this file
// includes part 1's shim (boolean_topology_intersector_p1.cpp) with SC_TI_EXTRA_OPS: every
// op of part 1 works unchanged and the script ops 41 to 52 below are part 2's. The entry
// point is sc_boolean_topology_intersector_p2_eval(op, path, in, n_in, out, cap, out_len);
// layouts are documented in crates/solidcore-ffi/src/units/boolean_topology_intersector_p2.rs,
// which validates every input (on top of part 1's validation).
#define SC_TI_EXTRA_OPS 1
#define SC_TI_EVAL_NAME sc_boolean_topology_intersector_p2_eval
#include "boolean_topology_intersector_p1.cpp"

namespace
{
enum TiScriptOp2 : int64_t
{
    S_REMOVE_REL = 41,
    S_SQUEEZE = 42,
    S_DELETE_FACES = 43,
    S_FIX_GAPS = 44,
    S_INTERSECT_TOPO = 45,
    S_SET_RELS = 46,
    S_IIR = 47,
    S_FIXUP_CLASS = 48,
    S_FIXUP_SPLIT = 49,
    S_EDGE_EDGE = 50,
    S_EDGE_FACE_COIN = 51,
    S_CLOSE_LOOP = 52,
};

bool is_vertex(const SmObject* p) { return p != nullptr && p->IsKindOf(SmVertex_TYPE); }

// A mate that upstream casts to SmVertex*: fine when null or a vertex.
bool vertex_mate_ok(const SmObject* p) { return p == nullptr || is_vertex(p); }

// A classification: `curve t0 t1 tol n (a b pc3*)*`, each pc `class h t has_t dev`.
// Returns false (after reading everything) when a vertex classification names no vertex.
bool read_cc(Rd& rd, const Two& t, const SmContext& ctx, SmCurveClassification*& cc)
{
    SmCurve* c = (SmCurve*)rd_h(rd, t);
    double t0 = rd.num(), t1 = rd.num(), tol = rd.num();
    int64_t n = rd.idx();
    cc = new (ctx) SmCurveClassification(c, SmExtent1d(t0, t1), nullptr, tol);
    cc->SetSize((ULONG)n);
    bool ok = c != nullptr;
    for (int64_t i = 0; i < n; i++)
    {
        double a = rd.num(), b = rd.num();
        (*cc)[(ULONG)i] = SmCurveInterval(SmExtent1d(a, b), &ctx, cc);
        SmPointClassification* pcs[3] = {&(*cc)[(ULONG)i].m_vStart, &(*cc)[(ULONG)i].m_vMid, &(*cc)[(ULONG)i].m_vEnd};
        for (SmPointClassification* pc : pcs)
        {
            int64_t cls = rd.idx();
            SmObject* obj = rd_h(rd, t);
            double tp = rd.num();
            bool has_t = rd.flag();
            double dev = rd.num();
            pc->SetClassObject((SmPointClassificationType)cls, obj);
            if (has_t)
                pc->SetTParam(tp);
            pc->SetDeviation(dev);
            if (cls == SM_PC_VERTEX && !is_vertex(obj))
                ok = false;
        }
    }
    return ok;
}

void put_cc(std::vector<double>& o, const Two& t, const SmCurveClassification& cc)
{
    o.push_back((double)cc.GetSize());
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        o.push_back(cc[i].GetInterval().GetMin());
        o.push_back(cc[i].GetInterval().GetMax());
        put_pc_full(o, t, cc[i].m_vStart);
        put_pc_full(o, t, cc[i].m_vMid);
        put_pc_full(o, t, cc[i].m_vEnd);
    }
}

template <class T> void rd_list(Rd& rd, const Two& t, SmTArray<T*>& a)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
        a.Add((T*)rd_h(rd, t));
}

// The edges of a vertex all have a curve and vertices (FixGaps, EdgeFaceCoincidence
// dereference them).
bool vertex_edges_ok(SmVertex* v)
{
    SmTArray<SmEdge*> es;
    v->GetEdges(es);
    for (ULONG i = 0; i < es.GetSize(); i++)
        if (!edge_ok(es[i]))
            return false;
    return true;
}

bool vertex_faces_ok(SmVertex* v)
{
    SmTArray<SmFace*> fs;
    v->GetFaces(fs);
    for (ULONG i = 0; i < fs.GetSize(); i++)
        if (!face_ok(fs[i]))
            return false;
    return true;
}
// CloseIntersectionLoop dereferences the edges of both vertices (curves, vertices) and the
// primary edge use of the B-rep vertex's edges (NormalizedEvaluate).
bool close_loop_edges_ok(SmTopologyIntersector* ti, SmVertex* bv)
{
    if (!vertex_edges_ok(bv))
        return false;
    SmTArray<SmEdge*> es;
    bv->GetEdges(es);
    for (ULONG i = 0; i < es.GetSize(); i++)
        if (es[i]->GetPrimaryEdgeuse() == nullptr)
            return false;
    SmObject* om = ti->GetOtherMate(bv);
    return !is_vertex(om) || vertex_edges_ok((SmVertex*)om);
}

// FixGaps' scan up to its first squeeze candidate (read-only): whether CombineCoincidentVertices
// would run with a null B-rep or a mate that is not a vertex (the steps before it return early).
bool fix_gaps_first_candidate_ok(SmTopologyIntersector* ti, const SmTArray<SmVertex*>& verts)
{
    ULONG n = verts.GetSize();
    for (ULONG ii = 0; ii < n; ii++)
        for (ULONG jj = ii + 1; jj < n; jj++)
        {
            SmVertex *v1 = verts[ii], *v2 = verts[jj];
            double tol = 1.01 * (v1->GetTolerance() + v2->GetTolerance());
            if (!(v1->GetPoint().DistanceBetween(v2->GetPoint()) < tol))
                continue;
            SmTArray<SmEdge*> e1, e2;
            v1->GetEdges(e1);
            v2->GetEdges(e2);
            bool common = false;
            for (ULONG k = 0; k < e1.GetSize(); k++)
            {
                ULONG idx;
                if (e2.FindElement(e1[k], idx) && e1[k]->GetCurve()->ApproximateLength(e1[k]->GetInterval(), 10) > tol)
                    common = true;
            }
            if (common)
                continue;
            SmObject* o1 = ti->GetOtherMate(v1);
            SmObject* o2 = ti->GetOtherMate(v2);
            if (o1 == nullptr || o2 == nullptr)
                return true; // NER returns SM_ERR_NULL_POINTER first
            return is_vertex(o1) && is_vertex(o2) && ti->GetPrimaryBrep() != nullptr && ti->GetOtherBrep() != nullptr;
        }
    return true;
}

// CheckForSqueezeVertices: whether a combination would run with a null B-rep (each block
// combines in its B-rep unless the previous relationship is better, which returns SM_ERR),
// or the second block would dereference a B-rep mate of the other vertex that is not a
// vertex (the first block's SM_ERR returns before; a first block that combines is assumed
// to reach the second). Since topology.brep_p5 the port runs CombineCoincidentVertices
// and reports InvalidInput (-2) exactly where the second block dereferences, so this guard
// can only over-refuse when RemoveRelationship or CombineCoincidentVertices fails partway
// through the first block: the port then returns that status where the shim answers -2,
// a mismatch the difftest would report (none in the generated or fixture cases).
// The other mate of the B-rep vertex is checked by the caller: the first block
// dereferences it at once.
// Each combination must also be given vertices of the B-rep it runs in: a vertex
// related to one of its own B-rep (Relate does not check) would be combined through
// the wrong B-rep, which upstream does without complaint and the port refuses
// ("not a vertex of this B-rep").
bool squeeze_breps_ok(SmTopologyIntersector* ti, SmVertex* bv, SmVertex* ov)
{
    bool need_other = false, need_brep = false;
    SmVertex* other_v = (SmVertex*)ti->GetOtherMate(bv);
    SmObject* brep_mate = ti->GetBrepMate(ov);
    SmVertex* brep_v = (SmVertex*)brep_mate;
    if (other_v && other_v != ov)
    {
        double d_old = bv->GetPoint().DistanceBetween(other_v->GetPoint());
        double d_new = bv->GetPoint().DistanceBetween(ov->GetPoint());
        if (d_new > d_old + SM_EFF_ZERO_SQRT)
            return true;
        need_other = true;
    }
    if (brep_v && brep_v != bv)
    {
        if (!is_vertex(brep_mate))
            return false;
        double d_old = ov->GetPoint().DistanceBetween(brep_v->GetPoint());
        double d_new = ov->GetPoint().DistanceBetween(bv->GetPoint());
        if (!(d_new > d_old + SM_EFF_ZERO_SQRT))
            need_brep = true;
    }
    SmBrep* other = ti->GetOtherBrep();
    SmBrep* primary = ti->GetPrimaryBrep();
    return (!need_other || (other != nullptr && ov->GetBrep() == other && other_v->GetBrep() == other)) &&
           (!need_brep || (primary != nullptr && bv->GetBrep() == primary && brep_v->GetBrep() == primary));
}

// SetRelationships without the maps (constructor 1): Relate dereferences the null maps only
// for a definition with both final topologies (the others are skipped with a warning).
bool set_rels_maps_ok(SmProtoTopologyManager* mgr, int64_t ctor_kind)
{
    if (ctor_kind == 0)
        return true;
    SmTArray<SmVertexDefinition*> vds;
    mgr->GetAllVertexDefinitions(vds);
    for (ULONG i = 0; i < vds.GetSize(); i++)
        if (vds[i]->GetFinalVertex(1) != nullptr && vds[i]->GetFinalVertex(2) != nullptr)
            return false;
    SmTArray<SmEdgeDefinition*> eds;
    mgr->GetAllEdgeDefinitions(eds);
    for (ULONG i = 0; i < eds.GetSize(); i++)
        if (eds[i] != nullptr && eds[i]->GetFinalEdge(1) != nullptr && eds[i]->GetFinalEdge(2) != nullptr)
            return false;
    return true;
}

// ImprintIntersections (SmTopologyIntersector.cpp:2302-2413, upstream commit c9c5979)
// VERBATIM, apart from the SM_DEBUG_CODE blocks and the members reached through accessors,
// plus one addition: `edited` answers whether a failing imprint may have edited a B-rep,
// as the port's ImprintOutcome::failure_edited does. A vertex definition's imprint and the
// glue edit first; an edge definition's imprint edits nothing when it stops before its
// first MakeEdgeInFace (a NULL final vertex or source face, SmProtoTopology.cpp:1375-1445),
// which the addition tests with the same const lookups beforehand.
bool ed_stops_before_edit(SmEdgeDefinition* pED)
{
  SmProtoVertex *pStartPV = pED->GetStartProtoVertex();
  SmProtoVertex *pEndPV   = pED->GetEndProtoVertex();
  if ( pStartPV->GetFinalVertex( 1, pED, TRUE ) == NULL || pEndPV->GetFinalVertex( 1, pED, FALSE ) == NULL
    || pStartPV->GetFinalVertex( 2, pED, TRUE ) == NULL || pEndPV->GetFinalVertex( 2, pED, FALSE ) == NULL )
    { return true; }
  SmPointClassificationType eType1 = pED->GetSrcTopo_TYPE( 1 );
  SmPointClassificationType eType2 = pED->GetSrcTopo_TYPE( 2 );
  if ( eType1 == SM_PC_FACE )
    { return SM_CAST_PTR( SmFace, pED->GetSrcTopo( 1 ) ) == NULL; }
  if ( eType2 == SM_PC_FACE )
    { return SM_CAST_PTR( SmFace, pED->GetSrcTopo( 2 ) ) == NULL; }
  return true;
}

SmStatus ii_verbatim(SmTopologyIntersector* ti, bool& edited)
{
  SmProtoTopologyManager*& m_pProtoTopoMgr = ti->*ti_get(TiMgr());
  SmBrep* m_pBrep = ti->GetPrimaryBrep();
  SmBrep* m_pOther = ti->GetOtherBrep();
  edited = false;

  SmStatus eStat, eRetStat = SM_SUCCESS;

  SmTemporaryChangeValue <SmBoolean> sTempChange1( m_pBrep ->m_bEditingEnabled, TRUE );
  SmTemporaryChangeValue <SmBoolean> sTempChange2( m_pOther->m_bEditingEnabled, TRUE );
  const SmContext *cpContext = m_pBrep->GetContext();
  SmTemporaryChangeValue <SmBoolean> sTempChangeDB( ((SmContext*)cpContext)->GetDoingBooleanRef(),   TRUE );

  SmTArray< SmProtoVertex* > rPVs = m_pProtoTopoMgr->GetProtoVertices();

  ULONG ii, jj, lNumObjs = rPVs.GetSize();
  for(ii=0;ii<lNumObjs;ii++)
    {
      SmProtoVertex *pPV = rPVs[ii];
      SmTArray< SmVertexDefinition* > & rVDs = pPV->GetVertexDefinitions();

      for(jj=0;jj<rVDs.GetSize();jj++)
        {
          SmVertexDefinition *pVD = rVDs[jj];
          eStat = pVD->Imprint();

          if ( eStat != SM_SUCCESS ) { edited = true; } // (addition)
          if ( eStat != SM_SUCCESS && eRetStat == SM_SUCCESS ) // Record the first problem.
            { eRetStat = eStat; }
        }
    }

  SmTArray< SmProtoEdge* > rPEs = m_pProtoTopoMgr->GetProtoEdges();
  lNumObjs = rPEs.GetSize();
  for(ii=0;ii<lNumObjs;ii++)
    {
      SmProtoEdge *pPE = rPEs[ii];
      SmTArray< SmEdgeDefinition* > & rEDs = pPE->GetEdgeDefinitions();
      for(jj=0;jj<rEDs.GetSize();jj++)
        {
          SmEdgeDefinition *pED = rEDs[jj];
          bool bStopsBeforeEdit = ed_stops_before_edit( pED ); // (addition)
          eStat = pED->Imprint();

          if(eStat != SM_SUCCESS)
            {
              if ( !bStopsBeforeEdit ) { edited = true; } // (addition)
              if(eRetStat == SM_SUCCESS) // Record the first problem.
                { eRetStat = eStat; }

              pPE->RemoveEdgeDefinition( pED );
              pED->Destruct();
              delete pED; pED = NULL;
              jj--;
            }
        }
    }

  eStat = m_pProtoTopoMgr->GlueCoincidentBrepVertices();

  if ( eStat != SM_SUCCESS ) { edited = true; } // (addition)
  if(eStat != SM_SUCCESS && eRetStat == SM_SUCCESS) // Record the first problem.
    { eRetStat = eStat; }

  return eRetStat;
}

// IntersectInsertRelate (SmTopologyIntersector.cpp:1092-1270, upstream commit c9c5979)
// VERBATIM, apart from the
// SM_VALIDATE_TOPOLOGY, SM_DEBUG_CODE and SM_USE_TBB blocks (not defined in the reference
// build) and the members reached through accessors, plus one addition: before the delete,
// whether ~SmProtoTopologyManager would read a freed proto edge (UB-175); if so the stale
// entries are unlinked first, so the run stays defined, and `ub` is set (the port answers
// InvalidInput there). A second addition stops after a failed ImprintIntersections (UB-242)
// whose failure may have edited a B-rep (ii_verbatim above), where upstream goes on with
// half-imprinted B-reps: `ub` is set there too. The real IntersectInsertRelate is no longer exercised by this
// difftest, so re-diff this copy against upstream whenever the pinned commit moves.
SmStatus iir_verbatim(SmTopologyIntersector* ti, bool& ub)
{
  SmProtoTopologyManager*& m_pProtoTopoMgr = ti->*ti_get(TiMgr());
  SmBrep* m_pBrep = ti->GetPrimaryBrep();
  SmBrep* m_pOther = ti->GetOtherBrep();

  // Capture Distance
  double dCaptureDistance = 0.01;

  // Make sure it's not smaller than our tolerance value.
  double dTopoIntTol = ti->GetThisApproxTol3d();
  if ( dCaptureDistance < dTopoIntTol * 2000 )
    { dCaptureDistance = dTopoIntTol * 2000; }

  m_pProtoTopoMgr = new( m_pBrep ) SmProtoTopologyManager( m_pBrep, m_pOther, ti, dCaptureDistance );

  SmStatus eStat = ti->IntersectTopology();

  SER_MSG( eStat, _T("\nSmProtoExecutive: Failure returned from IntersectTopology().\n") );

  eStat = ti->ResolveIntersections();

  SER_MSG( eStat, _T("\nSmProtoExecutive: Failure returned from ResolveProtoTopology().\n") );

  // Imprint Intersections, now we'll modify the Breps.
  bool bEdited = false;
  eStat = ii_verbatim( ti, bEdited );

  // If that didn't work, inform the public
  SE_MSG( eStat, _T("\nSmProtoExecutive: Error returned from ImprintIntersections().\n") );

  // (addition, UB-242: upstream goes on to SetRelationships with the half-imprinted
  // B-reps; stop here as the port does, deleting the manager as below, and answer -2)
  if (eStat != SM_SUCCESS && bEdited)
    {
      if(m_pProtoTopoMgr) { mgr_teardown_stale(m_pProtoTopoMgr, true); delete m_pProtoTopoMgr ; m_pProtoTopoMgr = NULL; }
      ub = true;
      return eStat;
    }

  // Set relationship maps.
  eStat = ti->SetRelationships();

  // all done - clean up and exit
  if(m_pProtoTopoMgr) { ub = mgr_teardown_stale(m_pProtoTopoMgr, true); delete m_pProtoTopoMgr ; m_pProtoTopoMgr = NULL; }
  return eStat;
}
} // namespace

namespace
{
bool run_extra_op(int64_t op, Rd& rd, Two& t, SmTopologyIntersector* ti, SmProtoTopologyManager*& mgr,
                  int64_t ctor_kind, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case S_REMOVE_REL:
    {
        SmTopology* a = (SmTopology*)rd_h(rd, t);
        SmTopology* b = (SmTopology*)rd_h(rd, t);
        // (IsMatedPair compares pointers only; without maps nothing is mated)
        o.push_back(st(ti->RemoveRelationship(a, b)));
        return true;
    }
    case S_SQUEEZE:
    {
        SmVertex* bv = (SmVertex*)rd_h(rd, t);
        SmVertex* ov = (SmVertex*)rd_h(rd, t);
        if (!is_vertex(bv) || !is_vertex(ov) || !vertex_mate_ok(ti->GetOtherMate(bv)) ||
            !squeeze_breps_ok(ti, bv, ov))
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmTArray<SmVertex*> dv, sv;
        SmTArray<SmEdge*> de;
        SmStatus s = ti->CheckForSqueezeVertices(bv, ov, dv, sv, de);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return true;
        o.push_back((double)dv.GetSize());
        o.push_back((double)sv.GetSize());
        o.push_back((double)de.GetSize());
        return true;
    }
    case S_DELETE_FACES:
    {
        SmTArray<SmFace*> faces;
        rd_list(rd, t, faces);
        bool has_keep = rd.flag();
        SmTArray<SmEdge*> keep;
        rd_list(rd, t, keep);
        bool ok = true;
        for (ULONG i = 0; i < faces.GetSize(); i++)
            if (faces[i] == nullptr)
                ok = false;
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        o.push_back(st(ti->DeleteFaces(faces, has_keep ? &keep : nullptr)));
        return true;
    }
    case S_FIX_GAPS:
    {
        SmTArray<SmVertex*> verts;
        rd_list(rd, t, verts);
        bool ok = true;
        for (ULONG i = 0; i < verts.GetSize(); i++)
            if (!is_vertex(verts[i]) || !vertex_edges_ok(verts[i]))
                ok = false;
        if (ok && !fix_gaps_first_candidate_ok(ti, verts))
            ok = false;
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmBoolean modified = FALSE;
        SmTArray<SmVertex*> dv;
        SmTArray<SmEdge*> de;
        SmStatus s = ti->FixGaps(verts, modified, dv, de);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return true;
        o.push_back(flg(modified));
        o.push_back((double)dv.GetSize());
        o.push_back((double)de.GetSize());
        return true;
    }
    case S_INTERSECT_TOPO:
    case S_SET_RELS:
    {
        // (IntersectTopology reads both B-reps' faces; SetRelationships' Relate needs the maps)
        if (mgr == nullptr || (op == S_INTERSECT_TOPO && (ti->GetPrimaryBrep() == nullptr || ti->GetOtherBrep() == nullptr)) ||
            (op == S_SET_RELS && !set_rels_maps_ok(mgr, ctor_kind)))
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmStatus s = (op == S_INTERSECT_TOPO) ? ti->IntersectTopology() : ti->SetRelationships();
        o.push_back(st(s));
        o.push_back((double)mgr->GetProtoVertices().GetSize());
        o.push_back((double)mgr->GetProtoEdges().GetSize());
        o.push_back((double)mgr->GetProtoFaces().GetSize());
        return true;
    }
    case S_IIR:
    {
        if (ctor_kind != 0 || ti->GetPrimaryBrep() == nullptr || ti->GetOtherBrep() == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        // (the manager IntersectInsertRelate makes replaces the installed one, which stays
        // owned by the script; it is the script's last op)
        bool ub = false;
        SmStatus s = iir_verbatim(ti, ub);
        o.push_back(ub ? ST_NULL_DEREF : st(s));
        return true;
    }
    case S_FIXUP_CLASS:
    case S_FIXUP_SPLIT:
    {
        int64_t index = (op == S_FIXUP_SPLIT) ? rd.idx() : 0;
        SmCurveClassification *cb = nullptr, *co = nullptr;
        bool ok_b = read_cc(rd, t, ctx, cb);
        bool ok_o = read_cc(rd, t, ctx, co);
        struct Del
        {
            SmCurveClassification*& a;
            SmCurveClassification*& b;
            ~Del()
            {
                delete a;
                delete b;
            }
        } del{cb, co};
        if (!ok_b || !ok_o || (op == S_FIXUP_SPLIT && ((ULONG)index >= cb->GetSize() || (ULONG)index >= co->GetSize())))
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmBoolean changed = FALSE;
        SmStatus s = (op == S_FIXUP_CLASS) ? ti->FixupClassMates(*cb, *co)
                                           : ti->FixupSplitVertexPair((ULONG)index, *cb, *co, changed);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return true;
        if (op == S_FIXUP_SPLIT)
            o.push_back(flg(changed));
        put_cc(o, t, *cb);
        put_cc(o, t, *co);
        return true;
    }
    case S_EDGE_EDGE:
    {
        SmTArray<SmEdge*> e1, e2;
        rd_list(rd, t, e1);
        rd_list(rd, t, e2);
        SmBoolean manifold = rd.bflag();
        SmBoolean ignore = rd.bflag();
        bool ok = true;
        for (ULONG i = 0; i < e1.GetSize(); i++)
            if (e1[i] != nullptr && !edge_ok(e1[i]))
                ok = false;
        for (ULONG i = 0; i < e2.GetSize(); i++)
            if (e2[i] != nullptr && !edge_ok(e2[i]))
                ok = false;
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmTArray<SmEdge*> s1, n1, s2, n2;
        SmTArray<SmVertex*> nv;
        double mx = 0.0, mn = 0.0;
        SmStatus s = ti->EdgeEdgeIntersect(e1, e2, s1, n1, s2, n2, nv, mx, mn, manifold, ignore);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return true;
        put_list(o, t, as_objs(s1));
        o.push_back((double)n1.GetSize());
        put_list(o, t, as_objs(s2));
        o.push_back((double)n2.GetSize());
        o.push_back((double)nv.GetSize());
        o.push_back(mx);
        o.push_back(mn);
        return true;
    }
    case S_EDGE_FACE_COIN:
    {
        SmTArray<SmVertex*> verts;
        SmTArray<SmEdge*> excl;
        rd_list(rd, t, verts);
        rd_list(rd, t, excl);
        SmBoolean ignore = rd.bflag();
        SmBoolean nesting = rd.bflag();
        bool has_null = false;
        for (ULONG i = 0; i < verts.GetSize(); i++)
            if (verts[i] == nullptr)
                has_null = true;
        bool ok = true;
        for (ULONG i = 0; i < verts.GetSize(); i++)
        {
            if (verts[i] == nullptr)
                continue;
            if (!is_vertex(verts[i]) || !vertex_edges_ok(verts[i]) || !vertex_faces_ok(verts[i]))
            {
                ok = false;
                continue;
            }
            // a null other vertex matches a null entry of the list and is then dereferenced
            SmTArray<SmEdge*> es;
            verts[i]->GetEdges(es);
            for (ULONG j = 0; j < es.GetSize(); j++)
                if (has_null && es[j]->GetOtherVertex(verts[i]) == nullptr)
                    ok = false;
        }
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmTArray<SmEdge*> ie;
        SmTArray<SmFace*> pf, nf, sf;
        double mx = 0.0, mn = 0.0;
        SmStatus s = ti->EdgeFaceCoincidence(verts, excl, ie, pf, nf, sf, mx, mn, ignore, nesting);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return true;
        put_list(o, t, as_objs(ie));
        put_list(o, t, as_objs(pf));
        o.push_back((double)nf.GetSize());
        put_list(o, t, as_objs(sf));
        o.push_back(mx);
        o.push_back(mn);
        return true;
    }
    case S_CLOSE_LOOP:
    {
        SmVertex* bv = (SmVertex*)rd_h(rd, t);
        if (bv != nullptr && (!is_vertex(bv) || !close_loop_edges_ok(ti, bv)))
        {
            o.push_back(ST_NULL_DEREF);
            return true;
        }
        SmBoolean made = FALSE;
        SmTArray<SmVertex*> dv;
        SmTArray<SmEdge*> de;
        SmStatus s = ti->CloseIntersectionLoop(bv, made, dv, de);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return true;
        o.push_back(flg(made));
        // the counts of the vertices and edges its squeezes deleted
        o.push_back((double)dv.GetSize());
        o.push_back((double)de.GetSize());
        // the relationships it made: the other mate of each edge of the vertex (op 6
        // cannot follow an op that may change topology)
        SmTArray<SmEdge*> es;
        bv->GetEdges(es);
        o.push_back((double)es.GetSize());
        for (ULONG i = 0; i < es.GetSize(); i++)
            t.put(o, ti->GetOtherMate(es[i]));
        return true;
    }
    default: return false;
    }
}
} // namespace
