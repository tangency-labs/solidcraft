// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p10`: SmBrep::MakeTopologyFromData,
// SmBrep::TestClosureSplitRegion (edge version), SmBrep::RemoveAllTopology and
// SmBrep::ProjectAndTrim (source/SMLib/src/SmBrep.cpp) on B-rep data read from an
// upstream test file.
//
// One entry point, sc_topology_brep_p10_run(path, ints, n_ints, dbls, n_dbls, out, cap,
// out_len). The request and output layouts are documented in
// crates/solidcore-ffi/src/units/topology_brep_p10.rs. The output is a stream of int64
// pairs (tag, value): tag 0 an integer, tag 1 the bits of a double.
//
// The B-rep data is read with SmBrepData::ReadFromFile, patched as the request says,
// then checked by a simulation of MakeTopologyFromData's record indexing (`guard`):
// data on which upstream would index past an array or dereference the null object of
// a record never built (or build lists the Rust arena model cannot hold and upstream
// would loop on) is not passed to upstream; the shim outputs -2 instead.
//
// The B-rep graph is exported in traversal order from the regions (each pointer mapped
// to its first-seen index per kind; -1 is null), reading raw members.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmNurbsCrv.h>
#include <NL_Nurbsdef.h>
#include <nurbs.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <algorithm>
#include <climits>
#include <cstring>
#include <map>
#include <vector>

