// Issue #1462: caller attributes through the public fillet APIs. Every face (GetFaces
// order) and edge (GetEdges order) of the body gets its own SmLongAttribute before the
// call; the record is the API status, the body's counts and, per face (edge), its area
// (length) and the attributes it carries (the Rust side tags its own body the same way).
#include <SmApiQueries.h>
#include <SmAttribute.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmFilletSolver.h>
#include <algorithm>

namespace fillet_attributed {
// The first application attribute id, and the accuracies of the face areas and edge
// lengths (those of boolean_api_brep.cpp's OP_ATTRIBUTED).
const ULONG ATTRIBUTE_BASE=10007;
const double AREA_ACCURACY=1e-4;
const double LENGTH_ACCURACY=1e-9;

// Face i: id ATTRIBUTE_BASE + i; edge i: ATTRIBUTE_BASE + 500 + i; value the id.
void tag(SmBrep* b, SmAttributeBehaviorType behavior) {
    const SmContext& ctx=*SmApiGetOrCreateContext();
    SmTArray<SmFace*> faces; b->GetFaces(faces);
    for (ULONG i=0; i<faces.GetSize(); i++) {
        const ULONG id=ATTRIBUTE_BASE+i;
        faces[i]->AddAttribute(new (ctx) SmLongAttribute(id,(long)id,behavior));
    }
    SmTArray<SmEdge*> edges; b->GetEdges(edges);
    for (ULONG i=0; i<edges.GetSize(); i++) {
        const ULONG id=ATTRIBUTE_BASE+500+i;
        edges[i]->AddAttribute(new (ctx) SmLongAttribute(id,(long)id,behavior));
    }
}

// `measure n` then per attribute `id behavior value users`, ascending (the set is
// compared, not the list order).
std::vector<double> row(const SmAObject* obj, double measure) {
    SmTArray<SmAttribute*> attrs; obj->GetAttributes(attrs);
    std::vector<std::vector<double>> entries;
    for (ULONG i=0; i<attrs.GetSize(); i++) {
        SmAttribute* a=attrs[i];
        SmLongAttribute* l=dynamic_cast<SmLongAttribute*>(a);
        SmTArray<SmAObject*> users; a->GetUsers(users);
        entries.push_back({(double)a->GetAttributeID(),(double)a->GetBehavior(),
            l != NULL ? (double)l->GetValue() : 0.0,(double)users.GetSize()});
    }
    std::sort(entries.begin(),entries.end());
    std::vector<double> out{measure,(double)entries.size()};
    for (const auto& e : entries) out.insert(out.end(),e.begin(),e.end());
    return out;
}

// Rows sorted by their attribute part, then by measure (solidcore_ffi's sort_rows).
void put_rows(std::vector<double>& o, std::vector<std::vector<double>>& rows) {
    std::sort(rows.begin(),rows.end(),[](const std::vector<double>& x,const std::vector<double>& y) {
        if (std::lexicographical_compare(x.begin()+1,x.end(),y.begin()+1,y.end())) return true;
        if (std::lexicographical_compare(y.begin()+1,y.end(),x.begin()+1,x.end())) return false;
        return x[0] < y[0];
    });
    o.push_back((double)rows.size());
    for (const auto& r : rows) o.insert(o.end(),r.begin(),r.end());
}

double st(SmApiStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

void put_attributes(std::vector<double>& o, SmBrep* b) {
    std::vector<std::vector<double>> rows;
    SmTArray<SmFace*> faces; b->GetFaces(faces);
    for (ULONG i=0; i<faces.GetSize(); i++) {
        double area=0.0;
        const SmApiStatus s=SmApiFaceComputeArea(faces[i],AREA_ACCURACY,area);
        rows.push_back(row(faces[i],s == SM_SUCCESS ? area : -st(s)));
    }
    put_rows(o,rows);
    rows.clear();
    SmTArray<SmEdge*> edges; b->GetEdges(edges);
    for (ULONG i=0; i<edges.GetSize(); i++) {
        double length=0.0;
        const SmApiStatus s=SmApiEdgeComputeLength(edges[i],LENGTH_ACCURACY,length);
        rows.push_back(row(edges[i],s == SM_SUCCESS ? length : -st(s)));
    }
    put_rows(o,rows);
}
}

// Wire (validated by the Rust binding): api (0 SmApiFilletEdges, 1 SmApiFilletEdgesPerEdge,
// 2 SmApiVariableRadiusFillet, 3 SmApiCircularFillet, 4 SmApiChamferFillet), behaviour
// (SmAttributeBehaviorType), body kind (0 SmApiCreateBox(origin p0..p2, size p3..p5),
// then the build_source kinds 2 cylinder, 3 box union, 4 box difference), 12 body
// parameters, radius, end radius (api 2), cross-section (api 0/2: 0 linear, 1 circular,
// 2 blend curve; api 1: the SmFilletSurfaceGeneratorType of every edge), continuity,
// thumbweight, edge count, then GetEdges indices. Records `status V E F` then the face
// and edge rows of the body (also after a failed call: the body keeps its edits).
SC_EXPORT int32_t sc_blend_fillet_executive_p3_attributed(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    const int64_t head=3+fillet_real_solid::SOURCE_PARAMS+6;
    if (!input || n < head) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    const int api=int(input[0]);
    const int kind=int(input[2]);
    const double* p=input+3;
    const double* f=p+fillet_real_solid::SOURCE_PARAMS;
    const int64_t count=int64_t(f[5]);
    if (count != n-head) return SC_ERR_ARG;
    SC_TRY
    std::unique_ptr<SmBrep> body;
    if (kind == 0) {
        SmBrep* raw=nullptr;
        if (SmApiCreateBox(SmVector3d(p[0],p[1],p[2]),p[3],p[4],p[5],raw) != SM_SUCCESS || !raw) {
            delete raw; return SC_ERR_ARG;
        }
        body.reset(raw);
    } else {
        body.reset(fillet_real_solid::build_source(kind,nullptr,p));
    }
    if (!body) return SC_ERR_ARG;
    fillet_attributed::tag(body.get(),(SmAttributeBehaviorType)int(input[1]));
    SmTArray<SmEdge*> edges, selected; body->GetEdges(edges);
    for (int64_t i=0; i<count; ++i) {
        const double index=f[6+i];
        if (index >= edges.GetSize()) return SC_ERR_ARG;
        selected.Add(edges[ULONG(index)]);
    }
    SmApiStatus stat=SM_SUCCESS;
    switch (api) {
    case 0: stat=SmApiFilletEdges(body.get(),selected,f[0],ULONG(f[2]),ULONG(f[3]),f[4]); break;
    case 1: {
        SmTArray<double> radii; SmTArray<SmFilletSurfaceGeneratorType> types;
        for (int64_t i=0; i<count; ++i) {
            radii.Add(f[0]); types.Add((SmFilletSurfaceGeneratorType)int(f[2]));
        }
        stat=SmApiFilletEdgesPerEdge(body.get(),selected,radii,types,ULONG(f[3]),f[4]);
        break;
    }
    case 2: stat=SmApiVariableRadiusFillet(body.get(),selected,f[0],f[1],ULONG(f[2]),ULONG(f[3])); break;
    case 3: stat=SmApiCircularFillet(body.get(),f[0]); break;
    case 4: stat=SmApiChamferFillet(body.get(),f[0]); break;
    default: return SC_ERR_ARG;
    }
    std::vector<double> values{fillet_real_solid::status(stat)};
    fillet_real_solid::counts(values,body.get());
    fillet_attributed::put_attributes(values,body.get());
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}
