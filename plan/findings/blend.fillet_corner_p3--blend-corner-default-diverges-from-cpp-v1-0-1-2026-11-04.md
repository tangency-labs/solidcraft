# blend.fillet_corner_p3: with the blend corner the default, 13 units disagree with C++ v1.0.1 on 381 cases (2026-11-04)

- **Units:** `blend.fillet_corner_p2` (2 cases), `blend.fillet_corner_p3` (95),
  `blend.fillet_executive_p3` (155), `blend.fillet_geom_p4` (23), `intersect.api_intersectors`
  (1), `intersect.surface_intersector_p2` (40), `primitive.api_trim_ops` (9), `surface.torus`
  (12), `topology.api_files` (2), `topology.api_projection` (5), `topology.brep_cutting` (19),
  `topology.brep_local_operation` (6), `topology.brep_planar_section` (12).
- **Change:** tangency-labs/solidcore#2400 removes `UpstreamFixes::fillet_blend_corner`: the
  default kernel is the blend corner (WR-697's addendum of 2026-11-04), and the old corner code
  it replaced is deleted. The C++ reference (v1.0.1, `scripts/upstream-pin.env`) runs the pinned
  corner code, so the cases whose result the blend corner changes no longer agree with it.

## What the cases are

Every one is the flag's result, not the deletions': on main (9d6dba1b8) with
`fillet_blend_corner` and `fillet_nx1_tangent_side_edge_same_convexity` on by default (a local
build, not committed), the same 13 units give the same outcome and the same first mismatch on
every case they share with this branch. Which of the blend corner's behaviours (WR-697's
addendum lists them) moves each case was read off for these:

- `blend.fillet_corner_p3`'s `create` cases: `Create` reports corner type 9 (the Join, or the
  End at a smooth vertex) where upstream reports 8 (Nx1), 2 (degenerate), 3 (1x1) or 4 (2x2);
  in 8 cases the corner made has no closed Nx1 `m_bG1Case` (`-1`) where upstream's is `0`.
- `blend.fillet_corner_p2`'s two tangent-solver cases: the closed Nx1 corner's
  `GetTangentFilletSolver` override is deleted, a closed edge meeting itself G1 being the Join.
- `surface.torus`: its only flag site is the horn torus kept by
  `SmTorus::RebuildSTEPFromNURBParameters` (af7a4d9).
- `intersect.surface_intersector_p2`: its flag site is `FlushCurve`'s short last span, merged
  into the span before only when the merged span keeps the trace's accuracy (the record
  lengths differ).

- `blend.fillet_executive_p3`'s runner compares a refusal that names the size that builds
  (`SolidError::FilletRefused`) as its status 1001, as it compares `SolidError::Upstream`;
  it took it for a harness error before. 18 such refusals are the C++'s 1001 too and pass;
  4 (`stubs4-delete-edge-bottle-*`) are refusals where the C++ builds.

The others (`blend.fillet_executive_p3`'s end-to-end fillets, `blend.fillet_geom_p4`, and the
topology and primitive units, whose cuts, sections and projections intersect surfaces) reach
several of the flag's sites; their cases are not split by site.

Each is a known mismatch (`crates/difftest/known_mismatches/<unit>.toml`, `why` naming the
field and both values), and each unit is `partial`: the reference is not wrong, solidcore no
longer follows it there.

## Retired cases

The runners no longer generate the cases whose Rust code is deleted: corners of the 1x1 and 2x2
classes (`blend.fillet_corner_p1`, `_p2`, `_p3`), `AdjustTangentCorner`
(`blend.fillet_corner_p3`), `TestAndConvertBigRadFillet` (its curated `big_rad_*` cases, and the
`AdjustTopology` cases on a blending Nx2 corner of other than three edges, which reach it). The
geometry units' stand-in corners take the 4x3 type in place of the 2x2 one on both sides
(`blend.fillet_geom_p2`, `_p3`, `_p6`). Their disappearance, and the passes that became known
mismatches, are waived for this change (`plan/difftest-waivers.toml`).

## Each case

Every case this change lists, with its first mismatch as measured on this branch (C++ v1.0.1 against solidcore). Each one gives the same outcome and first mismatch on main with `fillet_blend_corner` on.

### `blend.fillet_corner_p2` (2)

- `p1_corner_tangent_solver_24`: values.solvers[0]: cpp=0e0 rust=-1e0
- `p1_corner_tangent_nx1_closed_g1_query0`: values.solvers[0]: cpp=0e0 rust=-1e0

### `blend.fillet_corner_p3` (95)

