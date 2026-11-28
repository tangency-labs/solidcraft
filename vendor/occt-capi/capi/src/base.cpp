// The original entry points: lifetime, primitives, simple operations, STEP geometry.
// SPDX-License-Identifier: MIT OR Apache-2.0
#include "internal.hpp"

using namespace occt_capi;

namespace {

template <typename Op>
uint32_t boolean(uint32_t a, uint32_t b) {
  return guard_handle([&]() -> uint32_t {
    Op op(get_shape(a), get_shape(b));
    if (op.HasErrors() || !op.IsDone()) {
      std::ostringstream msg;
      op.DumpErrors(msg);
      throw std::runtime_error("boolean operation failed: " + msg.str());
    }
    return put_shape(unwrap_single(op.Shape()));
  });
}

uint32_t apply(uint32_t h, const gp_Trsf& trsf) {
  BRepBuilderAPI_Transform transform(get_shape(h), trsf, /*Copy=*/true);
  if (!transform.IsDone()) throw std::runtime_error("transform failed");
  return put_shape(transform.Shape());
}

template <typename Maker>
uint32_t blend(uint32_t h, const void* edges, size_t count, double value, const char* what) {
  if (count == 0) throw std::invalid_argument("no edges given");
  const TopoDS_Shape shape = get_shape(h);
  ShapeMap own_edges;
  TopExp::MapShapes(shape, TopAbs_EDGE, own_edges);
  Maker maker(shape);
  for (size_t i = 0; i < count; ++i) {
    const TopoDS_Edge edge = TopoDS::Edge(get_shape(read_unaligned<uint32_t>(edges, i)));
    // The makers silently ignore a foreign edge.
    if (!own_edges.Contains(edge)) throw std::invalid_argument("the edge is not part of the shape");
    maker.Add(value, edge);
  }
  maker.Build();
  if (!maker.IsDone()) throw std::runtime_error(std::string(what) + " failed");
  return put_shape(maker.Shape());
}

}  // namespace

// --- module -----------------------------------------------------------------

API uint32_t occt_abi_version() { return 3; }
API const char* occt_library_version_ptr() { return OCC_VERSION_STRING_EXT; }
API size_t occt_library_version_len() { return std::strlen(OCC_VERSION_STRING_EXT); }
API const char* occt_last_error_ptr() { return last_error().c_str(); }
API size_t occt_last_error_len() { return last_error().size(); }

// --- input memory, needed when the caller has its own linear memory ----------

API void* occt_alloc(size_t len) { return std::malloc(len ? len : 1); }
API void occt_dealloc(void* ptr) { std::free(ptr); }

// --- buffers ----------------------------------------------------------------

API const uint8_t* occt_buffer_ptr(uint32_t h) {
  KernelLock lock(g_kernel_mutex);
  const auto it = g_buffers.find(h);
  return it == g_buffers.end() ? nullptr : it->second.data();
}
API size_t occt_buffer_len(uint32_t h) {
  KernelLock lock(g_kernel_mutex);
  const auto it = g_buffers.find(h);
  return it == g_buffers.end() ? 0 : it->second.size();
}
API void occt_buffer_free(uint32_t h) {
  KernelLock lock(g_kernel_mutex);
  g_buffers.erase(h);
}

// --- shape lifetime and identity ----------------------------------------------