namespace
{
enum Op : int64_t
{
    OP_MAKE = 0,
    OP_CLOSURE = 1,
    OP_REMOVE_ALL = 2,
    OP_PROJECT = 3,
};

enum Patch : int64_t
{
    P_ZONE_TOL = 0,  // d: m_sZoneTol3d (stored raw, without SmZoneTol3d's fabs)
    P_MANIFOLD = 1,  // i: m_bManifold
    P_HEALER = 2,    // i: SetHealerVersion
    P_BORROWED = 3,  // i: m_bIsGeomBorrowed
    P_STAMP = 4,     // i: flags / user indices of every record from a base value
    P_IGES = 5,      // keep only region 1 and its shells (an IGES solid); needs two regions
    P_SCALE = 6,     // d: scale every vertex point
    P_DROP_UV = 7,   // i: every i-th edge use record loses its UV curve
    P_SET = 8,       // i: field code, then two more ints: record index, value
};

// Status of a request the guard keeps away from upstream.
const int64_t ST_GUARD = -2;
// Matches the per-unit Rust FFI status; no unsafe projection is attempted.
const int32_t STATUS_UNDEFINED_PROJECTION_DEGREE = -1101;

struct Out
{
    std::vector<int64_t> v;
    void i(int64_t x)
    {
        v.push_back(0);
        v.push_back(x);
    }
    void d(double x)
    {
        int64_t bits;
        std::memcpy(&bits, &x, sizeof bits);
        v.push_back(1);
        v.push_back(bits);
    }
};

int64_t st(SmStatus s) { return s == SM_SUCCESS ? 0 : (int64_t)s; }

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

// ---- raw member access ------------------------------------------------------------------

struct EuPeek : SmEdgeuse
{
    static SmTopology* SmEdgeuse::*sorlu() { return &EuPeek::m_pSorLU; }
    static SmBSplineCurve* SmEdgeuse::*uv() { return &EuPeek::m_pUVTrimCurve; }
    static SmVertexuse* SmEdgeuse::*vu() { return &EuPeek::m_pVU; }
    static SmDropCurveFail* SmEdgeuse::*fail() { return &EuPeek::m_pDropCurveFail; }
    static SmEdgeFaceGap SmEdgeuse::*face_gap() { return &EuPeek::m_sMaxEdgeFaceGap3d; }
    static SmEdgeEdgeGap SmEdgeuse::*ccw_gap() { return &EuPeek::m_sEdgeCCWEdgeGap3d; }
};
struct FuPeek : SmFaceuse
{
    static SmFace* SmFaceuse::*face() { return &FuPeek::m_pF; }
    static SmFaceuse* SmFaceuse::*mate() { return &FuPeek::m_pFUMate; }
};
struct LuPeek : SmLoopuse
{
    static SmLoop* SmLoopuse::*loop() { return &LuPeek::m_pL; }
    static SmLoopuse* SmLoopuse::*mate() { return &LuPeek::m_pLUMate; }
    static SM_TYPE SmLoopuse::*type() { return &LuPeek::m_tLoopuseType; }
    static SmTopology* SmLoopuse::*start() { return &LuPeek::m_pEUorVU; }
    static SmOrientType SmLoopuse::*orient() { return &LuPeek::m_eOrientation; }
};
struct VuPeek : SmVertexuse
{
    static SmTopology* SmVertexuse::*owner() { return &VuPeek::m_pSorLUorEU; }
    static SmVertexFaceGap SmVertexuse::*face_gap() { return &VuPeek::m_sVertexFaceGap3d; }
    static SmVertexEdgeGap SmVertexuse::*edge_gap() { return &VuPeek::m_sVertexEdgeGap3d; }
    static SmPoint2d SmVertexuse::*uv() { return &VuPeek::m_sUVPoint; }
};
struct FPeek : SmFace
{
    static SmFaceuse* SmFace::*fu() { return &FPeek::m_pFU; }
    static SmExtent2d SmFace::*dom() { return &FPeek::m_vUVDomain; }
    static SmBoolean SmFace::*rect() { return &FPeek::m_bRectangularTrim; }
    static SmSurface* SmFace::*srf() { return &FPeek::m_pSurface; }
    static SmZoneTol3d SmFace::*tol() { return &FPeek::m_sZoneTol3d; }
};
struct ShPeek : SmShell
{
    static SM_TYPE SmShell::*type() { return &ShPeek::m_tShellType; }
};

// ---- patches ------------------------------------------------------------------------------

struct Rd
{
    const int64_t* ints;
    int64_t ni;
    int64_t ai = 0;
    const double* dbls;
    int64_t nd;
    int64_t ad = 0;
    bool ok = true;
    int64_t i()
    {
        if (ai >= ni)
        {
            ok = false;
            return 0;
        }
        return ints[ai++];
    }
    double d()
    {
        if (ad >= nd)
        {
            ok = false;
            return 0.0;
        }
        return dbls[ad++];
    }
};

void stamp(SmTopologyData& t, uint64_t base, uint64_t i, uint64_t k)
{
    t.m_lFlags = (ULONG)(base + 7 * i + k);
    t.m_lUserIndex1 = (ULONG)(base * 3 + i);
    t.m_lUserIndex2 = (ULONG)(i * 11 + k);
}

// Field codes of P_SET (layout in the FFI wrapper); an index past the array is a no-op.
bool set_field(SmBrepData& d, int64_t field, int64_t idx, int64_t value)
{
    auto in = [idx](ULONG n) { return idx >= 0 && (uint64_t)idx < (uint64_t)n; };
    if (idx < 0)
        return false;
    ULONG u = (ULONG)value;
    switch (field)
    {
    case 0: if (!in(d.m_vFaces.GetSize())) return true; d.m_vFaces[(ULONG)idx].m_lSurface = u; return true;
    case 1: if (!in(d.m_vFaces.GetSize())) return true; d.m_vFaces[(ULONG)idx].m_lStartLoop = u; return true;
    case 2: if (!in(d.m_vLoops.GetSize())) return true; d.m_vLoops[(ULONG)idx].m_lStartEU = u; return true;
    case 3: if (!in(d.m_vLoops.GetSize())) return true; d.m_vLoops[(ULONG)idx].m_lVertex = u; return true;
    case 4: if (!in(d.m_vEdgeuses.GetSize())) return true; d.m_vEdgeuses[(ULONG)idx].m_lEdge = u; return true;
    case 5: if (!in(d.m_vEdgeuses.GetSize())) return true; d.m_vEdgeuses[(ULONG)idx].m_lLoop = u; return true;
    case 6: if (!in(d.m_vEdgeuses.GetSize())) return true; d.m_vEdgeuses[(ULONG)idx].m_lNextEU = (long)value; return true;
    case 7: if (!in(d.m_vEdges.GetSize())) return true; d.m_vEdges[(ULONG)idx].m_lCurve = u; return true;
    case 8: if (!in(d.m_vEdges.GetSize())) return true; d.m_vEdges[(ULONG)idx].m_lPrimEU = (long)value; return true;
    case 9: if (!in(d.m_vShells.GetSize())) return true; d.m_vShells[(ULONG)idx].m_lShellType = u; return true;
    case 10: if (!in(d.m_vRegions.GetSize())) return true; d.m_vRegions[(ULONG)idx].m_lStartShell = u; return true;
    case 11: if (!in(d.m_vEdgeuses.GetSize())) return true; d.m_vEdgeuses[(ULONG)idx].m_lUVCurve = u; return true;
    case 12: if (!in(d.m_vEdgeuses.GetSize())) return true; d.m_vEdgeuses[(ULONG)idx].m_lEUType = u; return true;
    case 13: if (!in(d.m_vLoops.GetSize())) return true; d.m_vLoops[(ULONG)idx].m_lNumEU = u; return true;
    case 14: if (!in(d.m_vFaceuses.GetSize())) return true; d.m_vFaceuses[(ULONG)idx].m_lFace = u; return true;
    case 15: if (!in(d.m_vEdges.GetSize())) return true; d.m_vEdges[(ULONG)idx].m_lStartVertex = u; return true;
    default: return false;
    }
}

bool apply_patches(Rd& rd, SmBrepData& d, bool* set_used)
{
    int64_t n = rd.i();
    for (int64_t p = 0; p < n && rd.ok; p++)
    {
        int64_t code = rd.i();
        switch (code)
        {
        case P_ZONE_TOL: static_cast<double&>(d.m_sZoneTol3d) = rd.d(); break; // raw, as a read stores it
        case P_MANIFOLD: d.m_bManifold = (SmBoolean)rd.i(); break;
        case P_HEALER: d.SetHealerVersion((ULONG)rd.i()); break;
        case P_BORROWED: d.m_bIsGeomBorrowed = rd.i() != 0 ? TRUE : FALSE; break;
        case P_STAMP:
        {
            uint64_t base = (uint64_t)rd.i();
            for (ULONG k = 0; k < d.m_vRegions.GetSize(); k++) stamp(d.m_vRegions[k], base, k, 0);
            for (ULONG k = 0; k < d.m_vShells.GetSize(); k++) stamp(d.m_vShells[k], base, k, 1);
            for (ULONG k = 0; k < d.m_vFaceuses.GetSize(); k++) stamp(d.m_vFaceuses[k], base, k, 2);
            for (ULONG k = 0; k < d.m_vFaces.GetSize(); k++) stamp(d.m_vFaces[k], base, k, 3);
            for (ULONG k = 0; k < d.m_vLoops.GetSize(); k++) stamp(d.m_vLoops[k], base, k, 4);
            for (ULONG k = 0; k < d.m_vEdgeuses.GetSize(); k++)
            {
                SmEUData& r = d.m_vEdgeuses[k];
                stamp(r, base, k, 5);
                r.m_lMateFlags = (ULONG)(base + 5 * k);
                r.m_lMateUserIndex1 = (ULONG)(k + 1);
                r.m_lMateUserIndex2 = (ULONG)(2 * k);
            }
            for (ULONG k = 0; k < d.m_vEdges.GetSize(); k++) stamp(d.m_vEdges[k], base, k, 6);
            for (ULONG k = 0; k < d.m_vVertices.GetSize(); k++) stamp(d.m_vVertices[k], base, k, 7);
            break;
        }
        case P_IGES:
        {
            // a no-op unless there are two regions whose second's shells exist
            if (d.m_vRegions.GetSize() != 2)
                break;
            SmRegionData r = d.m_vRegions[1];
            ULONG s0 = r.m_lStartShell, ns = r.m_lNumShells;
            if (s0 > d.m_vShells.GetSize() || ns > d.m_vShells.GetSize() - s0)
                break;
            SmTArray<SmShellData> shells;
            for (ULONG k = 0; k < ns; k++)
                shells.Add(d.m_vShells[s0 + k]);
            d.m_vShells = shells;
            r.m_lStartShell = 0;
            SmTArray<SmRegionData> regions;
            regions.Add(r);
            d.m_vRegions = regions;
            d.m_bManifold = TRUE;
            break;
        }
        case P_SCALE:
        {
            double s = rd.d();
            for (ULONG k = 0; k < d.m_vVertices.GetSize(); k++)
                d.m_vVertices[k].m_vPoint = d.m_vVertices[k].m_vPoint * s;
            break;
        }
        case P_DROP_UV:
        {
            int64_t every = rd.i();
            if (every < 1)
                return false;
            for (ULONG k = 0; k < d.m_vEdgeuses.GetSize(); k += (ULONG)every)
                d.m_vEdgeuses[k].m_lUVCurve = 999999;
            break;
        }
        case P_SET:
        {
            int64_t field = rd.i(), idx = rd.i(), value = rd.i();
            *set_used = true;
            if (!set_field(d, field, idx, value))
                return false;
            break;
        }
        default: return false;
        }
    }
    return rd.ok;
}

// ---- guard: MakeTopologyFromData's record indexing, simulated ------------------------------

enum Sim
{
    SIM_OK,     // upstream runs without undefined behaviour
    SIM_REJECT, // undefined behaviour / a list the arena model cannot hold
};

bool attrs_empty(const SmTopologyData& t) { return t.m_sAttributes.GetSize() == 0; }

Sim guard(const SmBrepData& d)
{
    const uint64_t nR = d.m_vRegions.GetSize(), nS = d.m_vShells.GetSize(), nFU = d.m_vFaceuses.GetSize(),
                   nF = d.m_vFaces.GetSize(), nL = d.m_vLoops.GetSize(), nEU = d.m_vEdgeuses.GetSize(),
                   nE = d.m_vEdges.GetSize(), nV = d.m_vVertices.GetSize(), nUV = d.m_vUVCurves.GetSize(),
                   n3D = d.m_v3DCurves.GetSize(), nSrf = d.m_vSurfaces.GetSize();
    const uint64_t NO = SM_NO_OBJECT;
    // sm_AddAttributes indexes the (empty) attribute table
    if (d.m_vAttributes.GetSize() != 0)
        return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vRegions.GetSize(); k++) if (!attrs_empty(d.m_vRegions[k])) return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vShells.GetSize(); k++) if (!attrs_empty(d.m_vShells[k])) return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vFaceuses.GetSize(); k++) if (!attrs_empty(d.m_vFaceuses[k])) return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vFaces.GetSize(); k++) if (!attrs_empty(d.m_vFaces[k])) return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vLoops.GetSize(); k++)
        if (!attrs_empty(d.m_vLoops[k]) || d.m_vLoops[k].m_sLUUpperAttributes.GetSize() ||
            d.m_vLoops[k].m_sLULowerAttributes.GetSize())
            return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vEdgeuses.GetSize(); k++)
        if (!attrs_empty(d.m_vEdgeuses[k]) || d.m_vEdgeuses[k].m_sMateAttributes.GetSize()) return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vEdges.GetSize(); k++) if (!attrs_empty(d.m_vEdges[k])) return SIM_REJECT;
    for (ULONG k = 0; k < d.m_vVertices.GetSize(); k++) if (!attrs_empty(d.m_vVertices[k])) return SIM_REJECT;

    long manifold = d.m_bManifold == UNSURE ? 0 : (long)d.m_bManifold;

    // regions and shells
    std::vector<uint64_t> shell_map;
    std::vector<int> shell_type; // per created shell, in region traversal order: 0 fu, 1 wire, 2 vertex
    if (manifold == 1 && nR == 1)
    {
        for (uint64_t ii = 0; ii < nS; ii++)
        {
            if (ii == 0)
            {
                shell_map.push_back(ii);
                shell_map.push_back(ii + 20000);
            }
            else
            {
                shell_map.insert(shell_map.begin() + (long)(ii + 1), ii + 20000);
                shell_map.push_back(ii);
            }
        }
        shell_type.assign(shell_map.size(), 0);
    }
    else
    {
        for (uint64_t ii = 0; ii < nR; ii++)
        {
            const SmRegionData& r = d.m_vRegions[(ULONG)ii];
            for (uint64_t jj = 0; jj < r.m_lNumShells; jj++)
            {
                uint64_t si = (uint64_t)r.m_lStartShell + jj;
                if (si < jj || si >= nS)
                    return SIM_REJECT;
                shell_map.push_back(si);
                if (d.m_vShells[(ULONG)si].m_lShellType > 2)
                    return SIM_OK; // SER(SM_ERR)
                shell_type.push_back((int)d.m_vShells[(ULONG)si].m_lShellType);
            }
        }
    }

    // vertices: use counts as the vertex use lists grow
    std::vector<uint64_t> vu_count(nV, 0);

    // faces and loops
    std::vector<char> loop_built(nL, 0);
    for (uint64_t ii = 0; ii < nF; ii++)
    {
        const SmFaceData& f = d.m_vFaces[(ULONG)ii];
        if (f.m_lSurface >= nSrf)
            return SIM_REJECT;
        for (uint64_t jj = 0; jj < f.m_lNumLoops; jj++)
        {
            uint64_t li = (uint64_t)f.m_lStartLoop + jj;
            if (li < jj || li >= nL)
                return SIM_REJECT;
            const SmLoopData& l = d.m_vLoops[(ULONG)li];
            loop_built[li] = 1;
            if (l.m_lNumEU == 1 && l.m_lStartEU >= nEU)
                return SIM_REJECT;
            if (l.m_lLoopType == 1)
            {
                if (l.m_lVertex >= nV)
                    return SIM_REJECT;
                vu_count[l.m_lVertex] += 2;
            }
        }
    }

    // edges
    for (uint64_t ii = 0; ii < nE; ii++)
        if (d.m_vEdges[(ULONG)ii].m_lCurve >= n3D)
            return SIM_REJECT;

    // edge uses
    std::vector<char> built(nEU, 0);
    std::vector<std::vector<uint64_t>> lists(nE); // radial lists: 2*i edge use 1, 2*i+1 edge use 2
    std::vector<uint64_t> eu_edge(nEU, 0);
    for (uint64_t ii = 0; ii < nEU; ii++)
    {
        const SmEUData& r = d.m_vEdgeuses[(ULONG)ii];
        if (r.m_lEUType == 3)
            continue;
        if (r.m_lEUType == 1)
        {
            if (r.m_lVertex >= nV)
                return SIM_REJECT;
            if (vu_count[r.m_lVertex] == 0)
                return SIM_OK; // NER: SM_ERR_NULL_POINTER
            continue;
        }
        if (r.m_lEdge >= NO)
            continue;
        if (r.m_lEdge >= nE)
            return SIM_REJECT;
        const SmEdgeData& e = d.m_vEdges[(ULONG)r.m_lEdge];
        if (e.m_lStartVertex >= nV || e.m_lEndVertex >= nV)
            return SIM_REJECT;
        vu_count[e.m_lStartVertex]++;
        vu_count[e.m_lEndVertex]++;
        if (r.m_lEUType == 2)
        {
            if (r.m_lShell >= nS)
                return SIM_REJECT;
        }
        else
        {
            if (r.m_lLoop >= nL || !loop_built[r.m_lLoop])
                return SIM_REJECT;
            if (r.m_lUVCurve < 999999 && r.m_lUVCurve >= nUV)
                return SIM_REJECT;
        }
        built[ii] = 1;
        eu_edge[ii] = r.m_lEdge;
        std::vector<uint64_t>& l = lists[r.m_lEdge];
        if (r.m_bOrientation)
        {
            l.push_back(2 * ii);
            l.push_back(2 * ii + 1);
        }
        else
        {
            l.push_back(2 * ii + 1);
            l.push_back(2 * ii);
        }
    }

    // the non-manifold radial reordering
    if (manifold == 0)
    {
        std::map<uint64_t, uint64_t> next, last;
        for (const auto& l : lists)
            for (size_t k = 0; k < l.size(); k++)
            {
                next[l[k]] = l[(k + 1) % l.size()];
                last[l[(k + 1) % l.size()]] = l[k];
            }
        auto resolve = [&](long idx, uint64_t& out) -> bool {
            uint64_t j;
            bool mate = idx < 0;
            if (mate)
            {
                if (idx == LONG_MIN)
                    return false;
                j = (uint64_t)(-idx - 1);
            }
            else
                j = (uint64_t)idx;
            if (j >= nEU || !built[j])
                return false;
            out = 2 * j + (mate ? 1 : 0);
            return true;
        };
        for (uint64_t ii = 0; ii < nEU; ii++)
        {
            const SmEUData& r = d.m_vEdgeuses[(ULONG)ii];
            if (r.m_lEUType >= 1 && r.m_lEUType <= 3)
                continue;
            if (r.m_lEdge >= NO)
                continue;
            if (!built[ii])
                return SIM_REJECT;
            uint64_t n1, n2;
            if (!resolve(r.m_lNextEU, n1))
                return SIM_REJECT;
            next[2 * ii] = n1;
            last[n1] = 2 * ii;
            if (!resolve(r.m_lMateNextEU, n2))
                return SIM_REJECT;
            next[2 * ii + 1] = n2;
            last[n2] = 2 * ii + 1;
        }
        size_t bound = 2 * nEU;
        for (uint64_t e = 0; e < nE; e++)
        {
            const auto& l = lists[e];
            if (l.empty())
                continue;
            uint64_t head = l[0], cur = head;
            size_t len = 0;
            while (true)
            {
                if (eu_edge[cur / 2] != e)
                    return SIM_REJECT;
                len++;
                auto it = next.find(cur);
                if (it == next.end())
                    return SIM_REJECT;
                uint64_t n = it->second;
                auto lt = last.find(n);
                if (lt == last.end() || lt->second != cur)
                    return SIM_REJECT;
                cur = n;
                if (cur == head)
                    break;
                if (len > bound)
                    return SIM_REJECT;
            }
            if (len != l.size())
                return SIM_REJECT;
        }
    }

    // the regions block
    std::vector<char> fu_used(2 * nF, 0);
    for (size_t c = 0; c < shell_map.size(); c++)
    {
        uint64_t si = shell_map[c];
        bool inside = false;
        if (si >= 20000)
        {
            inside = true;
            si -= 20000;
        }
        if (si >= nS)
            return SIM_REJECT;
        const SmShellData& s = d.m_vShells[(ULONG)si];
        int ty = shell_type[c];
        if (ty == 2)
        {
            if (s.m_lVertex >= nV)
                return SIM_REJECT;
            vu_count[s.m_lVertex]++;
        }
        else if (ty == 1)
        {
            if (s.m_lEdge >= nE)
                return SIM_REJECT;
        }
        else
        {
            for (uint64_t kk = 0; kk < s.m_lNumFaceuses; kk++)
            {
                uint64_t fi = (uint64_t)s.m_lFaceuseStart + kk;
                if (fi < kk || fi >= nFU)
                    return SIM_REJECT;
                const SmFaceuseData& fu = d.m_vFaceuses[(ULONG)fi];
                if (fu.m_lFace >= nF)
                    return SIM_REJECT;
                // side 0 = FU1 (same), 1 = FU2; inside swaps them
                uint64_t side = fu.m_bOrientation ? 0 : 1;
                if (inside)
                    side = 1 - side;
                uint64_t slot = 2 * fu.m_lFace + side;
                if (fu_used[slot])
                    return SIM_REJECT;
                fu_used[slot] = 1;
            }
        }
    }

    // the loops block
    auto vertex_lu = [&](uint64_t li) {
        const SmLoopData& l = d.m_vLoops[(ULONG)li];
        if (l.m_lLoopType == 1)
            return true;
        return l.m_lNumEU == 1 && d.m_vEdgeuses[(ULONG)l.m_lStartEU].m_lEUType == 1;
    };
    for (uint64_t li = 0; li < nL; li++)
    {
        const SmLoopData& l = d.m_vLoops[(ULONG)li];
        if (l.m_lLoopType == 1)
            continue;
        long prev = (long)((ULONG)(l.m_lStartEU + l.m_lNumEU - 1));
        bool found = false;
        while (prev > -1)
        {
            if ((uint64_t)prev >= nEU)
                return SIM_REJECT;
            uint64_t pi = (uint64_t)prev;
            prev--;
            if (d.m_vEdgeuses[(ULONG)pi].m_lEUType != 3 && built[pi])
            {
                found = true;
                break;
            }
        }
        if (!found)
            return SIM_OK; // SER(SM_ERR)
        bool first = true;
        for (uint64_t jj = 0; jj < l.m_lNumEU; jj++)
        {
            uint64_t ei = (uint64_t)l.m_lStartEU + jj;
            if (ei < jj || ei >= nEU)
                return SIM_REJECT;
            const SmEUData& r = d.m_vEdgeuses[(ULONG)ei];
            if (r.m_lEdge >= NO)
                continue;
            if (r.m_lEUType == 3)
                continue;
            if (!built[ei])
                return SIM_REJECT;
            if (first)
            {
                first = false;
                if (r.m_lEUType == 2)
                    return SIM_REJECT; // a shell written as a loop use
                if (vertex_lu(r.m_lLoop))
                    return SIM_REJECT;
            }
        }
    }

    // the primary edge uses
    for (uint64_t ii = 0; ii < nE; ii++)
    {
        long p = d.m_vEdges[(ULONG)ii].m_lPrimEU;
        if (p < 0)
        {
            if (p == LONG_MIN || (uint64_t)(-p - 1) >= nEU)
                return SIM_REJECT;
        }
    }
    return SIM_OK;
}

