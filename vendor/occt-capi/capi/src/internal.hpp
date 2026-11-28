// occt_capi: a small C ABI over OCCT. SPDX-License-Identifier: MIT OR Apache-2.0
//
// Rules of the ABI, shared by the native library and the wasm module:
//  - Objects cross as nonzero uint32 handles; 0 means failure, see occt_last_error_*.
//  - No C++ type or exception ever crosses. Every entry point goes through guard().
//  - Only scalars and (pointer, length) byte ranges, so a module with its own
//    linear memory works the same way (the caller copies with occt_alloc).
//    Input ranges need no alignment.
//  - A stale or wrong handle is an error, never undefined behaviour.
//
// This header is what each domain file in this directory builds on. Add entry points as
// `API ... occt_name(...)` at the start of a line, wrapped in guard(): the export list is
// generated from that pattern.
#pragma once

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <NCollection_IndexedMap.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Shape.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <limits>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>
#include <BRepBuilderAPI_MakeShape.hxx>
#include <Message_ProgressIndicator.hxx>
#include <Message_ProgressRange.hxx>
#include <Message_ProgressScope.hxx>
#include <TopTools_ListOfShape.hxx>
#include <atomic>

#if defined(_WIN32)
#define API extern "C" __declspec(dllexport)
#else
#define API extern "C" __attribute__((visibility("default")))
#endif

namespace occt_capi {


using Bytes = std::vector<uint8_t>;
using ShapeMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;

// Handle tables. Handles are never reused within a process, so a stale handle
// fails cleanly instead of silently naming a newer object.
// One lock over the whole kernel. OCCT shapes share topology that operations can
// mutate (meshing attaches triangulations), so calls are serialised. That is what lets
// the Rust side treat shapes as Send + Sync.
inline std::recursive_mutex g_kernel_mutex;
inline uint32_t g_next = 1;
inline std::unordered_map<uint32_t, TopoDS_Shape> g_shapes;
inline std::unordered_map<uint32_t, Bytes> g_buffers;

// Inside a function because Apple's linker rejects an `inline thread_local` variable used from
// several files: each object file's copy of its initialisation routine counts as a duplicate.
inline std::string& last_error() {
  thread_local std::string error;
  return error;
}

using KernelLock = std::lock_guard<std::recursive_mutex>;

inline uint32_t alloc_handle() {
  if (g_next == 0) throw std::runtime_error("handle space exhausted");
  return g_next++;
}

inline uint32_t put_shape(TopoDS_Shape shape) {
  if (shape.IsNull()) throw std::runtime_error("the operation produced an empty shape");
  KernelLock lock(g_kernel_mutex);
  const uint32_t h = alloc_handle();
  g_shapes.emplace(h, std::move(shape));
  return h;
}

inline uint32_t put_buffer(Bytes bytes) {
  KernelLock lock(g_kernel_mutex);
  const uint32_t h = alloc_handle();
  g_buffers.emplace(h, std::move(bytes));
  return h;
}

inline TopoDS_Shape get_shape(uint32_t h) {
  KernelLock lock(g_kernel_mutex);
  const auto it = g_shapes.find(h);
  if (it == g_shapes.end()) throw std::invalid_argument("invalid shape handle");
  return it->second;
}

template <typename R, typename F>
R guard(R on_error, F&& f) noexcept {
  KernelLock lock(g_kernel_mutex);
  try {
    last_error().clear();
    return f();
  } catch (const Standard_Failure& e) {
    last_error() = e.ExceptionType();
    if (*e.what() != '\0') last_error() += std::string(": ") + e.what();
  } catch (const std::exception& e) {
    last_error() = e.what();
  } catch (...) {
    last_error() = "unknown error in OCCT";
  }
  if (last_error().empty()) last_error() = "unknown error in OCCT";
  return on_error;
}

template <typename F>
uint32_t guard_handle(F&& f) noexcept {
  return guard<uint32_t>(0u, std::forward<F>(f));
}

template <typename T>
void append(Bytes& out, const T& value) {
  const auto* p = reinterpret_cast<const uint8_t*>(&value);
  out.insert(out.end(), p, p + sizeof(T));
}

inline uint32_t point_buffer(const gp_Pnt& p) {
  Bytes out;
  append(out, p.X());
  append(out, p.Y());
  append(out, p.Z());
  return put_buffer(std::move(out));
}

template <typename T>
T read_unaligned(const void* base, size_t index) {
  T value;
  std::memcpy(&value, static_cast<const uint8_t*>(base) + index * sizeof(T), sizeof(T));
  return value;
}

inline constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// A boolean's result is always a compound; one holding a single shape is that shape.
inline TopoDS_Shape unwrap_single(const TopoDS_Shape& shape) {
  if (shape.IsNull() || shape.ShapeType() != TopAbs_COMPOUND) return shape;
  TopoDS_Iterator it(shape);
  if (!it.More()) return shape;
  TopoDS_Shape only = it.Value();
  it.Next();
  return it.More() ? shape : only;
}

// STEP translation logs to stdout by default; a library must not write there.
inline void silence_messages() {
  static std::once_flag once;
  std::call_once(once, [] {
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
  });
}


// --- input decoding ------------------------------------------------------------

// Reads an unaligned little byte range written by the Rust side. Overrunning is an
// error, not undefined behaviour.
class Reader {
 public:
  Reader(const void* data, size_t len) : p_(static_cast<const uint8_t*>(data)), left_(len) {}
  template <typename T>
  T get() {
    if (left_ < sizeof(T)) throw std::invalid_argument("truncated input");
    T value;
    std::memcpy(&value, p_, sizeof(T));
    p_ += sizeof(T);
    left_ -= sizeof(T);
    return value;
  }
  double f64() { return get<double>(); }
  uint32_t u32() { return get<uint32_t>(); }
  gp_Pnt point() { const double x = f64(), y = f64(), z = f64(); return gp_Pnt(x, y, z); }
  gp_Vec vec() { const double x = f64(), y = f64(), z = f64(); return gp_Vec(x, y, z); }
  gp_Dir dir() { const double x = f64(), y = f64(), z = f64(); return gp_Dir(x, y, z); }
  TopoDS_Shape shape() { return get_shape(u32()); }
  size_t remaining() const { return left_; }

