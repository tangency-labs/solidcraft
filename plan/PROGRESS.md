# Port progress

Generated from `plan/modules.toml` by `harness/plan/progress.py`; do not edit by hand.

**110 / 261 units ported** (83 green, 27 partial), **140,661 / 387,903 upstream lines (36.3%)**. 2 claimed, 0 blocked, 149 todo.

Legend: ✅ green (identical to C++) · 🟡 partial (passes; some cases wait on unported code) · 🔨 claimed · ▶️ ready to claim · ⏳ waiting on dependencies · ⛔ blocked.

## Ready to claim now

- [ ] ▶️ `curve.curve_p9` (1783 lines) — ready
- [ ] ▶️ `surface.plane_p1` (1934 lines) — ready
- [ ] ▶️ `surface.surf_of_revolution_p1` (1522 lines) — ready

## In progress

- [ ] 🔨 `surface.surface_p9` (1252 lines) — in progress: #6 @0xlillith (draft)
- [ ] 🔨 `surface.trim_srf_cache` (966 lines) — in progress: #4 @0xlillith (ready for review)

## By module

### geom (29/29)

- [x] ✅ `geom.extent3d` (1122 lines) — green, 3086 cases
- [x] ✅ `geom.vector2d` (339 lines) — green, 4060 cases
- [ ] 🟡 `geom.attribute` (920 lines) — partial, 2029 cases
- [x] ✅ `geom.tol` (923 lines) — green, 3435 cases
- [ ] 🟡 `geom.tree` (1259 lines) — partial, 618 cases
- [x] ✅ `geom.assert_array` (373 lines) — green, 3734 cases
- [x] ✅ `geom.context` (355 lines) — green
- [x] ✅ `geom.extent1d` (974 lines) — green, 3091 cases
- [x] ✅ `geom.matrix` (816 lines) — green, 4044 cases
- [x] ✅ `geom.periodic_extent1d` (406 lines) — green
- [x] ✅ `geom.vector3d` (1145 lines) — green, 6114 cases
- [x] ✅ `geom.axis2_placement` (479 lines) — green, 4048 cases
- [x] ✅ `geom.extent2d` (766 lines) — green, 3069 cases
- [x] ✅ `geom.geom_utility_p2` (1617 lines) — green, 3270 cases
- [x] ✅ `geom.hash_table` (593 lines) — green, 619 cases
- [x] ✅ `geom.local_solve1d` (1294 lines) — green, 3316 cases
- [x] ✅ `geom.local_solve_nd` (860 lines) — green, 3043 cases
- [ ] 🟡 `geom.object` (673 lines) — partial, 624 cases
- [x] ✅ `geom.polynomial` (386 lines) — green, 6038 cases
- [x] ✅ `geom.geom_utility_p1` (2178 lines) — green, 4185 cases
- [ ] 🟡 `geom.grid` (876 lines) — partial, 615 cases
- [x] ✅ `geom.polar_box` (979 lines) — green
- [ ] 🟡 `geom.solution_array` (1319 lines) — partial, 810 cases
- [ ] 🟡 `geom.global_solver` (1775 lines) — partial, 620 cases
- [x] ✅ `geom.pseudo_box` (1508 lines) — green, 3011 cases
- [ ] 🟡 `geom.tools` (326 lines) — partial, 913 cases
- [ ] 🟡 `geom.transform` (681 lines) — partial, 3042 cases
- [ ] 🟡 `geom.a_object` (935 lines) — partial, 2040 cases
- [x] ✅ `geom.point_set` (694 lines) — green, 2029 cases

### nurbs (48/48)

