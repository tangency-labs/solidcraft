# blend.fillet_solver_p2: a revolved chamfer enters a spherical exact builder (2026-11-06)

Units: `blend.fillet_solver_p2`, `blend.fillet_solver_p4`, `blend::fillet_exact`.
Issue: #2905 (revolved vase only); PR: #2932.

## Reproduction and root cause

The captured operand in `crates/solidcore/tests/data/issue_2905_revolved_vase.smb`
is the application's pre-chamfer body from `04-revolve-vase`: 5 faces, 7 edges,
4 vertices. The CLI capture used application `74e0596` with an unknown kernel
revision; Core985 and main below are replay targets, not that producer.
On pristine main `513e6fcfc`, the operand is manifold, passes Level2,
and has its independent meridian volume. The z=60 circular edge's tangent chain
has one closed edge. Equal chamfer setbacks 2.995 prepare offsets
6.483916671439901, but execution returns status 1001 and "Fillet Surface
Computation Failure". The reported Core985 application wrapper independently
returns status 1001 for the same operand.
After the goldens migration, pristine main `3ed619475` still passes the operand
test and fails the chamfer test with status 1001; the production dispatch fix is
still needed on that baseline.

Temporary native tracing, removed before implementation, identifies
`BlendSpine::Point`, supports `[None,Cone]`, followed by
`ball_triangle_fillet_geom_parts`'s explicit circular-generator rejection. The
Point dispatch did not check the section type before choosing a spherical
builder. A straight section is the chord between contacts, not that ball's
spherical patch. Restrict the Point spherical builders to circular sections;
other generators use the existing trace/construction and its certificate.
No error is swallowed and no tolerance or certificate is weakened.

## Independent geometry

In the (radius,z) meridian the curved support is the circle centred at (-16,33),
radius sqrt(1690), between z = 30 and 60; the upper line is
r=15+0.15*(z-60). Their unit directions into the supports enclose
130.41442061908944 degrees. For equal setbacks d=2.995 the offsets are
R=d*(1-cos(alpha))/sin(alpha)=6.483916671439901.
Solving the meridian offset circle and line, selecting the root in both faces,
gives centre (21.837023752770484,61.87045851362839) and contacts
(16.68228678705657,57.937283941291334) and
(15.424842847414759,62.83228564943175).
The joining segment revolves into a cone. Integrate
pi*(-16+sqrt(1690-(z-33)^2))^2 and the conical frusta to obtain input volume
112499.47865571645 and chamfered volume 112773.94647120051. The concave waist
adds material. The test computes these from the meridian, independently of the
kernel result; the reported/cached old volume 112773.944 is not an expected value.

Research: Shene's [Surfaces of Revolution, cone example](https://pages.mtu.edu/~shene/COURSES/cs3621/LAB/surface/rev-surf.html)
constructs a cone by revolving a meridian line segment. This agrees with
`docs/generic-blend.md`'s chord section. The existing exact revolution builder
could be a future route only with proof of both supports' shared symmetry;
a circular input edge alone is insufficient. Sampling recognisers alone do not
supply that proof.

## Validation and limits

The dedicated native tests on pristine main: one passes (operand), one fails
(actual chamfer). With the Point dispatch guard, both pass in the pre-migration
optimized difftest profile: the chamfer completes, is manifold, passes Level2,
and its volume agrees with the independent meridian within 1e-9 relative.
A separate native run gives 6 faces, 9 edges, 5 vertices and volume
112773.94647118454 (about 1.4e-13 relative from the independent value).
The merged `3ed619475` baseline with the fix gives the byte-identical saved body.
The strengthened regression checks the exact definition on a 5-by-5 grid against
the meridian cone and checks both contact heights, as well as validity and volume.
The separate geometry probe's maximum relative radius error is 8.52e-16, with
contact heights within 3e-14. Both final regression tests pass on the merged
kernel; the same source still fails at the chamfer on pristine `3ed619475`.
Generic chord certification remains expensive (the two tests took 83.08 seconds
on this machine). A stack sample locates that work in `gn_certify_span` through
`GnSectionFsg::build_chord`; this is not a performance improvement claim.
The unoptimized diagnostic had exceeded 180 seconds. The existing profiler records **987 calls to `gn_certify_span` and 75,307 calls
to `gn_enclose_over`** during construction. These are invocation counts, not the
number of final net spans. The profiled kernel serializes the same body byte for
byte. This identifies certificate work for a later performance investigation;
no optimization is introduced here.

On main `3ed619475` with this fix, the full kernel suite passes 5,086 tests,
0 fail, 44 are ignored. Formatting, strict workspace all-target Clippy, wasm
and the affected goldens in both normal and reverse order pass. The full golden
sweep is 282/283: its sole difference, `curve.conic` / `gen2235` / `values.r[4]`,
is reproduced unchanged on pristine main (3.776586443654626e-14 expected,
3.552713678800501e-14 actual). It is an intersection distance with requested
tolerance 1e-9, tracked in the discussion on #487. No golden or floor changes.
After merging main `1483513fe` (the curve-ownership fix), the two changed
ownership test suites and this regression pass (14 tests). Updated Clippy,
wasm and all five affected golden units pass; the full sweep is still 282/283
with exactly the same pre-existing conic difference. Validation details are
in the PR. This fixes only the Vase action, not all eleven #2905 actions.
