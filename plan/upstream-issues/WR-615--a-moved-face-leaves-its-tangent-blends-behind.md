#### WR-615. A face moved along its normal leaves its tangent blends behind (a step on the round)
- **Severity:** Wrong result (a face move keeps a tangent blend at its old place, so the blend no longer meets the moved face tangentially and a step is built on it)
- **Area:** Offsets / local operation
- **Location:** `source/SMLib/src/SmBrep.cpp`, `SmBrep::LocalOperation` (with `SmOffsetGeometryCreation::FaceuseOffset` for the new surfaces): the faces around the moved ones are extended or trimmed to meet them, a blend face among them too.
- **What happens:** moving a boss wall off its foot round (`smlib_offset_faces`, the wall out by 1) keeps the round at the old radius and builds a step on it. The result is 27725.72 with 10 faces, the round still ending at r 10 under a wall at r 11.
- **Trigger:** tangency-labs/solidcraft#3468 (rig case shell/3468): a 60 x 40 x 10 plate with a boss r 10, 10 high, at (30, 20), an r 2 round at the boss's foot, the boss wall offset +1.
- **How found:** SolidCraft (the fork, tangency-labs/smlib a96ce9f), #1060 step 4.
- **Confirmed:** no. The pinned C++ has no face move whose blends follow; the fork's commit is the evidence.
- **Fix (fork):** tangency-labs/smlib a96ce9f (with 607d617 and 758829c) adds `SmOffsetFacesExecutive`, which the fork's capi `smlib_offset_faces` runs. Its rule, an owner decision: a blend tangent to a moved face follows it and stays tangent at its own size, so the body is blend(move(unblend(body))).
  1. It collects the blends beside a moved face across a tangent manifold edge that `SmFace::IdentifyFillet` recognises with that edge as a rail. Their kind is recovered exactly: a circular section at constant radius or at constant chord. Anything else is refused (`SM_OFR_UNSUPPORTED`).
  2. It keeps what each blend stands for (its attributes) and marks the faces around it (`SM_AI_FOLLOWED_BLEND`).
  3. It takes each blend off (`SmRingFilletRemoval`, or `SmFilletRemoval` exact only). Mitred groups come off together through `SmFeatureExecutive` where every face round them is a plane.
  4. It moves the faces (`FaceuseOffset`, `SmBrep::LocalOperation`).
  5. It blends the sharp edges again in one `SmFilletExecutive` call (`SM_OFR_NO_ROOM` where a blend no longer fits).
  6. It gives the new blend faces and their edges the kept attributes.
  For 3468: 24000 + pi 11^2 10 + 2 pi (R + d) A, R = 11, A = r^2 (1 - pi/4), d = r^3 (5/6 - pi/4) / A (Pappus), 27863.06545, 9 faces.
- **Rust port:** a fork-only API, not a `Kernel::fixes` field: `offset::offset_faces::OffsetFacesExecutive` (`crates/solidcore/src/offset/offset_faces.rs`); the pinned face move is unchanged. Part 1 (this record's first change) ports the marks (`FollowedBlendMark`, attribute id `ids::FOLLOWED_BLEND`), the collection of the followed blends (step 1), the kept attributes (step 2) and their restoring (step 6), with unit tests: the boss's foot round found as a constant-radius round r 2 following the wall, a block's top front round following its top with its attributes kept and restored. Steps 3 to 5 and the entry point follow; the mitred-group removal (`SmFeatureExecutive`, not ported) is refused there as `SM_OFR_UNSUPPORTED`.
