# offset.offset_executive_p2: second-shell bridges end at the old wall heights (2026-11-06)

Units: `offset.offset_executive_p2`, `offset.offset_geometry_creation`.
Issue: #2954. Diagnosis: #2999. Base: `b7002862b6c4aa40d1c23d6209e136e32b27e42b`.

The kernel case
`shell/cov-shell-again-open-at-an-inner-wall-of-a-shelled-box`
shells the 2 mm walls of a 70 × 50 × 30 rounded box (outer R6, inner R4)
by 0.5 mm inward, opening an inner wall. Its committed operand `b12` has
19 faces, 48 edges, 32 vertices and two regions. All surfaces are analytic:
11 planes and eight cylinders. Record face 14 is native face 14, the intended
inner wall y = −23, x = −29..29, z = 2..30. The measured input volume is
19449.486556869946 mm³.

The existing expectation is one closed solid with the original bounds and material
volume 9017.6..9038.3 mm³. The filtered canonical rig reproduces the listed failure;
it does not show a successful shell. A compiled direct kernel regression returns
`Upstream { operation: "offset::offset_executive::p2::shell_brep", code: 1001 }`.
The operation name alone does not distinguish the offset and final-result guards.

A private test invokes the existing `run_shell_offset` with those same options,
before the shell's guard and subtraction. It saves an intermediate with 23 faces,
66 edges, 44 vertices and one region, `is_manifold_solid == false`, volume 0.
The zero is a sheet's measured result, not a valid physical cavity volume.
The input was exported to ASCII SMB for this private probe; it was not synthesized.

Independent public-API readback localizes 16 nonmanifold edges to the ends of the
opened wall: two edges have three adjacent faces; fourteen have only one.

| Measured construction | z interval / height | Evidence |
| --- | --- | --- |
| Original vertical ends of the opened face | 2..30 | Captured input and selected-face bounds |
| Offset floor and rim | 1.5 and 29.5 | Intermediate faces 11 and 5 |
| Main bridging faces | 2..29.5 | Intermediate faces 20 and 22 |
| Upper bridge flaps | 29.5..30 | Intermediate faces 19 and 21 |
| Lower unclosed ends | 1.5..2 | One-face edges 47, 50, 51, 54 |
| Triple-face rim joins | 29.5 | Edges 29 and 31 |

The affected positions are x ≈ ±29 and y = −23..−23.5. The full edge coordinates
and face bounds, native intermediate SMB, exact diagnostic test and source/hash
receipt are published together in the `review-evidence/2999-double-shell`
directory on the `evidence/2999-double-shell` branch of tdamsma/solidcore.
The native renderer refuses to convert this intermediate to a record because it
is a sheet touching itself along an edge; it renders the original input normally.

The defect precedes the final shell subtraction. A second private trace captures the
actual two bridge rail pairs before and after the rail/centre-line manicuring in
`edgeuse_offset`. For both ends, both rails span z = 1.9999999999999996..30
before and after; the centre line is trimmed to those same heights. The resulting
intermediate SMB is byte-identical to the first capture (SHA-256
`f4d0e307aeb1f67347d44d25b456c09e35ac33c34df4bbf57b368f122761241d`).
The observed wrong endpoints are present at bridge creation, not introduced by the
final subtraction or by the diagnostic readback.

`do_extended_offset` constructs tangent pairs first, through `mapped_edge_prefix`,
then marks them so the later general mapped-edge pass skips them. These particular
pairs enter `shell_tangent_gap` and make a planar bridge from the old rails.
Their neighbours' non-tangent floor/rim mitres are processed later. The bridge
therefore retains the old rail interval while the adjacent offset boundaries end
at z = 1.5 and 29.5. The missing dependency is the bridge ends on those neighbouring
mitres. A repair must resolve that dependency for the shared construction; a later
bridge phase is a candidate, not an implemented or verified fix.

Additional public-API controls on the same saved operand:
- Opening the reported inner face at distances −0.25, −0.5 and −0.75 returns
  the same shell_brep 1001 status.
- Opening any of the four inner flat walls (record faces 12, 14, 16, 18) at −0.5
  is refused.
- Opening any of the four outer flat walls, the bottom, the rim or the inner floor
  returns a manifold solid. Their measured volumes are retained in the evidence,
  but are not independently graded here; successful status is not a correctness
  claim for those seven controls.

Initial pointer-integrity errors refer to cached topology. After
`refresh_cached_topology`, both readback bodies pass `integrity::check`; the
intermediate remains nonmanifold. A stale cache is not established as the cause.

No production repair, successful second shell, tolerance change, golden change or
known-failure reclassification is claimed. Both private cfg(test) probes were removed. p2.rs was verified against its original SHA-256
`de511e6119b4d576431a8688e7884c00f097538d8d678b6e66c2f527844e7826`.
The regression remains explicitly ignored while the draft diagnoses the failure.

The offset follows the signed-normal definition and analytic-radius relation in
Patrikalakis, Maekawa and Cho, *Shape Interrogation*, §11.3.1:
https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node224.html.
The shared bridge construction and the S2 mitres in `docs/offset-and-shell.md`
are the relevant design boundary; coordinate there before changing the construction.

The geometry creator was also restored byte for byte: SHA-256
`3c584bc98713f701845e5e24ffd131773a39313efbe58f940e97c34dd6ed2614`.
