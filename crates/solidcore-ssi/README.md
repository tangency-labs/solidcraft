# solidcore-ssi

The `ssi` crate's surface/surface intersection behind solidcore's own inputs and output. A caller
passes the two `Surface`s and their domains, as for the port's `Surface::global_surface_intersect`,
and gets back the same `SurfaceIntersection` (3D curves, a UV curve on each surface, a type and a
deviation per curve), with the singular points on the curves beside it. `intersect` takes an `Engine` (`Port`, `Ssi`, or `SsiThenPort`, which falls
back to the port wherever `ssi` gives no answer), so a caller can switch per call.

This is not a port unit, and nothing in `crates/solidcore` calls it. It's the seam for an opt-in SSI,
measured against the port first. It builds under the workspace lints, depends only on `solidcore`
and `ssi`, and builds for `wasm32-unknown-unknown`.

## In: surfaces as patches (`patch`)

A surface goes to `ssi` as its own NURB's control net, so the patch's (u, v) are the surface's
(u, v). solidcore evaluates its analytic surfaces at their NURB's parameters too, so nothing is
remapped. A plane, cylinder, cone, sphere or torus also carries its analytic surface, whose implicit
equation `ssi` traces the other surface against. It is attached only when the net's points lie on
it. A face's domain inside the surface's is cut out of the net with the port's own `split_srf`
(knot insertion), which keeps the parameters.

Curve-bounded, STEP, offset and in-volume surfaces aren't given to `ssi` yet (`Error::Unsupported`).

## Out: curves built together (`curves`)

Each `ssi` curve is a list of points, each carrying both surfaces' parameters. It becomes a 3D curve
and two UV curves that share one parameter (chord length) and one knot vector: cubic Hermite
interpolants with continuous tangents. The 3D tangent runs along the two normals' cross product
where the surfaces cross, and comes from the neighboring points where they're tangent. Each UV
tangent is the 3D tangent pulled back through that surface's Jacobian, or comes from the
neighboring points where the Jacobian is badly conditioned (near a pole). So the trims are the
intersection's own parameter curves, not fits projected onto the surfaces afterwards (upstream
issue #489).

A curve is split:

- where it crosses a closed surface's seam, so each UV curve stays in its domain;
- at a collapsed edge (a sphere's pole, a cone's apex, a triangular patch's corner), where the
  coordinate along the edge names no point. It's placed by extrapolating from the curve's own
  approach to the pole. A step that runs through a pole gets the pole as a point, once the other
  surface is shown to pass through it;
- at corners, such as a coincident region's boundary.

Curves pass through the crossings and meetings `ssi` reports, as the corpus's branches do, and
those vertices come back beside the curves (`Intersection::vertices`). That's where a B-rep builder
splits its edges, so each singular point ends them (#393). Splitting the curves themselves there
made the two arcs of a lens between two crossings read as one curve lying on another
(`y23_t2_ex3_loop_two_curves`), the same reason `ssi` joins branches through its vertices.

Points are added and settled onto both surfaces by Newton:

- on every knot line a curve crosses. A rational circle's parameterization has a jump in its
  second derivative at its double knots, and a span straddling one put the UV curve 4.6e-6 off the
  3D curve on a unit sphere (3.3e-9 with the point);
- in the middle of every span whose measured deviation is over the tolerance, for up to six
  rounds.

A settled point is used only if it's finite and lies on its own step, not at an end and not on
another branch Newton ran to. Next to a domain edge or a pole, the UV tangents are limited so each
coordinate is monotone where its points are (Fritsch and Carlson's condition), so a UV curve never
leaves its domain. Touch points become degenerate curves of type `Touching`, as the port marks them.

The deviation is measured at the points and at three points inside every span. It isn't yet a
proved bound.

## Measured

`difftest --ssi-corpus` runs this crate as a fourth engine, `solidcore-ssi`. Each case's surfaces are
built with solidcore's own types (the port's builder), intersected here, and sampled the way the
port's output is sampled (4001 points per curve). The other engines are the C++ reference, the
port and `ssi` alone, which writes its marched points. jyviko's corpus, 119 cases, approximation
tolerance 1e-6:

| Engine | Pass | Not passing |
| --- | --- | --- |
| C++ reference (SMLib, usd-brep 1.0.0) | 81 | 38 (from #638's run) |
| `ssi`, points as marched | 118 | 1 refused (`y23_fig1d_two_quads`) |
| **`solidcore-ssi`**, B-spline curves | **118** | the same refusal |

On the same run (8 jobs in parallel, not interleaved, so rough), the totals were 6.1 s against
`ssi`'s 5.8 s, with per-case medians of 2.98 ms against 0.76 ms. `ssi`'s timer covers the
intersection alone, and this one covers the whole call: patches in, filling in, curves out.
