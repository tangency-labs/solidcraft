# Hidden lines (W06a)

This is the geometry core of Drawing package W06 from the [published plan][plan] in
[#860][rfc]: [#2455][issue] / [PR #2457][pr], with its parallel work and shared limits in
[PR #3013][pr2]. It turns the subset that the W05a
investigation ([#2105][w05-issue], [draft #2106][w05-pr], [findings][findings])
showed to be ready into code: exact orthographic hidden lines for placed bodies whose
faces are planes, cylinders, cones, spheres or tori.

The code is `crates/geom/src/hlr` (layer L0). It reads the neutral `geom::brep`
records, uses no kernel or UI types, returns errors instead of panicking and builds for
`wasm32-unknown-unknown`. It does not register commands, create views, extract bodies
from the kernel or draw sections. The read-only kernel adapter follows the W02
extraction contract; until then callers build `Brep` records themselves.

## Using it

```text
prepare(&Brep, &Options) -> Result<Prepared, HlrError>            once per body
hidden_lines(&[Prepared], &[Placement], &View, &Options, &cancel) -> Result<Drawing, HlrError>
```

- `prepare` checks the body (`Brep::validate`), recognises each face's surface, builds
  its trim and classifies each edge as sharp, tangent or seam. `Prepared::report` says
  what it found: surfaces and curves by kind, tangent and seam edges, faces whose trim is
  unsupported, freeform faces and the largest boundary chord error.
- A `Placement` puts a prepared body in the scene: `world = rotation * local +
  translation`, the rotation given by its columns. Several placements may share one
  prepared body. A placement that is not a proper rotation (a mirror, a scale) is refused
  with `HlrError::Invalid`.
- `View::new(direction_of_sight, up)` is an orthographic view. Screen coordinates are
  `(p . x, p . y)` in millimetres, with `x = direction x up` normalised and `y = x x
  direction`.
- `cancel` is polled during the work, from every worker thread, so it must be `Sync`; when
  it returns true the call ends with `HlrError::Cancelled`.
- `Options::threads` sets the number of worker threads (0, the default, means one per core);
  see [Parallel work](#parallel-work).

The `Drawing` holds one `Line` per drawable edge and per silhouette curve of each
placement, in order of placement. A line names its `placement` and `source` (`Edge { edge, tangent }`
or `Silhouette { face }`) and is split into `pieces`. Each `Piece` has:

| Field | Meaning |
| --- | --- |
| `t` | Parameter interval on the source curve. |
| `visibility` | `Visible`, `Hidden` or `Uncertain`. |
| `covered` | Another piece already draws the same projected curve here; draw only uncovered pieces. |
| `geometry` | The piece in screen coordinates as `Seg2` segments. |
| `accuracy` | `CurveAccuracy` of `geometry` (the [W01a vocabulary](drawing-geometry.md)). |

`Drawing::unsupported` lists the faces whose silhouettes were not drawn and why;
`Drawing::stats` counts curves, crossings, pieces, rays and uncertain pieces, with phase
timings where the platform has a clock (zero on `wasm32-unknown-unknown`).

## What is exact

| Source | Geometry | Accuracy |
| --- | --- | --- |
| Line edges; cylinder and cone silhouette rulings | one `Seg2::Line` | `Analytic` |
| Circle and ellipse edges; sphere silhouettes; torus silhouettes seen along the axis | `Seg2::Conic` arcs of at most 90 degrees | `Analytic` |
| Spline and intersection edges | chords between exact points, halved until each piece's control hull is within the sag target | `Sampled` (a bound from that hull) |
| Torus silhouettes in other views | chords between traced points, halved to 1e-7 of the torus's size | `Sampled` (measured deviation) |

An orthographic projection is affine, so a circle projects to an ellipse, which a
rational quadratic arc represents exactly. Chords are refined until their sag on screen
is at most 1e-7 of the scene size. A fitted edge record also carries its own bound, and
the visibility work allows for it.

Surfaces are recognised whether they are stored as analytic records, as revolutions of
lines and circles or as spline patches (to 1e-9 relative; a patch whose control points lie
in one plane is that plane). A spline curve or patch is taken for a circle, quadric or torus
only when it lies on it at enough points of every knot span to make the check exact for its
degree, with its largest distance there, scaled by how far apart its weights are, within
the tolerance: weights near zero cannot hide a swerve, and widely spread weights mean chords.
A recognised patch is trimmed and intersected in its own analytic chart. Faces that lie on one surface (a sphere
stored as two hemispheres, a cylinder split in two) share their silhouettes: the edges
between them are seams and are not drawn, and a silhouette running along such a seam is
one curve. A face joins such a set only when the first face's surface passes through its
own points to rounding (1e-13 of its size); recognition alone groups surfaces more loosely,
and such faces keep silhouettes of their own.

## Visibility

Projected curves are split where they cross or touch, found on a screen grid and
refined on the exact curves. Each piece is classified by rays toward the viewer against
every trimmed face: closed forms for planes and quadrics, a sampled quartic for tori.
A ray that grazes a face, meets one within its boundary tolerance or meets another body
right at its start makes the piece `Uncertain`; nothing is guessed.

Coincident curves (an edge and a silhouette on top of each other, two touching bodies)
follow the W05a policy: visible wins and duplicates are marked `covered`. The tolerance
comes from the curves' own accuracy (chord sag and fit bounds) plus a rounding allowance
of 1e-7 mm and 1e-9 of the scene size, never from the scene size alone, so small parts in
large scenes keep their detail.

`prepare` moves each body to its own centre, and the work is done in a frame centred on
the scene and shifted back at the end, so coordinates stay small: a part 1e7 mm from the
origin, whether placed there or modelled there, is classified as at the origin.

## Parallel work

`hidden_lines` spreads its per-curve work over worker threads: silhouette tracing, screen
sampling, the cuts at the boxes of left-out faces, the classification of pieces, coverage,
and the crossing search (by rows of its screen grid) with the refinement of each crossing.
Each phase hands its items to the workers one at a time and stores every result at its
item's index, so the drawing is identical, bit for bit, for any number of threads: the same
pieces in the same order and the same statistics apart from the phase timings.

- **Thread count.** With `Options::threads` at 0 (the default) a phase uses up to one worker
  per core, but at most one per distinct silhouette tracing, 16 curves, 4 grid rows or 256
  crossings, so the per-curve phases of a small view stay on the calling thread. 1 runs
  everything on the calling thread; a larger number caps the workers. The browser build
  always runs on the calling thread.
- **Shared silhouettes.** Silhouettes depend on the body and the direction of sight in its
  own frame, not on its position, so placements that share both (repeated parts turned
  alike) share one tracing. This speeds up the browser build too.
- **Cancellation.** `cancel` is polled before every item, on whichever thread runs it, and
  inside long items: every 256 faces and before every face a silhouette is trimmed to, at
  every sampling pass, every 4096 chords and every 1024 boxes met when cutting at left-out
  faces, every 1024 rays, every 65 536 crossing candidates, and between the chords of a piece
  once 65 536 coverage checks have passed since the last poll.
  The work between two polls is bounded by the input limits, not by a time: in ordinary
  scenes the longest step is tracing one surface's silhouette curves, a few tenths of a second
  for a torus, but a scene built at the limits can make one step take longer.
- **Errors.** The first error, cancellation included, stops the other workers and is the one
  returned. When two different failures coincide on different threads (a cancellation while
  a budget is exceeded), the error returned may differ from a run on one thread; a successful
  result never does.
- **Panics.** Every parallel item runs on a spawned worker that the calling thread joins. A
  worker that panics loses its item and stops the other workers: the call returns
  `HlrError::Internal` and no drawing, and the panic does not reach the caller. Work on the calling thread (one thread, or the browser
  build) lets a panic propagate to the caller as before.

Measured on a 32-thread desktop (release build, isometric view of boxes, cylinders, spheres
and tori): 100 placements, each turned differently, take 0.8 s, against 9.9 s on one thread;
300 placements turned alike take 0.8 s, against 33 to 55 s before silhouettes were shared.

## Freeform faces

Faces that are not planes, cylinders, cones, spheres or tori are freeform. By default
they are listed in `Drawing::unsupported` with the reason `freeform face`, their
silhouettes are not drawn, and a piece whose ray meets one is `Uncertain`, never
`Hidden`. Without its silhouettes nothing marks where such a face's outline crosses a curve,
so curves are cut at the face's screen box and every piece inside the box that can reach
behind its nearest point (bounded by the curve's control points, not by samples) is
`Uncertain` too, the face's own edges included (faces left out for an unsupported trim
likewise). With `Options::freeform` set their silhouettes are contoured and their ray
tests use seeded Newton iterations: slower, `Sampled`, and subject to the misses listed
in the W05a findings.

## Limits

Inputs are checked before any work:

| Input | Limit | Otherwise |
| --- | --- | --- |
| Records of one body | `brep::MAX_ITEMS` (1 000 000), checked by `Brep::validate` in `prepare` | `Invalid("portable B-rep")` |
| Placements in one scene | `hlr::MAX_ITEMS` (100 000) | `Budget("placement")` |
| `Options::trim_samples` | 1 to `MAX_TRIM_SAMPLES` (4096) | `Invalid("trim samples")` |
| `Options::threads` | at most `MAX_THREADS` (64) | capped |

Work budgets are totals over the whole call and all worker threads. Whether one is exceeded
does not depend on the number of threads or on their scheduling:

| Work | Budget | Error |
| --- | --- | --- |
| Curves: drawable edges and silhouette pieces | `Options::max_tracks`, default `hlr::MAX_ITEMS` | `Budget("curve")` |
| Visibility rays | `Options::max_rays`, default 20 000 000 | `Budget("ray")` |
| Crossing grid entries | 50 million | `Budget("crossing grid")` |
| Crossing candidate pairs, counted before the search | 500 million | `Budget("crossing pair")` |
| Crossings | 50 million | `Budget("crossing")` |
| Left-out face grid entries | 50 million | `Budget("shadow grid")` |
| Left-out face box checks | 500 million | `Budget("shadow")` |
| Coverage point checks | 200 million | `Budget("coverage")` |

`hlr::MAX_ITEMS` is `section::MAX_ITEMS`, the aggregate item limit that `geom::section`
applies, and the body limit of `file.export_view_dxf`. It bounds the placements a caller
supplies and, by default, the number of curves, which is the number of lines in the drawing;
a caller drawing a large assembly may raise `max_tracks`. A body's own records have the
larger limit of `geom::brep`, which stores whole evaluated models.
The ray budget is HLR's own: it counts visibility classifications, one per ray. The 200 million
work steps of `file.export_view_dxf` count other work (placements, tree nodes and triangles of
its occlusion test), so the two numbers are not comparable. Every piece costs at least one
ray, so the ray budget also bounds the pieces. The remaining caps bound
the steps in between.

Other limits:

- `Options::tangent_edges` (default on) draws edges between tangent faces, flagged
  `tangent: true`; drafting policy for them is W17.
- Tolerances are relative to the faces and the scene. They are meant for faces up to
  about 10 m across and features down to about 1e-3 mm. A spline patch only a sliver wide
  next to its radii (a few millionths of them) cannot be told from its neighbours' surfaces
  reliably and is reported as freeform. Distinct surfaces that
  recognition takes for one (closer than 1e-9 of their size or position, a nearly flat
  cone's far apex included) share the own-face margin of 1e-6 of the face's size when a
  ray leaves one of them.
- Not in this slice: the kernel adapter (after W02), perspective, section regions (W03),
  views and commands (W07), and mirrored placements. Of the records the kernel now saves,
  planes through three points (`PlaneThrough`) are drawn as planes; a body with a procedural
  `SurfaceIntersection` curve or a `PlacedRevolution` surface (whose placement may scale it
  unevenly) is refused by `prepare` with `HlrError::Unsupported` until the adapter handles
  them. A single surface's silhouette is traced on one thread, so one torus seen obliquely
  still takes about 0.3 s whatever the thread count.

## Fixtures

`crates/geom/src/hlr_tests.rs` builds bodies from records and checks analytic positions
and intervals:

- a cube in an isometric view hides its three back edges; a box in front hides the
  middle of an edge over exactly the covered interval;
- a cylinder's silhouette rulings are exact lines, including one along the seam; a cone
  frustum seen from the front has exact rulings and draws each edge-on circle once;
- a sphere's outline is exact conic arcs at its radius in three views;
- a disc bounded by a rational circle with unevenly spaced knots, with a knot span narrower
  than rounding, or with a parameterisation that races through its first turn is drawn as
  arcs turning the full 360 degrees; splines off their circle between evenly spaced samples
  or inside such a narrow span are drawn as chords; exact arcs that turn almost all of
  their 358 degrees in the first hundredth of their parameter, or several radians in a
  knot span of 1e-6 or 1e-11, keep the turn of every piece;
- a torus stored as two halves, seen face on, is drawn at both equators, one silhouette
  each; an edge under it is hidden over exactly the intervals the tube covers; seen
  obliquely its outline spans R + r on both sides with a deviation under 1e-3 mm;
- cone frusta and narrow cone sectors a hair off one axis or with axes 3e-7 rad apart, and
  spherical caps through one circle with centres 5e-7 mm apart, keep their own outlines;
- a cylinder 1e5 and 1e7 mm from the origin draws as at the origin; a cone trimmed to its
  tip 1 mm below the apex hides nothing above it;
- a freeform patch is reported and makes the edges it covers uncertain, or hidden when
  freeform faces are enabled; a flat spline patch hides them for certain; a long edge behind
  a freeform tube is uncertain where the tube may cover it and visible elsewhere, and so is
  a sloping edge that only dips behind a freeform patch near its end, while one that leaves
  the patch's box below its level but crosses the box above it stays visible;
- a curve that swerves 2.9 mm off its circle where its weight is 1e-12, and a nearly straight
  cubic 0.002 mm off a circle of radius 1e7, are drawn as chords; an edge whose depth dips
  behind a freeform patch between its samples is uncertain there, and so is an arc seen
  along its plane whose projection folds back under a freeform patch between two samples,
  and the edge of a freeform sheet that curls back over it 0.005 mm above;
- a torus patch stored as a rational spline is recognised and draws its outer equator as an
  exact arc, and one with a bump in a knot span 0.002 of its parameters wide is not taken for
  a torus; a big circle passing under a thin freeform tube in a coarse scene is uncertain
  there;
- two strips of a torus 0.0003 rad wide keep their 0.006 mm of outline;
- two touching boxes draw each shared edge once;
- mirrored placements, bad body indices and bad views are refused; cancellation and
  budgets return errors;
- a box stored with planes through three points draws the same bits as one with
  axis-stored planes; placed revolutions and procedural intersection curves are refused as
  not supported yet;
- a mixed scene (touching boxes, cylinders, a sphere, an oblique torus and a left-out patch,
  with and without freeform faces) draws the same bits on 1, 2, 3, 8 and automatic threads;
  placements sharing a body and a turn draw as if each had its own copy of the body; the ray
  and curve budgets pass at the serial run's counts and fail one below them on 1 and 4
  threads; cancellation asked only from worker threads ends a parallel run, and a failing or
  panicking item ends the work with its error; scenes over `MAX_ITEMS` placements and trim
  sample counts out of range are refused; sampling one long spline and tracing a body's
  silhouettes ask for cancellation inside the item; spline points and part hulls come from
  their own knot spans;
- torus ray roots for thin, large and nearly tangent cases, and torus silhouettes near
  the axis direction.

[rfc]: https://github.com/tangency-labs/solidcraft/issues/860
[plan]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6234779815
[findings]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6243691483
[issue]: https://github.com/tangency-labs/solidcraft/issues/2455
[pr]: https://github.com/tangency-labs/solidcraft/pull/2457
[pr2]: https://github.com/tangency-labs/solidcraft/pull/3013
[w05-issue]: https://github.com/tangency-labs/solidcraft/issues/2105
[w05-pr]: https://github.com/tangency-labs/solidcraft/pull/2106
