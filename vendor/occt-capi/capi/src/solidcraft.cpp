// SolidCraft's entry points: what its kernel adapter asks for beyond base.cpp.
// SPDX-License-Identifier: MIT OR Apache-2.0
#include "internal.hpp"

#include <BRepAlgoAPI_Splitter.hxx>
#include <BRep_Builder.hxx>
#include <TopoDS_Shell.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepOffset_MakeOffset.hxx>
#include <TopTools_ListOfShape.hxx>
#include <gp_Pln.hxx>
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

namespace {

// The sub-shapes of `kind` nearest each of `count` points (`points` is count * 3 doubles), each
// once, in the order first picked.
ShapeMap nearest(const TopoDS_Shape& shape, TopAbs_ShapeEnum kind, const void* points, size_t count) {
  if (count == 0) throw std::invalid_argument("nothing picked");
  ShapeMap all;
  TopExp::MapShapes(shape, kind, all);
  if (all.Extent() == 0) throw std::invalid_argument("the shape has nothing of the kind picked");
  ShapeMap chosen;
  for (size_t i = 0; i < count; ++i) {
    const gp_Pnt p(read_unaligned<double>(points, 3 * i), read_unaligned<double>(points, 3 * i + 1),
                   read_unaligned<double>(points, 3 * i + 2));
    const TopoDS_Vertex at = BRepBuilderAPI_MakeVertex(p).Vertex();
    int best = 0;
    double least = std::numeric_limits<double>::infinity();
    for (int e = 1; e <= all.Extent(); ++e) {
      BRepExtrema_DistShapeShape d(at, all(e));
      if (d.IsDone() && d.Value() < least) {
        least = d.Value();
        best = e;
      }
    }
    if (best == 0) throw std::runtime_error("nothing is near a picked point");
    chosen.Add(all(best));
  }
  return chosen;
}

}  // namespace

// Constant-radius fillet of the edges nearest each point. `points` is count * 3 doubles. An edge
// nearest two points is filleted once.
API uint32_t occt_fillet_nearest(uint32_t h, const void* points, size_t count, double radius) {
  return guard_handle([&]() -> uint32_t {
    const TopoDS_Shape shape = get_shape(h);
    const ShapeMap chosen = nearest(shape, TopAbs_EDGE, points, count);
    BRepFilletAPI_MakeFillet maker(shape);
    for (int e = 1; e <= chosen.Extent(); ++e) maker.Add(radius, TopoDS::Edge(chosen(e)));
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("fillet failed");
    return put_shape(maker.Shape());
  });
}

// Whether two of the edges nearest each point (count * 3 doubles) share a vertex: 1 if so, 0 if
// not.
API int32_t occt_picked_edges_meet(uint32_t h, const void* points, size_t count) {
  return guard<int32_t>(-1, [&]() {
    const ShapeMap chosen = nearest(get_shape(h), TopAbs_EDGE, points, count);
    ShapeMap seen;
    for (int e = 1; e <= chosen.Extent(); ++e) {
      ShapeMap ends;
      TopExp::MapShapes(chosen(e), TopAbs_VERTEX, ends);
      for (int v = 1; v <= ends.Extent(); ++v) {
        if (seen.Contains(ends(v))) return 1;
      }
      for (int v = 1; v <= ends.Extent(); ++v) seen.Add(ends(v));
    }
    return 0;
  });
}

// Constant-radius fillets, the edge nearest each point (count * 3 doubles) at its own radius
// (count doubles). Two points nearest one edge are refused.
API uint32_t occt_fillet_each(uint32_t h, const void* points, const void* radii, size_t count) {
  return guard_handle([&]() -> uint32_t {
    const TopoDS_Shape shape = get_shape(h);
    const ShapeMap chosen = nearest(shape, TopAbs_EDGE, points, count);
    if (static_cast<size_t>(chosen.Extent()) != count) throw std::invalid_argument("two picks are nearest one edge");
    BRepFilletAPI_MakeFillet maker(shape);
    for (int e = 1; e <= chosen.Extent(); ++e) {
      const double r = read_unaligned<double>(radii, static_cast<size_t>(e - 1));
      if (!(r > 0.0) || !std::isfinite(r)) throw std::invalid_argument("a radius must be above 0");
      maker.Add(r, TopoDS::Edge(chosen(e)));
    }
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("fillet failed");
    return put_shape(maker.Shape());
  });
}

// Equal-distance chamfer of the edges nearest each point (count * 3 doubles).
API uint32_t occt_chamfer_nearest(uint32_t h, const void* points, size_t count, double distance) {
  return guard_handle([&]() -> uint32_t {
    const TopoDS_Shape shape = get_shape(h);
    const ShapeMap chosen = nearest(shape, TopAbs_EDGE, points, count);
    BRepFilletAPI_MakeChamfer maker(shape);
    for (int e = 1; e <= chosen.Extent(); ++e) maker.Add(distance, TopoDS::Edge(chosen(e)));
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("chamfer failed");
    return put_shape(maker.Shape());
  });
}

