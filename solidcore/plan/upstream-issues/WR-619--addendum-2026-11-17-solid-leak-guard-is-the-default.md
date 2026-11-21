Updates: WR-619

#### WR-619 (update, 2026-11-17). The union's solid-leak guard is the default; the flag is retired

`Kernel::fixes.boolean_union_refuses_solid_leak` is removed (tangency-labs/solidcraft#14246): the guard in
`boolean/merge/p4.rs` (`editing_stages`) always runs on a union. Since tangency-labs/solidcraft#14230 it
refuses with `BooleanFailure::SolidLeaks { face }` only when the union, after its face deletion and
post-processing, has no solid region left (`has_solid_region`). A face between a solid and the outside is also
deleted where the other body's solid lies just across it (a coincident wall); that union keeps its solid and is
returned, so a sound body never becomes a refusal.

Effect on the default kernel: no goldens row moves. A sweep of the plain strip unions (slopes 0.02 to 0.5, d from
5e-6 to 1e-4) finds no leak: each union keeps one solid region or is refused earlier. The inputs that still leak
are tangency-labs/solidcraft#16350's pocketed strips at d = 5e-6 (a hole or a notch in A's face beside the
crossing): their union deleted faces until no solid region was left (volume 0), and is now refused with
`SolidLeaks`. A union of two non-empty solids contains each of them, so a result with no solid region is wrong
whatever its faces; the refusal replaces a wrong body, never a sound one. #16350 itself (building the right union
there) stays open.

`crates/solidcore/tests/wr_619_union_solid_leak.rs` asserts the #16350 refusal (fails with the guard off), the 1.5e-5
strip union as one valid solid and an ordinary union of two overlapping boxes (11 faces, 24 edges, one solid region,
manifold).