// ---- traversal-order registry ------------------------------------------------------------

struct Graph
{
    std::vector<SmRegion*> regions;
    std::vector<SmShell*> shells;
    std::vector<SmFaceuse*> faceuses;
    std::vector<SmFace*> faces;
    std::vector<SmLoopuse*> loopuses;
    std::vector<SmLoop*> loops;
    std::vector<SmEdgeuse*> edgeuses;
    std::vector<SmEdge*> edges;
    std::vector<SmVertexuse*> vertexuses;
    std::vector<SmVertex*> vertices;
    std::map<const void*, int64_t> index[10];

    template <class T> int64_t add(int k, std::vector<T*>& v, T* p)
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        if (it != index[k].end())
            return it->second;
        int64_t i = (int64_t)v.size();
        index[k][(const void*)p] = i;
        v.push_back(p);
        return i;
    }
    template <class T> int64_t find(int k, const T* p) const
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        return it == index[k].end() ? -3 : it->second;
    }
    int64_t typed(SM_TYPE t, SmTopology* p)
    {
        if (p == nullptr)
            return -1;
        switch (t)
        {
        case SmShell_TYPE: return add(1, shells, (SmShell*)p);
        case SmLoopuse_TYPE: return add(4, loopuses, (SmLoopuse*)p);
        case SmEdgeuse_TYPE: return add(6, edgeuses, (SmEdgeuse*)p);
        case SmVertexuse_TYPE: return add(8, vertexuses, (SmVertexuse*)p);
        case SmFaceuse_TYPE: return add(2, faceuses, (SmFaceuse*)p);
        default: return -4;
        }
    }
    int64_t find_typed(SM_TYPE t, const SmTopology* p) const
    {
        if (p == nullptr)
            return -1;
        switch (t)
        {
        case SmShell_TYPE: return find(1, p);
        case SmLoopuse_TYPE: return find(4, p);
        case SmEdgeuse_TYPE: return find(6, p);
        case SmVertexuse_TYPE: return find(8, p);
        case SmFaceuse_TYPE: return find(2, p);
        default: return -4;
        }
    }
};

