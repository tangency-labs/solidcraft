# nurbs-grid-mesh

Meshes a trimmed NURBS face as a grid in its own (u, v). It's written for dense surfaces, with
control nets in the hundreds each way, where a refinement-tree mesher spends seconds per face.
It's pure Rust with no dependencies and no `unsafe`, and it builds for `wasm32-unknown-unknown`.
Library code never panics, unwraps or indexes. It's single-threaded and deterministic.

This is not a port unit. It's a separate workspace crate that builds under the workspace lints
and touches nothing in the ported kernel, `plan/` or `difftest`.

## Measured

The test surface is a sculpted "brain coral" surface: degree 7 x 7, 318 x 281 poles, about
490 x 430 mm, with 306 x 269 distinct knots. It's meshed as one face over its whole domain. Both
meshers ran single-threaded on the same Windows laptop.

Both meshes were graded by one checker, `smlib-bench --check`. It samples 3,000 triangles, takes
7 points on each, and projects every point onto the surface two independent ways: SMLib's
`DropPoint`, and a grid search on a separate de Boor evaluator.

| Mesher | Setting | Time | Triangles | Worst | Over 0.312 mm |
| --- | --- | --- | --- | --- | --- |
| SMLib (C++, native) | chord 0.312 mm, 14° | 11.6 – 14.3 s | 81,856 | 4.65 mm | 21.5% |
| SMLib | chord 0.1 mm, 10° | 20.7 – 22.4 s | 148,551 | 2.44 mm | (35.5% over 0.1) |
| **this crate** | 0.312 mm | **114 – 120 ms** | 799,120 | **0.10 mm** | **0%** |
| **this crate** | coarsest: one cell per knot span | **27 – 39 ms** | 164,626 | **0.49 mm** | 0.13% |

This crate's own `deviation`, run over every triangle rather than a sample, agrees. At 0.312 mm,
none of 799,120 triangles is over (worst 0.207 mm). At the coarsest grid the worst is 0.645 mm.

A second surface from the same model (318 x 321 poles) gives the same picture. SMLib took
14.1 – 15.3 s for 89,948 triangles, with a worst of 4.76 mm and 22.1% over 0.312 mm. This crate
took 139 – 194 ms for 936,382 triangles, with a worst of 0.219 mm and none over (all triangles checked).

### Surfaces not drawn on a grid