 private:
  const uint8_t* p_;
  size_t left_;
};

inline std::vector<TopoDS_Shape> read_shapes(const void* handles, size_t count) {
  std::vector<TopoDS_Shape> out;
  out.reserve(count);
  for (size_t i = 0; i < count; ++i) out.push_back(get_shape(read_unaligned<uint32_t>(handles, i)));
  return out;
}

// --- continuity -------------------------------------------------------------------

// How two faces meet along an edge, worst over samples of it.
struct Continuity {
  double gap = 0.0;            // model units
  double angle = 0.0;          // radians, 0 when smooth
  double curvature_gap = 0.0;  // 1 / model units
};

// Defined in surfacing.cpp. The edge needs to lie on both faces, not belong to them.
Continuity measure_continuity(const TopoDS_Edge& edge, const TopoDS_Face& a, const TopoDS_Face& b);

// --- output ----------------------------------------------------------------------

inline void append_point(Bytes& out, const gp_Pnt& p) {
  append(out, p.X());
  append(out, p.Y());
  append(out, p.Z());
}

// Appends u32 count, then a new handle per shape. The caller owns the handles.
inline void append_shapes(Bytes& out, const TopTools_ListOfShape& shapes) {
  append<uint32_t>(out, static_cast<uint32_t>(shapes.Extent()));
  for (TopTools_ListOfShape::Iterator it(shapes); it.More(); it.Next()) {
    append<uint32_t>(out, put_shape(it.Value()));
  }
}

// The layout decoded by `decode_modeled` on the Rust side:
//   u32 result handle
//   u32 modified count, then per entry: u32 input handle, u32 n, n handles
//   u32 generated count, the same way
//   u32 deleted count, then that many handles
// `Maker` needs Modified(s), Generated(s) and IsDeleted(s), as BRepBuilderAPI_MakeShape,
// BRepAlgoAPI_BuilderAlgo and BRepTools_History all do. Inputs are scanned for their
// distinct faces, edges and vertices.
template <typename Maker>
Bytes modeled_buffer(Maker& maker, const TopoDS_Shape& result,
                     const std::vector<TopoDS_Shape>& inputs) {
  Bytes modified, generated, deleted;
  uint32_t n_modified = 0, n_generated = 0, n_deleted = 0;
  ShapeMap seen;
  for (const TopoDS_Shape& input : inputs) {
    for (const TopAbs_ShapeEnum kind : {TopAbs_FACE, TopAbs_EDGE, TopAbs_VERTEX}) {
      ShapeMap subs;
      TopExp::MapShapes(input, kind, subs);
      for (int i = 1; i <= subs.Extent(); ++i) {
        const TopoDS_Shape& sub = subs(i);
        const int before = seen.Extent();
        seen.Add(sub);
        if (seen.Extent() == before) continue;
        const TopTools_ListOfShape& mod = maker.Modified(sub);
        if (!mod.IsEmpty()) {
          ++n_modified;
          append<uint32_t>(modified, put_shape(sub));
          append_shapes(modified, mod);
        }
        const TopTools_ListOfShape& gen = maker.Generated(sub);
        if (!gen.IsEmpty()) {
          ++n_generated;
          append<uint32_t>(generated, put_shape(sub));
          append_shapes(generated, gen);
        }
        if (maker.IsDeleted(sub)) {
          ++n_deleted;
          append<uint32_t>(deleted, put_shape(sub));
        }
      }
    }
  }
  Bytes out;
  append<uint32_t>(out, put_shape(result));
  append<uint32_t>(out, n_modified);
  out.insert(out.end(), modified.begin(), modified.end());
  append<uint32_t>(out, n_generated);
  out.insert(out.end(), generated.begin(), generated.end());
  append<uint32_t>(out, n_deleted);
  out.insert(out.end(), deleted.begin(), deleted.end());
  return out;
}

// --- cancellation ------------------------------------------------------------------

// Polls a byte owned by the caller, passed as an address. Address 0 means no flag, which
// is always the case on wasm: its memory is not the caller's.
class FlagProgress : public Message_ProgressIndicator {
 public:
  explicit FlagProgress(size_t flag_address)
      : flag_(reinterpret_cast<const std::atomic<uint8_t>*>(flag_address)) {}
  bool UserBreak() override { return flag_ != nullptr && flag_->load() != 0; }
  void Show(const Message_ProgressScope&, const bool) override {}

 private:
  const std::atomic<uint8_t>* flag_;
};

}  // namespace occt_capi