- [x] ✅ `nlib.knots` (768 lines) — green, 2953 cases
- [x] ✅ `nlib.math` (2971 lines) — green
- [x] ✅ `nlib.geom` (1244 lines) — green
- [x] ✅ `nlib.basis_basic` (786 lines) — green
- [x] ✅ `nlib.srf_basic` (1088 lines) — green
- [x] ✅ `nlib.basis_adv` (361 lines) — green
- [x] ✅ `nlib.bezier` (1154 lines) — green, 2223 cases
- [x] ✅ `nlib.funcs_basic` (538 lines) — green, 3130 cases
- [x] ✅ `nlib.crv_basic` (1542 lines) — green, 3417 cases
- [x] ✅ `nlib.crv_fit_p1` (1707 lines) — green, 2084 cases
- [x] ✅ `nlib.crv_tool_p1` (1371 lines) — green, 2143 cases
- [x] ✅ `nlib.iges` (1055 lines) — green, 2029 cases
- [x] ✅ `nlib.srf_tool_p1` (1678 lines) — green, 3132 cases
- [x] ✅ `nlib.crv_fit_p2` (1586 lines) — green, 2106 cases
- [x] ✅ `nlib.crv_nurb` (1879 lines) — green, 2871 cases
- [x] ✅ `nlib.funcs_adv` (1676 lines) — green, 2080 cases
- [x] ✅ `nlib.srf_fit` (1359 lines) — green, 1556 cases
- [x] ✅ `nlib.crv_fit_p3` (1234 lines) — green, 2242 cases
- [x] ✅ `nlib.crv_geom` (729 lines) — green, 1723 cases
- [x] ✅ `nlib.crv_tool_p2` (1748 lines) — green, 2124 cases
- [x] ✅ `nurbs.nurbs_crv` (2200 lines) — green, 4089 cases
- [x] ✅ `nlib.crv_adv_p1` (2031 lines) — green, 2670 cases
- [x] ✅ `nlib.crv_common` (803 lines) — green, 2137 cases
- [x] ✅ `nlib.spiral` (392 lines) — green, 2032 cases
- [x] ✅ `nlib.crv_adv_p2` (1722 lines) — green, 2815 cases
- [x] ✅ `nlib.srf_common` (661 lines) — green, 1550 cases
- [x] ✅ `nurbs.b_spline_curve_p1` (1894 lines) — green, 2216 cases
- [x] ✅ `nlib.crv_approx` (908 lines) — green, 1646 cases
- [x] ✅ `nlib.crv_offset` (346 lines) — green, 1555 cases
- [x] ✅ `nlib.srf_nurb` (2560 lines) — green, 2702 cases
- [x] ✅ `nlib.srf_tool_p2` (1455 lines) — green, 3102 cases
- [x] ✅ `nurbs.b_spline_curve_n_lib` (2794 lines) — green, 2282 cases
- [x] ✅ `nurbs.nurbs_srf` (1188 lines) — green, 3092 cases
- [x] ✅ `nlib.srf_adv_p2` (2097 lines) — green, 1695 cases
- [x] ✅ `nlib.srf_adv_p3` (2233 lines) — green, 3275 cases
- [x] ✅ `nlib.srf_tool_p3` (1380 lines) — green, 2518 cases
- [x] ✅ `nurbs.b_spline_curve_p2` (1872 lines) — green, 2328 cases
- [ ] 🟡 `nurbs.b_spline_surface_p1` (1790 lines) — partial, 2120 cases
- [x] ✅ `nlib.srf_tool_p4` (1005 lines) — green, 2681 cases
- [ ] 🟡 `nurbs.b_spline_curve_p3` (1187 lines) — partial, 2195 cases
- [x] ✅ `nlib.srf_adv_p4` (2228 lines) — green, 3155 cases
- [x] ✅ `nlib.srf_adv_p1` (1188 lines) — green
- [x] ✅ `nlib.srf_adv_p5` (1970 lines) — green, 3073 cases
- [x] ✅ `nlib.srf_adv_p6` (2214 lines) — green, 1016 cases
- [x] ✅ `nlib.srf_offset` (613 lines) — green, 424 cases
- [ ] 🟡 `nurbs.b_spline_surface_n_lib_p1` (2033 lines) — partial, 665 cases
- [ ] 🟡 `nurbs.b_spline_surface_n_lib_p2` (1932 lines) — partial, 654 cases
- [ ] 🟡 `nurbs.b_spline_surface_p2` (1338 lines) — partial, 763 cases

