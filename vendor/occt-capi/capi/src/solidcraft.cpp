// SolidCraft's entry points: what its kernel adapter asks for beyond base.cpp.
// SPDX-License-Identifier: MIT OR Apache-2.0
#include "internal.hpp"

#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>

using namespace occt_capi;

// A ring torus about the line through (x, y, z) along `dir`.
API uint32_t occt_make_torus_about(double x, double y, double z, double dx, double dy, double dz,
                                   double major, double minor) {
  return guard_handle([&]() -> uint32_t {
    if (!(minor > 0.0) || !(major > minor)) {
      throw std::invalid_argument("a torus needs a major radius above its minor radius above 0");
    }
    BRepPrimAPI_MakeTorus maker(gp_Ax2(gp_Pnt(x, y, z), gp_Dir(dx, dy, dz)), major, minor);
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("the torus could not be built");
    return put_shape(maker.Shape());
  });
}

// Constant-radius fillet of the edges nearest each point. `points` is count * 3 doubles. An edge
// nearest two points is filleted once.
API uint32_t occt_fillet_nearest(uint32_t h, const void* points, size_t count, double radius) {
  return guard_handle([&]() -> uint32_t {
    if (count == 0) throw std::invalid_argument("no edges given");
    const TopoDS_Shape shape = get_shape(h);
    ShapeMap edges;
    TopExp::MapShapes(shape, TopAbs_EDGE, edges);
    if (edges.Extent() == 0) throw std::invalid_argument("the shape has no edges");
    BRepFilletAPI_MakeFillet maker(shape);
    ShapeMap chosen;
    for (size_t i = 0; i < count; ++i) {
      const gp_Pnt p(read_unaligned<double>(points, 3 * i), read_unaligned<double>(points, 3 * i + 1),
                     read_unaligned<double>(points, 3 * i + 2));
      const TopoDS_Vertex at = BRepBuilderAPI_MakeVertex(p).Vertex();
      int best = 0;
      double nearest = std::numeric_limits<double>::infinity();
      for (int e = 1; e <= edges.Extent(); ++e) {
        BRepExtrema_DistShapeShape d(at, edges(e));
        if (d.IsDone() && d.Value() < nearest) {
          nearest = d.Value();
          best = e;
        }
      }
      if (best == 0) throw std::runtime_error("no edge is near a picked point");
      chosen.Add(edges(best));
    }
    for (int e = 1; e <= chosen.Extent(); ++e) maker.Add(radius, TopoDS::Edge(chosen(e)));
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("fillet failed");
    return put_shape(maker.Shape());
  });
}

// How many solids the shape holds.
API int32_t occt_solid_count(uint32_t h) {
  return guard<int32_t>(-1, [&]() {
    ShapeMap solids;
    TopExp::MapShapes(get_shape(h), TopAbs_SOLID, solids);
    return static_cast<int32_t>(solids.Extent());
  });
}
