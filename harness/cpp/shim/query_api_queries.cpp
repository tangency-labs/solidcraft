// SPDX-License-Identifier: Apache-2.0
// Native SmApiQueries.cpp calls on concrete analytic/NURBS geometry and B-reps.
// 22-double layout, optionally followed by the second box's origin and size
// for relationship/distance queries; validated by query_api_queries.rs. Graph construction
// and raw export reuse the established topology-edgeuse harness; no query result
// is manufactured from the graph export.
#include "sc_edgeuse_graph.h"
#include <SmApiQueries.h>
#include <SmApiPrimitives.h>
#include <SmCircle.h>
#include <SmEllipse.h>
#include <SmSurfOfRevolution.h>
#include <memory>
namespace
{
    SmCurve* api_curve(const double* a)
    {
        const auto& ctx  =  the_context();
        SmPoint3d p(a[0], a[1], a[2]);
        SmVector3d v(a[3], a[4], a[5]);
        double s = a[6];
        int kind = (int)a[21];
        if (kind == 0) return new (ctx) SmLine(p, v, SmExtent1d(a[7], a[8]), s, 3, &ctx, nullptr, a[18]?TRUE:FALSE);
        if (kind == 1) return new (ctx) SmCircle(p, SmVector3d(1, 0, 0), SmVector3d(0, 1, 0), SmExtent1d(0, 360), s, 3, &ctx, nullptr, a[18]?TRUE:FALSE);
        if (kind == 2) return new (ctx) SmEllipse(p, SmVector3d(1, 0, 0), SmVector3d(0, 1, 0), SmExtent1d(0, 360), s, s*0.5, 3, &ctx, nullptr, a[18]?TRUE:FALSE);
        if (kind == 10)
        {
            gw_CURVE* c = sm_AllocateNurbCurve(2, 1, 4);
            const SmPoint3d pts[] = {p, p+SmVector3d(s, 0, 0), p+SmVector3d(s, s, 0)};
            const double knots[] = {0, 0, 0.5, 1, 1};
            for (int i = 0; i < 3; ++i)
            {
                c->pol->Pw[i].x = pts[i].x;
                c->pol->Pw[i].y = pts[i].y;
                c->pol->Pw[i].z = pts[i].z;
                c->pol->Pw[i].w = 1;
            }
            for (int i = 0; i < 5; ++i)c->knt->U[i] = knots[i];
            return new (ctx) SmBSplineCurve(c, 3, FALSE, &ctx);
        }
        // Cubic Bezier, also exercising zero endpoint derivatives when v is zero.
        gw_CURVE* c = sm_AllocateNurbCurve(3, 3, 7);
        const SmPoint3d pts[] =
        {
            p, p+s*v, p+SmVector3d(s, s, 0), p+SmVector3d(2*s, 0, s)
        };
        for (int i = 0;i<4;++i)
        {
            c->pol->Pw[i].x = pts[i].x;
            c->pol->Pw[i].y = pts[i].y;
            c->pol->Pw[i].z = pts[i].z;
            c->pol->Pw[i].w = 1;
        }
        for (int i = 0;i<8;++i)c->knt->U[i] = i<4?0:1;
        return new (ctx) SmBSplineCurve(c, 3, FALSE, &ctx);
    }
    SmSurface* api_surface(const double* a)
    {
        const auto& ctx = the_context();
        if ((int)a[21] == 13)
        {
            const SmPoint3d origin(a[0], a[1], a[2]);
            SmLine* generator = new (ctx) SmLine(origin+SmVector3d(a[6], 0, 0), SmVector3d(0, 0, 1), SmExtent1d(0, 1), a[6], 3, &ctx);
            SmSurfOfRevolution* surface = nullptr;
            const SmStatus status = SmSurfOfRevolution::CreateCanonical(ctx, generator, origin, SmVector3d(0, 0, 1), surface);
            if (status != SM_SUCCESS)
            {
                if (surface)delete surface;
                else delete generator;
                return nullptr;
            }
            return surface;
        }
        if ((int)a[21] == 4 || (int)a[21] == 11)return new (ctx) SmPlane(SmPoint3d(a[0], a[1], a[2]), SmVector3d(a[3], a[4], a[5]), &ctx);
        if ((int)a[21] == 8 || (int)a[21] == 9)
        {
            const bool sphere = (int)a[21] == 9;
            const int n = sphere?4:1, degree = sphere?2:1, r = sphere?7:3;
            gw_SURFACE* s = sm_AllocateNurbSurface(n, 8, degree, 2, r, 11);
            const double xy[9][2] = {{1, 0}, {1, 1}, {0, 1}, {-1, 1}, {-1, 0},
                                     {-1, -1}, {0, -1}, {1, -1}, {1, 0}};
            const double radial[5] = {0, 1, 1, 1, 0}, height[5] = {1, 1, 0, -1, -1};
            const double w = smos_Sqrt(0.5);
            for (int i = 0;i <= n;++i)for (int j = 0;j<9;++j)
            {
                const double wi = sphere && i%2?w:1, wj = j%2?w:1, weight = wi*wj;
                const double rad = sphere?radial[i]:1, z = sphere?height[i]:(double)i;
                s->net->Pw[i][j].x = (a[0]+a[6]*rad*xy[j][0])*weight;
                s->net->Pw[i][j].y = (a[1]+a[6]*rad*xy[j][1])*weight;
                s->net->Pw[i][j].z = (a[2]+a[6]*z)*weight;
                s->net->Pw[i][j].w = weight;
            }
            const double ku_sphere[8] =
            {
                0, 0, 0, 0.5, 0.5, 1, 1, 1
            };
            const double kv[12] =
            {
                0, 0, 0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1, 1, 1
            };
            for (int i = 0;i <= r;++i)s->knu->U[i] = sphere?ku_sphere[i]:(i<2?0:1);
            for (int j = 0;j<12;++j)s->knv->U[j] = kv[j];
            return new (ctx) SmBSplineSurface(s, FALSE, nullptr);
        }
        gw_SURFACE* s = sm_AllocateNurbSurface(2, 2, 2, 2, 5, 5);
        for (int i = 0;i<3;++i)for (int j = 0;j<3;++j)
        {
            s->net->Pw[i][j].x = a[0]+a[6]*i;
            s->net->Pw[i][j].y = a[1]+a[6]*j;
            s->net->Pw[i][j].z = a[2]+(i == 1 && j == 1?a[6]:0);
            s->net->Pw[i][j].w = 1;
        }
        for (int i = 0;i<6;++i)
        {
            s->knu->U[i] = i<3?0:1;
            s->knv->U[i] = i<3?0:1;
        }
        return new (ctx) SmBSplineSurface(s, FALSE, nullptr);
    }
    void api_point(std::vector<double>& o, SmStatus status, const SmPoint3d& p, double dist)
    {
        o.push_back(st(status));
        if (status == SM_SUCCESS)
        {
            put_vec(o, p);
            o.push_back(dist);
        }
    }
}
SC_EXPORT int32_t sc_query_api_queries_eval(int32_t op, const double* a, int64_t n, double* out, int64_t cap, int64_t* len)
{
    if (!a || (n != 22 && !(n == 26 && (op == 7 || op == 8))) || op<0 || op>26)return SC_ERR_ARG;
    SC_TRY
    const auto& ctx = the_context();
    std::vector<double> o;
    SmPoint3d p(a[11], a[12], a[13]), q;
    double t = 0, d = 0;
    bool null = a[16] != 0;
    int kind = (int)a[21];
    if (op == 1 || op == 2)
    {
        std::unique_ptr<SmCurve> c(api_curve(a));
        if (!c)return SC_ERR_ARG;
        if (op == 1)
        {
            SmStatus s = SmApiCurveClosestPoint(null?nullptr:c.get(), p, q, t, d);
            api_point(o, s, q, d);
            if (s == SM_SUCCESS)o.push_back(t);
        }
        else
        {
            SmTArray<SmPoint3d> ps;
            SmTArray<double> ts, ds;
            SmStatus s = SmApiCurveClosestPointAll(null?nullptr:c.get(), p, ps, ts, ds);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(ps.GetSize());
                for (ULONG i = 0;i<ps.GetSize();++i)
                {
                    put_vec(o, ps[i]);
                    o.push_back(ds[i]);
                    o.push_back(ts[i]);
                }
            }
        }
    }
    else if (op == 3)
    {
        std::unique_ptr<SmSurface> s(api_surface(a));
        SmTArray<SmPoint2d> uv;
        SmStatus status = SmApiSurfaceClosestPoint(null?nullptr:s.get(), p, q, uv, d);
        api_point(o, status, q, d);
        if (status == SM_SUCCESS)
        {
            o.push_back(uv.GetSize());
            for (ULONG i = 0;i<uv.GetSize();++i)
            {
                o.push_back(uv[i].x);
                o.push_back(uv[i].y);
            }
        }
    }
    else
    {
        Built b;
        b.brep = new (ctx) SmBrep();
        SmStatus status = SM_SUCCESS;
        if (kind <= 3 || kind == 10)
        {
            SmEdge* e = nullptr;
            status = b.brep->CreateWireEdgeFromCurve(api_curve(a), SmExtent1d(a[7], a[8]), e);
        }
        else if (kind == 4 || kind == 5 || kind == 8 || kind == 9 || kind == 11)
        {
            SmFace* f = nullptr;
            status = b.brep->CreateFaceFromSurface(api_surface(a), SmExtent2d(SmPoint2d(a[7], a[8]), SmPoint2d(a[9], a[10])), f);
            if (status == SM_SUCCESS && kind == 11)
            {
                for (double fraction : {0.25, 0.75})
                {
                    SmPoint3d point;
                    status = f->GetSurface()->EvaluatePoint(SmPoint2d(a[7]+fraction*(a[9]-a[7]), a[8]+fraction*(a[10]-a[8])), point);
                    if (status != SM_SUCCESS)break;
                    SmLoop* loop = nullptr;
                    SmVertex* vertex = nullptr;
                    status = b.brep->MakeVertexLoop(f, point, loop, vertex);
                    if (status != SM_SUCCESS)break;
                }
            }
        }
        else if (kind == 6)
        {
            delete b.brep;
            b.brep = nullptr;
            status = SmApiCreateBox(SmPoint3d(a[0], a[1], a[2]), a[6], a[6], a[6], b.brep);
        }
        else if (kind == 12)
        {
            // Two real closed curve loops: an outer square and a square hole.
            const double xy[8][2] = {{0, 0}, {4, 0}, {4, 4}, {0, 4},
                                     {1, 1}, {1, 3}, {3, 3}, {3, 1}};
            SmTArray<SmCurve*> curves;
            for (int i = 0; i < 8; ++i)
            {
                const int next = (i/4)*4+(i+1)%4;
                const SmPoint3d start(a[0]+a[6]*xy[i][0], a[1]+a[6]*xy[i][1], a[2]);
                const SmPoint3d end(a[0]+a[6]*xy[next][0], a[1]+a[6]*xy[next][1], a[2]);
                SmBSplineCurve* c = nullptr;
                status = SmBSplineCurve::CreateLineSegment(ctx, 3, start, end, c);
                if (status != SM_SUCCESS)break;
                curves.Add(c);
            }
            SmFace* face = nullptr;
            if (status == SM_SUCCESS)
                status = b.brep->CreatePlanarFaceWith3DCurves(nullptr, curves, 1e-5, face);
            if (status != SM_SUCCESS)
                for (ULONG i = 0; i < curves.GetSize(); ++i)
                    if (!curves[i]->GetOwner())delete curves[i];
        }
        if (status != SM_SUCCESS || !b.brep)return SC_ERR_ARG;
        Graph g;
        walk(b.brep, g);
        size_t sel = (size_t)a[20];
        SmEdge* e = g.edges.empty()?nullptr:g.edges[sel%g.edges.size()];
        SmFace* f = g.faces.empty()?nullptr:g.faces[sel%g.faces.size()];
        SmVertex* v = g.vertices.empty()?nullptr:g.vertices[sel%g.vertices.size()];
        if (e && a[19] >= 0)e->SetTolerance(a[19], FALSE, FALSE);
        if (e && a[17])e->SetCurve(nullptr, TRUE, FALSE, FALSE);
        if ((op == 11 || op == 26) && f && a[18] != 0)
        {
            SmTArray<SmLoopuse*> loops;
            f->GetUpwardLoopuses(loops);
            for (ULONG i = loops.GetSize(); i > 0; --i)
            {
                SmTArray<SmEdgeuse*> uses;
                loops[i-1]->GetEdgeuses(uses);
                if (uses.GetSize() > 0)
                {
                    uses[uses.GetSize()-1]->SetOrientation(SM_OT_UNKNOWN);
                    break;
                }
            }
        }
        if (op == 0 || op == 26)
        {
            o.push_back(0);
            export_graph(b.brep, g, o);
            // Face back-links and trim flags are absent from the shared exporter.
            for (SmFace* face : g.faces)
            {
                o.push_back(g.find(2, face->GetUpwardFaceuse()));
                o.push_back(face->GetRectangularTrim());
            }
        }
        else
        {
            if (null)
            {
                e = nullptr;
                f = nullptr;
                v = nullptr;
            }
            SmBrep* bp = null?nullptr:b.brep;
            SmPoint3d lo, hi;
            int cl = 0;
            switch (op)
            {
                case 4:case 5:case 6:
                {
                    status = op == 4?SmApiGetClosestPoint(bp, p, q, d):op == 5?SmApiBrepClosestPoint(bp, p, q, d):SM_SUCCESS;
                    if (op == 6)
                    {
                        SmTArray<SmPoint3d> ps;
                        SmTArray<double> ds;
                        status = SmApiBrepClosestPointAll(bp, p, ps, ds);
                        o.push_back(st(status));
                        if (status == SM_SUCCESS)
                        {
                            o.push_back(ps.GetSize());
                            for (ULONG i = 0;i<ps.GetSize();++i)
                            {
                                put_vec(o, ps[i]);
                                o.push_back(ds[i]);
                            }
                        }
                    }
                    else api_point(o, status, q, d);
                    break;
                }
                case 7:case 8:
                {
                    SmBrep* other = nullptr;
                    const SmPoint3d origin = n == 26?SmPoint3d(a[22], a[23], a[24]):SmPoint3d(a[0]+3*a[6], a[1], a[2]);
                    const double size = n == 26?a[25]:a[6];
                    SmStatus build_status = SmApiCreateBox(origin, size, size, size, other);
                    if (build_status != SM_SUCCESS || !other) return SC_ERR_ARG;
                    status = op == 7?SmApiBrepRelationship(bp, other, cl, d):SmApiBrepDistance(bp, other, d);
                    delete other;
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)
                    {
                        o.push_back(d);
                        if (op == 7)o.push_back(cl);
                    }
                    break;
                }
                case 9:
                {
                    SmTArray<SmEdge*> es;
                    status = SmApiGetEdges(bp, es);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)
                    {
                        o.push_back(es.GetSize());
                        for (ULONG i = 0;i<es.GetSize();++i)o.push_back(g.find(7, es[i]));
                    }
                    break;
                }
                case 10:
                {
                    SmTArray<SmFace*> fs;
                    status = SmApiGetFaces(bp, fs);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)
                    {
                        o.push_back(fs.GetSize());
                        for (ULONG i = 0;i<fs.GetSize();++i)o.push_back(g.find(3, fs[i]));
                    }
                    break;
                }
                case 11:
                {
                    SmTArray<ULONG> ns;
                    SmTArray<SmEdge*> es;
                    SmTArray<SmOrientType> os;
                    // The API resets pre-existing outputs, including on failure.
                    ns.Add(99); es.Add(nullptr); os.Add(SM_OT_UNKNOWN);
                    status = SmApiFaceGetBoundaryLoops(f, ns, es, os);
                    if (status != SM_SUCCESS &&
                        (ns.GetSize() != 0 || es.GetSize() != 0 || os.GetSize() != 0))
                        return SC_ERR_ARG;
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)
                    {
                        o.push_back(ns.GetSize());
                        for (ULONG i = 0;i<ns.GetSize();++i)o.push_back(ns[i]);
                        o.push_back(es.GetSize());
                        for (ULONG i = 0;i<es.GetSize();++i)
                        {
                            o.push_back(g.find(7, es[i]));
                            o.push_back(os[i]);
                        }
                    }
                    break;
                }
                case 12:
                {
                    ULONG i = 0;
                    status = SmApiFindEdge(bp, p, i, t);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)
                    {
                        o.push_back(i);
                        o.push_back(t);
                    }
                    break;
                }
                case 13:
                {
                    SmFace* found = nullptr;
                    status = SmApiFindFaces(bp, p, found);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)o.push_back(g.find(3, found));
                    break;
                }
                case 14:
                {
                    SmBoolean yes = FALSE;
                    status = SmApiBrepIsManifoldSolid(bp, yes);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)o.push_back(yes?1:0);
                    break;
                }
                case 15:case 16:case 20:status = op == 15?SmApiBrepBoundingBox(bp, a[15]?TRUE:FALSE, lo, hi):op == 16?SmApiFaceBoundingBox(f, a[15]?TRUE:FALSE, lo, hi):SmApiEdgeBoundingBox(e, a[15]?TRUE:FALSE, lo, hi);
                o.push_back(st(status));
                if (status == SM_SUCCESS)
                {
                    put_vec(o, lo);
                    put_vec(o, hi);
                }
                break;
                case 17:status = SmApiFaceInternalPoint(f, q);
                o.push_back(st(status));
                if (status == SM_SUCCESS)put_vec(o, q);
                break;
                case 18:status = SmApiFaceClassifyUV(f, SmPoint2d(a[11], a[12]), cl);
                o.push_back(st(status));
                if (status == SM_SUCCESS)o.push_back(cl);
                break;
                case 19:status = SmApiFaceClosestPoint(f, p, q, d);
                api_point(o, status, q, d);
                break;
                case 21:status = SmApiEdgeClosestPoint(e, p, q, t, d);
                api_point(o, status, q, d);
                if (status == SM_SUCCESS)o.push_back(t);
                break;
                case 22:
                {
                    double hi_t = 0;
                    status = SmApiEdgeParameterRange(e, t, hi_t);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS)
                    {
                        o.push_back(t);
                        o.push_back(hi_t);
                    }
                    break;
                }
                case 23:status = SmApiEdgeClassifyParameter(e, a[14], cl);
                o.push_back(st(status));
                if (status == SM_SUCCESS)o.push_back(cl);
                break;
                case 24:status = SmApiEdgeTangent(e, a[14], q);
                o.push_back(st(status));
                if (status == SM_SUCCESS)put_vec(o, q);
                break;
                case 25:status = SmApiVertexGetPoint(v, q);
                o.push_back(st(status));
                if (status == SM_SUCCESS)put_vec(o, q);
                break;
                default:return SC_ERR_ARG;
            }
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, len);
    SC_CATCH
}
