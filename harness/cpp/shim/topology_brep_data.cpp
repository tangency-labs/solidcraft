// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_data`.
// Wraps upstream SmBrepData::ReadFromDB, SmBrepData::ReadFromFile,
// SmBrepData::ReadCurveFromDB, SmBrepData::ReadSurfaceFromDB, the SmBrepData
// constructor / destructor and Get/SetHealerVersion (source/SMLib/src/SmBrepData.cpp,
// source/SMLib/inc/SmBrepData.h).
//
// One entry point, sc_topology_brep_data_run. The output is a stream of int64 pairs
// (tag, value): tag 0 = integer, tag 1 = the bits of a double. Its layout is
// documented in crates/solidcore-ffi/src/units/topology_brep_data.rs, which also
// validates the database bytes (so that upstream never reads uninitialised values or
// allocates what a corrupt header asks for).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAttribute.h>
#include <SmBrepData.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <sstream>
#include <string>
#include <unistd.h>
#include <vector>

namespace
{
enum Mode : int32_t
{
    MODE_READ_DB = 0,
    MODE_READ_FILE = 1,
    MODE_READ_CURVE = 2,
    MODE_READ_SURFACE = 3,
    MODE_CONSTRUCT = 4,
};

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
    void list(const SmTArray<ULONG>& a)
    {
        i((int64_t)a.GetSize());
        for (ULONG k = 0; k < a.GetSize(); k++)
            i((int64_t)a[k]);
    }
};

int64_t slot_of(const SmTArray<SmAttribute*>& all, const SmAttribute* a)
{
    for (ULONG k = 0; k < all.GetSize(); k++)
        if (all[k] == a)
            return (int64_t)k;
    return -1;
}

void put_obj_attributes(Out& o, const SmAObject* obj, const SmTArray<SmAttribute*>& all)
{
    SmTArray<SmAttribute*> attrs;
    obj->GetAttributes(attrs);
    o.i((int64_t)attrs.GetSize());
    for (ULONG k = 0; k < attrs.GetSize(); k++)
        o.i(slot_of(all, attrs[k]));
}

void put_curve(Out& o, const SmCurve* c, const SmTArray<SmAttribute*>& all)
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
    const double ts[3] = {ivl.GetMin(), 0.5 * (ivl.GetMin() + ivl.GetMax()), ivl.GetMax()};
    for (int k = 0; k < 3; k++)
    {
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus st = c->EvaluatePoint(ts[k], p);
        o.i(st == SM_SUCCESS ? 0 : (int64_t)st);
        if (st == SM_SUCCESS)
        {
            o.d(p.x);
            o.d(p.y);
            o.d(p.z);
        }
    }
    put_obj_attributes(o, c, all);
}

void put_surface(Out& o, const SmSurface* s, const SmTArray<SmAttribute*>& all)
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
    const double fs[3] = {0.0, 0.5, 1.0};
    for (int k = 0; k < 3; k++)
    {
        SmPoint2d uv(dom.GetMin().x + fs[k] * (dom.GetMax().x - dom.GetMin().x),
                     dom.GetMin().y + fs[k] * (dom.GetMax().y - dom.GetMin().y));
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus st = s->EvaluatePoint(uv, p);
        o.i(st == SM_SUCCESS ? 0 : (int64_t)st);
        if (st == SM_SUCCESS)
        {
            o.d(p.x);
            o.d(p.y);
            o.d(p.z);
        }
    }
    put_obj_attributes(o, s, all);
}

void put_topo(Out& o, const SmTopologyData& t)
{
    o.list(t.m_sAttributes);
    o.i((int64_t)t.m_lFlags);
    o.i((int64_t)t.m_lUserIndex1);
    o.i((int64_t)t.m_lUserIndex2);
    o.i((int64_t)t.m_bIsSmallTopology);
}