- `box1x1x2_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `box1x1x2_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `brick_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `brick_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `corner_box_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `corner_box_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `l_shaped_box_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `l_shaped_box_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `6_sided_corner_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `6_sided_corner_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `box_w_slot_eu0_m0_circ`: ints.type[0]: cpp=2 rust=9
- `box_w_slot_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `nurb_box_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `nurb_box_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `filleted_box_eu0_m0_circ`: ints.type[0]: cpp=2 rust=9
- `filleted_box_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `box_on_box_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `box_on_box_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `cyl_on_box_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `cyl_on_box_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `open_shell_eu0_m0_circ`: ints.type[0]: cpp=1 rust=9
- `open_shell_eu0_m1_circ`: values.setback: cpp=present rust=missing
- `open_shell_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `open_shell_eu17_m1_circ`: ints.type[0]: cpp=4 rust=9
- `rollover_corner_eu0_m0_circ`: ints.type[0]: cpp=2 rust=9
- `rollover_corner_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `test1_eu0_m0_circ`: ints.type[0]: cpp=2 rust=9
- `test1_eu17_m0_circ`: ints.g1[0]: cpp=0 rust=-1
- `loft_brick_eu0_m0_circ`: ints.type[0]: cpp=8 rust=9
- `loft_brick_eu17_m0_circ`: ints.type[0]: cpp=8 rust=9
- `gen0017`: ints.type[0]: cpp=8 rust=9
- `gen0019`: ints.type[0]: cpp=2 rust=9
- `gen0021`: ints.type[0]: cpp=3 rust=9
- `gen0027`: ints.g1[0]: cpp=0 rust=-1
- `gen0028`: ints.type[0]: cpp=2 rust=9
- `gen0036`: ints.type[0]: cpp=8 rust=9
- `gen0039`: values.setback: cpp=present rust=missing
- `gen0046`: ints.type[0]: cpp=8 rust=9
- `gen0052`: ints.type[0]: cpp=3 rust=9
- `gen0054`: ints.type[0]: cpp=8 rust=9
- `gen0065`: ints.type[0]: cpp=3 rust=9
- `gen0069`: ints.type[0]: cpp=8 rust=9
- `gen0091`: ints.type[0]: cpp=8 rust=9
- `gen0097`: ints.type[0]: cpp=8 rust=9
- `gen0109`: ints.g1[0]: cpp=0 rust=-1
- `gen0115`: ints.type[0]: cpp=8 rust=9
- `gen0124`: ints.type[0]: cpp=2 rust=9
- `gen0133`: ints.type[0]: cpp=2 rust=9
- `gen0139`: ints.type[0]: cpp=2 rust=9
- `gen0146`: ints.g1[0]: cpp=0 rust=-1
- `gen0148`: ints.type[0]: cpp=2 rust=9
- `gen0150`: ints.type[0]: cpp=8 rust=9
- `gen0151`: ints.type[0]: cpp=2 rust=9
- `gen0157`: ints.type[0]: cpp=2 rust=9
- `gen0178`: ints.g1[0]: cpp=0 rust=-1
- `gen0183`: ints.type[0]: cpp=8 rust=9
- `gen0184`: ints.type[0]: cpp=4 rust=9
- `gen0192`: ints.type[0]: cpp=8 rust=9
- `gen0198`: ints.type[0]: cpp=8 rust=9
- `gen0202`: ints.type[0]: cpp=3 rust=9
- `gen0205`: ints.g1[0]: cpp=0 rust=-1
- `gen0207`: ints.type[0]: cpp=2 rust=9
- `gen0226`: ints.type[0]: cpp=2 rust=9
- `gen0229`: ints.type[0]: cpp=8 rust=9
- `gen0231`: ints.type[0]: cpp=2 rust=9
- `gen0235`: ints.type[0]: cpp=1 rust=9
- `gen0238`: ints.type[0]: cpp=8 rust=9
- `gen0240`: ints.type[0]: cpp=8 rust=9
- `gen0243`: ints.type[0]: cpp=8 rust=9
- `gen0253`: ints.type[0]: cpp=8 rust=9
- `gen0257`: ints.type[0]: cpp=8 rust=9
- `gen0267`: ints.type[0]: cpp=8 rust=9
- `gen0269`: ints.type[0]: cpp=8 rust=9
- `gen0273`: ints.type[0]: cpp=8 rust=9
- `gen0283`: ints.type[0]: cpp=8 rust=9
- `gen0284`: ints.type[0]: cpp=8 rust=9
- `gen0291`: ints.g1[0]: cpp=0 rust=-1
- `gen0303`: ints.type[0]: cpp=8 rust=9
- `gen0305`: ints.type[0]: cpp=8 rust=9
- `gen0311`: ints.type[0]: cpp=4 rust=9
- `gen0312`: values.setback: cpp=present rust=missing
- `gen0321`: ints.type[0]: cpp=2 rust=9
- `gen0340`: ints.type[0]: cpp=8 rust=9
- `gen0341`: ints.type[0]: cpp=2 rust=9
- `gen0343`: ints.type[0]: cpp=8 rust=9
- `gen0344`: ints.type[0]: cpp=8 rust=9
- `gen0355`: ints.type[0]: cpp=2 rust=9
- `gen0356`: ints.type[0]: cpp=2 rust=9
- `gen0359`: ints.type[0]: cpp=8 rust=9
- `gen0362`: ints.type[0]: cpp=8 rust=9
- `gen0367`: ints.type[0]: cpp=2 rust=9
- `gen0368`: ints.type[0]: cpp=8 rust=9
- `gen0383`: ints.type[0]: cpp=8 rust=9
- `gen0390`: ints.type[0]: cpp=2 rust=9
- `gen0392`: ints.g1[0]: cpp=0 rust=-1

