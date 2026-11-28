// SolidCraft's entry points: what its kernel adapter asks for beyond base.cpp.
// SPDX-License-Identifier: MIT OR Apache-2.0
#include "internal.hpp"

#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_Circle.hxx>
#include <ShapeFix_Face.hxx>
#include <TColStd_Array1OfInteger.hxx>
#include <TColStd_Array1OfReal.hxx>
#include <TColgp_Array1OfPnt.hxx>

#include <vector>

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

namespace {

// Reads a profile's doubles in order.
struct Reader {
  const void* data;
  size_t count;
  size_t at = 0;
  double next() {
    if (at >= count) throw std::invalid_argument("the profile's data ends early");
    return read_unaligned<double>(data, at++);
  }
  gp_Pnt point() {
    const double x = next();
    const double y = next();
    const double z = next();
    return gp_Pnt(x, y, z);
  }
  gp_Dir dir() {
    const gp_Pnt p = point();
    return gp_Dir(p.X(), p.Y(), p.Z());
  }
  size_t size(size_t most) {
    const double v = next();
    if (!(v >= 0.0) || v > static_cast<double>(most) || v != static_cast<double>(static_cast<size_t>(v))) {
      throw std::invalid_argument("a count in the profile's data is out of range");
    }
    return static_cast<size_t>(v);
  }
};

// One curve of a profile: 0 a line (start, end), 1 an arc (centre, unit axis, unit reference,
// radius, start and end angle counter-clockwise about the axis from the reference), 2 a NURBS
// (degree, point count, knot count, points as x y z weight, the clamped knots).
TopoDS_Edge read_edge(Reader& r) {
  const size_t kind = r.size(2);
  if (kind == 0) {
    const gp_Pnt a = r.point();
    const gp_Pnt b = r.point();
    BRepBuilderAPI_MakeEdge e(a, b);
    if (!e.IsDone()) throw std::runtime_error("a profile line could not be made");
    return e.Edge();
  }
  if (kind == 1) {
    const gp_Pnt c = r.point();
    const gp_Dir axis = r.dir();
    const gp_Dir reference = r.dir();
    const double radius = r.next();
    const double start = r.next();
    const double end = r.next();
    Handle(Geom_Circle) circle = new Geom_Circle(gp_Ax2(c, axis, reference), radius);
    BRepBuilderAPI_MakeEdge e(circle, start, end);
    if (!e.IsDone()) throw std::runtime_error("a profile arc could not be made");
    return e.Edge();
  }
  const size_t degree = r.size(25);
  const size_t points = r.size(100000);
  const size_t knots = r.size(100000);
  if (degree < 1 || points < degree + 1 || knots != points + degree + 1) {
    throw std::invalid_argument("a profile NURBS has the wrong number of points or knots");
  }
  TColgp_Array1OfPnt poles(1, static_cast<int>(points));
  TColStd_Array1OfReal weights(1, static_cast<int>(points));
  for (size_t i = 0; i < points; ++i) {
    poles.SetValue(static_cast<int>(i) + 1, r.point());
    weights.SetValue(static_cast<int>(i) + 1, r.next());
  }
  std::vector<double> flat(knots);
  for (size_t i = 0; i < knots; ++i) flat[i] = r.next();
  std::vector<double> distinct;
  std::vector<int> mults;
  for (double k : flat) {
    if (!distinct.empty() && k == distinct.back()) {
      ++mults.back();
    } else {
      distinct.push_back(k);
      mults.push_back(1);
    }
  }
  TColStd_Array1OfReal ks(1, static_cast<int>(distinct.size()));
  TColStd_Array1OfInteger ms(1, static_cast<int>(mults.size()));
  for (size_t i = 0; i < distinct.size(); ++i) {
    ks.SetValue(static_cast<int>(i) + 1, distinct[i]);
    ms.SetValue(static_cast<int>(i) + 1, mults[i]);
  }
  Handle(Geom_BSplineCurve) curve = new Geom_BSplineCurve(poles, weights, ks, ms, static_cast<int>(degree));
  BRepBuilderAPI_MakeEdge e(curve);
  if (!e.IsDone()) throw std::runtime_error("a profile NURBS could not be made");
  return e.Edge();
}

// A planar face of a profile: a loop count, then each loop (the outer first, then its holes) as
// a curve count and its curves in order, each starting where the one before ends.
TopoDS_Face read_profile(const void* data, size_t count) {
  Reader r{data, count};
  const size_t loops = r.size(100000);
  if (loops == 0) throw std::invalid_argument("the profile has no loop");
  std::vector<TopoDS_Wire> wires;
  for (size_t l = 0; l < loops; ++l) {
    const size_t curves = r.size(1000000);
    if (curves == 0) throw std::invalid_argument("a profile loop has no curve");
    BRepBuilderAPI_MakeWire w;
    for (size_t c = 0; c < curves; ++c) {
      w.Add(read_edge(r));
      if (!w.IsDone()) throw std::runtime_error("a profile loop's curves do not join");
    }
    if (!w.Wire().Closed()) throw std::invalid_argument("a profile loop is not closed");
    wires.push_back(w.Wire());
  }
  if (r.at != count) throw std::invalid_argument("the profile's data runs past its loops");
  BRepBuilderAPI_MakeFace f(wires[0], Standard_True);
  if (!f.IsDone()) throw std::runtime_error("the profile's outer loop is not a planar face");
  for (size_t l = 1; l < wires.size(); ++l) f.Add(wires[l]);
  if (!f.IsDone()) throw std::runtime_error("the profile's holes could not be added");
  // Holes run the other way round from the outer loop in a face; set each loop's orientation
  // from where it lies.
  ShapeFix_Face fix(f.Face());
  fix.FixOrientation();
  return fix.Face();
}

}  // namespace

// The profile's face (read_profile's data, `count` doubles) extruded `distance` along the unit
// direction.
API uint32_t occt_prism_profile(const void* data, size_t count, double dx, double dy, double dz, double distance) {
  return guard_handle([&]() -> uint32_t {
    if (!(distance > 0.0)) throw std::invalid_argument("the extrusion distance must be above 0");
    const TopoDS_Face face = read_profile(data, count);
    BRepPrimAPI_MakePrism maker(face, gp_Vec(gp_Dir(dx, dy, dz)) * distance);
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("the extrusion could not be built");
    return put_shape(maker.Shape());
  });
}

// The profile's face turned `angle` radians (0 < angle <= 2 pi) about the line through the
// origin along the unit direction.
API uint32_t occt_revol_profile(const void* data, size_t count, double ox, double oy, double oz, double dx, double dy, double dz,
                                double angle) {
  return guard_handle([&]() -> uint32_t {
    const double tau = 6.283185307179586;
    if (!(angle > 0.0) || angle > tau) throw std::invalid_argument("the revolve angle must be above 0 and at most a full turn");
    const TopoDS_Face face = read_profile(data, count);
    const bool full = angle >= tau - 1e-9;
    const gp_Ax1 axis(gp_Pnt(ox, oy, oz), gp_Dir(dx, dy, dz));
    BRepPrimAPI_MakeRevol maker = full ? BRepPrimAPI_MakeRevol(face, axis) : BRepPrimAPI_MakeRevol(face, axis, angle);
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("the revolution could not be built");
    return put_shape(maker.Shape());
  });
}