### curve (24/25)

- [x] ✅ `curve.hermite_curve` (383 lines) — green, 2131 cases
- [x] ✅ `curve.curve_p2` (2302 lines) — green, 3021 cases
- [x] ✅ `curve.conic` (859 lines) — green, 3240 cases
- [ ] 🟡 `curve.crv_in_volume` (949 lines) — partial, 2030 cases
- [x] ✅ `curve.curve_cache` (1474 lines) — green, 638 cases
- [x] ✅ `curve.hyperbola` (468 lines) — green, 2062 cases
- [ ] 🟡 `curve.iso_curve` (310 lines) — partial, 2874 cases
- [x] ✅ `curve.line` (1884 lines) — green, 2103 cases
- [x] ✅ `curve.offset_curve` (492 lines) — green, 2808 cases
- [x] ✅ `curve.parabola` (441 lines) — green, 3044 cases
- [x] ✅ `curve.projected_curve` (569 lines) — green, 2727 cases
- [ ] 🟡 `curve.tangent_field` (319 lines) — partial, 655 cases
- [x] ✅ `curve.ellipse_p2` (1916 lines) — green, 2102 cases
- [ ] 🟡 `curve.curve_p4` (2377 lines) — partial, 3259 cases
- [ ] 🟡 `curve.curve_p3` (2072 lines) — partial, 3076 cases
- [ ] 🟡 `curve.curve_p1` (2097 lines) — partial, 3083 cases
- [x] ✅ `curve.curve_p6` (2376 lines) — green, 3136 cases
- [ ] 🟡 `curve.crv_on_surf` (1728 lines) — partial, 2094 cases
- [x] ✅ `curve.curve_p7` (1666 lines) — green, 3027 cases
- [x] ✅ `curve.ellipse_p1` (1930 lines) — green, 2028 cases
- [x] ✅ `curve.curve_p8` (1670 lines) — green, 3058 cases
- [x] ✅ `curve.composite_curve_p1` (1700 lines) — green, 3093 cases
- [x] ✅ `curve.composite_curve_p2` (1345 lines) — green, 661 cases
- [ ] 🟡 `curve.curve_p5` (2299 lines) — partial, 3097 cases
- [ ] ▶️ `curve.curve_p9` (1783 lines) — ready

### surface (6/40)