API void occt_shape_free(uint32_t h) {
  KernelLock lock(g_kernel_mutex);
  g_shapes.erase(h);
}
API uint32_t occt_shape_clone(uint32_t h) {
  return guard_handle([&]() -> uint32_t { return put_shape(get_shape(h)); });
}
// TopAbs_ShapeEnum value, -1 on error.
API int32_t occt_shape_kind(uint32_t h) {
  return guard<int32_t>(-1, [&]() { return static_cast<int32_t>(get_shape(h).ShapeType()); });
}
// 1 same, 0 different, -1 error.
API int32_t occt_shape_is_same(uint32_t a, uint32_t b) {
  return guard<int32_t>(-1, [&]() { return get_shape(a).IsSame(get_shape(b)) ? 1 : 0; });
}
API uint32_t occt_shape_hash(uint32_t h) {
  return guard<uint32_t>(0u, [&]() {
    return static_cast<uint32_t>(TopTools_ShapeMapHasher{}(get_shape(h)));
  });
}
// 1 valid, 0 invalid, -1 error.
API int32_t occt_shape_is_valid(uint32_t h) {
  return guard<int32_t>(-1, [&]() { return BRepCheck_Analyzer(get_shape(h)).IsValid() ? 1 : 0; });
}
// Distinct subshapes of the kind, in stable order, as a buffer of uint32 handles.
API uint32_t occt_shape_subshapes(uint32_t h, int32_t kind) {
  return guard_handle([&]() -> uint32_t {
    if (kind < TopAbs_COMPOUND || kind > TopAbs_VERTEX) {
      throw std::invalid_argument("invalid shape kind");
    }
    ShapeMap map;
    TopExp::MapShapes(get_shape(h), static_cast<TopAbs_ShapeEnum>(kind), map);
    Bytes out;
    out.reserve(static_cast<size_t>(map.Extent()) * sizeof(uint32_t));
    for (int i = 1; i <= map.Extent(); ++i) append<uint32_t>(out, put_shape(map(i)));
    return put_buffer(std::move(out));
  });
}

// --- primitives ---------------------------------------------------------------

API uint32_t occt_make_box(double x, double y, double z, double dx, double dy, double dz) {
  return guard_handle([&]() -> uint32_t {
    return put_shape(BRepPrimAPI_MakeBox(gp_Pnt(x, y, z), dx, dy, dz).Shape());
  });
}
API uint32_t occt_make_cylinder(double px, double py, double pz, double dx, double dy, double dz,
                                double radius, double height) {
  return guard_handle([&]() -> uint32_t {
    const gp_Ax2 axis(gp_Pnt(px, py, pz), gp_Dir(dx, dy, dz));
    return put_shape(BRepPrimAPI_MakeCylinder(axis, radius, height).Shape());
  });
}
API uint32_t occt_make_cone(double px, double py, double pz, double dx, double dy, double dz,
                            double radius1, double radius2, double height) {
  return guard_handle([&]() -> uint32_t {
    const gp_Ax2 axis(gp_Pnt(px, py, pz), gp_Dir(dx, dy, dz));
    return put_shape(BRepPrimAPI_MakeCone(axis, radius1, radius2, height).Shape());
  });
}
API uint32_t occt_make_sphere(double x, double y, double z, double radius) {
  return guard_handle([&]() -> uint32_t {
    return put_shape(BRepPrimAPI_MakeSphere(gp_Pnt(x, y, z), radius).Shape());
  });
}
// `points` is count * 3 doubles; a closed planar polygon face.
API uint32_t occt_make_polygon_face(const void* points, size_t count) {
  return guard_handle([&]() -> uint32_t {
    if (count < 3) throw std::invalid_argument("a polygon needs at least 3 points");
    BRepBuilderAPI_MakePolygon polygon;
    for (size_t i = 0; i < count; ++i) {
      polygon.Add(gp_Pnt(read_unaligned<double>(points, 3 * i),
                         read_unaligned<double>(points, 3 * i + 1),
                         read_unaligned<double>(points, 3 * i + 2)));
    }
    polygon.Close();
    if (!polygon.IsDone()) throw std::runtime_error("the polygon is degenerate");
    BRepBuilderAPI_MakeFace face(polygon.Wire(), /*OnlyPlane=*/true);
    if (!face.IsDone()) throw std::runtime_error("the polygon is not planar");
    return put_shape(face.Shape());
  });
}

// --- sweeps -------------------------------------------------------------------

API uint32_t occt_extrude(uint32_t h, double dx, double dy, double dz) {
  return guard_handle([&]() -> uint32_t {
    BRepPrimAPI_MakePrism prism(get_shape(h), gp_Vec(dx, dy, dz));
    if (!prism.IsDone()) throw std::runtime_error("extrude failed");
    return put_shape(prism.Shape());
  });
}
API uint32_t occt_revolve(uint32_t h, double px, double py, double pz, double dx, double dy,
                          double dz, double angle) {
  return guard_handle([&]() -> uint32_t {
    BRepPrimAPI_MakeRevol revol(get_shape(h), gp_Ax1(gp_Pnt(px, py, pz), gp_Dir(dx, dy, dz)),
                                angle);
    if (!revol.IsDone()) throw std::runtime_error("revolve failed");
    return put_shape(revol.Shape());
  });
}

// --- booleans -----------------------------------------------------------------