std::vector<SmTopology*> list_of(SmTopology* head)
{
    std::vector<SmTopology*> v;
    SmTopology* p = head;
    while (p != nullptr && v.size() < 1000000)
    {
        v.push_back(p);
        p = p->GetNext();
        if (p == head)
            break;
    }
    return v;
}

void walk(SmBrep* brep, Graph& g)
{
    SmTArray<SmRegion*> regs;
    brep->GetRegions(regs);
    for (ULONG i = 0; i < regs.GetSize(); i++)
        g.add(0, g.regions, regs[i]);
    size_t r = 0, s = 0, fu = 0, lu = 0, eu = 0, vu = 0, l = 0, e = 0, v = 0;
    bool more = true;
    while (more)
    {
        more = false;
        for (; r < g.regions.size(); r++, more = true)
            for (SmTopology* t : list_of(g.regions[r]->GetList()))
                g.add(1, g.shells, (SmShell*)t);
        for (; s < g.shells.size(); s++, more = true)
        {
            SmShell* sh = g.shells[s];
            SM_TYPE ty = sh->*ShPeek::type();
            if (ty == SmFaceuse_TYPE)
                for (SmTopology* t : list_of(sh->GetList()))
                    g.add(2, g.faceuses, (SmFaceuse*)t);
            else
                g.typed(ty, sh->GetList());
        }
        for (; fu < g.faceuses.size(); fu++, more = true)
        {
            SmFaceuse* p = g.faceuses[fu];
            g.add(3, g.faces, p->*FuPeek::face());
            g.add(2, g.faceuses, p->*FuPeek::mate());
            for (SmTopology* t : list_of(p->GetList()))
                g.add(4, g.loopuses, (SmLoopuse*)t);
        }
        for (; lu < g.loopuses.size(); lu++, more = true)
        {
            SmLoopuse* p = g.loopuses[lu];
            g.add(5, g.loops, p->*LuPeek::loop());
            g.add(4, g.loopuses, p->*LuPeek::mate());
            SmTopology* start = p->*LuPeek::start();
            g.typed(p->*LuPeek::type(), start);
            if (p->*LuPeek::type() == SmEdgeuse_TYPE)
            {
                SmEdgeuse* q = (SmEdgeuse*)start;
                for (int k = 0; q != nullptr && k < 1000000; k++)
                {
                    g.add(6, g.edgeuses, q);
                    q = q->GetCCWEdgeuse();
                    if (q == (SmEdgeuse*)start)
                        break;
                }
            }
        }
        for (; eu < g.edgeuses.size(); eu++, more = true)
        {
            SmEdgeuse* p = g.edgeuses[eu];
            g.add(7, g.edges, (SmEdge*)p->GetOwner());
            g.typed(p->GetEdgeuseType(), p->*EuPeek::sorlu());
            g.add(6, g.edgeuses, p->GetCWEdgeuse());
            g.add(6, g.edgeuses, p->GetCCWEdgeuse());
            g.add(8, g.vertexuses, p->*EuPeek::vu());
            g.add(6, g.edgeuses, (SmEdgeuse*)p->GetNext());
            g.add(6, g.edgeuses, (SmEdgeuse*)p->GetLast());
        }
        for (; e < g.edges.size(); e++, more = true)
            for (SmTopology* t : list_of(g.edges[e]->GetList()))
                g.add(6, g.edgeuses, (SmEdgeuse*)t);
        for (; vu < g.vertexuses.size(); vu++, more = true)
        {
            SmVertexuse* p = g.vertexuses[vu];
            g.add(9, g.vertices, (SmVertex*)p->GetOwner());
            g.typed(p->GetVertexuseType(), p->*VuPeek::owner());
        }
        for (; v < g.vertices.size(); v++, more = true)
            for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                g.add(8, g.vertexuses, (SmVertexuse*)t);
        for (; l < g.loops.size(); l++, more = true)
            g.add(4, g.loopuses, g.loops[l]->GetLoopuse());
    }
}

