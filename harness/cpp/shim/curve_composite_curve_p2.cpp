// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.composite_curve_p2`: SmCompositeCurve part 2
// (source/SMLib/src/SmCompositeCurve.cpp, source/SMLib/inc/SmCompositeCurve.h):
// CreateOffsetsOfManyCurves, CreateTrimmedOffsets, IntersectAndTrimOffsets, sm_FixBowTie,
// MakeCompositeNurb, SmCompositeCurve::ReadFromDB (with SmCompositeCurveSegment::ReadFromDB),
// SmOffsetMapAttribute::MakeCopy, the SmCompositeCurveSegment constructor, and a verbatim
// copy of the file-static sm_CheckBowTies (the copy's only change: it returns
// SM_ERR_INVALID_INPUT where upstream would index crOrigCurves[0] of an empty array or read
// crOffsets past its end; the Rust wrapper never sends such input).
//
// One entry point, sc_curve_composite_curve_p2_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/curve_composite_curve_p2.rs,
// which validates every input. `out` receives the op status (0 = SM_SUCCESS, else the
// SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAttribute.h>
#include <SmBSplineCurve.h>
#include <SmCompositeCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmOffsetCurve.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

// Non-static upstream function without a header declaration (SmCompositeCurve.cpp).
SmStatus sm_FixBowTie(const SmCurve* pOffCrv, const SmTArray<SmPoint3d>& sPointsToTest, double dOffsetDist,
                      double d3DApproxTol, SmTArray<double>& sBreaks);

