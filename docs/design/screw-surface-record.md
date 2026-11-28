# Screw-swept surfaces in the body record

Status: approved by the owner on 2026-11-27 ("threads faster and exact"). Kernel work, decided
in solidcore. Linked from [kernel-layer.md §3](kernel-layer.md#3-the-body-exchange-format).

## The problem

The body record (`solidcore_record::brep`, schema 4) is what SolidCraft saves in a design's
model cache and what every kernel adapter takes and returns (kernel-layer.md §3). Its surface
kinds are plane, cylinder, cone, sphere, torus, ball patch, spline, rational spline, placed
revolution and blend. There is no swept or helical kind, so every screw-swept wall (a modelled
thread's flanks and crest, a coil) is written as its rational B-spline net alone:

- **It is large.** The net is the profile's control points times `2 × spans + 1` across the
  turns. An M30x1.5 thread 100 mm long has walls of about 13 000 × 2 control points.
- **It hits the record's caps.** The record takes at most 4096 control points along a
  direction, and solidcore reads a face that winds round its axis for at most 64 turns.
- **It loses what the wall is.** solidcore's walls keep their definition at run time
  (`solidcore::surface::procedural_sweep::ProceduralSweep`, #2572): the profile under a screw
  motion, and the net's certified distance from it. The record drops it, so a wall read back
  from a design or from another kernel is a plain spline, and another kernel can only take a
  thread as that spline. A kernel with an exact helical sweep of its own cannot cross one at
  all, in either direction.

## The new kind: `sweep`

A face's surface may be

```rust
Surface::Sweep { definition: Box<sweep::SweepDefinition> }
```

with the definition in a new module, `solidcore_record::brep::sweep`, as plain data:

```rust
pub struct SweepDefinition {
    /// The profile: a 3D `Curve::Spline` or `Curve::RationalSpline` (degree ≤ 16).
    pub profile: Curve,
    pub motion: SweepMotion,
    /// The stations across the turns, radians, strictly ascending, at least two: the net's
    /// knots across, and the face's turn range.
    pub stations: Vec<f64>,
    /// One half angle per span, each in (0, π/8].
    pub half_angles: Vec<f64>,
    /// From the face's (u, v) to the definition's (w, t).
    pub map: SweepMap,
    /// The net's distance from the definition as the kernel held it, mm.
    pub bound: f64,
}

pub enum SweepMotion {
    /// About the axis through `origin` along unit `axis`, rising `pitch` per turn.
    Screw { origin: [f64; 3], axis: [f64; 3], pitch: f64, law: Option<RadialLaw> },
}

/// The offset along `direction`, square to the axis, piecewise linear through
/// (`angles`, `offsets`) and held beyond its ends: a taper, or a run-out.
pub struct RadialLaw { pub direction: [f64; 3], pub angles: Vec<f64>, pub offsets: Vec<f64> }

/// `(w, t) = (a[0] + b[0] p, a[1] + b[1] q)`, `(p, q)` the face's `(u, v)`, or `(v, u)` when
/// `swapped`.
pub struct SweepMap { pub a: [f64; 2], pub b: [f64; 2], pub swapped: bool }
```

These are the fields of the kernel's `ProceduralSweep` with a screw motion, one for one, so
`solidcore-convert` copies them and nothing is worked out on the way.

- **Handedness** is the sign of `pitch` about the right-handed `axis`: positive is a right-hand
  thread. No separate flag, so there is nothing to disagree.
- **Taper** is the radial law. A tapered thread (NPT) is a two-point law, linear in the angle; a
  run-out is a law that ends at zero. The law the kernel takes today is the law the record
  stores.
- **A general path sweep** (`ProceduralSweep` with a path motion, slice 2 of #2572) is not in
  this kind yet. It goes in later as a second `SweepMotion` variant, the same way: additive, its
  own design note. Its walls stay splines until then.

### Exact evaluation

At the face's `(u, v)`, the map gives `(w, t)`. On the span `[t_i, t_i+1]`, with
`s = 2 (t − t_i) / (t_i+1 − t_i) − 1` and `τ_i = tan(h_i / 2)` (`h_i` the span's half angle):

```text
θ(t)    = (t_i + t_i+1) / 2 + 2 atan(τ_i s)
S(w, t) = o + R_k(θ) (P(w) − o + e δ(θ)) + k · pitch · θ / 2π
```

`P` is the profile, `R_k(θ)` the right-handed turn about `k` (Rodrigues), `e` and `δ` the law's
direction and offset (none: `δ = 0`). This is `ProceduralSweep::evaluate_definition`, which the
kernel already has; its partials are in closed form.

### Parametrisation: the net's, on purpose

The face's `(u, v)` are the net's, not the angle. `θ(t)` is the angle the net's rational
quadratic arc turns a point to at the same parameter, so S and its net are compared point for
point. Keeping the net's parameters means:

- every domain and pcurve in the record is the number the kernel holds today, bit for bit, so
  writing `sweep` instead of `rational_spline` moves no result;
- the net can be rebuilt from the definition exactly (below), in the same parameters;
- an exact consumer that wants the angle has it in closed form, `θ(t)` above, which is monotone
  on each span, so an iso line of one is an iso line of the other.

The natural normal is `∂S/∂u × ∂S/∂v` as for every kind, and agrees with the net's (the net and
S share their parameters and lie within the bound).

### Pcurves

Pcurves are in the face's `(u, v)`, as for every kind, and are the kernel's own trims:

- a helical edge (a profile vertex carried round) is `w` fixed: a straight pcurve;
- a wall's start or end (the profile at the first or last station) is `t` fixed: straight;
- a trim by another face (a thread's run-out against a chamfer) is the kernel's trim curve, as
  today.

Nothing is fitted or remapped on the way to or from the record.

### The net, and when it is stored

The net B is the rational B-spline the kernel builds from the definition (`sweep::screw`'s
wall): the profile's NURBS along `w`, and across the turns each control point's path as an exact
rational quadratic arc per span (weights `1, cos h_i, 1`), its lift and offset the same rational
quadratic of their values at the span's ends and middle.

- **Stage 1: the net is always stored**, as the face's `nurbs`, exactly as a `blend` face's is.
  Every consumer that reads a face's NURBS form reads it unchanged.
- **Stage 2: the net may be left out.** A writer leaves `nurbs` out only after rebuilding the
  net from the definition and finding it equal to the face's, number for number. A reader with
  no `nurbs` rebuilds it, through the kernel (`solidcore-ops`), so there is one implementation
  of the net. A face whose net is anything else (a piece cut by a boolean, keeping its
  parameters, or a reparametrised net) keeps its `nurbs`. The record then holds the definition
  (the profile's few points and the stations) instead of 13 000 × 2 points, and the 4096-point
  cap does not apply to a net that is not stored. Stations count against the record's item
  budget, at most 12 001 per face (`sweep::screw::MAX_STATIONS` spans).

### Tolerances and the bound

The kernel certifies the net against S, `|B − S| ≤ ε` at every parameter, in rounded interval
arithmetic (`ProceduralSweep::certify`; the derivation is in `procedural_sweep.rs`'s module
docs). The record stores ε as the kernel held it (`bound`: its certified bound plus the rounding
of any motion of the net since, which the kernel adds when it moves a wall). A reader never takes
it as proof: the kernel certifies ε again when it builds the face, as it re-certifies a blend's net
before the face is an operand, and keeps the larger of the two, so a file cannot hand the kernel a
tighter bound than it proves, and a reopened wall works to the very bound it had. A face's and its edges' tolerances are the record's, as for every kind, and are what the
kernel had.

A caller that asks for a tolerance at or above ε reads the net; a tighter one reads S
(`NetScope`, the procedural blends' contract). Intersections, booleans and tessellation run on
the net as today, so stage 1 and 2 move no result; exact evaluation is there for the operations
that ask for it and for other kernels.

## Validation

Structural, as for the rest of the record (`Brep::validate`): finite numbers within 10¹²; a
profile that is a 3D spline or rational spline with valid knots and positive weights; a unit
axis and a direction square to it within 10⁻⁹; a pitch that is finite and not zero; stations at
least two, strictly ascending; one half angle per span in (0, π/8]; a bound in [0, 10³]; law angles strictly
ascending, one offset each, at most 1024; a map with finite terms and non-zero scales. A
`sweep` face without `nurbs` is refused in stage 1 and taken in stage 2. Geometry (that the net
lies within ε of S) is the kernel's check, never the record's.

## Files and exchange

- **Designs.** The kind is new data in the model cache, so the design format bumps through the
  normal path: one file in `crates/doc/src/format/pending/`, no step (older designs have no such
  faces and are unchanged). Designs without threads or coils write the same bytes as before.
- **An older SolidCraft** opening a newer design warns about the newer format and opens it. A
  cached body with a `sweep` face does not decode (an unknown kind), which is a cache miss, so
  the design is evaluated again from its timeline: its threads are rebuilt as splines by the old
  kernel, and nothing in the design is lost. A body that has no timeline behind it (a
  native body file on its own) is refused by that body alone, with a warning; the rest opens.
- **STEP and IGES export.** STEP has no helicoid entity. A `sweep` face is written as its net,
  `B_SPLINE_SURFACE_WITH_KNOTS` (rational), with its pcurves, exactly as a thread is written
  today: the same surface as today's export, within the certified ε of the exact one. A
  net-less face's net is rebuilt for the writer. Files written stay valid AP214/AP242, and STEP import is unchanged.
- **Meshes (STL, 3MF, OBJ)** tessellate the net, as today.

## Each kernel's trip

| Record → backend → record | solidcore | loopback | another kernel (illustration) |
|---|---|---|---|
| `Sweep` | exact: the wall's definition given back, its net certified again | exact | exact where it has a helical sweep of its own: built from the definition's profile, axis, pitch and law; otherwise the net, as a `RationalSpline` (the definition lost, an `ExchangeLoss` row) |

| Backend → record | another kernel (illustration) |
|---|---|
| Its helical sweep of a curve | `Sweep`, when its adapter can state the definition and the net the record's kernel would build from it; otherwise its own spline, as today |

The second row is the adapter's job, and its pull request shows it (kernel-layer.md §8).

## Stages, each its own pull request into main

1. This note.
2. The record kind, its validation, the native file and every consumer (STEP and IGES writers,
   tessellation, `crates/geom`, the cadir writer): the net always stored, read as a `blend`
   face's is. Nothing produces it yet, so no result moves.
3. solidcore writes `sweep` for a screw-swept wall it built (the faces that carry a
   `ProceduralSweep` with a screw motion) and gives the definition back on import, with the
   design-format pending file. Results are the same numbers; the gate is that they don't move.
4. Net-less faces: the writer leaves out a net the definition rebuilds exactly, the reader
   rebuilds it. Measured: M20 full length and M30x1.5 × 100 mm, time and file size, volumes
   equal to 10⁻⁹.
5. Speed on the walls: the certified bound and the trims' bounds computed from the definition
   (one half angle per distinct span width, a straight pcurve proved straight from S) instead of
   from the net, where the profile shows the time goes.

Every stage: the full kernel rig base against branch, the naming corpus and goldens. A moved
result needs its derivation (the test-change rule), and valid input is never refused.

## Addendum, 2026-11-28: exact helix edges and straight trims

### What the record of a thread is made of

Measured on main at 2b7debcb04 with the `sweep` kind in place (the release CLI, a modelled M20 on
a D20×40 cylinder and a modelled M30x1.5 on a D30×100 one, the saved body decoded):

| M30x1.5 × 100 record | numbers |
|---|---|
| the two flank nets | about 40 000 |
| 140 cubic spline edges on the flanks | about 143 000 |
| pcurves on the flanks | about 97 000 |
| pcurves on the 203 half-turn cylinder faces | about 114 000 |

The nets are a small share, so leaving them out (stage 4 above) would save about 7% of a file.
Stage 4 is dropped. The bulk is the trims, and they are fitted where the answer is exact:

- **A flank meets the stock's cylinder along a helix.** A wall with no radial law cuts a coaxial
  cylinder of radius R where its profile point is at R from the axis, at one profile parameter
  `w₀` (or a few) and every turn. In the wall's own parameters that is a straight line,
  `w = w₀`. Today the boolean traces and fits it: in the M20, each of those edges is a cubic
  spline of 352 points with a pcurve of 352 points, whose `w` varies by 10⁻¹³ along it.
- **The edge it stands for is exact.** The net's iso curve at `w₀`, `B(w₀, t)`, lies on the net by
  construction, and at exactly the profile point's distance from the axis: the net turns every
  control point on an exact rational quadratic arc, so the net's distance from the axis is the
  profile's, whatever its lift error. So the iso curve lies on the cylinder too, to rounding. It
  is a rational quadratic per span: the profile point's path, as `sweep::screw`'s `path_curve`
  builds a profile vertex's edge today (those edges, 31 of them on the M20 flank, already have
  straight 2-point pcurves).
- **The cylinder's side is exact too.** On the cylinder the same edge is a helix, a straight
  line in the cylinder's (angle, height) only where the lift is linear in the angle. The net's
  lift is the rational quadratic of the lift (the bound's `E_l`), so on the cylinder's NURBS
  form the pcurve is the arc's own parameter map: a rational quadratic per span, exact, with no
  fitting.

### What changes

1. **The boolean takes the exact intersection.** When a face-face intersection pairs a
   screw-swept wall (live definition, no radial law over the face) with a cylinder whose axis is
   the screw's axis (within the record's tolerance, measured from the frames), the intersection
   is the set of iso curves `w = w₀` at the profile's crossings of the radius: their 3D curves
   the net's iso curves, their pcurves a straight line on the wall and the arc map on the
   cylinder. Anything else (a law that varies over the face, a tilted or offset cylinder, a
   tangent crossing where the profile touches R without crossing it) goes the general way, as
   today. This is one more exact case of the intersection, like the existing plane and cylinder
   ones: the general path stays as the fallback and nothing is refused.
2. **The record gets a helix edge kind** so those edges don't cross files and kernels as long
   splines: `Curve::ScrewPath { point, motion, stations, half_angles }`, the path of one point
   under the same screw motion and stations as the wall it lies on, with the edge's `nurbs` the
   rational quadratic the kernel builds (stored, as a sweep face's net is). The same reasoning
   as for `sweep`: an older build fails to decode the body, which is a cache miss, and the design
   is evaluated again from its timeline. It is one more file in
   `crates/doc/src/format/pending/`. STEP writes the edge's NURBS form, as it writes a thread's
   helix edges today.
3. **Stitching and checking read the exact case.** A trim that is a straight iso line on a sweep
   wall is proved on the wall by the definition (`w` fixed, so the deviation is the net's own,
   zero), not by a branch and bound over a fitted curve. That is the share of a thread's time
   that went to proving trim bounds.

### How it is measured

Against base main, for M20 full length, M30x1.5 × 100 and the 35 thread cases of the rig: time,
saved size and volume. The volumes move: the fitted edges lay within their tolerance (2·10⁻⁵ mm in
the M20) of the exact ones. Each moved rig row and golden gets that derivation, a bound on the
volume between the fitted and the exact trim: the trim's tolerance times the length of the edge
times the wall's extent across it. Every case must still pass its own expected values, and no
valid case may refuse.

### Stages

6. The exact wall-cylinder intersection in solidcore, using the existing record kinds (the
   helix edges are rational splines, as a profile vertex's are today). This is where the time and
   most of the size go: straight pcurves on the flanks and arc-map pcurves on the cylinders, in
   place of 352-point fits.
7. The `ScrewPath` edge kind in the record, written and read like `sweep`, with its pending
   file.
8. Trim proofs from the definition on sweep walls.
