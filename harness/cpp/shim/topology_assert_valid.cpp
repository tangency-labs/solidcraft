// SPDX-License-Identifier: Apache-2.0
// Native pinned SmBrep::AssertValid (#1419) and the capi smlib_brep_check sequence
// (ValidatePointers, then AssertValid when it reported no failure).
#include "sc_edgeuse_graph.h"
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>
#include <SmAssertArray.h>
#include <SmEllipse.h>
#include <SmPlane.h>
#include <array>
#include <cstdio>
#include <cstring>
#include <string>

// Defined (not declared) in SmAssertArray.cpp.
TCHAR const* SM_GetObjTypeString(SM_TYPE lExeClassType);
// The level-2 AssertValid tests, defined (not declared) in SmFace.cpp, SmEdge.cpp and
// SmVertex.cpp (their default arguments are NULL).
SmBoolean sm_AssertTestFace32(const SmFace*, SmTArray<SmVertex*>*, SmTArray<SmVertex*>*);
SmBoolean sm_AssertTestFace33(const SmFace*, SmTArray<SmEdge*>*, SmTArray<SmEdge*>*, SmTArray<SmVertex*>*,
                              SmTArray<SmVertex*>*);
SmBoolean sm_AssertTestFace38(const SmFace*);
SmBoolean sm_AssertTestEdge13(const SmEdge*, double*);
SmBoolean sm_AssertTestVertex1(const SmVertex*, double*, SmFace**, SmBoolean*, double*, SmPoint3d*);