namespace
{
// ---- verbatim copy of file-static upstream code (SmCompositeCurve.cpp) ------------------

static SmStatus sm_CheckBowTies
 (const  SmTArray<SmCurve*>       & crOffsets,
  double                            dOffsetDistance,
  double                            dXSectTol3d,
  const  SmTArray<const SmCurve*> & crOrigCurves,
  SmTArray<SmPoint3d>             & sPointsToTest)
{
  sPointsToTest.ReSet();

  ULONG lNumOrig = crOrigCurves.GetSize();

  // (shim guard: upstream reads crOrigCurves[0] of an empty array)
  if (lNumOrig == 0) return SM_ERR_INVALID_INPUT;

  SmPoint3d sStartPt, sEndPt;
  const SmCurve *pStartCurve = crOrigCurves[0];
  const SmCurve *pEndCurve   = crOrigCurves[lNumOrig-1];
  SmExtent1d     sStartIvl   = pStartCurve[0].GetNaturalInterval();
  SmExtent1d     sEndIvl     = pEndCurve[0].GetNaturalInterval();
  pStartCurve->EvaluatePoint( sStartIvl.GetMin(), sStartPt );
  pEndCurve  ->EvaluatePoint( sEndIvl  .GetMax(), sEndPt   );

  if(sStartPt.DistanceBetween( sEndPt ) < dXSectTol3d )
    { return SM_SUCCESS; }

  SmSolutionArray sSolutions;
  ULONG ii;

  double dCutoffDistance = dOffsetDistance *(1.0 - SM_EFF_ZERO );

  // (shim guard: upstream reads crOffsets[0] of an empty array)
  if (crOffsets.GetSize() == 0) return SM_ERR_INVALID_INPUT;

  SmPoint3d sStartOffsetPt;
  crOffsets[0]->EvaluatePoint( crOffsets[0]->GetNaturalInterval().GetMin(), sStartOffsetPt );

  for(ii=0;ii<crOrigCurves.GetSize();ii++ )
    {
      const SmCurve *pOrig = crOrigCurves[ii];

      SER( pOrig->GlobalPointSolve
            (pOrig->GetNaturalInterval(),
             SM_SO_MINIMIZE,
             sStartOffsetPt,
             dXSectTol3d,
             NULL,
             NULL,
             SM_SR_SINGLE,
             sSolutions)) ;

      if(sSolutions.GetSize() > 0 )
        {
          double dThisDist = sSolutions[0].m_vStart.m_dSolutionValue;
          if(dThisDist < dCutoffDistance )
            {
              sPointsToTest.Add( sStartPt );
              break;
            }
        }
    }

  // (shim guard: upstream reads crOffsets past its end)
  if (crOffsets.GetSize() < lNumOrig) return SM_ERR_INVALID_INPUT;

  SmPoint3d sEndOffsetPt;
  crOffsets[lNumOrig-1]->EvaluatePoint( crOffsets[lNumOrig-1]->GetNaturalInterval().GetMax(), sEndOffsetPt );

  for(ii=0; ii<crOrigCurves.GetSize(); ii++ )
    {
      const SmCurve *pOrig = crOrigCurves[ii];

      SER( pOrig->GlobalPointSolve
            (pOrig->GetNaturalInterval(),
             SM_SO_MINIMIZE,
             sEndOffsetPt,
             dXSectTol3d,
             NULL,
             NULL,
             SM_SR_SINGLE,
             sSolutions)) ;

      if(sSolutions.GetSize() > 0 )
        {
          double dThisDist = sSolutions[0].m_vStart.m_dSolutionValue;
          if(dThisDist < dCutoffDistance )
            {
              sPointsToTest.Add( sEndPt );
              break;
            }
        }
    }

  return SM_SUCCESS;
}

// ---- shim plumbing -----------------------------------------------------------------

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_composite_curve_p2.rs).
enum : int32_t
{
    OP_OFFSETS = 0,
    OP_TRIMMED = 1,
    OP_INTERSECT = 2,
    OP_CHECK_BOW = 3,
    OP_FIX_BOW = 4,
    OP_NURB = 5,
    OP_READ_DB = 6,
    OP_WRITE_DB = 7,
    OP_MISC = 8,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// Objects the shim made that upstream does not own (deleted at the end of the call).
struct Pool
{
    std::vector<SmCurve*> curves;
    std::vector<SmCompositeCurve*> composites;
    ~Pool()
    {
        for (SmCompositeCurve* c : composites)
            delete c;
        for (size_t i = curves.size(); i > 0; i--)
            delete curves[i - 1];
    }
    SmCurve* keep(SmCurve* c)
    {
        curves.push_back(c);
        return c;
    }
};

// A curve block `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w) knots`; 1:
// SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`; 3: SmOffsetCurve
// `base N dist` of a base curve block over its natural interval). The caller owns the
// result; an offset's base goes to `pool`.
SmCurve* read_curve(Rd& rd, const SmContext& ctx, Pool& pool)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 2)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    if (kind == 3)
    {
        SmCurve* base = pool.keep(read_curve(rd, ctx, pool));
        SmVector3d n = rd.vec();
        double d = rd.num();
        return new (ctx) SmOffsetCurve(3, *base, base->GetNaturalInterval(), n, d);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// A composite block `dim n curve*n closed has_senses sense*n has_gaps gap*n`.
SmCompositeCurve* read_composite(Rd& rd, const SmContext& ctx, Pool& pool)
{
    ULONG dim = (ULONG)rd.idx();
    int64_t n = rd.idx();
    SmTArray<SmCurve*> curves;
    // (~SmCompositeCurve deletes every segment curve)
    for (int64_t i = 0; i < n; i++)
        curves.Add(read_curve(rd, ctx, pool));
    bool closed = rd.flag();
    bool has_senses = rd.flag();
    SmTArray<SmBoolean> senses;
    if (has_senses)
        for (int64_t i = 0; i < n; i++)
            senses.Add(rd.flag() ? TRUE : FALSE);
    bool has_gaps = rd.flag();
    SmTArray<double> gaps;
    if (has_gaps)
        for (int64_t i = 0; i < n; i++)
            gaps.Add(rd.num());
    SmCompositeCurve* c = new (ctx) SmCompositeCurve(dim, curves, closed ? TRUE : FALSE,
                                                     has_senses ? &senses : NULL, has_gaps ? &gaps : NULL);
    pool.composites.push_back(c);
    return c;
}

// The state of a curve: `type a b P[3]*7 [n+1 Pw[4] m+1 U]` (the NURB for an
// SmBSplineCurve proper).
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 6; i++)
    {
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        c->EvaluatePoint(ivl.Evaluate(i / 6.0), p);
        put_vec(o, p);
    }
    if (c->GetType() == SmBSplineCurve_TYPE)
    {
        gw_CURVE* nurb = ((const SmBSplineCurve*)c)->GetGwNurbPointer();
        o.push_back((double)(nurb->pol->n + 1));
        for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
        {
            o.push_back(nurb->pol->Pw[k].x);
            o.push_back(nurb->pol->Pw[k].y);
            o.push_back(nurb->pol->Pw[k].z);
            o.push_back(nurb->pol->Pw[k].w);
        }
        o.push_back((double)(nurb->knt->m + 1));
        for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
            o.push_back(nurb->knt->U[k]);
    }
}