void put_header(Out& o, const SmBrepData& b)
{
    o.i((int64_t)b.m_eDataSourceType);
    o.i((int64_t)b.m_bManifold);
    o.d((double)b.m_sZoneTol3d);
    o.d(b.m_dThisModelSizeEstimate);
    o.d(b.m_dThisLargeSmallSizeRatio);
    o.d((double)b.m_sApproxTol3d);
    o.i((int64_t)b.m_lStartRegion);
    o.i((int64_t)b.m_lNumRegions);
    o.d(b.m_vColor.x);
    o.d(b.m_vColor.y);
    o.d(b.m_vColor.z);
    o.i((int64_t)b.m_bIsGeomBorrowed);
    o.i((int64_t)const_cast<SmBrepData&>(b).GetHealerVersion());
}

void put_brep(Out& o, const SmBrepData& b, const SmTArray<SmAttribute*>& all)
{
    put_header(o, b);
    o.list(b.m_vAttributes);

    o.i((int64_t)b.m_vRegions.GetSize());
    for (ULONG k = 0; k < b.m_vRegions.GetSize(); k++)
    {
        const SmRegionData& r = b.m_vRegions[k];
        put_topo(o, r);
        o.i((int64_t)r.m_lStartShell);
        o.i((int64_t)r.m_lNumShells);
        o.i((int64_t)r.m_bIsVoidFlag);
    }
    o.i((int64_t)b.m_vShells.GetSize());
    for (ULONG k = 0; k < b.m_vShells.GetSize(); k++)
    {
        const SmShellData& r = b.m_vShells[k];
        put_topo(o, r);
        o.i((int64_t)r.m_lShellType);
        o.i((int64_t)r.m_lVertex);
        o.i((int64_t)r.m_lEdge);
        o.i((int64_t)r.m_lFaceuseStart);
        o.i((int64_t)r.m_lNumFaceuses);
    }
    o.list(b.m_vCFaces);
    o.i((int64_t)b.m_vFaceuses.GetSize());
    for (ULONG k = 0; k < b.m_vFaceuses.GetSize(); k++)
    {
        const SmFaceuseData& r = b.m_vFaceuses[k];
        put_topo(o, r);
        o.i((int64_t)r.m_lFace);
        o.i((int64_t)r.m_bOrientation);
    }
    o.i((int64_t)b.m_vFaces.GetSize());
    for (ULONG k = 0; k < b.m_vFaces.GetSize(); k++)
    {
        const SmFaceData& r = b.m_vFaces[k];
        put_topo(o, r);
        o.d((double)r.m_sZoneTol3d);
        o.i((int64_t)r.m_lSurface);
        o.i((int64_t)r.m_lStartLoop);
        o.i((int64_t)r.m_lNumLoops);
        o.i((int64_t)r.m_bRectangularTrim);
        o.d(r.m_vUVDomain.GetMin().x);
        o.d(r.m_vUVDomain.GetMin().y);
        o.d(r.m_vUVDomain.GetMax().x);
        o.d(r.m_vUVDomain.GetMax().y);
    }
    o.i((int64_t)b.m_vLoops.GetSize());
    for (ULONG k = 0; k < b.m_vLoops.GetSize(); k++)
    {
        const SmLoopData& r = b.m_vLoops[k];
        put_topo(o, r);
        o.i((int64_t)r.m_lLoopType);
        o.i((int64_t)r.m_lVertex);
        o.i((int64_t)r.m_lStartEU);
        o.i((int64_t)r.m_lNumEU);
        o.list(r.m_sLUUpperAttributes);
        o.list(r.m_sLULowerAttributes);
    }
    o.list(b.m_vCEdges);
    o.i((int64_t)b.m_vEdgeuses.GetSize());
    for (ULONG k = 0; k < b.m_vEdgeuses.GetSize(); k++)
    {
        const SmEUData& r = b.m_vEdgeuses[k];
        put_topo(o, r);
        o.i((int64_t)r.m_lEUType);
        o.i((int64_t)r.m_lEdge);
        o.i((int64_t)r.m_lVertex);
        o.i((int64_t)r.m_lShell);
        o.i((int64_t)r.m_lLoop);
        o.i((int64_t)r.m_bOrientation);
        o.i((int64_t)r.m_lUVCurve);
        o.i((int64_t)r.m_lNextEU);
        o.i((int64_t)r.m_lMateNextEU);
        o.i((int64_t)r.m_bDownwardLU);
        o.i((int64_t)r.m_lMateFlags);
        o.i((int64_t)r.m_lMateUserIndex1);
        o.i((int64_t)r.m_lMateUserIndex2);
        o.list(r.m_sMateAttributes);
    }
    o.i((int64_t)b.m_vEdges.GetSize());
    for (ULONG k = 0; k < b.m_vEdges.GetSize(); k++)
    {
        const SmEdgeData& r = b.m_vEdges[k];
        put_topo(o, r);
        o.d((double)r.m_sZoneTol3d);
        o.d(r.m_vInterval.GetMin());
        o.d(r.m_vInterval.GetMax());
        o.i((int64_t)r.m_lCurve);
        o.i((int64_t)r.m_lStartVertex);
        o.i((int64_t)r.m_lEndVertex);
        o.i((int64_t)r.m_lPrimEU);
    }
    o.i((int64_t)b.m_vVertices.GetSize());
    for (ULONG k = 0; k < b.m_vVertices.GetSize(); k++)
    {
        const SmVertexData& r = b.m_vVertices[k];
        put_topo(o, r);
        o.d((double)r.m_sZoneTol3d);
        o.d(r.m_vPoint.x);
        o.d(r.m_vPoint.y);
        o.d(r.m_vPoint.z);
    }
    o.i((int64_t)b.m_vUVCurves.GetSize());
    for (ULONG k = 0; k < b.m_vUVCurves.GetSize(); k++)
        put_curve(o, b.m_vUVCurves[k], all);
    o.i((int64_t)b.m_v3DCurves.GetSize());
    for (ULONG k = 0; k < b.m_v3DCurves.GetSize(); k++)
        put_curve(o, b.m_v3DCurves[k], all);
    o.i((int64_t)b.m_vSurfaces.GetSize());
    for (ULONG k = 0; k < b.m_vSurfaces.GetSize(); k++)
        put_surface(o, b.m_vSurfaces[k], all);
}

