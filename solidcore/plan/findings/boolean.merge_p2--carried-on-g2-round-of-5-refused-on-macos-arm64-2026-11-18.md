# boolean.merge_p2: on macOS arm64 the carried-on G2 round of 5 mm is refused in `make_faceuse` (2026-11-18)

Unit: `boolean/merge/p2.rs`, `make_faceuse`: "no radial edge use classifies the face's edge use".
Related issue: tangency-labs/solidcraft#15417 (the same G2 round refused above 5.28 mm; at 8 mm
"a boolean of the build fails on its face").

**Where.** SolidCraft's kernel test
`blend::tests::concave_chord_and_g2_rounds_run_on_past_where_their_face_turns_into_a_round`
(`crates/kernel/src/blend.rs`) fails on macOS arm64 and passes on Linux x86-64, where CI runs. It
fails every run (three runs alone, and in full suite runs).

**Inputs.** `boss_on_rounded_plate()` and its boss's base edge `BOSS_EDGE`, rounded G2
(`fillet_g2`) with radius 5. The test builds it carried on, through `concave_overflow_round`. The
blend is built on the body with the flat face beside the edge carried on past where it turns into
the plate's rounded corner. Then the carried-on body is cut from the blended one. At 5 SMLib builds
the round on the face as it is, so the carried-on build must give the same volume (within 1e-6).

**Values.** Measured at main `380ecc231`, macOS 15.8.1, Apple Silicon, Rust 1.99.1:

- The chord rounds (7 carried on, then 8 and 10) pass.
- G2 5 carried on: `Err(Failed("the blend runs off the flat face beside its edge, and building it
  with that face carried on failed cutting the carried-on body from the blended one: SMLib
  boolean: operation: solidcore: SmApiBoolean: boolean::merge::p2::make_faceuse: no radial edge
  use classifies the face's edge use"))`.
- The test stops there, so G2 7, 8.5 and 8 were not reached on macOS.

**Reading.** The test's own comments say the boolean on the G2 face is near a threshold:

- At 8 mm it fails on Linux and Windows (#15417).
- At 9 mm it "goes either way" between Linux and Windows.

On arm64 it already fails at 5 mm. Probably the carried-on G2 face meets the cut's face at a
near-tangent, where the last bits of the intersection decide the radial classification of its edge
use; that is not yet confirmed. `topology.proto_topology_p3--one-proto-vertex-per-edge-face-crossing.md` records the
same error on another body (the block with a shared top ∪ prism).

**One libm.** tangency-labs/solidcraft#15405 (open, waiting on a decision) routes every
transcendental function in solidcore through one pure-Rust libm, so macOS would compute Linux's
bits. Its description says it already turns one glibc-only refusal into a build. It conflicts
with main in 108 files and predates this test, so it could not be tried here. Recheck this
finding once it lands; if the failure goes, this record is closed by that change.

**To pick up.**

1. Dump the cut's two operands on macOS. Nothing in SolidCraft sets `smlib_dump_dir`; a temporary
   environment fallback in `crates/smlib/src/core/dump.rs`'s `dump::failed` does it. Replay
   `a.boolean(&b, BooleanOp::Subtract)` in a small `crates/smlib` example (`Brep::read(path,
   false)`), which rebuilds in about 2 minutes.
2. Replay the same `.smb` files on Linux x86-64 (`ssh reddress`). If they pass there, the
   difference is in the boolean, not the blend's build. Otherwise compare the blend's output on both.
3. In `make_faceuse`, find the edge use no radial edge use classifies, and how far its
   classifying test is from deciding. Then fix with an exact or certified decision, not a
   tolerance. A fix should also move #15417's 8 mm case, and the test's comments should be updated.