// The solid hollowed to walls `thickness` thick inside it, open at the faces nearest each point
// (count * 3 doubles; none: a closed hollow is refused).
API uint32_t occt_shell_nearest(uint32_t h, const void* points, size_t count, double thickness) {
  return guard_handle([&]() -> uint32_t {
    if (!(thickness > 0.0)) throw std::invalid_argument("the wall must be thicker than 0");
    const TopoDS_Shape shape = get_shape(h);
    const ShapeMap chosen = nearest(shape, TopAbs_FACE, points, count);
    TopTools_ListOfShape open;
    for (int f = 1; f <= chosen.Extent(); ++f) open.Append(chosen(f));
    BRepOffsetAPI_MakeThickSolid maker;
    maker.MakeThickSolidByJoin(shape, open, -thickness, 1e-7, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Intersection);
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("shell failed");
    return put_shape(maker.Shape());
  });
}

// The faces nearest each point (count * 3 doubles) tilted by `angle` radians about their line
// on the neutral plane through (ox, oy, oz) square to (nx, ny, nz), leaning in going along the
// pull (px, py, pz).
API uint32_t occt_draft_nearest(uint32_t h, const void* points, size_t count, double ox, double oy, double oz, double nx, double ny,
                                double nz, double px, double py, double pz, double angle) {
  return guard_handle([&]() -> uint32_t {
    const TopoDS_Shape shape = get_shape(h);
    const ShapeMap chosen = nearest(shape, TopAbs_FACE, points, count);
    BRepOffsetAPI_DraftAngle maker(shape);
    const gp_Pln neutral(gp_Pnt(ox, oy, oz), gp_Dir(nx, ny, nz));
    for (int f = 1; f <= chosen.Extent(); ++f) {
      maker.Add(TopoDS::Face(chosen(f)), gp_Dir(px, py, pz), angle, neutral);
      if (!maker.AddDone()) throw std::runtime_error("a face can't be drafted");
    }
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("draft failed");
    return put_shape(maker.Shape());
  });
}

// The faces nearest each point (count * 3 doubles) moved `distance` along their outward normals
// (negative: into the solid), the faces around them extended or trimmed to meet them.
API uint32_t occt_offset_faces_nearest(uint32_t h, const void* points, size_t count, double distance) {
  return guard_handle([&]() -> uint32_t {
    if (!std::isfinite(distance) || distance == 0.0) throw std::invalid_argument("the offset must be finite and not 0");
    const TopoDS_Shape shape = get_shape(h);
    const ShapeMap chosen = nearest(shape, TopAbs_FACE, points, count);
    BRepOffset_MakeOffset maker;
    maker.Initialize(shape, 0.0, 1e-7, BRepOffset_Skin, Standard_True, Standard_False, GeomAbs_Intersection, Standard_False);
    for (int f = 1; f <= chosen.Extent(); ++f) maker.SetOffsetOnFace(TopoDS::Face(chosen(f)), distance);
    maker.MakeOffsetShape();
    if (!maker.IsDone()) throw std::runtime_error("offset failed");
    return put_shape(maker.Shape());
  });
}