- [ ] 🟡 `surface.volume_p1` (2014 lines) — partial, 2886 cases
- [x] ✅ `surface.b_spline_volume` (1226 lines) — green, 2333 cases
- [ ] 🟡 `surface.twist_volume` (649 lines) — partial, 1572 cases
- [ ] 🟡 `surface.surface_p2` (2398 lines) — partial, 869 cases
- [ ] 🟡 `surface.curve_bounded_surface` (324 lines) — partial, 535 cases
- [ ] 🟡 `surface.surface_cache_p1` (1944 lines) — partial, 872 cases
- [ ] ▶️ `surface.plane_p1` (1934 lines) — ready
- [ ] ▶️ `surface.surf_of_revolution_p1` (1522 lines) — ready
- [ ] 🔨 `surface.surface_p9` (1252 lines) — in progress: #6 @0xlillith (draft)
- [ ] 🔨 `surface.trim_srf_cache` (966 lines) — in progress: #4 @0xlillith (ready for review)
- [ ] ⏳ `surface.b_spline_volume_n_lib` (310 lines) — waiting
- [ ] ⏳ `surface.bend_volume` (945 lines) — waiting
- [ ] ⏳ `surface.cone_p1` (1835 lines) — waiting
- [ ] ⏳ `surface.surf_of_extrusion_p1` (1865 lines) — waiting
- [ ] ⏳ `surface.surf_of_revolution_p3` (1620 lines) — waiting
- [ ] ⏳ `surface.surface_cache_p2` (1245 lines) — waiting
- [ ] ⏳ `surface.torus` (1180 lines) — waiting
- [ ] ⏳ `surface.unbend_volume` (920 lines) — waiting
- [ ] ⏳ `surface.cone_p2` (1656 lines) — waiting
- [ ] ⏳ `surface.surf_of_revolution_p2` (1369 lines) — waiting
- [ ] ⏳ `surface.surface_cache_p3` (1703 lines) — waiting
- [ ] ⏳ `surface.volume_p2` (1490 lines) — waiting
- [ ] ⏳ `surface.surface_p7` (2406 lines) — waiting
- [ ] ⏳ `surface.plane_p2` (1627 lines) — waiting
- [ ] ⏳ `surface.surface_p4` (2270 lines) — waiting
- [ ] ⏳ `surface.sphere` (2861 lines) — waiting
- [ ] ⏳ `surface.surf_of_extrusion_p2` (1420 lines) — waiting
- [ ] ⏳ `surface.surface_p5` (2321 lines) — waiting
- [ ] ⏳ `surface.cone_p3` (1939 lines) — waiting
- [ ] ⏳ `surface.surface_p1` (2458 lines) — waiting
- [ ] ⏳ `surface.surface_p6` (2150 lines) — waiting
- [ ] ⏳ `surface.srf_in_volume` (779 lines) — waiting
- [ ] ⏳ `surface.surface_p10` (2172 lines) — waiting
- [ ] ⏳ `surface.surface_p3` (1794 lines) — waiting
- [ ] ⏳ `surface.offset_surface` (1787 lines) — waiting
- [ ] ⏳ `surface.surface_p11` (1789 lines) — waiting
- [ ] ⏳ `surface.surface_p8` (2374 lines) — waiting
- [ ] ⏳ `surface.surface_point_solve_p1` (1771 lines) — waiting
- [ ] ⏳ `surface.surface_point_solve_p2` (1882 lines) — waiting
- [ ] ⏳ `surface.surface_point_solve_p3` (1282 lines) — waiting

### topology (2/31)

- [x] ✅ `topology.database_io` (363 lines) — green, 2059 cases
- [x] ✅ `topology.topology` (389 lines) — green, 657 cases
- [ ] ⏳ `topology.brep_data` (1466 lines) — waiting
- [ ] ⏳ `topology.proto_topology_p1` (2746 lines) — waiting
- [ ] ⏳ `topology.edgeuse_p1` (1157 lines) — waiting
- [ ] ⏳ `topology.face_p2` (1827 lines) — waiting
- [ ] ⏳ `topology.edgeuse_p2` (600 lines) — waiting
- [ ] ⏳ `topology.shell` (750 lines) — waiting
- [ ] ⏳ `topology.vertexuse` (1429 lines) — waiting
- [ ] ⏳ `topology.vertex` (824 lines) — waiting
- [ ] ⏳ `topology.loopuse` (892 lines) — waiting
- [ ] ⏳ `topology.brep_p8` (1768 lines) — waiting
- [ ] ⏳ `topology.edge` (2213 lines) — waiting
- [ ] ⏳ `topology.edgeuse_p3` (2273 lines) — waiting
- [ ] ⏳ `topology.trimming_tools` (2878 lines) — waiting
- [ ] ⏳ `topology.face_p4` (1812 lines) — waiting
- [ ] ⏳ `topology.brep_p10` (1890 lines) — waiting
- [ ] ⏳ `topology.face_p3` (1881 lines) — waiting
- [ ] ⏳ `topology.topology_traverser` (784 lines) — waiting
- [ ] ⏳ `topology.brep_p7` (2503 lines) — waiting
- [ ] ⏳ `topology.face_p1` (2171 lines) — waiting
- [ ] ⏳ `topology.brep_p1` (2454 lines) — waiting
- [ ] ⏳ `topology.brep_p6` (1479 lines) — waiting
- [ ] ⏳ `topology.brep_cache` (389 lines) — waiting
- [ ] ⏳ `topology.brep_p4` (2113 lines) — waiting
- [ ] ⏳ `topology.brep_p9` (2443 lines) — waiting
- [ ] ⏳ `topology.brep_p2` (2443 lines) — waiting
- [ ] ⏳ `topology.brep_p5` (2193 lines) — waiting
- [ ] ⏳ `topology.brep_p3` (2503 lines) — waiting
- [ ] ⏳ `topology.proto_topology_p2` (2091 lines) — waiting
- [ ] ⏳ `topology.proto_topology_p3` (1577 lines) — waiting