API uint32_t occt_fuse(uint32_t a, uint32_t b) { return boolean<BRepAlgoAPI_Fuse>(a, b); }
API uint32_t occt_cut(uint32_t a, uint32_t b) { return boolean<BRepAlgoAPI_Cut>(a, b); }
API uint32_t occt_common(uint32_t a, uint32_t b) { return boolean<BRepAlgoAPI_Common>(a, b); }

// --- blends -------------------------------------------------------------------

// `edges` is count uint32 handles.
API uint32_t occt_fillet(uint32_t h, const void* edges, size_t count, double radius) {
  return guard_handle([&]() -> uint32_t {
    return blend<BRepFilletAPI_MakeFillet>(h, edges, count, radius, "fillet");
  });
}
// `edges` is count uint32 handles.
API uint32_t occt_chamfer(uint32_t h, const void* edges, size_t count, double distance) {
  return guard_handle([&]() -> uint32_t {
    return blend<BRepFilletAPI_MakeChamfer>(h, edges, count, distance, "chamfer");
  });
}

// --- transforms ---------------------------------------------------------------

API uint32_t occt_translate(uint32_t h, double x, double y, double z) {
  return guard_handle([&]() -> uint32_t {
    gp_Trsf t;
    t.SetTranslation(gp_Vec(x, y, z));
    return apply(h, t);
  });
}
API uint32_t occt_rotate(uint32_t h, double px, double py, double pz, double dx, double dy,
                         double dz, double angle) {
  return guard_handle([&]() -> uint32_t {
    gp_Trsf t;
    t.SetRotation(gp_Ax1(gp_Pnt(px, py, pz), gp_Dir(dx, dy, dz)), angle);
    return apply(h, t);
  });
}
API uint32_t occt_scale(uint32_t h, double x, double y, double z, double factor) {
  return guard_handle([&]() -> uint32_t {
    gp_Trsf t;
    t.SetScale(gp_Pnt(x, y, z), factor);
    return apply(h, t);
  });
}

// --- queries; NaN on error ------------------------------------------------------

API double occt_volume(uint32_t h) {
  return guard<double>(kNaN, [&]() {
    GProp_GProps props;
    BRepGProp::VolumeProperties(get_shape(h), props, /*OnlyClosed=*/true);
    return props.Mass();
  });
}
API double occt_area(uint32_t h) {
  return guard<double>(kNaN, [&]() {
    GProp_GProps props;
    BRepGProp::SurfaceProperties(get_shape(h), props);
    return props.Mass();
  });
}
API double occt_length(uint32_t h) {
  return guard<double>(kNaN, [&]() {
    GProp_GProps props;
    BRepGProp::LinearProperties(get_shape(h), props, /*SkipShared=*/true);
    return props.Mass();
  });
}
// Buffer of 3 doubles.
API uint32_t occt_center_of_mass(uint32_t h) {
  return guard_handle([&]() -> uint32_t {
    const TopoDS_Shape shape = get_shape(h);
    GProp_GProps props;
    BRepGProp::VolumeProperties(shape, props, /*OnlyClosed=*/true);
    if (props.Mass() <= 0.0) {
      props = GProp_GProps();
      BRepGProp::SurfaceProperties(shape, props);
    }
    if (props.Mass() <= 0.0) {
      props = GProp_GProps();
      BRepGProp::LinearProperties(shape, props, /*SkipShared=*/true);
    }
    return point_buffer(props.CentreOfMass());
  });
}
// Buffer of 6 doubles: min xyz, max xyz.
API uint32_t occt_bounding_box(uint32_t h) {
  return guard_handle([&]() -> uint32_t {
    Bnd_Box box;
    BRepBndLib::AddOptimal(get_shape(h), box, /*useTriangulation=*/false);
    if (box.IsVoid()) throw std::runtime_error("the shape has no extent");
    box.SetGap(0.0);
    double v[6];
    box.Get(v[0], v[1], v[2], v[3], v[4], v[5]);
    Bytes out;
    for (const double d : v) append(out, d);
    return put_buffer(std::move(out));
  });
}
// Buffer of 3 doubles.
API uint32_t occt_vertex_point(uint32_t h) {
  return guard_handle(
      [&]() -> uint32_t { return point_buffer(BRep_Tool::Pnt(TopoDS::Vertex(get_shape(h)))); });
}
// GeomAbs_SurfaceType value, -1 on error.
API int32_t occt_face_surface_kind(uint32_t h) {
  return guard<int32_t>(-1, [&]() {
    return static_cast<int32_t>(BRepAdaptor_Surface(TopoDS::Face(get_shape(h))).GetType());
  });
}
// GeomAbs_CurveType value, -1 on error.
API int32_t occt_edge_curve_kind(uint32_t h) {
  return guard<int32_t>(-1, [&]() {
    return static_cast<int32_t>(BRepAdaptor_Curve(TopoDS::Edge(get_shape(h))).GetType());
  });
}