The coral's control points sit on an even grid, which suits a grid mesher. These twelve are built
not to: six made for it (uneven knots, skewed parameter speed, a sharp local feature, a pole,
heavy twist, a dense jittered fit) and six from the public
[SSI corpus](https://github.com/jyviko/ssi-corpus), including rational spheres, tori and an
ellipsoid. Each is one untrimmed face over its whole domain. SMLib ran at chord = tolerance and
15°; its bench was extended to read weights for the rational ones. **Worst** is the largest
triangle error as a multiple of the tolerance: the larger of every triangle at 7 points and
SMLib's checker on 3,000 triangles. Tolerances without a unit are for corpus surfaces 1 to 10
units across.

| Surface | Tolerance | SMLib time | SMLib triangles | SMLib worst | Grid time | Grid triangles | Grid worst |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Car hood, bicubic 7 x 5 | 0.05 mm | 57 – 72 ms | 3,328 | **4.49x** | 5.5 – 8.9 ms | 23,710 | 0.36x |
| Local bump, 8 mm on a flat 200 mm square | 0.05 mm | 38 – 44 ms | 755 | **65.8x** | 1.5 – 2.0 ms | 6,076 | 0.70x |
| Skewed, parameter speed 28 : 1 | 0.05 mm | 21 – 32 ms | 1,820 | **6.24x** | 3.0 – 3.1 ms | 7,432 | 0.70x |
| Scan fit, 80 x 80 jittered | 0.05 mm | 1.48 – 1.61 s | 104,590 | **1.66x** | 47 – 49 ms | 384,809 | 0.52x |
| Twisted strip, two full turns | 0.05 mm | 9.4 – 10.5 ms | 256 | **19.0x** | 4.3 – 4.6 ms | 7,372 | 0.53x |
| Pole dome, one edge collapsed | 0.05 mm | 85 – 194 ms | 4,512 | 0.76x | 3.8 – 4.0 ms | 12,513 | 0.41x |
| Sphere, rational (corpus) | 0.001 | 134 – 152 ms | 16,128 | **1.21x** | 8.7 – 9.1 ms | 51,662 | 0.40x |
| Ellipsoid, rational (corpus) | 0.001 | 521 – 569 ms | 28,368 | **109x** | 14 – 15 ms | 114,818 | 0.50x |
| Torus, rational (corpus) | 0.001 | 614 – 670 ms | 57,348 | 0.70x | 18 – 19 ms | 94,396 | 0.80x |
| Tube arm, closed (corpus) | 0.001 | 73 – 78 ms | 7,532 | **1.10x** | 5.6 – 6.9 ms | 19,926 | 0.51x |
| S-wave ribbon, degree 2 x 5 (corpus) | 0.001 | 49 – 51 ms | 3,842 | **1.30x** | 5.9 – 6.1 ms | 17,709 | 0.40x |
| Warped plane, flat (corpus) | 0.001 | 0.4 – 0.9 ms | 2 | 0x | 0.8 – 1.1 ms | 1,052 | 0x |

The grid holds the tolerance on 12 of 12 and SMLib on 3 of 12. The grid is 2 – 37x faster on 11;
SMLib is faster on the flat plane. Where SMLib does hold the tolerance it uses fewer triangles:
1.6x fewer on the torus, 2.8x on the dome, and 2 against 1,052 on the plane.

These surfaces found two problems in the first version, both fixed before the run above. The
curvature bound let the rational surfaces and the dome go up to 2x over tolerance, and poles
left zero-area triangles. The measured refinement (step 2 below) and the zero-area filter are the
fix; they made the coral about 40 ms slower and 21% denser.

The ranges are three runs each. The SMLib runs are from an earlier session on the same machine,
not interleaved with these. At a gap of more than 100x, interleaving won't change the conclusion.
For scale, OpenCascade 7.6 compiled to WebAssembly took 33 s to mesh the model these surfaces
come from, plus 23 s in ShapeFix to find the face boundary in (u, v). That ran in a different
pipeline and wasn't graded by this checker.

The speed comes from having no refinement tree, no per-triangle surface test, and no adaptive
subdivision. Evaluation runs a grid row at a time: basis functions once per grid line, the sum
over u contracted into one row of poles, then a short dot product per point.

## The method

1. **Grid lines.** Lines are placed in u and in v, as many in each knot span as that span's
   curvature needs. The second derivative of a B-spline is bounded by its second-derivative
   control points, and a chord of parameter length h sags at most h²|C''|/8. Every iso-curve is
   a convex combination of the pole rows, so the largest value over all rows bounds every one.
2. **Measured refinement.** That bound is an estimate for rational surfaces, and it ignores the
   twist a cell's diagonal sees. So every cell's edge midpoints and both diagonal midpoints are
   compared with the surface at the same (u, v), and any column or row past ¾ of the tolerance is
   split, until none is (`TrimmedMesh::refined_lines` counts the lines added).
3. **The boundary.** The face's loops, given in (u, v) with their 3D points, are split wherever
   they cross a grid line.
   - A cell no loop touches is wholly in or wholly out, decided once per row by counting
     crossings, and becomes two triangles.
   - A cell a loop does touch is cut along it. The loop pieces inside the cell, joined by the
     cell's corners going anticlockwise, are the face's polygons in that cell, and those are
     ear-clipped.
   - A hole smaller than one cell gets a grid line put through it.
4. **The points.** Interior points are the surface's own. Boundary points are the loops' own 3D
   points, with crossings interpolated along each loop's 3D segment. The face ends exactly where
   its edge polylines are, so a neighbor meshed from the same polylines meets it. Zero-area
   triangles (at a pole, or fanned over a straight run of boundary) are left out.

## Use

```rust
use nurbs_grid_mesh::{mesh_trimmed, deviation, LoopPoint, MeshOptions, NurbsSurface};

// Poles flat by i over u, then j over v; weights None for a polynomial surface.
let s = NurbsSurface::new(p, q, u_knots, v_knots, nu, nv, poles, weights)?;

// One Vec per loop: the edge polylines' points with their (u, v) on this surface.
// Any winding; the loop with the largest (u, v) area is taken as the outer one.
let loops: Vec<Vec<LoopPoint>> = edge_polylines_of_the_face();

let m = mesh_trimmed(&s, &loops, face_is_reversed, &MeshOptions::new(0.05))?;
// m.positions, m.uvs, m.triangles ([u32; 3], anticlockwise about the face normal)

let d = deviation(&s, &m, 0.05)?;   // measured, not assumed: d.max, d.mean, d.over
```

## What the tolerance does and doesn't promise

- **What's measured:** every cell's edge midpoints and both diagonal midpoints are within ¾ of
  the tolerance. For a locally quadratic surface the error inside a triangle is at most 4/3 of
  the worst of those, so ¾ keeps the triangle within tolerance. That's a measurement at the
  places the error peaks, not a proof. On the 14 surfaces above (polynomial and rational) every
  triangle came out within tolerance, the worst at 0.80x.
- **`MeshOptions::measured_fraction`:** set it to 1.0 to trust the measured points as they
  are. On the twelve surfaces that made up to 46% fewer triangles, about a quarter fewer on most;
  the worst interior triangle was 1.0x (the torus) and the worst boundary triangle 1.5x.
- **Line cap:** `MeshOptions::max_lines_per_span` (default 64) caps the first estimate. Capped
  spans are counted in `TrimmedMesh::capped_spans`; the measured refinement isn't capped.
- **Minimum density:** the grid has at least one cell per knot span. On a surface with dense
  knots, that sets a floor (164,626 triangles on the coral).
- **More triangles than a tree mesher where both hold tolerance:** grid lines run the full width
  of the face, so detail in one place adds lines everywhere. A flat plane with a warped
  parameterization gets about a thousand triangles where SMLib makes 2.
- **Thin triangles at poles:** cells next to a collapsed edge are thin wedges. The ellipsoid
  had 11,936 triangles under 5° against SMLib's 1,748.
- **Hairline slivers:** a boundary point within a billionth of the face's extent of a grid line
  is moved off it by that much, which can leave a hairline triangle. There were at most 18 on one
  face of the 14.
- **Boundary chords are the caller's:** boundary vertices lie on the edge polylines, not on the
  surface. Edge polylines need to be within tolerance themselves.
- **Neighbors:** a neighbor meshed with a different grid meets along the same 3D polyline but
  with T-junctions. That's geometrically closed, not topologically.
- **Seams:** the caller hands in loops already split at a periodic seam, within one parameter
  period.

## Fitting it into solidcore

- **Add it beside `query::tess`, don't replace it.** The ported tessellator keeps its 1e-9
  parity. This becomes a second entry point, judged against exact truth with `deviation`
  instead of against the C++. The dispatch we use is NURBS faces with at least 1,000 control
  points go here, and everything else stays on the general mesher.
- **Types:** `[f64; 3]` maps to `Point3`, and `MeshError` maps to `SolidError`. `LoopPoint`s
  come from the face's loops: each coedge's pcurve gives (u, v), and the edge's tessellation
  gives the 3D point.
- **`deviation` works on any mesh.** It measures the centroid, the edge midpoints and three
  interior points per triangle. That makes it usable as the tolerance gate for the ported
  mesher too (the issue #390 kind of check).

## Run it

```
cargo test -p nurbs-grid-mesh                               # 11 tests against closed-form answers
cargo clippy -p nurbs-grid-mesh --all-targets -- -D warnings
cargo run -p nurbs-grid-mesh --release --example coral -- <surface.json> [tolerance] [out.obj]
```

The example's JSON holds `uDegree`, `vDegree`, `nu`, `nv`, flat `uKnots` and `vKnots`, `poles`
flat at `(i * nv + j) * 3`, and optionally `weights`. It meshes the whole domain three times,
then with five round holes, then measures every triangle (about a minute at 0.312 mm on the
coral). The coral surface isn't bundled; ask @h0witzer for it.

## The tests

The 11 tests cover:

- **Exact areas.** A trimmed circle's area matches its polygon to 1e-9. A hole is cut out, and a
  hole smaller than a cell is meshed too.
- **Orientation.** Every triangle faces the right way, a reversed face turns every triangle
  over, and loops are accepted in either winding.
- **Grid spacing.** The spacing meets the tolerance, and a grid 16x coarser fails the same check.
- **Within tolerance.** A trimmed bumpy face, a rational cylinder and a dome with a pole all
  measure within the tolerance over every triangle, and a mesh 16x too loose is caught. Interior
  vertices of the cylinder land on its radius to 1e-12. The pole leaves no zero-area triangle.
- **Shared edges.** Two faces that share a wavy edge weld with no open edge.
- **Projection and bad input.** Points project back onto the surface, and bad input is refused
  with a named error.