### intersect (0/25)

- [ ] ⏳ `intersect.surface_tracer` (1900 lines) — waiting
- [ ] ⏳ `intersect.point_class` (909 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p1` (2087 lines) — waiting
- [ ] ⏳ `intersect.surface_drop_lib_p1` (1478 lines) — waiting
- [ ] ⏳ `intersect.surface_silhouette` (979 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p7` (2248 lines) — waiting
- [ ] ⏳ `intersect.ray_tracer` (1246 lines) — waiting
- [ ] ⏳ `intersect.surface_drop_curve` (2014 lines) — waiting
- [ ] ⏳ `intersect.surface_drop_lib_p2` (1080 lines) — waiting
- [ ] ⏳ `intersect.surface_intersector_p1` (1816 lines) — waiting
- [ ] ⏳ `intersect.topology_solver_p1` (1838 lines) — waiting
- [ ] ⏳ `intersect.adv_surface_intersector` (1646 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p2` (1928 lines) — waiting
- [ ] ⏳ `intersect.surface_drop_lib_p3` (2062 lines) — waiting
- [ ] ⏳ `intersect.surface_intersector_p2` (1677 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p3` (1986 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p4` (2022 lines) — waiting
- [ ] ⏳ `intersect.surface_intersector_p3` (1932 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p6` (2213 lines) — waiting
- [ ] ⏳ `intersect.surface_intersector_p4` (1570 lines) — waiting
- [ ] ⏳ `intersect.topology_solver_p2` (1391 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p5` (2017 lines) — waiting
- [ ] ⏳ `intersect.surface_intersector_p5` (622 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p8` (1900 lines) — waiting
- [ ] ⏳ `intersect.curve_class_p9` (1220 lines) — waiting

### primitive (1/5)

- [x] ✅ `primitive.api_curves` (649 lines) — green, 1629 cases
- [ ] ⏳ `primitive.primitive_creation_p1` (2322 lines) — waiting
- [ ] ⏳ `primitive.primitive_creation_p3` (1842 lines) — waiting
- [ ] ⏳ `primitive.primitive_creation_p2` (2258 lines) — waiting
- [ ] ⏳ `primitive.api_primitives` (885 lines) — waiting

### query (0/10)

- [ ] ⏳ `query.edge` (1025 lines) — waiting
- [ ] ⏳ `query.loop` (830 lines) — waiting
- [ ] ⏳ `query.face_p2` (1161 lines) — waiting
- [ ] ⏳ `query.face_p3` (1620 lines) — waiting
- [ ] ⏳ `query.poly` (2119 lines) — waiting
- [ ] ⏳ `query.face_p1` (465 lines) — waiting
- [ ] ⏳ `query.face_p4` (1432 lines) — waiting
- [ ] ⏳ `query.tess` (838 lines) — waiting
- [ ] ⏳ `query.brep` (517 lines) — waiting
- [ ] ⏳ `query.api_queries` (809 lines) — waiting

### mass (0/1)

- [ ] ⏳ `mass.integrator` (1031 lines) — waiting

### sweep (0/5)

- [ ] ⏳ `sweep.topology_sweep` (2456 lines) — waiting
- [ ] ⏳ `sweep.rotational_sweep_geometry` (1542 lines) — waiting
- [ ] ⏳ `sweep.trans_sweep_geometry` (817 lines) — waiting
- [ ] ⏳ `sweep.api_brep` (529 lines) — waiting
- [ ] ⏳ `sweep.api_surfaces` (381 lines) — waiting

### heal (0/7)

- [ ] ⏳ `heal.gap` (1475 lines) — waiting
- [ ] ⏳ `heal.face_props` (1410 lines) — waiting
- [ ] ⏳ `heal.heal_data_p2` (2160 lines) — waiting
- [ ] ⏳ `heal.heal_data_p1` (922 lines) — waiting
- [ ] ⏳ `heal.stitch_p1` (2037 lines) — waiting
- [ ] ⏳ `heal.heal_data_p3` (1967 lines) — waiting
- [ ] ⏳ `heal.stitch_p2` (1906 lines) — waiting

### offset (0/3)

- [ ] ⏳ `offset.offset_geometry_creation` (2652 lines) — waiting
- [ ] ⏳ `offset.offset_executive_p1` (2107 lines) — waiting
- [ ] ⏳ `offset.offset_executive_p2` (1846 lines) — waiting

### boolean (0/9)

- [ ] ⏳ `boolean.topology_intersector_p1` (2491 lines) — waiting
- [ ] ⏳ `boolean.topology_intersector_p2` (2195 lines) — waiting
- [ ] ⏳ `boolean.topology_intersector_p3` (1849 lines) — waiting
- [ ] ⏳ `boolean.merge_p1` (993 lines) — waiting
- [ ] ⏳ `boolean.merge_p2` (1967 lines) — waiting
- [ ] ⏳ `boolean.merge_p5` (475 lines) — waiting
- [ ] ⏳ `boolean.merge_p4` (2590 lines) — waiting
- [ ] ⏳ `boolean.merge_p3` (1152 lines) — waiting
- [ ] ⏳ `boolean.api_brep` (410 lines) — waiting

### blend (0/22)

- [ ] ⏳ `blend.fillet_custom_solver` (1236 lines) — waiting
- [ ] ⏳ `blend.fillet_geom_p1` (2113 lines) — waiting
- [ ] ⏳ `blend.fillet_solver_p1` (1861 lines) — waiting
- [ ] ⏳ `blend.hcr` (1875 lines) — waiting
- [ ] ⏳ `blend.fillet_intersector` (1708 lines) — waiting
- [ ] ⏳ `blend.fillet_solver_p5` (848 lines) — waiting
- [ ] ⏳ `blend.fillet_standard_solver_p1` (2336 lines) — waiting
- [ ] ⏳ `blend.fillet_solver_p4` (1404 lines) — waiting
- [ ] ⏳ `blend.fillet_executive_p3` (1608 lines) — waiting
- [ ] ⏳ `blend.fillet_executive_p1` (2063 lines) — waiting
- [ ] ⏳ `blend.fillet_corner_p1` (1971 lines) — waiting
- [ ] ⏳ `blend.fillet_solver_p2` (1939 lines) — waiting
- [ ] ⏳ `blend.fillet_solver_p3` (1112 lines) — waiting
- [ ] ⏳ `blend.fillet_geom_p3` (1032 lines) — waiting
- [ ] ⏳ `blend.fillet_geom_p4` (2074 lines) — waiting
- [ ] ⏳ `blend.fillet_geom_p6` (1813 lines) — waiting
- [ ] ⏳ `blend.fillet_standard_solver_p2` (1989 lines) — waiting
- [ ] ⏳ `blend.fillet_corner_p3` (1697 lines) — waiting
- [ ] ⏳ `blend.fillet_executive_p2` (1152 lines) — waiting
- [ ] ⏳ `blend.fillet_geom_p2` (2077 lines) — waiting
- [ ] ⏳ `blend.fillet_geom_p5` (1952 lines) — waiting
- [ ] ⏳ `blend.fillet_corner_p2` (1892 lines) — waiting

### (crate-wide) (0/1)

- [ ] ⏳ `audit.recursion` (0 lines) — waiting
