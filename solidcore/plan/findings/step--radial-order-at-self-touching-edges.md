# Radial order at an imported self-touching edge

The import assembly collected an edge's uses while visiting faces. That is a file
order, not a geometric order. The record-to-kernel conversion preserves the
record's radial cycle, so a four-use edge could reach export with two consecutive
uses entering material. Export then correctly refused that cycle.

The synthetic regression is in `solidcore-step/src/assembly_tests.rs`,
`a_partial_edge_contact_is_ordered_by_geometry_instead_of_face_order`. Its solid
is the union of three axis-aligned boxes:

- `[0,1] × [-1,0] × [0,2]`;
- `[-1,0] × [0,1] × [0,2]`;
- `[-1,0] × [-1,0] × [1,2]`.

Their interiors are disjoint, so the volume is `2 + 2 + 1 = 5`. The boundary has
12 planar faces, 26 edges used twice and one edge used four times. The four-use
edge is `(0,0,0)` to `(0,0,1)`. Two of its faces continue to `(0,0,2)` and share
the separate two-use edge above the contact. This distinguishes a partial
self-contact from two boxes touching along their entire edge.

The first four faces leave the contact along `+x, -y, +y, -x`, respectively.
Counter-clockwise about the positive curve tangent `+z`, their cycle is therefore
`[0,2,3,1]`. The material occupies the southeast and northwest quadrants. The
old cycle `[0,1,2,3]` has entry flags `[false,true,true,false]`; the geometric
cycle has `[false,true,false,true]`. Of all 24 permutations of these faces, only
8 happen to alternate without sorting.

The reported BarrelJack planes give these same rays: `#1203` has outward
normal `+y`, `#1408` has `-x`, `#1925` has `+x` and `#1982` has `-y`.
They correspond to fixture faces `[0,1,2,3]`. The fixture's contact becomes
the reported `(1.5,-0.25,3.5)` to `(1.5,-0.25,5.0)` under
`(x,y,z) → (x+1.5,y-0.25,3.5+1.5z)`, which preserves this radial cycle.
The report's `#1203` and `#1925` continue above the contact and share
`#1224`, just as fixture faces 0 and 2 do.

The construction follows the radial-edge representation described in Weiler,
*The radial edge structure: a topological representation for non-manifold
geometric boundary modeling* (1988), referenced by
`docs/validity-and-tolerances.md`, sections 3 and 12. For a regular edge tangent
`t` and natural surface derivatives `S_u, S_v`, the direction into a face is
`r = (S_u × S_v) × t`, negated when its loop runs backwards. Its pcurve selects
the surface point on this edge, independently of how far the face extends.
For a valid edge with regular, distinct inward rays, continuity keeps the cyclic
order constant along its interior: a change would require coincident rays or a
singularity. This local ordering query is not a global validity certificate.

With reference ray `b`, the polar coordinates are `x = b · r` and
`y = t · (b × r)`. Writing the true angle as θ gives
`x = |b||r| cos θ` and `y = |t||b||r| sin θ`. Omitting normalization applies
one positive scale to every y coordinate and preserves cyclic order. The kernel
sorts these angles and returns a permutation. The assembly applies it to edges
used more than twice, preserving side flags and two-use edges.

No tolerance or stepped probe chooses a direction. A singular direction,
nonfinite arithmetic, or coincident angular directions gives a typed error;
ordering tangent faces would require higher-order contact information.

A STEP written from these same polygons reproduces the refusal on the installed
SolidCraft 0.44.1 CLI. Surface recovery returns a closed body of volume 5, but its
timeline warning says that the solid's faces do not alternate. A check requiring
a clean solid import fails on that warning. The issue's KiCad files could not be
downloaded in the lane because DNS resolution for GitLab failed.

Changing only the synthetic STEP's shell face list from `[0,1,2,3,...]` to
`[0,2,3,1,...]`, with every point, curve, surface and loop unchanged, makes the
same installed CLI import one closed 12-face body of volume 5 without a warning.
This isolates the defect to the file's face order becoming its radial order.
Running all 24 contact-face permutations in that CLI confirms 16 refusals with
surface recovery and 8 clean imports. All 24 have one closed 12-face body of
volume 5 after recovery.
