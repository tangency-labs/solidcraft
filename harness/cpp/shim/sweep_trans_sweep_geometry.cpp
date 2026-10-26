// SPDX-License-Identifier: Apache-2.0
// Native SmTranslationalSweepGeometry calls; layout in the safe FFI wrapper.
#include "StdAfx.h"
#include "sc_shim.h"
#include <SmApiCurves.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmPlane.h>
#include <SmTransSweepGeometry.h>
#include <SmVertex.h>
#include <memory>
#include <nurbs.h>
#include <set>
#include <vector>
namespace {
SmVector3d v3(const double *p) { return SmVector3d(p[0], p[1], p[2]); }
void put(std::vector<double> &o, const SmVector3d &v) {
  o.push_back(v.x);
  o.push_back(v.y);
  o.push_back(v.z);
}
double status(SmStatus s) { return s == SM_SUCCESS ? 0 : double(s); }
void put_curve(std::vector<double> &o, SmCurve *c) {
  o.push_back(double(c->GetType()));
  SmExtent1d iv = c->GetNaturalInterval();
  o.push_back(iv.GetMin());
  o.push_back(iv.GetMax());
  for (int j = 0; j <= 8; ++j) {
    SmVector3d pv[2];
    SmStatus s = c->Evaluate(iv.Evaluate(double(j) / 8), 1, TRUE, pv, TRUE);
    o.push_back(status(s));
    if (s == SM_SUCCESS) {
      put(o, pv[0]);
      put(o, pv[1]);
    }
  }
}
void put_surface(std::vector<double> &o, SmSurface *s) {
  o.push_back(double(s->GetType()));
  SmExtent2d dom = s->GetNaturalUVDomain();
  o.push_back(dom.GetMin().x);
  o.push_back(dom.GetMin().y);
  o.push_back(dom.GetMax().x);
  o.push_back(dom.GetMax().y);
  for (int u = 0; u <= 4; ++u)
    for (int v = 0; v <= 4; ++v) {
      SmPoint2d uv = dom.Evaluate(double(u) / 4, double(v) / 4);
      SmPoint3d p;
      SmStatus st = s->EvaluatePoint(uv, p);
      o.push_back(status(st));
      if (st == SM_SUCCESS)
        put(o, p);
      SmVector3d normal;
      st = s->EvaluateNormal(uv, TRUE, TRUE, normal);
      o.push_back(status(st));
      if (st == SM_SUCCESS)
        put(o, normal);
    }
}
SmCurve *build(const SmContext &ctx, const double *in, SmStatus &cst) {
  cst = SM_SUCCESS;
  int kind = int(in[13]), n = int(in[14]);
  const double *p = in + 19;
  if (kind == 0) {
    SmLine *c = nullptr;
    cst = SmLine::CreateLineSegment(ctx, 3, v3(p), v3(p + 4 * (n - 1)), c);
    if (cst != SM_SUCCESS)
      return nullptr;
    return c;
  }
  if (kind == 2) {
    SmCircle *c = nullptr;
    cst = SmApiCreateCircle(v3(p), p[3], c);
    if (cst != SM_SUCCESS)
      return nullptr;
    return c;
  }
  if (kind == 5) {
    SmEllipse *c = nullptr;
    SmAxis2Placement frame;
    frame.SetCanonical(v3(p), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0));
    cst = SmEllipse::CreateCanonical(ctx, frame, p[3], p[4 * (n - 1) + 3], c);
    if (cst != SM_SUCCESS)
      return nullptr;
    return c;
  }
  if (kind == 3) {
    return new (ctx)
        SmHermiteCurve(v3(p), SmVector3d(1, 1, 0), v3(p + 4 * (n - 1)),
                       SmVector3d(1, -1, 0), 3);
  }
  gw_CURVE *c = sm_AllocateNurbCurve(n - 1, n - 1, 2 * n - 1);
  for (int j = 0; j < n; ++j) {
    c->pol->Pw[j].x = p[4 * j];
    c->pol->Pw[j].y = p[4 * j + 1];
    c->pol->Pw[j].z = kind == 4 ? NL_NOZ : p[4 * j + 2];
    c->pol->Pw[j].w = p[4 * j + 3];
  }
  for (int j = 0; j < 2 * n; ++j)
    c->knt->U[j] = j < n ? 0 : 1;
  return new (ctx) SmBSplineCurve(c, kind == 4 ? 2 : 3, FALSE);
}
} // namespace
SC_EXPORT int32_t sc_sweep_trans_sweep_geometry_eval(const double *in,
                                                     int64_t n, double *out,
                                                     int64_t cap,
                                                     int64_t *len) {
  if (n < 19 || n != 19 + 4 * int64_t(in[14]))
    return SC_ERR_ARG;
  SC_TRY
  SmContext ctx;
  SmTranslationalSweepGeometry original(v3(in + 1), in[11]);
  original.SetMakeAnalytics(in[12] != 0);
  // Exercise Copy and its inherited analytics flag on every operation.
  std::unique_ptr<SmSweepGeometryCreation> copied(original.Copy());
  SmTranslationalSweepGeometry &sweep =
      static_cast<SmTranslationalSweepGeometry &>(*copied);
  int op = int(in[0]);
  std::vector<double> o;
  SmStatus st = SM_SUCCESS;
  if (op <= 2) {
    SmAxis2Placement frame;
    st = op == 0   ? sweep.GetStartCoordSystem(v3(in + 4), frame)
         : op == 1 ? sweep.GetEndCoordSystem(v3(in + 4), frame)
                   : sweep.GetSweepTransform(frame);
    o.push_back(status(st));
    if (st == SM_SUCCESS) {
      put(o, frame.GetOrigin());
      put(o, frame.GetXAxis());
      put(o, frame.GetYAxis());
      put(o, frame.GetZAxis());
    }
  } else if (op <= 4) {
    struct Vertex : SmVertex {
      Vertex() : SmVertex() {}
    };
    Vertex vertex;
    vertex.SetPoint(v3(in + 4));
    vertex.SetTolerance(in[7], FALSE, FALSE);
    double tol = 0;
    SmPoint3d p;
    SmCurve *c = nullptr;
    st = op == 3 ? sweep.VertexSweepSame(&vertex, p, tol)
                 : sweep.VertexSweepHigher(ctx, &vertex, c, tol);
    std::unique_ptr<SmCurve> clean(c);
    o.push_back(status(st));
    if (st == SM_SUCCESS) {
      o.push_back(tol);
      if (op == 3)
        put(o, p);
      else
        put_curve(o, c);
    }
  } else if (op == 5) {
    SmStatus cst;
    std::unique_ptr<SmCurve> c(build(ctx, in, cst));
    // A constructor that returns a failure status is a defined outcome (no
    // crash): its status is compared, as the Rust side propagates the error.
    if (!c && cst != SM_SUCCESS) {
      o.push_back(status(cst));
      return sc_copy_out(o.data(), int64_t(o.size()), out, cap, len);
    }
    if (!c)
      return SC_ERR_ARG;
    SmSweepCheckType state;
    st = sweep.CheckCurveSweep(*c, SmExtent1d(in[15], in[16]), in[11],
                               v3(in + 4), v3(in + 1), v3(in + 1).Length(),
                               state);
    o.push_back(status(st));
    o.push_back(double(state));
  } else if (op <= 7) {
    std::unique_ptr<SmBrep> b(new (ctx) SmBrep());
    SmEdge *e = nullptr;
    SmStatus cst;
    SmCurve *c = build(ctx, in, cst);
    // CreateWireEdgeFromCurve dereferences its consumed argument before
    // checking any status, so it is never called with null: a constructor
    // failure status is itself the compared outcome.
    if (!c && cst != SM_SUCCESS) {
      o.push_back(status(cst));
      return sc_copy_out(o.data(), int64_t(o.size()), out, cap, len);
    }
    if (!c)
      return SC_ERR_ARG;
    st = b->CreateWireEdgeFromCurve(c, SmExtent1d(in[15], in[16]), e);
    if (st != SM_SUCCESS || !e)
      return SC_ERR_ARG;
    e->SetTolerance(in[9], FALSE, FALSE);
    SmVertex *a = e->GetStartVertex();
    SmVertex *z = e->GetOtherVertex(a);
    if (!a || !z)
      return SC_ERR_ARG;
    a->SetTolerance(in[7], FALSE, FALSE);
    if (z != a)
      z->SetTolerance(in[8], FALSE, FALSE);
    if (in[17] != 0) {
      SmPoint3d p = a->GetPoint();
      a->SetPoint(z->GetPoint());
      z->SetPoint(p);
    }
    if (op == 6) {
      SmCurve *c = nullptr;
      double tol = 0;
      st = sweep.EdgeSweepSame(ctx, e, c, tol);
      std::unique_ptr<SmCurve> clean(c);
      o.push_back(status(st));
      if (st == SM_SUCCESS) {
        o.push_back(tol);
        put_curve(o, c);
      }
    } else {
      SmSurface *s = nullptr;
      SmCurve *start = nullptr, *far = nullptr, *sv = nullptr, *ev = nullptr;
      double ft = 0, at = 0, zt = 0, svt = 0, evt = 0;
      SmVector3d normal;
      SmTArray<SmCurve *> curves;
      SmTArray<SmBSplineCurve *> uv;
      SmTArray<SmOrientType> orient;
      st = sweep.EdgeSweepHigher(ctx, e, s, ft, start, at, far, zt, sv, svt, ev,
                                 evt, curves, uv, orient, normal);
      o.push_back(status(st));
      // Failed upstream outputs can point to already destroyed cleanup objects.
      // Only successful outputs are owned by this shim.
      if (st == SM_SUCCESS) {
        o.push_back(ft);
        o.push_back(at);
        o.push_back(zt);
        o.push_back(svt);
        o.push_back(evt);
        put(o, normal);
        put_surface(o, s);
        o.push_back(curves.GetSize());
        for (ULONG j = 0; j < curves.GetSize(); ++j) {
          o.push_back(double(orient[j]));
          o.push_back(curves[j] == sv ? 1 : 0);
          o.push_back(curves[j] == ev ? 1 : 0);
          put_curve(o, curves[j]);
          put_curve(o, uv[j]);
        }
        std::set<SmCurve *> owned;
        for (ULONG j = 0; j < curves.GetSize(); ++j)
          owned.insert(curves[j]);
        for (auto c : owned)
          delete c;
        for (ULONG j = 0; j < uv.GetSize(); ++j)
          delete uv[j];
        delete s;
      }
    }
  } else {
    std::unique_ptr<SmBrep> b(new (ctx) SmBrep());
    SmFace *f = nullptr;
    SmSurface *input = nullptr;
    if (in[18] == 0)
      input = new (ctx) SmPlane(
          v3(in + 4), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
          SmVector2d(1, 1), SmExtent2d(SmPoint2d(0, 0), SmPoint2d(1, 1)), &ctx);
    else {
      SmStatus cst;
      std::unique_ptr<SmCurve> c(build(ctx, in, cst));
      if (!c && cst != SM_SUCCESS) {
        o.push_back(status(cst));
        return sc_copy_out(o.data(), int64_t(o.size()), out, cap, len);
      }
      auto bs = SM_CAST_PTR(SmBSplineCurve, c.get());
      if (!bs)
        return SC_ERR_ARG;
      SmBSplineSurface *s = nullptr;
      st =
          SmBSplineSurface::CreateLinearSweep(ctx, *bs, SmVector3d(0, 0, 2), s);
      if (st != SM_SUCCESS)
        return SC_ERR_ARG;
      input = s;
    }
    // A failed CreateFaceFromSurface (e.g. a swept surface that degenerates to
    // a curve) is a defined outcome: its status is compared, as the Rust side
    // makes the face the same way.
    st = b->CreateFaceFromSurface(input, input->GetNaturalUVDomain(), f);
    if (st != SM_SUCCESS)
      o.push_back(status(st));
    else if (!f)
      return SC_ERR_ARG;
    else {
      f->SetTolerance(in[10], FALSE, FALSE);
      SmSurface *s = nullptr;
      double tol = 0;
      st = sweep.FaceSweepSame(ctx, f, s, tol);
      std::unique_ptr<SmSurface> clean(s);
      o.push_back(status(st));
      if (st == SM_SUCCESS) {
        o.push_back(tol);
        put_surface(o, s);
      }
    }
  }
  return sc_copy_out(o.data(), int64_t(o.size()), out, cap, len);
  SC_CATCH
}
