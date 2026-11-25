Updates: WR-616

#### WR-616 (update, 2026-11-02). The setback corner's fill reaches the executive, certified convex and grown to fit (`fillet_blend_corner`)

The SolidCraft fork's tangency-labs/smlib#104 (merged into `replay/1.0.1` at `b5bb747`; SMLib
commits `61547af`, `d1de555`, `1506e2f`, `ac8b4f8`, `590a7f3`, `08d4b02`, the SMLib part of
`250cb3f`, and `f1d81f4`) routes a setback corner through `SmBlendSetback::FillHole`, the fill
ported in the WR-616 addendum of 2026-11-02 (`setback-split-fill`). That addendum left the flag
"to whichever change first routes a corner through `fill_hole`". This is that change. It runs
under WR-697's existing opt-in `Kernel::fixes.fillet_blend_corner`, with no new flag and no
new record id.

- **The Setback corner** (`61547af`): a vertex of three or more blends of one convexity, on
  planes meeting at a point, with straight edges, that no Ball takes is the blend corner's
  Setback (`SmBlendCorner::ClassifySetBack`). That covers rounds of unequal radii, rounds of
  curvature sections, and a vertex the caller sets back with `SetSetBackFill`. Each blend ends
  at its setback along its edge, and the hole is filled by the setback split, one patch per
  round plus the n-sided interior (`MakeSetBackTopology`, `BuildSetBackFill`,
  `CreateSetBackPatches`).
- **The rounds' geometry** (`d1de555`): the fill takes each round's jets over its rails' mean
  weight and by their mean arc length, so two parametrizations of one round give one fill.
- **Room** (`1506e2f`, `ac8b4f8`): a default setback that is too long for its edge shrinks to
  the edge's room less a quarter of its radius. A setback asked for that does not fit is
  refused as no room, naming the setback that fits.
- **G1 as a graph** (`590a7f3`, `08d4b02`): a G1 fill is a graph over the corner's axis. Its
  spring curves are the parabolas tangent to the rails at their ends, and its heights are held
  convex by linear sufficient conditions on their Bernstein coefficients (a dual quadratic
  programme). It is taken only once certified convex.
- **Certified, grown to fit** (`f1d81f4`): the rule's setbacks grow to the least scale whose G1
  fill is certified convex (`SmBlendSetback::CertifiedScale`, to 1e-2, each setback held to its
  room). A setback asked for that is too short for a certified fill is refused as no room,
  naming the setback that builds (`GetFitSetBack`).

The fork's capi-only parts (`SMLIB_FIT_SETBACK_AT_LEAST`, `b5bb747`, `14d5807`) are not ported.

**Rust port:** `crates/solidcore/src/blend/blend_corner/setback.rs` holds the Setback's
classification, topology, fill and patches. `crates/solidcore/src/blend/blend_setback/` holds
`graph.rs` (the graph fill), `certify.rs` (Bernstein certification), `qp.rs` (the quadratic
programme) and `scale.rs` (`certified_scale`). The session gains `set_set_back_fill` and
`fit_setback`. `Round`'s surface is now a `Surface`: the fork's `const SmBSplineSurface *`
dispatches virtually, so an exact round's cylinder evaluates as itself. `fill_hole` and
`certified_scale` take the kernel (the fork's context).

**Default unchanged:** without `fillet_blend_corner`, every corner is the pinned one (the
flag-off cases of the tests below).

**Tests:** `crates/solidcore/tests/fork_smlib104_setback_corner.rs`, from the fork's capi
setback cases, run through the executive with the flag on and off: three unequal rounds at a
box corner, a setback asked for (8, and 8 eased over 2), asked setbacks that do not fit (20),
stop short (0) or cross behind their ends (5), one too short (4.25) naming 4.7515 and building
at it, a default that shrinks to fit, and a concave corner. The old G1 cases of
`fork_870c970_blend_setback.rs` grow their setbacks first, as the fork's capi now does.

**Confirmed:** yes, against the fork's own C++ build (`b5bb747`, the harness's flags). A
throwaway driver ran `SmFilletExecutive` on the twelve cases above, built as the tests build
them. Statuses, comments and `GetFitSetBack` agree on every case, apart from the documented
code numbering: the fork's `SM_FILERR_CORNER_UNSUPPORTED` is 10, this port's 9. Where a body
is built, its face, edge and vertex counts agree, the volume agrees to the 15 digits the fork
prints, and every vertex agrees bit for bit (175 vertices). The fill alone (`FillHole`,
`CertifiedScale`) was compared on four box corners: statuses, scales, solve counts, degrees,
refusal reasons and containment agree, and all 2700 sampled patch points are bit-equal.