namespace {
using Id = std::array<int64_t, 2>;
struct ElPeek : SmEllipse {
    static SmPolarConversion SmEllipse::*polar() { return &ElPeek::m_vPolarConverter; }
};
const void* polar_of(const SmCurve* c) {
    if (!c || !c->IsKindOf(SmEllipse_TYPE)) return nullptr;
    return (const void*)&(((const SmEllipse*)c)->*ElPeek::polar());
}
Id identity(const Graph& g, const void* p, const SmBrep* b) {
    if (!p) return {-1, -1};
    if (p == b) return {16011, 0};
    const int64_t types[] = {16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021};
    for (int k = 0; k < 10; k++) {
        auto found = g.index[k].find(p);
        if (found != g.index[k].end()) return {types[k], found->second};
    }
    for (size_t i = 0; i < g.edges.size(); i++) {
        if (g.edges[i]->GetCurve() == p) return {20001, (int64_t)i};
        if (polar_of(g.edges[i]->GetCurve()) == p) return {20004, (int64_t)i};
    }
    for (size_t i = 0; i < g.faces.size(); i++) {
        const SmSurface* s = g.faces[i]->GetSurface();
        if (s == p) return {20002, (int64_t)i};
        // SmPlane::AssertValid's m_vPosition.AssertValid reports name the placement; the
        // port has no separate placement object and names its surface.
        if (s && s->IsKindOf(SmPlane_TYPE) && (const void*)&((const SmPlane*)s)->GetPosition() == p)
            return {20002, (int64_t)i};
    }
    for (size_t i = 0; i < g.edgeuses.size(); i++) {
        SmBSplineCurve* uv = g.edgeuses[i]->*EuPeek::uv();
        if (uv == p) return {20003, (int64_t)i};
        if (polar_of(uv) == p) return {20005, (int64_t)i};
    }
    return {-3, -3};
}
// The report text with every edge address the messages print (`%p`) replaced by the
// edge's report identity `0x<(16016 << 32) | index>`, as the Rust side prints it.
std::string normalised(const Graph& g, const TCHAR* text) {
    std::string s = text ? text : "";
    if (s.find("0x") == std::string::npos) return s;
    for (size_t i = 0; i < g.edges.size(); i++) {
        char ptr[64], key[64];
        std::snprintf(ptr, sizeof ptr, "%p", (const void*)g.edges[i]);
        std::snprintf(key, sizeof key, "%#llx", (unsigned long long)((16016ULL << 32) | (unsigned long long)i));
        for (size_t at = s.find(ptr); at != std::string::npos; at = s.find(ptr, at + std::strlen(key)))
            s.replace(at, std::strlen(ptr), key);
    }
    return s;
}
void put_id(std::vector<double>& out, const Id& id) { out.push_back((double)id[0]); out.push_back((double)id[1]); }
void put_text(std::vector<double>& out, const std::string& s) {
    out.push_back((double)s.size());
    for (unsigned char c : s) out.push_back((double)c);
}
void put_text(std::vector<double>& out, const TCHAR* text) { put_text(out, std::string(text ? text : "")); }
void put_reports(std::vector<double>& out, const Graph& g, SmBrep* b, SmAssertArray& reports) {
    out.push_back((double)reports.GetSize());
    for (ULONG i = 0; i < reports.GetSize(); i++) {
        SmAssertReport* r = reports[i];
        out.insert(out.end(), {(double)r->m_lReportingType, (double)r->m_lListIndex, (double)r->m_lTestIndex,
                               (double)r->m_eAssertType, flg(r->m_bOK)});
        put_id(out, identity(g, r->m_pReporter, b));
        put_id(out, identity(g, r->m_pOwner, b));
        put_id(out, identity(g, r->m_pOther, b));
        out.insert(out.end(), {(double)r->m_lOwnerType, (double)r->m_lOtherType, (double)r->m_sTol3d, r->m_dValue});
        put_text(out, r->m_pName);
        put_text(out, normalised(g, r->m_pMessage));
        put_text(out, r->m_pMessageForFile);
        put_text(out, r->m_sOwnerTypeString);
        put_text(out, r->m_sOtherTypeString);
        TCHAR buff[4096];
        r->FormatLogMessage(buff, sizeof buff / sizeof buff[0]);
        put_text(out, normalised(g, buff));
        put_text(out, SM_GetObjTypeString(r->m_lReportingType));
    }
    out.push_back(flg(reports.GetLastAdd()));
}
struct EdCurvePeek : SmEdge { static SmCurve* SmEdge::*curve() { return &EdCurvePeek::m_pCurve; } };
struct EuCcwPeek : SmEdgeuse { static SmEdgeuse* SmEdgeuse::*ccw() { return &EuCcwPeek::m_pCCW; } };
struct FcSurfacePeek : SmFace { static SmSurface* SmFace::*surface() { return &FcSurfacePeek::m_pSurface; } };
struct FuOrientPeek : SmFaceuse { static SmOrientType SmFaceuse::*orient() { return &FuOrientPeek::m_eOrientation; } };
struct LuOrientPeek : SmLoopuse { static SmOrientType SmLoopuse::*orient() { return &LuOrientPeek::m_eOrientation; } };
struct VertexPointPeek : SmVertex { static SmPoint3d SmVertex::*point() { return &VertexPointPeek::m_vPoint; } };
// The corruptions of topology_brep_validation.cpp (kinds 1-10), restored before the
// B-rep is destroyed.
struct Mutation {
    Graph& g; int kind; size_t index; double old = 0; SmObject* owner = nullptr; SmCurve* curve = nullptr;
    SmSurface* surface = nullptr; SmEdgeuse* ccw = nullptr; SmPoint3d point;
    Mutation(Graph& graph, int k, size_t ix, double value) : g(graph), kind(k), index(ix) {
        if (kind == 1 && !g.vertices.empty()) { index %= g.vertices.size(); old = g.vertices[index]->GetTolerance(); g.vertices[index]->SetTolerance(value, FALSE); }
        if (kind == 2 && !g.edges.empty()) { index %= g.edges.size(); old = g.edges[index]->GetTolerance(); g.edges[index]->SetTolerance(value, FALSE, FALSE); }
        if (kind == 3 && !g.faces.empty()) { index %= g.faces.size(); old = g.faces[index]->GetTolerance(); g.faces[index]->SetTolerance(value, FALSE, FALSE); }
        if (kind == 4 && !g.faceuses.empty()) { index %= g.faceuses.size(); old = g.faceuses[index]->GetOrientation(); g.faceuses[index]->*FuOrientPeek::orient() = (SmOrientType)(int)value; }
        if (kind == 5 && !g.loopuses.empty()) { index %= g.loopuses.size(); old = g.loopuses[index]->GetOrientation(); g.loopuses[index]->*LuOrientPeek::orient() = (SmOrientType)(int)value; }
        if (kind == 6 && !g.vertices.empty()) { index %= g.vertices.size(); point = g.vertices[index]->GetPoint(); g.vertices[index]->*VertexPointPeek::point() = point + SmVector3d(value, 0, 0); }
        if (kind == 7 && !g.edges.empty()) { index %= g.edges.size(); curve = g.edges[index]->GetCurve(); if (curve) { owner = curve->GetOwner(); curve->SetOwner(NULL); } }
        if (kind == 8 && !g.faces.empty()) { index %= g.faces.size(); surface = g.faces[index]->GetSurface(); g.faces[index]->*FcSurfacePeek::surface() = nullptr; }
        if (kind == 9 && !g.edges.empty()) { index %= g.edges.size(); curve = g.edges[index]->GetCurve(); g.edges[index]->*EdCurvePeek::curve() = nullptr; }
        if (kind == 10 && !g.edgeuses.empty()) { index %= g.edgeuses.size(); ccw = g.edgeuses[index]->GetCCWEdgeuse(); g.edgeuses[index]->*EuCcwPeek::ccw() = nullptr; }
    }
    ~Mutation() {
        if (kind == 1 && !g.vertices.empty()) g.vertices[index]->SetTolerance(old, FALSE);
        if (kind == 2 && !g.edges.empty()) g.edges[index]->SetTolerance(old, FALSE, FALSE);
        if (kind == 3 && !g.faces.empty()) g.faces[index]->SetTolerance(old, FALSE, FALSE);
        if (kind == 4 && !g.faceuses.empty()) g.faceuses[index]->*FuOrientPeek::orient() = (SmOrientType)(int)old;
        if (kind == 5 && !g.loopuses.empty()) g.loopuses[index]->*LuOrientPeek::orient() = (SmOrientType)(int)old;
        if (kind == 6 && !g.vertices.empty()) g.vertices[index]->*VertexPointPeek::point() = point;
        if (kind == 7 && curve) curve->SetOwner(owner);
        if (kind == 8 && !g.faces.empty()) g.faces[index]->*FcSurfacePeek::surface() = surface;
        if (kind == 9 && !g.edges.empty()) g.edges[index]->*EdCurvePeek::curve() = curve;
        if (kind == 10 && !g.edgeuses.empty()) g.edgeuses[index]->*EuCcwPeek::ccw() = ccw;
    }
};
// `200 origin[3] r`: SmApiCreateSphere; `201 origin[3] r h`: SmApiCreateCylinder;
// `202 origin[3] rb rt h`: SmApiCreateCone; `203 origin[3] R r`: SmApiCreateTorus.
SmBrep* primitive(const double* in, int64_t n, SmStatus& status) {
    const int kind = (int)in[0];
    const int64_t want = kind == 200 ? 5 : kind == 201 ? 6 : kind == 202 ? 7 : 6;
    if (n != want) { status = SM_ERR; return nullptr; }
    SmVector3d o(in[1], in[2], in[3]);
    SmBrep* b = nullptr;
    SmApiStatus s = kind == 200 ? SmApiCreateSphere(o, in[4], b)
                  : kind == 201 ? SmApiCreateCylinder(o, in[4], in[5], b)
                  : kind == 202 ? SmApiCreateCone(o, in[4], in[5], in[6], b)
                                : SmApiCreateTorus(o, in[4], in[5], b);
    status = (SmStatus)s;
    if (s != SM_SUCCESS) { delete b; return nullptr; }
    return b;
}
} // namespace

