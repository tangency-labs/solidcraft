# surface.implicit: addendum (2026-11-18): on macOS arm64 the short-swept round is still refused, margin 1.16

Updates: plan/findings/surface.implicit--short-sweep-revolution-centre.md

The finding's change (the centre projected onto the chord's perpendicular bisector, the x axis on
the polar converter's circle) passes on Linux x86-64, where CI runs. On macOS arm64 the body
from tangency-labs/solidcraft#16948 is still refused when it is read back, with the error the
change fixed (#17004, closed): "an exact net's NURBS form is not a surface of revolution's
parametrisation".

**Where.** SolidCraft's engine test
`boolean_regression_tests::a_body_with_round_pieces_swept_under_a_degree_takes_a_later_round`
(`crates/engine/src/boolean_regression_tests.rs`, the design
`examples/regressions/round_pieces_swept_under_a_degree.json`) fails at its first step: replaying
the design leaves an error on the fillet that follows the rounds. It passes on Linux x86-64.
It fails every run on macOS (three runs alone, and in full suite runs).

**Values.** Measured at main `380ecc231` with a temporary print in `derive_revolution`:

| Check | macOS 15.8.1, Apple Silicon, Rust 1.99.1 |
| --- | --- |
| recogniser (`is_nurb_surface_surf_of_revolution`) | finds a surface of revolution |
| `revolution_from_rows` | derives one |
| `evaluates_as` (`evaluation_margin`) | **1.1635912512574929** (must be at most 1), three times |

The three refusals print the same margin. That suggests one surface read back three times, not
three surfaces. On Linux the finding measured 0.59 and 0.51 for the two short nets after the change.

**What it is not.** The bound is the certified rounding of both evaluations and both parameter
conversions. It assumes `sin`, `cos`, `tan` and `atan2` within one ulp, which macOS's libm also
claims. Widening it would forgive a wrong axis or centre, which is what the comparison proves.
More likely the derived axis or centre comes out a little differently on arm64. The finding's
cause applies: the bisector itself is fixed only to the control points' rounding over the sweep
angle. That leaves a centre off across it, which turns the swept points by the sweep. The
finding's **Not changed** list (`torus_from_net`, `revolved_from_rows` taking the triangle's centre
as is) may also apply, if the refused surface is not one of the two short nets.

**One libm.** tangency-labs/solidcraft#15405 (open, waiting on a decision) routes every
transcendental function in solidcore through one pure-Rust libm, so macOS would compute Linux's
bits. Its description says it already turns one glibc-only refusal into a build. It conflicts
with main in 108 files and predates this test, so it could not be tried here. Recheck this
finding once it lands; if the failure goes, this record is closed by that change.

**To pick up.**

1. Find which surface is refused. Print the form's net from `derive_revolution` when it refuses
   (as `short_swept_revolution_nets.jsonl` holds the Linux ones) and compare with those nets.
2. Run solidcore's own tests of this change on macOS arm64: the short and long swept nets'
   tests in `crates/solidcore` (`a_revolution_net_swept_past_half_a_turn_derives_about_its_own_axis`
   and its neighbours). If they pass, the refused net is a different one and belongs in the data
   file.
3. Measure the margin's parts on arm64: the axis's tilt, the centre's offset across and along the
   bisector. Do this for the triangle chosen and for the other rows' triangles.
4. A derivation whose error does not depend on the platform's last bits: for example a centre
   fitted over several rows' triangles, or an axis from the generator's end meridians. Check it
   against the bound on both platforms.

The reproduction, faster than the engine test once the net is saved: the net file, and
`AnalyticSupport::derive_revolution` on it in a `crates/solidcore` test, as the finding's tests do.
