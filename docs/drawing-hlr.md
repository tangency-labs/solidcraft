# Hidden lines (W06a)

This is the geometry core of Drawing package W06 from the [published plan][plan] in
[#860][rfc]: [#2455][issue] / [PR #2457][pr]. It turns the subset that the W05a
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
- `cancel` is polled during the work; when it returns true the call ends with
  `HlrError::Cancelled`. `prepare_cancellable(&Brep, &Options, &cancel)` is `prepare` with
  such a callback, polled between edges, faces and surface comparisons.

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

- `Options::max_tracks` (curves) and `Options::max_rays` bound the work, as do internal
  caps: crossing grid entries (50 million), crossing candidate pairs (500 million, counted
  before the search, so no set of pairs met grows with them), crossings (50 million) and
  the surface comparisons that group a body's faces (50 million). Exceeding one returns
  `HlrError::Budget`.
- `Options::trim_samples` is 1 to `MAX_TRIM_SAMPLES` (4096), and every coordinate of a
  placed body at most `MAX_COORDINATE` (1e12 mm, the bound `geom::section` and
  `geom::brep` put on stored numbers); otherwise the call returns `HlrError::Invalid`.
- `Options::tangent_edges` (default on) draws edges between tangent faces, flagged
  `tangent: true`; drafting policy for them is W17.
- Tolerances are relative to the faces and the scene. They are meant for faces up to
  about 10 m across and features down to about 1e-3 mm. A spline patch only a sliver wide
  next to its radii (a few millionths of them) cannot be told from its neighbours' surfaces
  reliably and is reported as freeform. Distinct surfaces that
  recognition takes for one (closer than 1e-9 of their size or position, a nearly flat
  cone's far apex included) share the own-face margin of 1e-6 of the face's size when a
  ray leaves one of them.
- Not in this slice: parallel per-curve work and the shared work limits of
  `geom::section`, the kernel adapter (after W02), perspective, section regions (W03),
  views and commands (W07), and mirrored placements. Of the records the kernel now saves,
  planes through three points (`PlaneThrough`) are drawn as planes; a body with a procedural
  `SurfaceIntersection` curve or a `PlacedRevolution` surface (whose placement may scale it
  unevenly) is refused by `prepare` with `HlrError::Unsupported` until the adapter handles
  them.

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
- a box whose top plane record stands one rounding (or 1e-11 of 100 mm) off its edges still
  hides its back edges, and so does a 0.001 mm cube 1e7 mm out whose top plane is one step
  above its edges; a plate's sixteen holes each hide the same part of their bottom circle;
  eight coaxial cylinders seen along their axis show one circle; 300 boxes apart on screen
  are each drawn as the first one; an arc that bulges across a line 1 mm off its chord still
  crosses it twice; the trim's chord grid agrees with a scan of every chord; surface groups
  agree with comparing every pair of faces, and preparing can be cancelled;
- mirrored placements, bad body indices and bad views are refused; cancellation and
  budgets return errors;
- a box stored with planes through three points draws the same bits as one with
  axis-stored planes; placed revolutions and procedural intersection curves are refused as
  not supported yet;
- torus ray roots for thin, large and nearly tangent cases, and torus silhouettes near
  the axis direction.

[rfc]: https://github.com/tangency-labs/solidcraft/issues/860
[plan]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6234779815
[findings]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6243691483
[issue]: https://github.com/tangency-labs/solidcraft/issues/2455
[pr]: https://github.com/tangency-labs/solidcraft/pull/2457
[w05-issue]: https://github.com/tangency-labs/solidcraft/issues/2105
[w05-pr]: https://github.com/tangency-labs/solidcraft/pull/2106