// A sheet thickened into a solid by `thickness` along its normals (negative: against them; to
// both sides, half each, when `symmetric`).
API uint32_t occt_thicken(uint32_t h, double thickness, int32_t symmetric) {
  return guard_handle([&]() -> uint32_t {
    if (!(thickness != 0.0) || !std::isfinite(thickness)) throw std::invalid_argument("the thickness must not be 0");
    TopoDS_Shape shape = get_shape(h);
    if (symmetric != 0) {
      thickness = std::fabs(thickness);
      BRepOffset_MakeOffset back;
      back.Initialize(shape, -thickness / 2.0, 1e-7, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Intersection, Standard_False);
      back.MakeOffsetShape();
      if (!back.IsDone()) throw std::runtime_error("thicken failed");
      shape = back.Shape();
    }
    BRepOffset_MakeOffset maker;
    maker.Initialize(shape, thickness, 1e-7, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Intersection, Standard_True);
    maker.MakeOffsetShape();
    if (!maker.IsDone()) throw std::runtime_error("thicken failed");
    ShapeMap solids;
    TopExp::MapShapes(maker.Shape(), TopAbs_SOLID, solids);
    if (solids.Extent() == 0) throw std::runtime_error("thicken made no solid");
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
struct ProfileReader {
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
TopoDS_Edge read_edge(ProfileReader& r) {
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
std::vector<TopoDS_Wire> read_loops(ProfileReader& r) {
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
  return wires;
}

TopoDS_Face read_profile(const void* data, size_t count) {
  ProfileReader r{data, count};
  const std::vector<TopoDS_Wire> wires = read_loops(r);
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

// A solid ruled between closed sections: a section count, then each section as read_profile's
// data of one loop. Each curve of a section is ruled to the curve in the same place of the next.
API uint32_t occt_loft_ruled(const void* data, size_t count) {
  return guard_handle([&]() -> uint32_t {
    ProfileReader r{data, count};
    const size_t sections = r.size(50);
    if (sections < 2) throw std::invalid_argument("a loft needs two sections or more");
    BRepOffsetAPI_ThruSections maker(Standard_True, Standard_True, 1e-7);
    maker.CheckCompatibility(Standard_False);
    for (size_t i = 0; i < sections; ++i) {
      const std::vector<TopoDS_Wire> wires = read_loops(r);
      if (wires.size() != 1) throw std::invalid_argument("a loft section is one loop");
      maker.AddWire(wires[0]);
    }
    if (r.at != count) throw std::invalid_argument("the loft's data runs past its sections");
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("the loft could not be built");
    return put_shape(maker.Shape());
  });
}

namespace {

// `shape` cut by the plane through `origin` square to `normal`, bounded well past the shape.
TopoDS_Shape split_by_plane(const TopoDS_Shape& shape, const gp_Pnt& origin, const gp_Dir& normal) {
  Bnd_Box box;
  BRepBndLib::Add(shape, box);
  if (box.IsVoid()) throw std::invalid_argument("an empty shape");
  double reach = 1.0;
  for (const gp_Pnt& corner : {box.CornerMin(), box.CornerMax()}) reach = std::max(reach, 2.0 * origin.Distance(corner));
  BRepBuilderAPI_MakeFace face(gp_Pln(origin, normal), -reach, reach, -reach, reach);
  if (!face.IsDone()) throw std::runtime_error("the split plane could not be made");
  TopTools_ListOfShape arguments, tools;
  arguments.Append(shape);
  tools.Append(face.Face());
  BRepAlgoAPI_Splitter maker;
  maker.SetArguments(arguments);
  maker.SetTools(tools);
  maker.Build();
  if (!maker.IsDone() || maker.HasErrors()) throw std::runtime_error("the split failed");
  return maker.Shape();
}

}  // namespace

// The shape cut by the plane through (ox, oy, oz) square to (nx, ny, nz): a compound of its
// pieces.
API uint32_t occt_split_plane(uint32_t h, double ox, double oy, double oz, double nx, double ny, double nz) {
  return guard_handle([&]() -> uint32_t { return put_shape(split_by_plane(get_shape(h), gp_Pnt(ox, oy, oz), gp_Dir(nx, ny, nz))); });
}

// A sheet cut by the plane through (ox, oy, oz) square to (nx, ny, nz), keeping as one shell the
// faces on the side `side` says (above 0: the normal's side; below: the back).
API uint32_t occt_trim_plane(uint32_t h, double ox, double oy, double oz, double nx, double ny, double nz, int32_t side) {
  return guard_handle([&]() -> uint32_t {
    if (side == 0) throw std::invalid_argument("no side to keep");
    const gp_Pnt origin(ox, oy, oz);
    const gp_Dir normal(nx, ny, nz);
    const TopoDS_Shape cut = split_by_plane(get_shape(h), origin, normal);
    BRep_Builder builder;
    TopoDS_Shell shell;
    builder.MakeShell(shell);
    int kept = 0;
    ShapeMap faces;
    TopExp::MapShapes(cut, TopAbs_FACE, faces);
    for (int f = 1; f <= faces.Extent(); ++f) {
      GProp_GProps props;
      BRepGProp::SurfaceProperties(faces(f), props);
      const double height = gp_Vec(origin, props.CentreOfMass()).Dot(gp_Vec(normal));
      if ((side > 0) ? height > 0.0 : height < 0.0) {
        builder.Add(shell, faces(f));
        ++kept;
      }
    }
    if (kept == 0) throw std::runtime_error("nothing is left on that side of the plane");
    return put_shape(shell);
  });
}

// The shape's solid `index` (from 0, in TopExp order).
API uint32_t occt_solid_at(uint32_t h, int32_t index) {
  return guard_handle([&]() -> uint32_t {
    ShapeMap solids;
    TopExp::MapShapes(get_shape(h), TopAbs_SOLID, solids);
    if (index < 0 || index >= solids.Extent()) throw std::out_of_range("the shape has no such solid");
    return put_shape(solids(index + 1));
  });
}

// A solid ruled from a closed section (read_profile's data of one loop) to the point (x, y, z).
API uint32_t occt_loft_to_point(const void* data, size_t count, double x, double y, double z) {
  return guard_handle([&]() -> uint32_t {
    ProfileReader r{data, count};
    const std::vector<TopoDS_Wire> wires = read_loops(r);
    if (wires.size() != 1) throw std::invalid_argument("a loft section is one loop");
    if (r.at != count) throw std::invalid_argument("the loft's data runs past its section");
    BRepOffsetAPI_ThruSections maker(Standard_True, Standard_True, 1e-7);
    maker.CheckCompatibility(Standard_False);
    maker.AddWire(wires[0]);
    maker.AddVertex(BRepBuilderAPI_MakeVertex(gp_Pnt(x, y, z)).Vertex());
    maker.Build();
    if (!maker.IsDone()) throw std::runtime_error("the loft to a point could not be built");
    return put_shape(maker.Shape());
  });
}