// op 0: AssertValid(&reports, level, walk); op 1: the capi sequence at `level` with
// the walk; op 2: the level-2 tests one by one, as SmFace/SmEdge/SmVertex::AssertValid
// call them: per face sm_AssertTestFace32, 33 and 38 (-1 without surface), then per
// edge sm_AssertTestEdge13 (passed, max gap), then per vertex sm_AssertTestVertex1
// (passed, max gap 3d, max gap face index or -1), after the three counts; op 3: the capi
// sequence of op 1 on the body's SmApiBrepCopy (no mutation), as a caller checks a copy
// (#1584). args: level, walk, mutation kind, index, value.
SC_EXPORT int32_t sc_topology_assert_valid_eval(int32_t op, const char* path, const double* input, int64_t n,
                                               const double* args, int64_t na, double* out, int64_t cap, int64_t* len) {
    if (op < 0 || op > 3 || !input || n < 1 || !args || na != 5) return SC_ERR_ARG;
    if (op == 3 && args[2] != 0) return SC_ERR_ARG;
    SC_TRY
    Rd rd{input, 0};
    SmStatus status = SM_SUCCESS;
    Built built;
    const bool prim = input[0] >= 200.0 && input[0] <= 203.0;
    built.brep = prim ? primitive(input, n, status) : build(rd, path, status);
    std::vector<double> values;
    if (built.brep && op == 3) {
        SmBrep* copy = nullptr;
        status = (SmStatus)SmApiBrepCopy(built.brep, copy);
        delete built.brep;
        built.brep = status == SM_SUCCESS ? copy : nullptr;
        if (!built.brep) delete copy;
    }
    if (!built.brep) {
        values = {-1, (double)status};
        return sc_copy_out(values.data(), (int64_t)values.size(), out, cap, len);
    }
    Graph g;
    walk(built.brep, g);
    values.push_back(0);
    {
        Mutation mutation(g, (int)args[2], (size_t)args[3], args[4]);
        const SmAssertTestLevel level = (SmAssertTestLevel)(int)args[0];
        if (op == 2) {
            values.insert(values.end(), {(double)g.faces.size(), (double)g.edges.size(), (double)g.vertices.size()});
            for (SmFace* f : g.faces) {
                values.push_back(flg(sm_AssertTestFace32(f, NULL, NULL)));
                values.push_back(flg(sm_AssertTestFace33(f, NULL, NULL, NULL, NULL)));
                values.push_back(f->GetSurface() ? flg(sm_AssertTestFace38(f)) : -1.0);
            }
            for (SmEdge* e : g.edges) {
                double gap = 0.0;
                values.push_back(flg(sm_AssertTestEdge13(e, &gap)));
                values.push_back(gap);
            }
            for (SmVertex* v : g.vertices) {
                double gap = 0.0;
                SmFace* face = NULL;
                values.push_back(flg(sm_AssertTestVertex1(v, &gap, &face, NULL, NULL, NULL)));
                values.push_back(gap);
                double at = -1;
                for (size_t i = 0; i < g.faces.size(); i++)
                    if (g.faces[i] == face) at = (double)i;
                values.push_back(at);
            }
        } else if (op == 0) {
            SmAssertArray reports;
            values.push_back(flg(built.brep->AssertValid(&reports, level, args[1] != 0 ? SM_WALK : SM_NO_WALK)));
            put_reports(values, g, built.brep, reports);
        } else {
            SmAssertArray pointers, asserts;
            values.push_back((double)built.brep->ValidatePointers(&pointers, level));
            put_reports(values, g, built.brep, pointers);
            bool failed = false;
            for (ULONG i = 0; i < pointers.GetSize(); i++) failed |= !pointers[i]->m_bOK;
            values.push_back(failed ? -1.0 : flg(built.brep->AssertValid(&asserts, level, SM_WALK)));
            put_reports(values, g, built.brep, asserts);
        }
    }
    return sc_copy_out(values.data(), (int64_t)values.size(), out, cap, len);
    SC_CATCH
}
