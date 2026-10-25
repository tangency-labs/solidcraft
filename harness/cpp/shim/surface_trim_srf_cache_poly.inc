// Polygon-edge subdivision coverage. Included inside surface_trim_srf_cache.cpp's namespace.
// Real SmPolyEdge -> SmPolyLoop -> SmPolyFace -> SmFace -> SmSurface query chain.
struct PolyOriginalFace : SmFace
{
    explicit PolyOriginalFace(SmSurface* s) { m_pSurface = s; }
    ~PolyOriginalFace() { m_pSurface = NULL; } // the fixture owns the surface
};

void op_implant_poly(Rd& rd, SmBSplineSurface* s, const SmContext& ctx, std::vector<double>& o)
{
    TrimPeek* c = new (ctx) TrimPeek(*s, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001);
    c->domain() = rd.domain();
    c->factor() = rd.num();
    c->m_bPointTestEnabled = FALSE;
    c->eu_mgr().Initialize(ALIGN_SIZE(sizeof(SmEdgeuseList)), 30);
    c->poly_mgr().Initialize(ALIGN_SIZE(sizeof(SmPolyEdgeList)), 30);
    std::vector<SmTreeNode*> nodes = read_tree(rd, c, s, ctx);
    SmBSplineSurface* original_surface = read_bspline(rd, ctx);
    PolyOriginalFace* original = new (static_cast<SmObject*>(c)) PolyOriginalFace(original_surface);
    SmPolyBrep* poly = new (ctx) SmPolyBrep(SmZoneTol3d(1e-6));
    SmPolyFace* face = new (poly) SmPolyFace(SmZoneTol3d(1e-6), poly);
    face->SetOriginalFace(original);
    std::vector<SmPolyEdge*> edges;
    int64_t count = rd.idx();
    o.push_back(0.0);
    for (int64_t i = 0; i < count; ++i)
    {
        double ax = rd.num(), ay = rd.num(), az = rd.num();
        double bx = rd.num(), by = rd.num(), bz = rd.num();
        SmPoint3d a(ax, ay, az), b(bx, by, bz);
        SmZoneTol3d tol(rd.num());
        int64_t node = rd.idx();
        SmPolyLoop* loop = NULL;
        SmPolyEdge* edge = NULL;
        SmPolyEdge* closing = NULL;
        SmStatus r = face->StartPolyEdgeLoop(loop);
        if (r == SM_SUCCESS)
            r = loop->AddPolyEdge(tol, a, b, NULL, NULL, poly, edge);
        if (r == SM_SUCCESS)
            r = loop->AddPolyEdge(tol, b, a, NULL, NULL, poly, closing);
        if (r == SM_SUCCESS)
            r = loop->FinishPolyEdgeLoop(poly);
        if (r != SM_SUCCESS) // fixture construction is distinct from the tested operation
            throw std::runtime_error("polygon fixture construction failed");
        edges.push_back(edge);
        o.push_back(st(c->ImplantPolyEdge(edge, nodes[(size_t)node])));
    }
    (void)rd.flag(); // Rust additionally maps handles through two handle spaces
    std::vector<EdgeusePeek*> eus;
    count = rd.idx();
    SmExtent2d extent;
    bool failed = false;
    for (int64_t i = 0; i < count; ++i)
    {
        SmBSplineCurve* curve = read_curve(rd, ctx);
        bool subdivide = rd.flag();
        int64_t node = rd.idx();
        EdgeusePeek* eu = new (static_cast<SmObject*>(c)) EdgeusePeek();
        eu->set_curve(curve);
        eus.push_back(eu);
        if (!failed)
        {
            SmStatus r = c->ImplantEdgeuse(eu, subdivide ? TRUE : FALSE, extent,
                                         node < 0 ? NULL : nodes[(size_t)node]);
            o.push_back(st(r));
            failed = r != SM_SUCCESS;
        }
    }
    put_dom(o, extent);
    SmTArray<void*> active;
    ((TreePeek*)c->tree())->node_mgr().GetActiveElements(active);
    auto node_id = [&active](const SmTreeNode* node) {
        for (ULONG i = 0; i < active.GetSize(); ++i)
            if (active[i] == node) return (double)i;
        return -1.0;
    };
    o.push_back((double)active.GetSize());
    for (ULONG i = 0; i < active.GetSize(); ++i)
    {
        const SmTreeNode* node = (const SmTreeNode*)active[i];
        const SmBezierAux2d* a = (const SmBezierAux2d*)node->m_pData;
        o.push_back(node_id(node->m_pParent));
        o.push_back(node_id(node->m_pChild1));
        o.push_back(node_id(node->m_pChild2));
        put_dom(o, a->m_sUVDomain);
        o.push_back((double)a->m_eSplitDir);
        o.push_back(flg(a->m_bIsMarked == TRUE));
        o.push_back((double)a->m_eNodeClass);
        SmTArray<SmEdgeuseList*> eu_list;
        a->m_sEdgeuseList.GetAllNodes(eu_list);
        o.push_back((double)eu_list.GetSize());
        for (ULONG j = 0; j < eu_list.GetSize(); ++j)
        {
            double id = -2.0;
            for (size_t n = 0; n < eus.size(); ++n)
                if (eus[n] == eu_list[j]->m_pEU) id = (double)n;
            o.push_back(id);
        }
        SmTArray<SmPolyEdgeList*> poly_list;
        a->m_sPolyEdgeList.GetAllNodes(poly_list);
        o.push_back((double)poly_list.GetSize());
        for (ULONG j = 0; j < poly_list.GetSize(); ++j)
        {
            double id = -2.0;
            for (size_t n = 0; n < edges.size(); ++n)
                if (edges[n] == poly_list[j]->m_pPolyEdge) id = (double)n;
            o.push_back(id);
        }
    }
    delete c;
    for (EdgeusePeek* eu : eus) delete eu;
    delete poly;
    delete original;
    delete original_surface;
}