// The segment records of a composite: `dim n closed (transition sense gap0 gap1 owns curve)*n`.
void put_composite(std::vector<double>& o, const SmCompositeCurve* c)
{
    ULONG n = c->GetNumSegments();
    o.push_back((double)c->GetDim());
    o.push_back((double)n);
    o.push_back(flg(c->IsClosed(c->GetNaturalInterval())));
    for (ULONG i = 0; i < n; i++)
    {
        const SmCompositeCurveSegment* s = c->GetCurveSegment(i);
        o.push_back((double)s->m_eTransition);
        o.push_back(flg(s->m_bSameSense));
        o.push_back(s->m_dGapStart);
        o.push_back(s->m_dGapEnd);
        o.push_back(flg(s->m_bOwnsCurve));
        put_curve(o, s->m_pParentCurve);
    }
}

// An offset piece: `has_attr [corner left comp curve other orig err] curve`, where `orig`
// is the originator's index in `origs` (`-1` for NULL; `-2` when not found).
void put_piece(std::vector<double>& o, const SmCurve* c, const SmTArray<const SmCurve*>* origs)
{
    SmAttribute* pAttr = ((SmCurve*)c)->FindAttribute(SM_AI_OFFSETMAP);
    SmOffsetMapAttribute* a = SM_CAST_PTR(SmOffsetMapAttribute, pAttr);
    o.push_back(a ? 1.0 : 0.0);
    if (a)
    {
        o.push_back(flg(a->GetCornerOffset()));
        o.push_back(flg(a->GetLeftHandOffset()));
        o.push_back((double)a->GetCompositeIndex());
        o.push_back((double)a->GetCurve());
        o.push_back((double)a->GetOtherCurve());
        const SmCurve* orig = a->GetOrigCurve();
        double idx = -1.0;
        if (orig != NULL)
        {
            idx = -2.0;
            if (origs == NULL)
                idx = 0.0; // pointer only compared with NULL (it dangles once the call returns)
            else
                for (ULONG i = 0; i < origs->GetSize(); i++)
                    if ((*origs)[i] == orig)
                    {
                        idx = (double)i;
                        break;
                    }
        }
        o.push_back(idx);
        o.push_back(a->GetApproxError());
    }
    put_curve(o, c);
}

// Offset parameters `xtol atol N[3] corner trim dist match`.
struct Params
{
    double xtol, atol;
    SmVector3d normal;
    SmOffsetCornerType corner;
    bool trim;
    double dist;
    bool match;
};

Params read_params(Rd& rd)
{
    Params p;
    p.xtol = rd.num();
    p.atol = rd.num();
    p.normal = rd.vec();
    p.corner = (SmOffsetCornerType)rd.idx();
    p.trim = rd.flag();
    p.dist = rd.num();
    p.match = rd.flag();
    return p;
}

