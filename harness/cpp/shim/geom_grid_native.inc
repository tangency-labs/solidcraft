// SPDX-License-Identifier: Apache-2.0
// Unit-local explicit affine surfaces and read-only native grid/cache observations.
// Included inside geom_grid.cpp's anonymous namespace, after ShimGrid/put helpers.
// No synthetic patches, manually filled leaves, or pointer-derived output identities.

void put_domain(std::vector<double>& out, const SmExtent2d& d)
{
    out.push_back(d.GetMin().x);
    out.push_back(d.GetMin().y);
    out.push_back(d.GetMax().x);
    out.push_back(d.GetMax().y);
}

struct NativeSurface
{
    int64_t id;
    std::unique_ptr<SmSurface> surface;
    SmSurfaceCache* cache;
    std::vector<SmTreeNode*> leaves;
};

void put_native_leaf(std::vector<double>& out, const SmSurfaceGridElement& e)
{
    put_domain(out, e.m_sUVDomain);
    put_ext(out, e.m_sBBox);
    put3(out, e.m_sMidPoint);
    put3(out, e.m_sMidDU);
    put3(out, e.m_sMidDV);
    put3(out, e.m_sMidNormal);
    SmVector3d basis[3];
    e.m_sPseudoBox.GetBasis(basis[0], basis[1], basis[2]);
    for (int i = 0; i < 3; ++i)
        put3(out, basis[i]);
    SmExtent1d iv[3];
    e.m_sPseudoBox.GetIntervals(iv[0], iv[1], iv[2]);
    for (int i = 0; i < 3; ++i)
    {
        out.push_back(iv[i].GetMin());
        out.push_back(iv[i].GetMax());
    }
    const SmPolarBox& polar = e.m_sPolarBox;
    int ps = (int)polar.GetPolarBoxState();
    out.push_back((double)ps);
    bool bounded = ps >= SM_PS_SINGLE_POINT && ps <= SM_PS_REGION;
    SmExtent2d d = polar.GetDomain();
    if (bounded)
        put_domain(out, d);
    else
        out.insert(out.end(), 4, 0.0);
    for (ULONG j = 0; j < 3; ++j)
        put3(out, bounded ? *polar.GetBasis(j) : SmVector3d(0.0, 0.0, 0.0));
}

struct NativeCaches
{
    std::vector<NativeSurface> surfaces;
    std::map<const SmHierarchyElement*, int64_t> hierarchies;

    NativeSurface& find(int64_t id)
    {
        for (NativeSurface& s : surfaces)
            if (s.id == id)
                return s;
        throw std::runtime_error("unknown explicit grid surface");
    }

    void define(const SmContext& ctx, const double* a)
    {
        // The Rust FFI guard bounds/conditions this affine family before any native call.
        int64_t id = (int64_t)a[0];
        for (const NativeSurface& s : surfaces)
            if (s.id == id)
                throw std::runtime_error("duplicate explicit grid surface");
        gw_INDEX nu = (gw_INDEX)a[14], nv = (gw_INDEX)a[15];
        gw_SURFACE* nurb = sm_AllocateNurbSurface(nu, nv, 1, 1, nu + 2, nv + 2);
        if (nurb == nullptr)
            throw std::runtime_error("cannot allocate affine surface");
        auto u = [&](gw_INDEX i) { return a[10] + (a[12] - a[10]) * (double)i / (double)nu; };
        auto v = [&](gw_INDEX j) { return a[11] + (a[13] - a[11]) * (double)j / (double)nv; };
        for (gw_INDEX i = 0; i <= nu; ++i)
            for (gw_INDEX j = 0; j <= nv; ++j)
            {
                nurb->net->Pw[i][j].x = a[1] + a[4] * u(i) + a[7] * v(j);
                nurb->net->Pw[i][j].y = a[2] + a[5] * u(i) + a[8] * v(j);
                nurb->net->Pw[i][j].z = a[3] + a[6] * u(i) + a[9] * v(j);
                nurb->net->Pw[i][j].w = NL_NOW;
            }
        for (gw_INDEX i = 0; i <= nu + 2; ++i)
            nurb->knu->U[i] = i == 0 ? u(0) : (i > nu ? u(nu) : u(i - 1));
        for (gw_INDEX j = 0; j <= nv + 2; ++j)
            nurb->knv->U[j] = j == 0 ? v(0) : (j > nv ? v(nv) : v(j - 1));
        NativeSurface s;
        s.id = id;
        s.surface.reset(new (ctx) SmBSplineSurface(nurb, FALSE, &ctx));
        s.cache = smsurf_GetSurfaceCache(s.surface.get());
        if (s.cache == nullptr || s.cache->GetSurface() != s.surface.get())
            throw std::runtime_error("missing or misowned native surface cache");
        SmTree* tree = s.cache->GetTree();
        if (tree == nullptr)
            throw std::runtime_error("missing native cache tree");
        SmTArray<SmTreeNode*> nodes;
        tree->GetAllTreeNodes(nodes);
        for (ULONG i = 0; i < nodes.GetSize(); ++i)
        {
            SmTreeNode* n = nodes[i];
            if (n->m_eAuxDataType != SM_AD_BEZIER_SURFACE ||
                (n->m_pChild1 != nullptr && n->m_pChild1->m_eAuxDataType == SM_AD_BEZIER_SURFACE))
                continue;
            SmBezierPatch* patch = static_cast<SmBezierPatch*>(n->m_pData);
            if (patch == nullptr || n->GetOwnerObject() != s.surface.get())
                throw std::runtime_error("invalid native cache patch ownership");
            if (patch->m_eNodeClass != SM_NC_OUTSIDE)
                s.leaves.push_back(n);
        }
        if (s.leaves.empty() || s.leaves.size() > 256)
            throw std::runtime_error("unexpected affine cache leaf count");
        surfaces.push_back(std::move(s));
    }