// --- tessellation -------------------------------------------------------------

// Buffer: u32 vertex_count, u32 triangle_count, then vertex_count * 3 f64
// positions, vertex_count * 3 f64 normals, triangle_count * 3 u32 indices.
API uint32_t occt_mesh(uint32_t h, double linear_deflection, double angular_deflection,
                       int32_t relative) {
  return guard_handle([&]() -> uint32_t {
    const TopoDS_Shape shape = get_shape(h);
    BRepMesh_IncrementalMesh mesher(shape, linear_deflection, relative != 0, angular_deflection);
    if (!mesher.IsDone()) throw std::runtime_error("meshing failed");

    std::vector<double> positions, normals;
    std::vector<uint32_t> indices;
    for (TopExp_Explorer it(shape, TopAbs_FACE); it.More(); it.Next()) {
      const TopoDS_Face face = TopoDS::Face(it.Current());
      TopLoc_Location location;
      const occ::handle<Poly_Triangulation> tri = BRep_Tool::Triangulation(face, location);
      if (tri.IsNull()) throw std::runtime_error("a face has no triangulation");
      BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);

      const gp_Trsf trsf = location.Transformation();
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      const uint32_t base = static_cast<uint32_t>(positions.size() / 3);
      for (int i = 1; i <= tri->NbNodes(); ++i) {
        const gp_Pnt p = tri->Node(i).Transformed(trsf);
        positions.insert(positions.end(), {p.X(), p.Y(), p.Z()});
        gp_Dir n = tri->Normal(i).Transformed(trsf);
        if (reversed) n.Reverse();
        normals.insert(normals.end(), {n.X(), n.Y(), n.Z()});
      }
      for (int i = 1; i <= tri->NbTriangles(); ++i) {
        int a, b, c;
        tri->Triangle(i).Get(a, b, c);
        if (reversed) std::swap(b, c);
        indices.insert(indices.end(), {base + a - 1, base + b - 1, base + c - 1});
      }
    }

    Bytes out;
    append<uint32_t>(out, static_cast<uint32_t>(positions.size() / 3));
    append<uint32_t>(out, static_cast<uint32_t>(indices.size() / 3));
    const auto extend = [&out](const auto& v) {
      const auto* p = reinterpret_cast<const uint8_t*>(v.data());
      out.insert(out.end(), p, p + v.size() * sizeof(v[0]));
    };
    extend(positions);
    extend(normals);
    extend(indices);
    return put_buffer(std::move(out));
  });
}

// --- STEP -----------------------------------------------------------------------

API uint32_t occt_step_write(uint32_t h) {
  return guard_handle([&]() -> uint32_t {
    silence_messages();
    STEPControl_Writer writer;
    if (writer.Transfer(get_shape(h), STEPControl_AsIs) != IFSelect_RetDone) {
      throw std::runtime_error("STEP transfer failed");
    }
    std::ostringstream stream;
    if (writer.WriteStream(stream) != IFSelect_RetDone) {
      throw std::runtime_error("STEP write failed");
    }
    const std::string text = stream.str();
    return put_buffer(Bytes(text.begin(), text.end()));
  });
}
API uint32_t occt_step_read(const uint8_t* data, size_t len) {
  return guard_handle([&]() -> uint32_t {
    silence_messages();
    std::istringstream stream(std::string(reinterpret_cast<const char*>(data), len));
    STEPControl_Reader reader;
    if (reader.ReadStream("input.step", stream) != IFSelect_RetDone) {
      throw std::runtime_error("the data is not readable as STEP");
    }
    if (reader.TransferRoots() == 0) throw std::runtime_error("the STEP data has no shapes");
    return put_shape(reader.OneShape());
  });
}
