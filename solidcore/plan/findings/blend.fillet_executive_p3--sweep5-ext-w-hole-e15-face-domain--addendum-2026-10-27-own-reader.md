# blend.fillet_executive_p3: file sources read by the Rust reader; three sweep5 mismatches resolved (2026-10-27)

Updates: plan/findings/blend.fillet_executive_p3--sweep5-ext-w-hole-e15-face-domain.md

Also covers `sweep5-ext_w_hole-e14-r1.0`
(`plan/findings/blend.fillet_executive_p3--sweep5-ext-w-hole-e14-near-zero-vertex.md`) and
`sweep5-letter_g-e10-r1.0`
(`plan/findings/blend.fillet_executive_p3--sweep5-mass-on-mate-held-uv-trims.md` and its
addenda).

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

**Change.** The runner's `.smb` file sources (`Source::File` in
`crates/difftest/src/units/blend_fillet_executive_p3/end_to_end.rs`) no longer import the
C++-built B-rep through the `topology.brep_p1` export. The Rust side reads the file itself
with `topology::brep::p7::read_from_file` (ASCII, no rebuild), as the shim's
`SmBrep::ReadFromFile` does. No C++-built geometry or topology reaches the Rust side for
these cases.

**Why it resolves them.** The plane-NURB addendum traced e15/e14 to the import building
planes without their NURB, so the Rust side rebuilt a NURB 1 ulp off from the one C++ made
in place (`SmBSplineSurface::GetOrCreateGwNurbPointer`). With the lazy plane/extrusion NURB
cache (PR #1300) the Rust reader's B-rep reaches the same cache state as C++, and the three
sweep5 cases agree within 1e-9 relative. Before #1300 the own reader gave 567 pass, 14 fail
and 5 known (nine of them fillet corner failures with status 1001), so this switch depends on
that cache.

**Counts** (592 cases, C++ reference v1.0.1 `3fd5b95`):

| Input | pass | fail | known | rejected |
|---|---|---|---|---|
| Before: C++ import (#1300 head `3ed14ef7a`) | 586 | 0 | 4 | 2 |
| After: Rust reader | 589 | 0 | 1 | 2 |

Remaining known mismatch: `private-upstream-rollover-404-cyl-on-box1-edge12-r2`
(mass product of inertia, 2.4e-9 relative), unchanged and described by
`plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin.md`.

The known-mismatch entries for e15, e14 and letter_g e10 are removed.