void put_attribute_table(Out& o, const SmTArray<SmAttribute*>& all)
{
    o.i((int64_t)all.GetSize());
    for (ULONG k = 0; k < all.GetSize(); k++)
    {
        const SmAttribute* a = all[k];
        if (a == NULL)
        {
            o.i(0);
            continue;
        }
        SmTArray<SmAObject*> users;
        a->GetUsers(users);
        o.i(1);
        o.i((int64_t)a->GetAttributeID());
        o.i((int64_t)a->GetBehavior());
        o.i((int64_t)users.GetSize());
    }
}

int64_t st_of(SmStatus s) { return s == SM_SUCCESS ? 0 : (int64_t)s; }

// Reads `data` through a temporary file (SmBrepData::ReadFromFile opens a path).
int32_t read_file(const SmContext& ctx, int32_t file_type, const uint8_t* data, int64_t n, Out& o)
{
    char path[] = "/tmp/sc_brep_data_XXXXXX";
    int fd = mkstemp(path);
    if (fd < 0)
        return SC_ERR_ARG;
    int64_t done = 0;
    while (done < n)
    {
        ssize_t w = write(fd, data + done, (size_t)(n - done));
        if (w <= 0)
        {
            close(fd);
            unlink(path);
            return SC_ERR_ARG;
        }
        done += w;
    }
    close(fd);
    SmBrepData* bd = NULL;
    SmStatus st = SmBrepData::ReadFromFile(ctx, path, bd, file_type == 0 ? SM_ASCII : SM_BINARY);
    unlink(path);
    o.i(st_of(st));
    SmTArray<SmAttribute*> none;
    if (st == SM_SUCCESS && bd != NULL)
        put_brep(o, *bd, none);
    if (st == SM_SUCCESS)
        delete bd;
    return SC_OK;
}
} // namespace