void put_offsets(std::vector<double>& o, SmTArray<SmBSplineCurve*>& offs)
{
    o.push_back((double)offs.GetSize());
    for (ULONG i = 0; i < offs.GetSize(); i++)
        put_piece(o, offs[i], NULL);
    for (ULONG i = 0; i < offs.GetSize(); i++)
        delete offs[i];
    offs.ReSet();
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Pool pool;
    switch (op)
    {
    case OP_OFFSETS:
    {
        int64_t nc = rd.idx();
        SmTArray<SmCompositeCurve*> comps;
        for (int64_t i = 0; i < nc; i++)
            comps.Add(read_composite(rd, ctx, pool));
        SmTArray<ULONG> dirs;
        for (int64_t i = 0; i < nc; i++)
            dirs.Add((ULONG)rd.idx());
        Params p = read_params(rd);
        SmTArray<SmBSplineCurve*> offs;
        SmTArray<double> gaps;
        SmStatus s = SmCompositeCurve::CreateOffsetsOfManyCurves(ctx, comps, dirs, p.xtol, p.atol, p.normal, p.corner,
                                                                 p.trim ? TRUE : FALSE, p.dist, offs,
                                                                 p.match ? TRUE : FALSE, &gaps);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_offsets(o, offs);
            o.push_back((double)gaps.GetSize());
            for (ULONG i = 0; i < gaps.GetSize(); i++)
                o.push_back(gaps[i]);
        }
        for (ULONG i = 0; i < offs.GetSize(); i++)
            delete offs[i];
        break;
    }
    case OP_TRIMMED:
    {
        SmCompositeCurve* c = read_composite(rd, ctx, pool);
        SmOffsetDirectionType dir = (SmOffsetDirectionType)rd.idx();
        Params p = read_params(rd);
        SmTArray<SmBSplineCurve*> offs;
        SmStatus s = c->CreateTrimmedOffsets(ctx, p.xtol, p.atol, p.normal, p.corner, dir, p.trim ? TRUE : FALSE,
                                             p.dist, offs, p.match ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_offsets(o, offs);
        for (ULONG i = 0; i < offs.GetSize(); i++)
            delete offs[i];
        break;
    }
    case OP_INTERSECT:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> raws;
        for (int64_t i = 0; i < n; i++)
            raws.Add(pool.keep(read_curve(rd, ctx, pool)));
        struct A
        {
            bool has, corner, left;
            ULONG comp, crv, other;
            int64_t orig;
            double err;
        };
        std::vector<A> attrs;
        for (int64_t i = 0; i < n; i++)
        {
            A a;
            a.has = rd.flag();
            a.corner = rd.flag();
            a.left = rd.flag();
            a.comp = (ULONG)rd.idx();
            a.crv = (ULONG)rd.idx();
            a.other = (ULONG)rd.idx();
            a.orig = rd.idx();
            a.err = rd.num();
            attrs.push_back(a);
        }
        double dist = rd.num();
        double tol = rd.num();
        int64_t norig = rd.idx();
        SmTArray<const SmCurve*> origs;
        for (int64_t i = 0; i < norig; i++)
            origs.Add(pool.keep(read_curve(rd, ctx, pool)));
        for (int64_t i = 0; i < n; i++)
        {
            const A& a = attrs[(size_t)i];
            if (!a.has)
                continue;
            SmCurve* orig = a.orig < 0 ? NULL : (SmCurve*)origs[(ULONG)a.orig];
            raws[(ULONG)i]->AddAttribute(new (ctx) SmOffsetMapAttribute(SM_AI_OFFSETMAP, a.corner ? TRUE : FALSE,
                                                                        a.left ? TRUE : FALSE, a.comp, a.crv,
                                                                        a.other, orig, a.err));
        }
        SmTArray<SmCurve*> out;
        SmStatus s = SmCompositeCurve::IntersectAndTrimOffsets(ctx, raws, dist, tol, origs, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)out.GetSize());
            for (ULONG i = 0; i < out.GetSize(); i++)
                put_piece(o, out[i], &origs);
            o.push_back((double)raws.GetSize());
            for (ULONG i = 0; i < raws.GetSize(); i++)
                put_curve(o, raws[i]);
        }
        for (ULONG i = 0; i < out.GetSize(); i++)
            delete out[i];
        break;
    }
    case OP_CHECK_BOW:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> offs;
        for (int64_t i = 0; i < n; i++)
            offs.Add(pool.keep(read_curve(rd, ctx, pool)));
        double dist = rd.num();
        double tol = rd.num();
        int64_t norig = rd.idx();
        SmTArray<const SmCurve*> origs;
        for (int64_t i = 0; i < norig; i++)
            origs.Add(pool.keep(read_curve(rd, ctx, pool)));
        SmTArray<SmPoint3d> pts;
        SmStatus s = sm_CheckBowTies(offs, dist, tol, origs, pts);
        o.push_back(st(s));
        o.push_back((double)pts.GetSize());
        for (ULONG i = 0; i < pts.GetSize(); i++)
            put_vec(o, pts[i]);
        break;
    }
    case OP_FIX_BOW:
    {
        SmCurve* c = pool.keep(read_curve(rd, ctx, pool));
        int64_t np = rd.idx();
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        double dist = rd.num();
        int64_t nb = rd.idx();
        SmTArray<double> breaks;
        for (int64_t i = 0; i < nb; i++)
            breaks.Add(rd.num());
        SmStatus s = sm_FixBowTie(c, pts, dist, 0.0, breaks);
        o.push_back(st(s));
        o.push_back((double)breaks.GetSize());
        for (ULONG i = 0; i < breaks.GetSize(); i++)
            o.push_back(breaks[i]);
        break;
    }
    case OP_NURB:
    {
        SmCompositeCurve* c = read_composite(rd, ctx, pool);
        SmBSplineCurve* j = NULL;
        SmStatus s = c->MakeCompositeNurb(ctx, j);
        o.push_back(st(s));
        if (s == SM_SUCCESS && j != NULL)
        {
            put_curve(o, j);
            put_composite(o, c);
        }
        delete j;
        break;
    }
    case OP_READ_DB:
    {
        ULONG dim = (ULONG)rd.idx();
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmCompositeCurve::ReadFromDB(SmCompositeCurve_TYPE, db, dim, ctx, c, version);
        SmCompositeCurve* cc = (SmCompositeCurve*)c;
        if (s == SM_SUCCESS && cc != NULL)
        {
            // a segment without a curve (not stored, or failed to read) is reported as
            // SM_ERR_INVALID_INPUT: upstream keeps a NULL segment curve
            for (ULONG i = 0; i < cc->GetNumSegments(); i++)
                if (cc->GetCurveSegment(i)->m_pParentCurve == NULL)
                    s = SM_ERR_INVALID_INPUT;
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS && cc != NULL)
            put_composite(o, cc);
        delete c;
        break;
    }
    case OP_WRITE_DB:
    {
        // fixture helper: the bytes upstream WriteToDB produces (ASCII with 17 digits)
        SmCompositeCurve* c = read_composite(rd, ctx, pool);
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        std::ostringstream oss;
        oss.precision(17);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &oss);
        SmStatus s = c->WriteToDB(db, version);
        o.push_back(st(s));
        std::string bytes = oss.str();
        for (char ch : bytes)
            o.push_back((double)(unsigned char)ch);
        break;
    }
    case OP_MISC:
    {
        ULONG id = (ULONG)rd.idx();
        bool corner = rd.flag();
        bool left = rd.flag();
        ULONG comp = (ULONG)rd.idx();
        ULONG crv = (ULONG)rd.idx();
        ULONG other = (ULONG)rd.idx();
        double err = rd.num();
        SmCurve* c = pool.keep(read_curve(rd, ctx, pool));
        bool sense = rd.flag();
        double g0 = rd.num();
        double g1 = rd.num();
        o.push_back(0.0);
        SmOffsetMapAttribute attr(id, corner ? TRUE : FALSE, left ? TRUE : FALSE, comp, crv, other, NULL, err);
        SmOffsetMapAttribute* cp = (SmOffsetMapAttribute*)attr.MakeCopy(ctx);
        o.push_back((double)cp->GetAttributeID());
        o.push_back(flg(cp->GetCornerOffset()));
        o.push_back(flg(cp->GetLeftHandOffset()));
        o.push_back((double)cp->GetCompositeIndex());
        o.push_back((double)cp->GetCurve());
        o.push_back((double)cp->GetOtherCurve());
        o.push_back(cp->GetOrigCurve() == NULL ? 1.0 : 0.0);
        o.push_back(cp->GetApproxError());
        o.push_back((double)cp->GetType());
        delete cp;
        SmCompositeCurveSegment seg(c, sense ? TRUE : FALSE, g0, g1);
        o.push_back((double)seg.m_eTransition);
        o.push_back(flg(seg.m_bSameSense));
        o.push_back(seg.m_dGapStart);
        o.push_back(seg.m_dGapEnd);
        o.push_back(flg(seg.m_bOwnsCurve));
        put_curve(o, seg.GetCurve());
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_composite_curve_p2_eval(int32_t op, const double* in, int64_t n_in,
                                                   double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, ctx, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