    std::pair<int64_t, int64_t> identity(const SmSurfaceGridElement& e) const
    {
        for (const NativeSurface& s : surfaces)
            if (s.surface.get() == e.m_pSurface)
                for (size_t j = 0; j < s.leaves.size(); ++j)
                {
                    const SmExtent2d& d = static_cast<SmBezierPatch*>(s.leaves[j]->m_pData)->m_sUVDomain;
                    const SmExtent2d& q = e.m_sUVDomain;
                    if (d.GetMin().x == q.GetMin().x && d.GetMin().y == q.GetMin().y &&
                        d.GetMax().x == q.GetMax().x && d.GetMax().y == q.GetMax().y)
                        return {s.id, (int64_t)j};
                }
        throw std::runtime_error("grid leaf absent from its owning cache");
    }

    void observe(ShimGrid* grid, const std::vector<SmHierarchyElement*>& hier,
                 const std::map<const SmHierarchyElement*, int64_t>& hidx,
                 std::vector<int64_t>& oi, std::vector<double>& of) const
    {
        oi.push_back((int64_t)surfaces.size());
        for (const NativeSurface& s : surfaces)
        {
            oi.push_back(s.id);
            oi.push_back((int64_t)s.leaves.size());
            for (SmTreeNode* node : s.leaves)
            {
                put_domain(of, static_cast<SmBezierPatch*>(node->m_pData)->m_sUVDomain);
                put_ext(of, node->m_sBBox);
            }
        }
        oi.push_back((int64_t)hier.size());
        for (SmHierarchyElement* h : hier)
        {
            auto it = hierarchies.find(h);
            oi.push_back(it == hierarchies.end() ? -1 : it->second);
        }
        std::vector<SmSurfaceGridElement*> leaves = grid->surface_elements();
        oi.push_back((int64_t)leaves.size());
        for (SmSurfaceGridElement* e : leaves)
        {
            auto key = identity(*e);
            oi.push_back(key.first);
            oi.push_back(key.second);
            put_sphere_box(of, *e);
            put_native_leaf(of, *e);
        }
        for (long ix = 0; ix < grid->size(0); ++ix)
            for (long iy = 0; iy < grid->size(1); ++iy)
                for (long iz = 0; iz < grid->size(2); ++iz)
                {
                    long ad[3] = {ix, iy, iz};
                    SmVoxel* voxel = grid->GetVoxel(ad);
                    SmTArray<SmGridElement*>* es = voxel->GetGridElements();
                    oi.push_back(es == nullptr ? -1 : (int64_t)es->GetSize());
                    if (es != nullptr)
                        for (ULONG j = 0; j < es->GetSize(); ++j)
                        {
                            SmGridElement* e = (*es)[j];
                            put_elem(oi, e); // preserve the original type/object record
                            int64_t index = -1;
                            for (size_t k = 0; k < leaves.size(); ++k)
                                if (static_cast<SmGridElement*>(leaves[k]) == e)
                                    index = (int64_t)k;
                            oi.push_back(index);
                        }
                    SmTArray<SmHierarchyElement*>* hs = voxel->GetHierarchyElements();
                    oi.push_back(hs == nullptr ? -1 : (int64_t)hs->GetSize());
                    if (hs != nullptr)
                        for (ULONG j = 0; j < hs->GetSize(); ++j)
                            oi.push_back(hidx.at((*hs)[j]));
                }
    }
};
