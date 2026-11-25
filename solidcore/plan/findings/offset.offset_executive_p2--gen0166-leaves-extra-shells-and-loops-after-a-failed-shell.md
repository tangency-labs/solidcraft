# offset.offset_executive_p2: gen0166 leaves extra shells and loops after a failed shell

Unit `offset.offset_executive_p2`, case `gen0166` (`static_shell` of a 20x30x40 box at
(-8.005, -18.639, -17.799), face 2 selected twice, distance -0.5, tolerance 1e-6).
Deferred at `SmBrep::MakeEdgeInFace` before; now both sides return status 1000 with
12 faces, 24 edges and 16 vertices, but the C++ result has 3 regions / 3 shells /
13 loops and the port 3 / 7 / 16, and the selected faces after the call are (0, 2) in
C++ and (4, none) in the port. No edge use binormal is evaluated in this case.
Suspected to be the same Rust merge region/shell handling as `gen0000` / `gen0003`
above (the inset shell's Difference); not traced further. Listed in
`known_mismatches.toml`.

### Actual CreateFilletBrep: production constructor matches native on real box

A new same-box checkpoint calls Session::create_fillet_brep directly against native SmFilletExecutive::CreateFilletBrep after shared parameter/corner setup. It returns before the harness's manual component sequence. The actual constructor result has matching directed intermediate B-rep topology, Line/Circle/Cylinder types and geometry samples, actual source-FG-to-managed-Face alias/owner, and retained executive E attributes. New comparison171 exact fields/124 values matches exactly. No internal substage success flags, before-state vectors or Stitch counters are fabricated.

Whole executive_p3 now307 cases/260 value passes/47 existing deferrals/0 failures; same box15 exact passes plus1 honest full SmApiFilletEdges deferral. Every30 prior CPP/Rust output remains byte-identical across actual constructor wiring/latest-main integration; perturb catches15/15 implemented cases. This is actual CreateFilletBrep in unchanged direct context, not DoFilleting, target integration, mass or complete geometry notification/cache parity.

### Actual SetFilletBrepEdgeMap: ordered managed mappings without interval repair

The same ordinary box calls actual CreateFilletBrep, shared first-FG owner-face shrink, then Session::set_fillet_brep_edge_map against native. All four owning pseudo edges begin unmapped and map in order to managed-edge labels [0,2,3,1], each with one actual entry. Source and observations preserve raw rail intervals [+1e20,-1e20]: native evaluates the pseudo CURVE natural-interval midpoint and managed EDGE raw-interval midpoint. No interval fallback or repair is introduced.

New checkpoint255 exact fields/140 values agrees exactly, including before/after type/status/raw intervals, first/ordered mappings, actual error code/target handles/comment bytes, whole body geometry, surface alias and E attributes. Unknown managed-map identities are harness errors rather than shared foreign sentinels. Whole executive_p3=308cases261valuepasses47existingdeferrals0fail; all32 prior CPP/Rust outputs byte-identical; perturb16/16 caught. Direct context flags remain unchanged. Target insertion/merge, full DoFilleting/public API and lower geometry notification/cache parity remain unfinished.