// ---- export -------------------------------------------------------------------------------

void put_list(Out& o, const std::vector<int64_t>& v)
{
    o.i((int64_t)v.size());
    for (int64_t x : v)
        o.i(x);
}

void put_topo(Out& o, SmTopology* t)
{
    o.i((int64_t)t->GetAllFlags());
    o.i((int64_t)t->GetUserIndex1());
    o.i((int64_t)t->GetUserIndex2());
    o.i(t->GetUserPtr1() != NULL ? 1 : 0);
}

void put_point(Out& o, SmStatus s, const SmPoint3d& p)
{
    o.i(st(s));
    if (s == SM_SUCCESS)
    {
        o.d(p.x);
        o.d(p.y);
        o.d(p.z);
    }
}

// `0` (none) or `1, type, dim, interval (2 d)` and five evaluations at 0, 1/4, 1/2, 3/4, 1
// of the natural interval (`status`, on success `x y z`), then whether `owner` owns it.
void put_curve(Out& o, const SmCurve* c, const SmObject* owner)
{
    if (c == NULL)
    {
        o.i(0);
        return;
    }
    o.i(1);
    o.i((int64_t)c->GetType());
    o.i((int64_t)c->GetDim());
    SmExtent1d ivl = c->GetNaturalInterval();
    o.d(ivl.GetMin());
    o.d(ivl.GetMax());
    for (int k = 0; k <= 4; k++)
    {
        double t = ivl.GetMin() + 0.25 * k * (ivl.GetMax() - ivl.GetMin());
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus s = c->EvaluatePoint(t, p);
        put_point(o, s, p);
    }
    o.i(c->GetOwner() == owner ? 1 : 0);
}

