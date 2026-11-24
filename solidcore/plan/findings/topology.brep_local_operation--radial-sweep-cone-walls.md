# Radial removal for a complete conical draft

Addendum to `topology.brep_local_operation--18805-radial-sweep-step.md`, which
scoped the radial removal to a complete cylindrical wall and listed "other
supports" as left to the local operation.

## The case

A frustum whose side already leans (a cone, semi-angle atan 0.3 = 16.70 degrees,
r 100 at z = 0 to r 40 at z = 200) with six blind holes r 5 from the top, their
centres on r 32, drafted about its base to lean 18 degrees:
r(z) = 100 - z tan 18 degrees, r 35.02 at the top. Each hole reaches r 37, which
the new side passes at z = 193.9, so every hole breaks out through the side.

On main the draft went through the local operation and was refused
`LoopsCross { crossing: Unmerged }` on the top face: its new outer loop (r 35.02)
crosses each hole's rim twice. At both crossings the cone and the hole's wall
leave the top face on the same side (downward) and pass through each other
along a curve between the crossings, which `walls_part_at_crossings`
(`offset_executive/crossing_loops.rs`) rightly does not accept: the local
operation's stitch only splits existing edges and cannot make that curve.

## The construction

The radial removal needs nothing that only a cylinder has. For a coaxial old
cone r_old(z) = b_old + z tan a_old and new cone r_new(z) = b_new + z tan a_new
over the wall's axial slab, the removed set is

    W = { (rho, theta, z): z0 <= z <= z1, r_new(z) <= rho <= r_old(z) }

and r_new - r_old is affine in z, so its sign at the two ends bounds it over
the slab. `contract_complete_cone` (renamed from `contract_complete_cylinder`)
no longer requires a zero semi-angle; the old cone's own radius takes the place
of the cylinder's constant one. The other selectors (one face, one material
region, bare supports, the natural UV box, the four box sides as the whole
boundary, bare planes across both rims, the `net_sense` proof of the radial
side) are unchanged. The boolean B - W cuts the hole walls along their
intersection with the new cone and keeps the material side of both walls.

## One axis and one slab, to the body's tolerance

The cylinder step compared the two supports bit for bit: the same frame, the
same stored axial interval, and the radii's signs in expansion arithmetic.
Neither comparison holds for the acid-7 body in SolidCraft.

- After SolidCraft's six hole cuts the side's cone is recognised again with its
  frame at the top (origin z = 200, z axis -Z, base radius 40, semi-angle
  +16.70) and its origin and axes off by rounding (x, y of the origin about
  5e-14 and 1e-14, axes off Z by 3e-16). `drafted_on_source_axis` puts the
  drafted cone's origin on the hinge circle's centre, z = 0, so the frames
  differ by the axis's length and by rounding (dpos y -7.6e-14 against
  -9.8e-15), and the drafted cone's interval is -200 to 2.8e-45 where the
  old one is 0 to 200.00000000000003.
- Even in the same frame, `on_axis_from` recovers the drafted interval from the
  net's evaluated corners, a rational evaluation that rounds: at r 100 and
  height 200 the drafted top height is 200.00000000000003 against 200.
- With axes off Z by rounding, a radial move perpendicular to the pull changes
  a point's height on the frame's own axis by about 1e-15, so the drafted net's
  end rows are not exactly in the old end planes either.

`coaxial_slab` compares them as one axis and one slab to the body's tolerance
instead: the new axis's offset from the old one plus its tilt times the slab's
extent, the x axes' twist (the seams) times the largest radius, and each end row
of the drafted net against the old row's plane, each at most the tolerance; the
old end planes taken onto the new axis (`heights`); both radii above the
tolerance at both ends and the new one nowhere past the old by more than it;
an end closed by an annular cap where the new radius is below the old by more
than the tolerance, at the hinge otherwise. `radial_sweep` draws both circles
of a cap in the old support's frame on its end plane (the inner one of the new
cone's radius at that plane), so `fixed_rim_plane` still proves the cap on the
end plane exactly, and the stitch joins the drafted face's own rim to it within
the body's tolerance, which is the tolerance the stitch already runs at. This
replaces the cylinder step's exact frame and interval equality and the
expansion-arithmetic signs of the radii.

## SolidCraft's draft used its own drafted cone

SolidCraft's `crates/smlib/src/core/offset.rs` built the drafted cone with
its own copy of `drafted_about_plane` (`drafted_cone`, the capi's), without a
draft definition, so `contract_complete_cone` never selected a SolidCraft
draft (it reads the definition for the source and the sense). That copy is
gone: a cone or cylinder face is drafted by solidcore's `draft_surface`, the
one implementation, which carries the definition. Planes are still turned by
SolidCraft's own code.

## Checks

`a_cones_side_drafted_through_blind_holes_near_its_top_breaks_them_out`
(`solidcore-ops/src/offset/tests.rs`): the case above. The expected volume is
pi int r^2 dz less 6 int lens(r(z), 5, 32) dz over the holes, the lens the
closed-form overlap of the two discs (25 pi below z0); above z0 by
Gauss-Legendre after z = z0 + s^2, 20, 40 and 80 nodes agreeing:
3061182.2920648409. At 1/10 of that size the drafted interval does not round and
the exact interval comparison passed; the test is at the full size.

## A body with a closed cavity is left to the local operation

With the cone admitted, `a_shelled_cones_side_drafted_into_its_cavity_is_refused_naming_the_largest_angle`
(a cone r 2 to r 1 over 3, shelled 0.5 inward to a closed cavity, its side
drafted 3 degrees) was selected too, and B - W was refused by the mass
enclosure check ("a face whose sides are not one material region and one
void"), where the local operation builds the body the test derives. A shelled
cylinder would be selected the same way before this change. The selector now
also requires no void region but the outside one (the body's regions are the
outside and its one material region), so such a body keeps the local
operation's result. Why the Boolean's result for a body with a cavity fails
the enclosure check is not investigated here.