// mode: 0 ReadFromDB, 1 ReadFromFile, 2 ReadCurveFromDB, 3 ReadSurfaceFromDB,
//       4 constructor (+ Set/GetHealerVersion).
// file_type: 0 SM_ASCII, 1 SM_BINARY. version: database version (mode 4: the healer
// version to set). dim: curve dimension (mode 2; mode 4: bManifold). persist:
// bPersistAttribs (mode 4: eDSType). pre: initial rAllAttributes slots (even slots hold
// an SmAttribute(1000 + slot, SM_AB_STANDALONE_REFERENCE), odd slots are NULL).
SC_EXPORT int32_t sc_topology_brep_data_run(int32_t mode, int32_t file_type, int64_t version,
                                            int64_t dim, int32_t persist, int64_t pre,
                                            const uint8_t* data, int64_t n, int64_t* out,
                                            int64_t cap, int64_t* out_len)
{
    if (mode < 0 || mode > 4 || file_type < 0 || file_type > 1 || n < 0 || pre < 0 ||
        pre > 64 || version < 0)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Out o;
    if (mode == MODE_CONSTRUCT)
    {
        if (persist < 0 || persist > 14)
            return SC_ERR_ARG;
        SmBrepData* bd = new (ctx) SmBrepData((SmBoolean)dim, (SmDataSourceType)persist);
        o.i(0);
        put_header(o, *bd);
        bd->SetHealerVersion((ULONG)version);
        o.i((int64_t)bd->GetHealerVersion());
        o.i((int64_t)bd->m_vRegions.GetSize());
        o.i((int64_t)bd->m_vUVCurves.GetSize());
        delete bd;
        return sc_copy_out(o.v.data(), (int64_t)o.v.size(), out, cap, out_len);
    }
    if (mode == MODE_READ_FILE)
    {
        int32_t r = read_file(ctx, file_type, data, n, o);
        if (r != SC_OK)
            return r;
        return sc_copy_out(o.v.data(), (int64_t)o.v.size(), out, cap, out_len);
    }

    SmTArray<SmAttribute*> all;
    for (int64_t k = 0; k < pre; k++)
        all.Add(k % 2 == 0 ? new (ctx) SmAttribute((ULONG)(1000 + k), SM_AB_STANDALONE_REFERENCE)
                           : (SmAttribute*)NULL);
    std::istringstream iss(std::string((const char*)data, (size_t)n));
    SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
    db.SetVersionNum((ULONG)version);
    SmBoolean bPersist = persist ? TRUE : FALSE;

    SmBrepData* bd = NULL;
    SmCurve* crv = NULL;
    SmSurface* srf = NULL;
    SmStatus st = SM_SUCCESS;
    switch (mode)
    {
    case MODE_READ_DB:
        st = SmBrepData::ReadFromDB(ctx, (ULONG)version, all, bd, db, bPersist);
        o.i(st_of(st));
        if (st == SM_SUCCESS && bd != NULL)
            put_brep(o, *bd, all);
        break;
    case MODE_READ_CURVE:
        st = SmBrepData::ReadCurveFromDB(ctx, (ULONG)dim, (ULONG)version, all, crv, db, bPersist);
        o.i(st_of(st));
        if (st == SM_SUCCESS)
            put_curve(o, crv, all);
        break;
    default:
        st = SmBrepData::ReadSurfaceFromDB(ctx, (ULONG)version, all, srf, db, bPersist);
        o.i(st_of(st));
        if (st == SM_SUCCESS)
            put_surface(o, srf, all);
        break;
    }
    put_attribute_table(o, all);

    // Failed reads leak what they built, as upstream; successful ones are released.
    if (st == SM_SUCCESS)
    {
        delete bd;
        delete crv;
        delete srf;
        for (ULONG k = 0; k < all.GetSize(); k++)
            delete all[k];
    }
    return sc_copy_out(o.v.data(), (int64_t)o.v.size(), out, cap, out_len);
    SC_CATCH
}