// `0` or `1, type, domain (4 d)` and a 3 x 3 grid of evaluations, then ownership.
void put_surface(Out& o, const SmSurface* s, const SmObject* owner)
{
    if (s == NULL)
    {
        o.i(0);
        return;
    }
    o.i(1);
    o.i((int64_t)s->GetType());
    SmExtent2d dom = s->GetNaturalUVDomain();
    o.d(dom.GetMin().x);
    o.d(dom.GetMin().y);
    o.d(dom.GetMax().x);
    o.d(dom.GetMax().y);
    for (int a = 0; a <= 2; a++)
        for (int b = 0; b <= 2; b++)
        {
            SmPoint2d uv(dom.GetMin().x + 0.5 * a * (dom.GetMax().x - dom.GetMin().x),
                         dom.GetMin().y + 0.5 * b * (dom.GetMax().y - dom.GetMin().y));
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus st2 = s->EvaluatePoint(uv, p);
            put_point(o, st2, p);
        }
    o.i(s->GetOwner() == owner ? 1 : 0);
}

void export_graph(SmBrep* brep, Graph& g, Out& o)
{
    o.d((double)brep->GetTolerance());
    o.i(brep->m_bEditingEnabled ? 1 : 0);
    for (auto n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                   g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.i((int64_t)n);
    for (SmRegion* p : g.regions)
    {
        o.i(p == brep->GetInfiniteRegion() ? 1 : 0);
        o.i(p->GetOwner() == (SmOwningTopology*)brep ? 1 : 0);
        o.i((int64_t)p->IsVoid());
        put_topo(o, p);
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(1, t));
        put_list(o, v);
    }
    for (SmShell* p : g.shells)
    {
        o.i(g.find(0, p->GetOwner()));
        SM_TYPE ty = p->*ShPeek::type();
        o.i((int64_t)ty);
        put_topo(o, p);
        std::vector<int64_t> v;
        if (ty == SmFaceuse_TYPE)
            for (SmTopology* t : list_of(p->GetList()))
                v.push_back(g.find(2, t));
        else
            v.push_back(g.find_typed(ty, p->GetList()));
        put_list(o, v);
    }
    for (SmFaceuse* p : g.faceuses)
    {
        o.i(g.find(1, p->GetOwner()));
        o.i(g.find(3, p->*FuPeek::face()));
        o.i(g.find(2, p->*FuPeek::mate()));
        o.i((int64_t)p->GetOrientation());
        put_topo(o, p);
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(4, t));
        put_list(o, v);
    }
    for (SmFace* p : g.faces)
    {
        put_surface(o, p->*FPeek::srf(), p);
        o.i(g.find(2, p->*FPeek::fu()));
        SmExtent2d dm = p->*FPeek::dom();
        o.d(dm.GetMin().x);
        o.d(dm.GetMin().y);
        o.d(dm.GetMax().x);
        o.d(dm.GetMax().y);
        o.i((int64_t)(p->*FPeek::rect()));
        o.d((double)(p->*FPeek::tol()));
        put_topo(o, p);
    }
    for (SmLoopuse* p : g.loopuses)
    {
        o.i(g.find(2, p->GetOwner()));
        o.i(g.find(5, p->*LuPeek::loop()));
        o.i(g.find(4, p->*LuPeek::mate()));
        SM_TYPE ty = p->*LuPeek::type();
        o.i((int64_t)ty);
        o.i(g.find_typed(ty, p->*LuPeek::start()));
        o.i((int64_t)(p->*LuPeek::orient()));
        put_topo(o, p);
    }
    for (SmLoop* p : g.loops)
    {
        o.i(g.find(4, p->GetLoopuse()));
        put_topo(o, p);
    }
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.i(g.find(7, p->GetOwner()));
        SM_TYPE ty = p->GetEdgeuseType();
        o.i((int64_t)ty);
        o.i(g.find_typed(ty, p->*EuPeek::sorlu()));
        o.i(g.find(6, p->GetCWEdgeuse()));
        o.i(g.find(6, p->GetCCWEdgeuse()));
        o.i(g.find(8, p->*EuPeek::vu()));
        o.i((int64_t)p->GetOrientation());
        o.i(g.find(6, p->GetNext()));
        o.i(g.find(6, p->GetLast()));
        put_curve(o, p->*EuPeek::uv(), p);
        put_topo(o, p);
    }
    for (SmEdge* p : g.edges)
    {
        o.d((double)p->GetTolerance());
        SmExtent1d ivl = p->GetInterval();
        o.d(ivl.GetMin());
        o.d(ivl.GetMax());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(6, t));
        put_list(o, v);
        put_curve(o, p->GetCurve(), p);
        put_topo(o, p);
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.i(g.find(9, p->GetOwner()));
        SM_TYPE ty = p->GetVertexuseType();
        o.i((int64_t)ty);
        o.i(g.find_typed(ty, p->*VuPeek::owner()));
        put_topo(o, p);
    }
    for (SmVertex* p : g.vertices)
    {
        SmPoint3d pt = p->GetPoint();
        o.d(pt.x);
        o.d(pt.y);
        o.d(pt.z);
        o.d((double)p->GetTolerance());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(8, t));
        put_list(o, v);
        put_topo(o, p);
    }
}

