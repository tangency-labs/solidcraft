Updates: WR-75

#### WR-75 (update, 2026-11-19). The tangent curve tracer's probe and nodes: coincident regions, poles and contacts along one curve (#18930)

Not the fork's; changes to the opt-in `Kernel::fixes.ssi_tangent_curve_tracer` itself. Three of
the fork's checks in the tangential curve pass are replaced, each for a case where the pass, with
the flag on, returned a wrong result. With the flag off nothing changes.

- **Coincident surfaces at a domain corner** (`TangentTracer::probe`). The probe circles a
  contact at `r = 2e-3` model sizes in 24 directions and solves, for each, the contact on the
  plane through the circle point normal to that direction. The fork called the surfaces
  coincident when 5 contacts lay within `0.01 r` of their circle point, or when more than 12
  directions found one. The circle point lies in the tangent plane, `r^2 kappa / 2` off a
  surface of curvature `kappa`, so the first check misses wherever `kappa > 0.02 / r`; the
  second misses on a domain edge or corner, where at most half of the circle lies in the
  domains. On upstream's `ssi_rev_rev_coin` and `ssi_coincident_torii` the seeds are the ends
  of the marcher's boundary curves, on domain edges and corners, and the pass traced one and
  two tangent curves across the coincident region. Now the surfaces coincide where three
  neighbouring directions each find the contact along themselves, to within half the angle
  between two directions in the tangent plane. A tangential curve gives such a contact in at
  most one direction on each side of the point; a coincident region gives it in every
  direction that stays in the domains, so in three neighbours at a corner of 45 degrees or
  more.
- **A pole** (`TangentTracer::probe`, `regular_at`). At a degenerate point of a parametrization,
  a sphere's pole, a tangent-plane direction has no determined drop into the parameters: the
  tracer's evaluation puts a tangent in for the partial that vanishes, and every direction is
  dropped along the meridian. On `ssi_coincident_spheres2` the probe of a pole of one sphere
  found the contact in the two directions of the meridian only, and the pass traced the
  coincident region's boundary there as a tangent curve, which replaced the marcher's
  coincident curve and stopped 0.17 short of its end. Now the probe judges no direction at a
  point where either surface's partials, as evaluated, do not span the tangent plane.
- **A contact on one curve is not a node** (`find_tangent_curves`). A contact point with no
  tangential direction and two or more crossing directions was reported as a node, a touching
  point where crossing branches meet. The round of edge 8 of upstream's `Fillet/bottle.smb` at
  r 0.1 got 16 and 11 such touching points in two intersections of a round with its support
  face, and failed with status 1001. All of them lie on the rail, on the edge of the round's
  domain where it touches the face, and the marcher already returns that rail as tangent
  curves; without the touching points the round builds the body it builds with the flag off.
  Now a node needs crossing directions on more than one line through it.
- **Limits:** the three checks judge from the probe's 24 sampled directions, not from a
  certificate. A coincident region whose corner at the seed is narrower than 45 degrees in the
  tangent plane is still taken for curves. Two branches crossing at less than 30 degrees fall in
  one direction cluster and are not reported as a node. A node at a pole gets no directions, so
  no branch is traced on through it. Nodes the march finds at a minimum of the crossing angle
  are not checked for one line.
- **Tests:** goldens `surface_surface_p2::tests::coincident_surfaces_get_no_tangent_curve_from_the_tracer`
  and `a_pole_of_two_coincident_spheres_starts_no_tangent_curve` (the curve types as with the
  flag off), `blend_fillet_executive_p3::end_to_end::wr75_rail_contacts` (the bottle's round is
  a manifold solid of 31 vertices, 47 edges and 18 faces). Each turns the flag on, and each was
  red before its change.
- **With the flag on and these changes,** 16 goldens rows move. 13 rows of `surface.surface_p2`
  get touching points at the nodes where crossing branches meet (`ssi_tor_purpen_tor_sing`,
  `ssi_bumppy_plane`, `ssi_cyl_cyl_sing`, `ssi_cyl_cyl_sing2`, `ssi_clasic_ruled`, the
  corpus' `steinmetz_equal_cylinders`) or the contact curve traced (the corpus'
  `quartic_plane_circle`, `cubic_plane_line`, `quartic_ridge_plane_line`), which the pass is
  for. 3 rows of `topology.brep_planar_section` get a curve 0.175 long through the one point
  where a plane at the torus' bounding box touches it, where the section is that point
  (tangency-labs/solidcraft#19274). `offset.offset_geometry_creation`,
  `surface.surf_of_revolution_p2` and `blend.fillet_executive_p3` do not move.
- **Still in the way of retiring the flag:** besides the torus section, three tests of
  `issue_394_ssi_tangent_contact_points` (`nearby_distinct_tangent_contacts_are_not_one_contact`,
  `a_distant_c0_knot_preserves_nearby_distinct_contacts`,
  `nearby_distinct_contacts_survive_certification_against_a_trim_edge`) fail with the flag on:
  their two touching points lie `5e-5` apart and the surfaces stay within `1.6e-10` of each
  other between them, and the pass returns a near-tangent curve where the tests ask for two
  distinct touching points. The probe's radius, `2e-3` of the model size, is about `4e-5` there
  and so reaches from one point to the other (computed from the sizes, not traced).

![Left: a seed at the corner of the domains where two surfaces coincide, with the probe's circle of 24 directions; the 7 directions inside the domains each find the contact along themselves, the other 17 leave the domains. Right: a rail along which a round touches its face, where the probe finds crossing directions on one line through the seed, beside two crossing branches, where it finds them on two lines](WR-75--addendum-2026-11-19-coincident-regions-poles-rail-contacts.svg)

*Look at the directions the probe finds: at the corner, every one inside the domains; on the
rail, one line; at a node, two lines.*
