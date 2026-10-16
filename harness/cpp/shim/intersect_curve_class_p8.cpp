// SPDX-License-Identifier: Apache-2.0
// Native oracle for the five p8 members in SmCurveClass.cpp (c9c5979).
// Wire layout is documented and validated by intersect_curve_class_p8.rs.
#include "StdAfx.h"
#include "sc_shim.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBSplineVolume.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurveClass.h>
#include <SmLine.h>
#include <SmMemory.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmNurbsVol.h>
#include <SmPlane.h>
#include <SmTol.h>
#include <SmTransform.h>
#include <map>
#include <memory>
#include <nurbs.h>
#include <vector>

SC_EXPORT int32_t sc_intersect_curve_class_p8_run(const double *in, int64_t n,
                                                  double *out, int64_t cap,
                                                  int64_t *len) {
  if (!in || n < 23)
    return SC_ERR_ARG;
  SC_TRY
  const int op = (int)in[0], dim = (int)in[1], ck = (int)in[2], sk = (int)in[3],
            nu = (int)in[4], nv = (int)in[5], count = (int)in[22];
  if (op < 0 || op > 4 || (dim != 2 && dim != 3) || ck < 0 || ck > 2 ||
      sk < 0 || sk > 3 || nu < 2 || nu > 4 || nv < 2 || nv > 4 || count < 1 ||
      count > 16 || n != 23 + count * 3)
    return SC_ERR_ARG;
  if (in[12] <= in[10] || in[13] <= in[11] || in[14] <= 0 || in[15] <= 0 ||
      in[18] > in[19])
    return SC_ERR_ARG;
  SmContext ctx;
  SmPoint3d a(in[6], in[7], in[8]), b(in[9], in[16], in[17]);
  std::unique_ptr<SmCurve> curve;
  if (ck == 0) {
    SmLine *p = NULL;
    SmStatus s = SmLine::CreateLineSegment(ctx, dim, a, b, p);
    if (s != SM_SUCCESS)
      return SC_ERR_ARG;
    curve.reset(p);
  } else if (ck == 1) {
    curve.reset(new (ctx) SmCircle(a, SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                                   SmExtent1d(0, 360), in[9], dim, &ctx));
  } else {
    gw_CURVE *r = sm_AllocateNurbCurve(2, 2, 5);
    if (!r)
      return SC_ERR_EXCEPTION;
    for (int i = 0; i < 6; ++i)
      r->knt->U[i] = i < 3 ? 0 : 1;
    for (int i = 0; i < 3; ++i) {
      NL_CPOINT &p = r->pol->Pw[i];
      p.x = a.x;
      p.y = a.y;
      p.z = a.z;
      p.w = 1;
    }
    curve.reset(new (ctx) SmBSplineCurve(r, dim, FALSE, &ctx));
  }
  SmExtent2d dom(in[10], in[11], in[12], in[13]);
  std::unique_ptr<SmSurface> surface;
  std::unique_ptr<SmVolume> volume;
  if (sk == 0) {
    SmPlane *p = new (ctx)
        SmPlane(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                SmVector2d(in[14], in[15]), dom, &ctx);
    p->MakeNurb();
    surface.reset(p);
  } else {
    gw_SURFACE *s =
        sm_AllocateNurbSurface(nu - 1, nv - 1, 1, 1, nu + 1, nv + 1);
    if (!s)
      return SC_ERR_EXCEPTION;
    for (int i = 0; i < nu + 2; ++i)
      s->knu->U[i] = i == 0 ? in[10]
                     : i > nu - 1
                         ? in[12]
                         : in[10] + (in[12] - in[10]) * (i - 1) / (nu - 1);
    for (int j = 0; j < nv + 2; ++j)
      s->knv->U[j] = j == 0 ? in[11]
                     : j > nv - 1
                         ? in[13]
                         : in[11] + (in[13] - in[11]) * (j - 1) / (nv - 1);
    for (int i = 0; i < nu; ++i)
      for (int j = 0; j < nv; ++j) {
        NL_CPOINT &p = s->net->Pw[i][j];
        p.x = in[14] * (in[10] + (in[12] - in[10]) * i / (nu - 1));
        p.y = in[15] * (in[11] + (in[13] - in[11]) * j / (nv - 1));
        p.z = sk == 2 ? in[21] * i * j : 0;
        p.w = 1;
      }
    surface.reset(new (ctx) SmBSplineSurface(s, FALSE, &ctx));
  }
  if (op == 4) {
    if (sk < 2)
      volume.reset(new (ctx) SmTransform(&ctx));
    else {
      // Equal counts keep native W-from-V indexing and borrowed solution slots
      // in bounds.
      gw_VOLUME *r = sm_AllocateNurbVolume(2, 2, 2, 1, 1, 1, 4, 4, 4);
      if (!r)
        return SC_ERR_EXCEPTION;
      for (int i = 0; i < 5; ++i)
        r->knu->U[i] = r->knv->U[i] = r->knw->U[i] = i < 2    ? 0
                                                     : i == 2 ? 0.5
                                                              : 1;
      for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j)
          for (int k = 0; k < 3; ++k) {
            NL_CPOINT &p = r->mesh->Pw[i][j][k];
            p.x = i * 0.5;
            p.y = j * 0.5;
            p.z = k * 0.5;
            p.w = 1;
          }
      volume.reset(new (ctx) SmBSplineVolume(&ctx, r));
      smos_Free(r);
    }
  }
  SmCurveClassification cc;
  cc.SetCurve(curve.get(), FALSE);
  cc.SetInterval(SmExtent1d(in[18], in[19]));
  SmTol::SetZoneTol3d(&cc, SmZoneTol3d(in[20]));
  cc.SetSize(count);
  for (int i = 0; i < count; ++i) {
    int at = 23 + 3 * i;
    SmCurveInterval &v = cc.GetAt(i);
    v.SetInterval(SmExtent1d(in[at], in[at + 1]));
    int c = (int)in[at + 2];
    if (c != SM_PC_UNKNOWN && c != SM_PC_NOTHING && c != SM_PC_SURFACE &&
        c != SM_PC_VOLUME)
      return SC_ERR_ARG;
    v.m_vMid.SetClassObject((SmPointClassificationType)c,
                            c == SM_PC_SURFACE  ? surface.get()
                            : c == SM_PC_VOLUME ? (SmObject *)volume.get()
                                                : NULL);
  }
  SmStatus status = SM_ERR;
  switch (op) {
  case 0:
    status = cc.IntersectNaturalBounds(*surface);
    break;
  case 1:
    status = cc.IntersectNaturalUVBounds(*surface);
    break;
  case 2:
    status = cc.IntersectUVDiscontinuties(*surface, ctx);
    break;
  case 3:
    status = cc.Intersect3DDiscontinuties(*surface, ctx);
    break;
  case 4:
    status = cc.Intersect3DDiscontinuities(*volume, in[21] != 0, ctx);
    break;
  }
  std::vector<double> result;
  result.push_back(status == SM_SUCCESS ? 0 : status);
  result.push_back(cc.GetSize());
  std::map<SmObject *, int64_t> object_ids;
  object_ids.emplace(curve.get(), 0);
  object_ids.emplace(surface.get(), 1);
  if (volume)
    object_ids.emplace(volume.get(), 2);
  int64_t next_id = 3;
  for (ULONG i = 0; i < cc.GetSize(); ++i) {
    const SmCurveInterval &v = cc.GetAt(i);
    result.push_back(v.GetMin());
    result.push_back(v.GetMax());
    for (const SmPointClassification *p : {&v.m_vStart, &v.m_vMid, &v.m_vEnd}) {
      result.push_back(p->GetPointClass());
      SmObject *obj = p->GetObject();
      if (!obj)
        result.push_back(-1);
      else {
        auto found = object_ids.find(obj);
        if (found == object_ids.end())
          found = object_ids.emplace(obj, next_id++).first;
        result.push_back((double)found->second);
      }
      result.push_back(p->GetTParam());
      result.push_back(p->GetGap3d());
    }
  }
  return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, len);
  SC_CATCH
}