struct Owned
{
    SmBrepData* data = nullptr;
    SmBrep* brep = nullptr;
    ~Owned()
    {
        delete brep;
        delete data;
    }
};

int32_t run(const char* path, Rd& rd, Out& o)
{
    const SmContext& ctx = the_context();
    int64_t op = rd.i();
    int64_t file_type = rd.i();
    SmBoolean opt[6];
    for (int k = 0; k < 6; k++)
        opt[k] = rd.i() != 0 ? TRUE : FALSE;
    if (!rd.ok || op < OP_MAKE || op > OP_PROJECT)
        return SC_ERR_ARG;

    Owned own;
    SmStatus rs = SmBrepData::ReadFromFile(ctx, path, own.data, file_type == 1 ? SM_BINARY : SM_ASCII);
    if (rs != SM_SUCCESS || own.data == nullptr)
        return SC_ERR_ARG; // the runner only names files both sides read
    bool set_used = false;
    if (!apply_patches(rd, *own.data, &set_used))
        return SC_ERR_ARG;
    // upstream walks a B-rep built from malformed indices without checks: the ops after
    // the build only run on unmodified indices
    if (set_used && op != OP_MAKE)
        return SC_ERR_ARG;

    // the op's arguments
    int64_t flag = 0, trim = 0, has_approx = 0, has_angle = 0;
    double pts[12] = {0};
    double approx = 0.0, angle = 0.0;
    bool empty_subset = false;
    int64_t raw_n = -1, raw_p = 0, raw_m = 0;
    std::vector<double> raw_points, raw_knots;
    if (op == OP_CLOSURE || op == OP_REMOVE_ALL)
        flag = rd.i();
    if (op == OP_PROJECT)
    {
        trim = rd.i();
        has_approx = rd.i();
        has_angle = rd.i();
        for (int k = 0; k < 12; k++)
            pts[k] = rd.d();
        approx = rd.d();
        angle = rd.d();
        empty_subset = rd.i() != 0;
        if (rd.i() != 0)
        {
            raw_n = rd.i();
            raw_p = rd.i();
            raw_m = rd.i();
            if (raw_n < 1 || raw_n >= 1024 || raw_p < 1 || raw_p >= GW_MAX_DEGREE ||
                raw_p > raw_n || raw_m != raw_n + raw_p + 1)
                return SC_ERR_ARG;
            for (int64_t k = 0; k < 4 * (raw_n + 1); ++k)
                raw_points.push_back(rd.d());
            for (int64_t k = 0; k <= raw_m; ++k)
                raw_knots.push_back(rd.d());
        }
        if (trim < 0 || trim > 2)
            return SC_ERR_ARG;
    }
    if (!rd.ok || rd.ai != rd.ni || rd.ad != rd.nd)
        return SC_ERR_ARG;

    if (guard(*own.data) == SIM_REJECT)
    {
        o.i(ST_GUARD);
        return SC_OK;
    }

    own.brep = new (ctx) SmBrep();
    SmTArray<SmAttribute*> attrs;
    SmStatus ms = own.brep->MakeTopologyFromData(own.data, attrs, opt[0], opt[1], opt[2], opt[3], opt[4], opt[5]);
    o.i(st(ms));
    if (ms != SM_SUCCESS)
        return SC_OK;
    o.i((int64_t)own.data->m_bManifold);
    o.i((int64_t)own.data->m_bIsGeomBorrowed);
    Graph g;
    walk(own.brep, g);
    // Projection extrudes the source degree into an NLib surface (NL_MAXDEG).
    // Empty subsets and B-reps without faces only classify with SMLib's evaluator.
    if (op == OP_PROJECT && raw_n >= 0 && !empty_subset && !g.faces.empty() && raw_p > NL_MAXDEG)
        return STATUS_UNDEFINED_PROJECTION_DEGREE;
    export_graph(own.brep, g, o);

    if (op == OP_CLOSURE)
    {
        for (SmEdge* e : g.edges)
        {
            SmTArray<SmShell*> shells;
            SmTArray<SmRegion*> regions;
            SmStatus cs = own.brep->TestClosureSplitRegion(flag ? TRUE : FALSE, e, shells, regions);
            o.i(st(cs));
            o.i((int64_t)shells.GetSize());
            o.i((int64_t)regions.GetSize());
        }
        Graph g2;
        walk(own.brep, g2);
        export_graph(own.brep, g2, o);
    }
    else if (op == OP_REMOVE_ALL)
    {
        own.brep->m_bEditingEnabled = flag ? TRUE : FALSE;
        SmStatus s2 = own.brep->RemoveAllTopology();
        o.i(st(s2));
        Graph g2;
        walk(own.brep, g2);
        export_graph(own.brep, g2, o);
    }
    else if (op == OP_PROJECT)
    {
        SmPoint3d a(pts[0], pts[1], pts[2]), b(pts[3], pts[4], pts[5]);
        SmVector3d dir(pts[6], pts[7], pts[8]);
        SmPoint3d ref(pts[9], pts[10], pts[11]);
        struct ProjectionInput
        {
            SmBSplineCurve* curve = nullptr;
            gw_CURVE* raw = nullptr;
            ~ProjectionInput()
            {
                delete curve;
                if (raw != nullptr)
                    smos_Free(raw);
            }
        } input;
        SmStatus cs = SM_SUCCESS;
        if (raw_n >= 0)
        {
            input.raw = sm_AllocateNurbCurve((gw_INDEX)raw_n, (gw_DEGREE)raw_p, (gw_INDEX)raw_m);
            for (int64_t k = 0; k <= raw_n; ++k)
            {
                input.raw->pol->Pw[k].x = raw_points[4 * k];
                input.raw->pol->Pw[k].y = raw_points[4 * k + 1];
                input.raw->pol->Pw[k].z = raw_points[4 * k + 2];
                input.raw->pol->Pw[k].w = raw_points[4 * k + 3];
            }
            for (int64_t k = 0; k <= raw_m; ++k)
                input.raw->knt->U[k] = raw_knots[k];
            input.curve = new (ctx) SmBSplineCurve(input.raw, 3, TRUE);
        }
        else
            cs = SmBSplineCurve::CreateLineSegment(ctx, 3, a, b, input.curve);
        o.i(st(cs));
        if (cs != SM_SUCCESS)
            return SC_OK;
        SmTArray<SmFace*> subset;
        SmStatus ps = own.brep->ProjectAndTrim(*input.curve, dir, ref, (SmTrimType)trim,
                                               has_approx ? &approx : NULL,
                                               has_angle ? &angle : NULL,
                                               empty_subset ? &subset : NULL);
        o.i(st(ps));
        Graph g2;
        walk(own.brep, g2);
        export_graph(own.brep, g2, o);
        if (raw_n >= 0)
        {
            o.i((int64_t)g.faces.size());
            int64_t retained = 0;
            for (size_t index = 0; index < g.faces.size(); ++index)
                if (std::find(g2.faces.begin(), g2.faces.end(), g.faces[index]) != g2.faces.end())
                {
                    o.i((int64_t)index);
                    ++retained;
                }
            o.i(retained);
        }
    }
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_p10_run(const char* path, const int64_t* ints, int64_t n_ints, const double* dbls,
                                           int64_t n_dbls, int64_t* out, int64_t cap, int64_t* out_len)
{
    if (path == nullptr || (n_ints > 0 && ints == nullptr) || (n_dbls > 0 && dbls == nullptr) || n_ints < 0 ||
        n_dbls < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{ints, n_ints, 0, dbls, n_dbls, 0, true};
    Out o;
    int32_t s = run(path, rd, o);
    if (s != SC_OK)
        return s;
    return sc_copy_out(o.v.data(), (int64_t)o.v.size(), out, cap, out_len);
    SC_CATCH
}

// C++-only observer reuse by the positive original-trim control. The caller holds
// SC_NATIVE_LOCK; no operation, geometry conversion or ownership mutation occurs here.
void sc_topology_brep_p10_append_snapshot(SmBrep* brep, const SmTArray<SmFace*>* returned,
                                         std::vector<double>& output)
{
    Graph graph;
    walk(brep, graph);
    if (returned != nullptr)
    {
        output.push_back((double)returned->GetSize());
        for (ULONG i = 0; i < returned->GetSize(); ++i)
            output.push_back((double)graph.find(3, (*returned)[i]));
    }
    Out snapshot;
    export_graph(brep, graph, snapshot);
    output.push_back((double)(snapshot.v.size() / 2));
    for (size_t i = 0; i < snapshot.v.size(); i += 2)
    {
        int64_t tag = snapshot.v[i];
        output.push_back((double)tag);
        double value;
        if (tag == 0)
            value = (double)snapshot.v[i + 1];
        else
            std::memcpy(&value, &snapshot.v[i + 1], sizeof value);
        output.push_back(value);
    }
    output.push_back(brep->m_bValidateOnly ? 1.0 : 0.0);
    output.push_back((double)(2 * graph.faceuses.size()));
    for (SmFaceuse* faceuse : graph.faceuses)
    {
        output.push_back((double)graph.find(2, faceuse->GetNext()));
        output.push_back((double)graph.find(2, faceuse->GetLast()));
    }
    output.push_back((double)(5 * graph.edgeuses.size() + 2 * graph.vertexuses.size()));
    for (SmEdgeuse* edgeuse : graph.edgeuses)
    {
        SmDropCurveFail* failure = edgeuse->*EuPeek::fail();
        output.push_back(failure != nullptr ? 1.0 : 0.0);
        output.push_back(failure != nullptr ? (double)failure->m_sFailStatus : 0.0);
        output.push_back(failure != nullptr && failure->m_pBrep != nullptr ? 1.0 : 0.0);
        output.push_back((edgeuse->*EuPeek::face_gap()).IsInit() ? 1.0 : 0.0);
        output.push_back((edgeuse->*EuPeek::ccw_gap()).IsInit() ? 1.0 : 0.0);
    }
    for (SmVertexuse* vertexuse : graph.vertexuses)
    {
        output.push_back((vertexuse->*VuPeek::face_gap()).IsInit() ? 1.0 : 0.0);
        output.push_back((vertexuse->*VuPeek::edge_gap()).IsInit() ? 1.0 : 0.0);
    }
    output.push_back((double)(2 * graph.vertexuses.size()));
    for (SmVertexuse* vertexuse : graph.vertexuses)
    {
        SmPoint2d uv = vertexuse->*VuPeek::uv();
        output.push_back(uv.x);
        output.push_back(uv.y);
    }
}
