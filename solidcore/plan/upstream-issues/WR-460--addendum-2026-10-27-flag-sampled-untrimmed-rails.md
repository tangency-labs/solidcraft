Updates: WR-460

#### WR-460 (update, 2026-10-27). The opt-in refusal sampled rails that are not trimmed yet

No new C++ finding: upstream still has no rail comparison in `SmFilletExecutive::DoFilleting` (`SmFilletExecutive.cpp:2957`, v1.0.1 3fd5b95). This changes only the Rust opt-in `fixes.fillet_refuse_crossing_rails`.

The refusal runs right after `CreateFilletBrep`, where the rail edges of the fillet body still have the inside-out edge interval of `SmExtent1d()` (`[1e20, -1e20]`). It sampled that interval, so every rail point but the middle one lay ~1e21 away (a rail line at `z = -15 + 60 t`, `t` ~ 1e20). On a planar target edge the closest-point query still answered and the distances were simply large; on a curved target edge (a round or arc from an earlier blend) the query found no solution and the fillet failed with status 1001 (`query::api_queries`), though no rails cross. In SolidCraft's oracle (83 designs) designs 28, 30, 33, 34 and 36 went from pass to error with the flag on.

With the flag, a still-inside-out rail interval now falls back to the rail curve's natural interval, which is the stretch alongside the edge. Rails with a real interval are sampled as before. Tests `wr460_*` (`crates/solidcore/tests/wr460_untrimmed_rails.rs`): a 50 x 40 x 30 box with R8 verticals and an R4 top loop (oracle design 36), and a 40 x 30 x 20 box with 5 mm vertical chamfers and an R2 top loop (design 34), both build a closed manifold solid with the flag, bit-identical to the unflagged volume; the first also matches a Pappus estimate to 1e-5. The five `issue_1073_*` tests are unchanged. Default path untouched.