### `blend.fillet_executive_p3` (155)

- `attributed-box-edges-0-1-2-r1`: values.attributes[1]: cpp=1.51371659147012e1 rust=1.5137166701561846e1
- `attributed-box-all-edges-r1`: values.attributes[1]: cpp=1.5707959637652982e0 rust=1.5707977565211646e0
- `attributed-per-edge-box-edges-4-5`: values.attributes[1]: cpp=2.227765008008809e1 rust=2.2277652634651364e1
- `attributed-circular-box-r1`: values.attributes[1]: cpp=1.5707959636710958e0 rust=1.570797756630218e0
- `attributed-box-cylinder-arc-round-seam-mid-r2`: values.attributes[1]: cpp=1.6348722007276933e1 rust=1.6348717865071997e1
- `attributed-box-cylinder-arc-round-seam-quarter-r2`: values.attributes[1]: cpp=3.269744415425451e1 rust=3.269743999303879e1
- `law-cube20-e0-deg1-4-2-4-c0-corner`: values.mass_properties[0]: cpp=2.3780696234557445e3 rust=2.378074050023597e3
- `law-cube20-e3-deg1-4-2-4-c0-corner`: values.mass_properties[0]: cpp=2.3780713377902525e3 rust=2.3780812378537685e3
- `law-cube20-e0-deg2-c0-knot`: values.mass_properties[0]: cpp=2.3780637780600227e3 rust=2.3780729638719945e3
- `law-cube20-e0-deg1-linear-2-4`: values.mass_properties[0]: cpp=2.3699972405849726e3 rust=2.369997237948705e3
- `law-cube20-e0-deg2-smooth-4-2-4`: values.mass_properties[0]: cpp=2.364585839284445e3 rust=2.364585829344723e3
- `law-cube20-e0-deg2-smooth-xsect-blend-curve`: values.mass_properties[7]: cpp=2.10004806255316e6 rust=2.100048064728172e6
- `law-cube20-e0-deg2-smooth-g2`: values.mass_properties[0]: cpp=2.364585839284445e3 rust=2.364585829344723e3
- `big-rad-filleted_box-r2.0-top-edges`: values.fillet_point_step[0]: cpp=1e-3 rust=1e-5
- `big-rad-filleted_box-r3.0-top-edges`: values.fillet_point_step[0]: cpp=1e-3 rust=1e-5
- `corner3x2-union-inset-side-step-edges1-18-r1`: values.result_geometry[565]: cpp=0e0 rust=1e0
- `real-box-high-corner-setback-eight`: values.mass_properties[0]: cpp=8.506678458592518e3 rust=8.8e3
- `real-box-high-corner-setback-zero-control`: values.mass_properties[0]: cpp=8.522842931228559e3 rust=8.52284290055832e3
- `curve-sih-box-all-edges`: values.mass_properties[0]: cpp=2.1482124746091645e3 rust=2.1482123682459182e3
- `curve-sih-box_diff_cyl-all-edges-r0.1`: values.mass_properties[0]: cpp=6.272438632022713e2 rust=6.27243808974239e2
- `curve-sih-box_diff_cyl-all-edges-r0.25`: values.mass_properties[1]: cpp=9.492763716049255e2 rust=9.492763190765646e2
- `curve-sih-box_diff_cyl-all-edges-r0.5`: values.mass_properties[0]: cpp=6.009561001914067e2 rust=6.009562871616752e2
- `curve-sih-box_diff_cyl-all-edges-r1.0`: values.mass_properties[0]: cpp=5.686905366319031e2 rust=5.686912498552141e2
- `curve-sih-box_diff_cyl-all-edges-step2`: values.mass_properties[0]: cpp=6.272438632022713e2 rust=6.27243808974239e2
- `issue_2020_mitred_put_back_d9_solver_geometry`: values.generated_geometry_samples[5]: cpp=-2.459999999999999e1 rust=-3.4499999999999986e1
- `issue_2020_mitred_put_back_d9_corner_edges`: values.all_corner_vertices[1]: cpp=6.661338147750939e-16 rust=-1.1102230246251565e-15
- `issue_2176_tabbed_plate_corner_arc_r0_5`: values.mass_properties[0]: cpp=4.399441905611055e3 rust=4.398846781734744e3
- `issue_2176_tabbed_plate_tab_side_r0_5`: values.mass_properties[0]: cpp=4.397942519679468e3 rust=4.398161782524585e3
- `issue_2176_tabbed_plate_tab_top_r0_5`: values.mass_properties[0]: cpp=4.398252074845071e3 rust=4.3986904300294145e3
- `issue_2176_tabbed_plate_disc_wall_r0_5`: values.mass_properties[0]: cpp=4.367449516043326e3 rust=4.367449505761713e3
- `real-box-fillet12-r3-shell-closed-issue-460`: values.shell_geometry[1]: cpp=2.9999999999999982e0 rust=1.6999999999999986e1
- `real-box-fillet12-r3-shell-open-top-issue-460`: values.shell_geometry[1]: cpp=2.9999999999999982e0 rust=1.6999999999999986e1
- `overhang-union-y-overhang-all-edges-r0.5`: values.mass_properties[0]: cpp=1.3034713487073839e3 rust=1.3034712201001912e3
- `overhang-union-y-both-overhang-all-edges-r0.5`: values.mass_properties[0]: cpp=1.4966449354812767e3 rust=1.496644853366766e3
- `overhang-union-xy-overhang-all-edges-r0.5`: values.mass_properties[0]: cpp=1.3194713778722116e3 rust=1.3194712187212856e3
- `piecewise-box-all-edges`: values.mass_properties[0]: cpp=2.1482124755792665e3 rust=2.14821236824592e3
- `piecewise-box-edges-0-1`: values.mass_properties[0]: cpp=2.193419244240802e3 rust=2.193419247875058e3
- `piecewise-file-box1x1x2-all-edges`: values.mass_properties[0]: cpp=9.30194538324081e0 rust=9.301946794595143e0
- `piecewise-box-edges-0-1-2`: values.mass_properties[0]: cpp=2.1890199082702225e3 rust=2.1890199119044787e3
- `piecewise-box-edges-0-1-3`: values.mass_properties[0]: cpp=2.189091807433971e3 rust=2.1890918147009347e3
- `piecewise-box-edges-0-1-4`: values.mass_properties[0]: cpp=2.1890199105871693e3 rust=2.1890199142213405e3
- `piecewise-box-edges-0-1-5`: values.mass_properties[1]: cpp=5.996802080307567e3 rust=5.996802025046345e3
- `piecewise-box-edges-0-4-8`: values.mass_properties[0]: cpp=2.184799771940774e3 rust=2.1847997792065776e3
- `piecewise-box-edges-0-1-2-3`: values.mass_properties[0]: cpp=2.1846924714631823e3 rust=2.1846924787301464e3
- `piecewise-box-edges-0-3-8`: values.mass_properties[0]: cpp=2.184727871705279e3 rust=2.184727875337034e3
- `piecewise-box-edges-0-4-5`: values.mass_properties[0]: cpp=2.1847997681506254e3 rust=2.184799775420751e3
- `radius-supported-at-limit-accepted`: values.mass_properties[0]: cpp=2.2000000000183927e3 rust=2.199999313231904e3
- `radius-supported-8x-tolerance-accepted`: values.mass_properties[0]: cpp=2.2000000000183927e3 rust=2.1999993132312175e3
- `real-box-corners-actual-session`: ints.prefix_state[5]: cpp=8 rust=9
- `real-box-first_topology-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-first-vertex-geometry-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-corner-vertices-component-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-adjusted-topology-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-solver-geometry-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-corner-edges-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-trim-rails-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-classified-rails-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-intermediate-faces-actual-session`: values.intermediate_face_geometry[34]: cpp=0e0 rust=1e0
- `real-box-attribute-stitch-actual-session`: values.intermediate_face_geometry[34]: cpp=0e0 rust=1e0
- `real-box-direct-shrink-actual-session`: ints.before_direct_topology[5]: cpp=8 rust=9
- `real-box-create-fillet-brep-actual-session`: values.intermediate_face_geometry[34]: cpp=0e0 rust=1e0
- `real-box-trihedral-equal-radius-analytic-corner`: values.intermediate_face_geometry[5]: cpp=0e0 rust=5e-1
- `real-box-trihedral-equal-radius-full-api`: values.result_geometry[43]: cpp=7.347763673217594e-5 rust=4e-5
- `source-file-l_shaped_box-all-edges`: values.mass_properties[0]: cpp=3.4527480404409465e2 rust=3.4527480064158067e2
- `source-file-6_sided_corner-all-edges`: values.mass_properties[0]: cpp=4.435716453988927e2 rust=4.4357164052076826e2
- `source-file-brick-all-edges`: values.mass_properties[1]: cpp=2.949643218829163e5 rust=2.949643201768648e5
- `source-file-box_on_box-all-edges`: values.mass_properties[0]: cpp=1.623981019897444e2 rust=1.623980995262507e2
- `source-file-box_diff_cyl-all-edges`: values.mass_properties[0]: cpp=6.272445423347634e2 rust=6.272444874195922e2
- `source-file-box1x1x2-all-edges`: values.mass_properties[0]: cpp=9.301945383062504e0 rust=9.301946794595839e0
- `source-file-corner_box-all-edges`: values.mass_properties[0]: cpp=3.6029209899977093e2 rust=3.602920981708457e2
- `source-box-union-all-edges`: values.mass_properties[0]: cpp=1.3051789423524506e3 rust=1.3051793137278682e3
- `source-box-difference-all-edges`: values.mass_properties[0]: cpp=1.305179394450937e3 rust=1.3051793161840712e3
- `source-file-box_with_large_fillet-edge4`: values.mass_properties[0]: cpp=5.382227848348099e2 rust=5.37671281975981e2
- `surface-surface-test-sm-surface-surface-fillet`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `stubs3-overhang-union-nx1-multi-neighbour-end-face-edge9-r1.0`: values.result_geometry[395]: cpp=0e0 rust=2e0
- `stubs3-overhang-union-nx1-multi-neighbour-end-face-edge12-r1.0`: values.result_geometry[431]: cpp=0e0 rust=2e0
- `stubs3-overhang-union-nx1-multi-neighbour-end-face-edge9-r0.5`: values.result_geometry[395]: cpp=0e0 rust=2e0
- `stubs3-overhang-union-nx1-multi-neighbour-end-face-edge12-r0.5`: values.result_geometry[431]: cpp=0e0 rust=2e0
- `stubs3-overhang-union-nx1-multi-neighbour-end-face-edge9-r2.0`: values.result_geometry[395]: cpp=0e0 rust=2e0
- `stubs3-overhang-union-nx1-multi-neighbour-end-face-edge12-r2.0`: values.result_geometry[431]: cpp=0e0 rust=2e0
- `stubs3-union-inset-side-step-edges17-22-r1`: values.result_geometry[439]: cpp=0e0 rust=1e0
- `stubs4-open_shell-e5-r0.5`: values.result_geometry[414]: cpp=4e-5 rust=1e-5
- `stubs4-shell_fillet2-e5-r0.5`: values.result_geometry[332]: cpp=7.499965358490174e-1 rust=7.5e-1
- `stubs4-torii_unioned-e2-r0.5`: values.mass_properties[4]: cpp=7.557343408739472e-6 rust=7.557343420275544e-6
- `stubs4-degenerate-caps_coons-e5-r0.5`: values.result_geometry.len: cpp=164 rust=360
- `stubs4-degenerate-nurb_cone-e5-r0.5`: values.mass_properties[0]: cpp=3.148221363928385e2 rust=3.1483252600610484e2
- `stubs4-degenerate-nurb_cone-e8-r0.5`: values.mass_properties[0]: cpp=3.14832648986208e2 rust=3.1483252600610484e2
- `stubs4-degenerate-test5-e2-r0.5`: values.result_geometry[41]: cpp=1.0519358666616144e-1 rust=1.051937448189453e-1
- `stubs4-degenerate-test5-e3-r0.5`: values.result_geometry[40]: cpp=1.0519358666616269e-1 rust=1.0519374481894433e-1
- `stubs4-degenerate-test5-e5-r0.5`: values.result_geometry[41]: cpp=-1.0519358666616185e-1 rust=-1.0519374481894439e-1
- `stubs4-degenerate-test5-e9-r0.5`: values.result_geometry[40]: cpp=-1.0519358666616241e-1 rust=-1.0519374481894439e-1
- `stubs4-rollover-bague-e1-r0.5`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-rollover-cap1-e3-r0.5`: values.result_geometry.len: cpp=276 rust=138
- `stubs4-delete-edge-bottle-e7-r0.1`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-bottle-e8-r0.1`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-bottle-e4-r0.5`: rust: blend::fillet_executive::session::finish: upstream status 1001: refused; the largest size that builds is 0.4502721965004639
- `stubs4-delete-edge-bottle-e6-r0.5`: rust: blend::fillet_executive::session::finish: upstream status 1001: refused; the largest size that builds is 0.4502702207096263
- `stubs4-delete-edge-bottle-e16-r0.5`: rust: blend::fillet_executive::session::finish: upstream status 1001: refused; the largest size that builds is 0.4502702307386244
- `stubs4-delete-edge-bottle-e19-r0.5`: rust: blend::fillet_executive::session::finish: upstream status 1001: refused; the largest size that builds is 0.45027219654139117
- `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.5`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5`: values.mass_properties.len: cpp=0 rust=12
- `stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5`: values.mass_properties.len: cpp=0 rust=12
- `surface-surface-linear-xsect`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-approx-circular-xsect`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-mirror`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-complement`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-mirror-complement-approx`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-negative-second-radius`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-negative-both-radii`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-tilted-second`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-partial-overlap-in-v`: values.result_geometry[41]: cpp=0e0 rust=1e0
- `surface-surface-small-radius-tight-tolerance`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-huge-scale`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `surface-surface-tiny-scale`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `owned-first-free-second-x0`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `free-first-owned-second-x0`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `two-faces-one-body-x0`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `two-faces-two-bodies-x0`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `owned-first-free-second-x2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `free-first-owned-second-x2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `two-faces-one-body-x2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `two-faces-two-bodies-x2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-free-r2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-one-body-r2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-two-bodies-r2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-owned-first-r2`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-free-small`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-one-body-small`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-two-bodies-small`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-owned-first-small`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-free-radius-twice-tolerance`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-one-body-radius-twice-tolerance`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-two-bodies-radius-twice-tolerance`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `adjacent-owned-first-radius-twice-tolerance`: values.result_geometry[33]: cpp=0e0 rust=1e0
- `sweep4-bottle-e29-r0.1`: values.mass_properties[0]: cpp=3.311823065493446e1 rust=3.31182287598083e1
- `sweep5-letter_g-e1-r1.0`: values.mass_properties[0]: cpp=8.591432001436689e2 rust=8.591376729744185e2
- `sweep5-bottle-e29-r1.0`: values.mass_properties.len: cpp=0 rust=12
- `sweep5-letter_g-e10-r1.0`: values.mass_properties[0]: cpp=8.555500031740756e2 rust=8.554938754361581e2
- `sweep5-sphere_diff_3cylinders-e29-r1.0`: values.mass_properties.len: cpp=0 rust=12
- `sweep5-ext_w_hole-e15-r1.0`: values.result_geometry[51]: cpp=5.297559784473877e-5 rust=5.297041359877074e-5
- `sweep5-nurb_box1-all-r0.5`: values.mass_properties[0]: cpp=1.572351595052851e3 rust=1.5723515574254377e3
- `sweep5-ext_w_hole-e14-r1.0`: values.result_geometry[62]: cpp=1.4453956777755315e-15 rust=-7.131077187453804e-15
- `upstream-TestSmSetBevelCorners-bevel`: ints.api_status_counts[1]: cpp=25 rust=26
- `preview0004`: values.preview[32]: cpp=8.881784197001252e-16 rust=2.829432357461615e-15
- `preview0006`: values.preview[112]: cpp=1.5002e4 rust=1.5012e4
- `preview0009`: values.preview[6]: cpp=1.5002e4 rust=1.5012e4
- `preview0018`: values.preview[6]: cpp=1.5002e4 rust=1.5012e4
- `preview0052`: values.preview[14]: cpp=-2.220446049250313e-16 rust=-1.5543122344752192e-15
- `preview0057`: values.preview[13]: cpp=6.661338147750939e-16 rust=-1.1102230246251565e-16
- `preview0058`: values.preview[6]: cpp=1.5002e4 rust=1.5012e4

### `blend.fillet_geom_p4` (23)

- `apex_start`: values.uv1_cpts[0]: cpp=0e0 rust=4.6551724137931033e-1
- `apex_end`: values.uv1_cpts[4]: cpp=0e0 rust=9.310344827586207e-1
- `rail_ext_no_surface`: values.status[0]: cpp=0e0 rust=1.006e3
- `rail_ext_unprocessed`: values.status[0]: cpp=1.001e3 rust=1.006e3
- `rev_spindle_apex_to_mid_1`: values.uv1_cpts[0]: cpp=0e0 rust=4.3528084378370735e-1
- `rev_spindle_mid_to_apex_1`: values.uv1_cpts[4]: cpp=0e0 rust=4.3528084378370735e-1
- `rev_spindle_apex_to_mid_0.0001`: values.uv1_cpts[0]: cpp=0e0 rust=4.352808437837307e-1
- `rev_spindle_mid_to_apex_0.0001`: values.uv1_cpts[4]: cpp=0e0 rust=4.352808437837307e-1
- `rev_spindle_apex_to_mid_100000`: values.uv1_cpts[0]: cpp=0e0 rust=4.3528084378370735e-1
- `rev_spindle_mid_to_apex_100000`: values.uv1_cpts[4]: cpp=0e0 rust=4.3528084378370735e-1
- `rev_fan_apex_start`: values.uv1_cpts[1]: cpp=0e0 rust=4.975968621913598e-1
- `rev_fan_apex_end`: values.uv1_cpts[5]: cpp=0e0 rust=4.975968621913598e-1
- `rev_fan_apex_bigtol`: values.uv1_cpts[1]: cpp=0e0 rust=4.975968621913598e-1
- `gen0015`: values.status[0]: cpp=1.001e3 rust=1.006e3
- `gen0040`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0100`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0103`: values.status[0]: cpp=1.001e3 rust=1.006e3
- `gen0130`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0217`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0257`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0273`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0280`: values.status[0]: cpp=0e0 rust=1.006e3
- `gen0297`: values.status[0]: cpp=0e0 rust=1.006e3

### `intersect.api_intersectors` (1)

- `gen0037`: values.record[78]: cpp=-2.665105324977061e0 rust=-2.6651204387573064e0

### `intersect.surface_intersector_p2` (40)

- `adv_perp_trace_1e-06`: values.rec.len: cpp=2127 rust=2262
- `gen0000`: values.rec.len: cpp=2081 rust=1800
- `gen0036`: values.rec.len: cpp=1527 rust=1418
- `gen0076`: values.rec.len: cpp=1932 rust=1755
- `gen0080`: values.rec.len: cpp=1917 rust=1808
- `gen0083`: values.rec[925]: cpp=1.001e3 rust=1.007e3
- `gen0139`: values.rec.len: cpp=3515 rust=3270
- `gen0187`: values.rec.len: cpp=1109 rust=1000
- `gen0262`: values.rec.len: cpp=2536 rust=2427
- `gen0312`: values.rec.len: cpp=4303 rust=4104
- `gen0345`: values.rec.len: cpp=3826 rust=3613
- `gen0400`: values.rec.len: cpp=2183 rust=2074
- `gen0410`: values.rec.len: cpp=1619 rust=1510
- `gen0423`: values.rec.len: cpp=3891 rust=3692
- `gen0426`: values.rec.len: cpp=2295 rust=2186
- `gen0428`: values.rec.len: cpp=2104 rust=1995
- `gen0524`: values.rec.len: cpp=3062 rust=2908
- `gen0534`: values.rec.len: cpp=1197 rust=1088
- `gen0596`: values.rec.len: cpp=1922 rust=1813
- `gen0641`: values.rec.len: cpp=4210 rust=4101
- `gen0680`: values.rec.len: cpp=3400 rust=3291
- `gen0784`: values.rec.len: cpp=1942 rust=1743
- `gen0876`: values.rec.len: cpp=4331 rust=4086
- `gen0947`: values.rec.len: cpp=1780 rust=1671
- `gen0954`: values.rec.len: cpp=1356 rust=1247
- `gen1016`: values.rec.len: cpp=3162 rust=3053
- `gen1052`: values.rec.len: cpp=3353 rust=3108
- `gen1070`: values.rec.len: cpp=2024 rust=1825
- `gen1113`: values.rec.len: cpp=2161 rust=2052
- `gen1162`: values.rec.len: cpp=2574 rust=2420
- `gen1227`: values.rec.len: cpp=2721 rust=2508
- `gen1262`: values.rec.len: cpp=1020 rust=843
- `gen1275`: values.rec.len: cpp=1455 rust=1346
- `gen1291`: values.rec.len: cpp=1019 rust=910
- `gen1303`: values.rec.len: cpp=1516 rust=1407
- `gen1311`: values.rec.len: cpp=2782 rust=2605
- `gen1341`: values.rec.len: cpp=2077 rust=1900
- `gen1347`: values.rec.len: cpp=2750 rust=2433
- `gen1440`: values.rec[842]: cpp=1.001e3 rust=1.007e3
- `gen1456`: values.rec.len: cpp=1814 rust=1660

### `primitive.api_trim_ops` (9)

- `pat_upstream_filleted_box`: values.rec[5]: cpp=0e0 rust=9.5e0
- `pat_long_direction`: values.rec[5]: cpp=0e0 rust=9.5e0
- `gen0018`: values.rec[5]: cpp=1.1566500740742075e1 rust=1.2041349858786546e1
- `gen0043`: values.rec[5]: cpp=6.154961544195698e0 rust=2.4727472564510693e0
- `gen0072`: values.rec[5]: cpp=2.511523290570555e0 rust=-5.633374507787798e-1
- `gen0073`: values.rec[5]: cpp=3.516038448762102e0 rust=-9.68963188411752e-1
- `gen0075`: values.rec[7]: cpp=9.349950725243353e0 rust=-5.253357982678253e-2
- `gen0112`: values.rec[5]: cpp=6.962519980387759e0 rust=7.347046551623312e0
- `gen0126`: values.rec[5]: cpp=9.703821739618014e0 rust=2.818685040338975e-2

### `surface.torus` (12)

- `edge_swap_inside_rebuild`: values.out[129]: cpp=8.728832706816693e-16 rust=9.54457398780101e-16
- `edge_swap_inside_offset`: values.out[131]: cpp=2.0679121553410856e-16 rust=2.019356992757579e-16
- `v_neg_plain_rebuild`: values.out[138]: cpp=2.1700694667505123e-17 rust=4.5478826405508014e-32
- `gen0001`: values.out[162]: cpp=-1.7277712092890423e-15 rust=-2.904707741103669e-15
- `gen0047`: values.out[130]: cpp=-4.101126376011248e-16 rust=-4.345177889262588e-16
- `gen0139`: values.out[162]: cpp=6.796169582148275e-16 rust=9.140443269770489e-16
- `gen0219`: values.out[161]: cpp=0e0 rust=4.8290695977012424e-17
- `gen0286`: values.out[160]: cpp=-6.266905339025343e-17 rust=-9.780708814033087e-17
- `gen0289`: values.out[164]: cpp=1.210990595648366e-16 rust=0e0
- `gen0355`: values.out[22]: cpp=1.5790521061418522e2 rust=1.6829229203210454e2
- `gen0358`: values.out[143]: cpp=-3.3999419826688897e-16 rust=-4.268325096133701e-16
- `gen0393`: values.out[131]: cpp=0e0 rust=3.2677068954531057e-15

### `topology.api_files` (2)

- `turn_fillet_box_10_r1`: values.record.len: cpp=3677 rust=4367
- `turn_fillet_box_5_10_20_r05`: values.record.len: cpp=3677 rust=4367

### `topology.api_projection` (5)

- `project_brep_fillet_box`: values.record[22]: cpp=-2.7755575615628914e-16 rust=0e0
- `project_brep_fillet_box_long_normal`: values.record[22]: cpp=-2.7755575615628914e-16 rust=0e0
- `silhouette_fillet_box_oblique`: values.record[3]: cpp=5.268161424175952e-1 rust=5.196102397445207e-1
- `gen0007`: values.record[3]: cpp=1.1472800253272268e1 rust=1.1475153969945342e1
- `gen0012`: values.record[22]: cpp=5.3290705182007514e-14 rust=0e0

### `topology.brep_cutting` (19)

- `api_cut_fillet_box_z7`: values.geometry[276]: cpp=0e0 rust=5e0
- `api_cut_fillet_box_zero_normal`: values.geometry[405]: cpp=9.5e0 rust=5e-1
- `api_cut_fillet_box_z7_long_normal`: values.geometry[276]: cpp=0e0 rust=5e0
- `cut_fillet_box_through_fillets`: values.geometry[405]: cpp=9.5e0 rust=5e-1
- `cut_fillet_box_oblique`: values.geometry[276]: cpp=0e0 rust=5e0
- `gen0002`: values.geometry[128]: cpp=1.1259913999168175e1 rust=2.2852719873518246e0
- `gen0014`: values.geometry[160]: cpp=5.599567221485971e1 rust=3.86651349619797e2
- `gen0021`: values.geometry[405]: cpp=1.4609958802499226e0 rust=1.721008713276019e-2
- `gen0022`: values.geometry[256]: cpp=1.2661937438367288e3 rust=0e0
- `gen0025`: values.geometry[384]: cpp=1.3035726323120764e2 rust=1.0965535607572615e3
- `gen0035`: values.geometry[257]: cpp=1.30796406441935e1 rust=1.6470725325922487e1
- `gen0056`: values.geometry[256]: cpp=2.366114799120697e0 rust=2.8136582294470056e0
- `gen0060`: values.geometry[384]: cpp=1.7885755975526768e-1 rust=5.6451903021652035e0
- `gen0063`: values.geometry[273]: cpp=5.135855721587581e-3 rust=1.5229388375470165e-1
- `gen0065`: values.geometry[276]: cpp=0e0 rust=5.017426898330775e-1
- `gen0070`: values.geometry[368]: cpp=2.636587132446384e-1 rust=1.9191383618689688e1
- `gen0072`: values.geometry[257]: cpp=7.04836831766197e0 rust=1.1014580834654245e1
- `gen0082`: values.geometry[257]: cpp=5.126938259585646e-2 rust=1.0688763691846785e0
- `gen0095`: values.geometry[389]: cpp=1.0374823296103688e0 rust=1.9851143569361462e-1

### `topology.brep_local_operation` (6)

- `gen0017`: values.geometry[77]: cpp=-8.711111533034435e1 rust=-8.711111519855812e1
- `gen0036`: values.geometry[77]: cpp=-8.72975231355388e1 rust=-8.72975230043746e1
- `gen0044`: values.geometry[77]: cpp=-8.711111533035697e1 rust=-8.711111519856698e1
- `gen0075`: values.geometry[75]: cpp=1.093294088884245e-5 rust=1.0772296664567252e-5
- `gen0076`: values.geometry[65]: cpp=-8.59862060194023e1 rust=-8.598620588388228e1
- `gen0083`: values.geometry[81]: cpp=-8.711111533034433e1 rust=-8.711111519855815e1

### `topology.brep_planar_section` (12)

- `fillet_box_y_forced`: values.record[3]: cpp=1e0 rust=6.269661697213444e-1
- `faces_fillet_box_y98`: values.record.len: cpp=367 rust=455
- `forced_fillet_box_y98`: values.record.len: cpp=1637 rust=677
- `fillet_box_y98_uv_unforced`: values.record.len: cpp=3 rust=187
- `gen0001`: values.record.len: cpp=367 rust=455
- `gen0063`: values.record[120]: cpp=-2.9559688030644793e-15 rust=-1.0269562977782698e-15
- `gen0070`: values.record[3]: cpp=1e0 rust=2.0439776075503892e2
- `gen0073`: values.record[3]: cpp=1e0 rust=1.072549971343126e1
- `gen0074`: values.record.len: cpp=3 rust=187
- `gen0089`: values.record[78]: cpp=-1.3877787807814454e-16 rust=-1.4441444458986683e-16
- `gen0093`: values.record[72]: cpp=1e0 rust=2.2841012212285203e-1
- `gen0094`: values.record[121]: cpp=-4.263256414560601e-14 rust=1.4210854715202004e-14
