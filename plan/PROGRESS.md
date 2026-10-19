# Port progress

Generated from `plan/modules.toml` and `reports/difftest/*.json` by `harness/plan/progress.py`; do not edit by hand.

**224 / 261 units ported** (114 green, 110 partial), **324,402 / 387,903 upstream lines (83.6%)**. 37 claimed, 0 blocked, 0 todo.

Legend: ✅ green (identical to C++) · 🟡 partial (passes; some cases wait on unported code) · 🔨 claimed · ▶️ ready (all dependencies done) · ⏳ claimable, dependencies still missing · ⛔ blocked.

## Ready to claim now

_None right now; see the next section._

## Claimable now, dependencies still missing

Anything `todo` can be claimed. These need stubs for, or co-porting of, the listed dependencies (AGENTS.md section 4).


## In progress

- [ ] 🔨 `intersect.surface_drop_lib_p3` (2062 lines) — in progress: #136 @froglemonade2 (ready for review)
- [ ] 🔨 `topology.brep_p8` (1768 lines) — in progress: #72 @bherbruck (ready for review)
- [ ] 🔨 `topology.face_p4` (1812 lines) — in progress: #134 @bherbruck (draft)
- [ ] 🔨 `query.face_p3` (1620 lines) — in progress: #132 @bherbruck (ready for review)
- [ ] 🔨 `query.face_p1` (465 lines) — in progress: #238 @denelson1-dot (ready for review)
- [ ] 🔨 `query.face_p4` (1432 lines) — in progress: #129 @bherbruck (ready for review)
- [ ] 🔨 `topology.brep_p4` (2113 lines) — in progress: #173 @froglemonade2 (draft)
- [ ] 🔨 `topology.brep_p2` (2443 lines) — in progress: #149 @bluecmd (ready for review)
- [ ] 🔨 `query.brep` (517 lines) — in progress: #101 @0xlillith (draft)
- [ ] 🔨 `topology.brep_p3` (2503 lines) — in progress: #204 @bluecmd (draft)
- [ ] 🔨 `blend.fillet_solver_p1` (1861 lines) — in progress: #192 @froglemonade2 (ready for review)
- [ ] 🔨 `primitive.primitive_creation_p1` (2322 lines) — in progress: #213 @damgru (ready for review)
- [ ] 🔨 `blend.fillet_solver_p5` (848 lines) — in progress: #208 @damgru (ready for review)
- [ ] 🔨 `blend.fillet_solver_p4` (1404 lines) — in progress: #212 @blackveilprecision (ready for review)
- [ ] 🔨 `sweep.topology_sweep` (2456 lines) — in progress: #222 @damgru (ready for review)
- [ ] 🔨 `boolean.topology_intersector_p3` (1849 lines) — in progress: #211 @oursland (draft)
- [ ] 🔨 `primitive.primitive_creation_p3` (1842 lines) — in progress: #233 @damgru (ready for review)
- [ ] 🔨 `blend.fillet_executive_p3` (1608 lines) — in progress: #244 @bluecmd (ready for review)
- [ ] 🔨 `boolean.merge_p2` (1967 lines) — in progress: #229 @Pluto-XI (ready for review)
- [ ] 🔨 `boolean.merge_p4` (2590 lines) — in progress: #243 @bluecmd (ready for review)
- [ ] 🔨 `boolean.merge_p3` (1152 lines) — in progress: #245 @bluecmd (ready for review)
- [ ] 🔨 `blend.fillet_executive_p1` (2063 lines) — in progress: #260 @H1D (draft)
- [ ] 🔨 `primitive.primitive_creation_p2` (2258 lines) — in progress: #250 @CamWight (ready for review)
- [ ] 🔨 `blend.fillet_corner_p1` (1971 lines) — in progress: #266 @bluecmd (draft)
- [ ] 🔨 `blend.fillet_solver_p2` (1939 lines) — in progress: #249 @froglemonade2 (draft)
- [ ] 🔨 `blend.fillet_solver_p3` (1112 lines) — in progress: #236 @denelson1-dot (ready for review)
- [ ] 🔨 `blend.fillet_geom_p3` (1032 lines) — in progress: #242 @denelson1-dot (ready for review)
- [ ] 🔨 `blend.fillet_geom_p4` (2074 lines) — in progress: #252 @bherbruck (ready for review)
- [ ] 🔨 `blend.fillet_geom_p6` (1813 lines) — in progress: #247 @brett19 (ready for review)
- [ ] 🔨 `blend.fillet_standard_solver_p2` (1989 lines) — in progress: #265 @froglemonade2 (draft)
- [ ] 🔨 `offset.offset_executive_p2` (1846 lines) — in progress: #201 @altvnk (ready for review)
- [ ] 🔨 `blend.fillet_corner_p3` (1697 lines) — in progress: #256 @bherbruck (ready for review)
- [ ] 🔨 `blend.fillet_executive_p2` (1152 lines) — in progress: #267 @bluecmd (draft)
- [ ] 🔨 `blend.fillet_geom_p2` (2077 lines) — in progress: #263 @bherbruck (ready for review)
- [ ] 🔨 `blend.fillet_geom_p5` (1952 lines) — in progress: #264 @froglemonade2 (draft)
- [ ] 🔨 `blend.fillet_corner_p2` (1892 lines) — in progress: #268 @bluecmd (draft)
- [ ] 🔨 `audit.recursion` (0 lines) — in progress: #273 @Karl-VonRichter (draft)

## By module

### geom (29/29)

- [x] ✅ `geom.extent3d` (1122 lines) — green, 3086 cases
- [x] ✅ `geom.vector2d` (339 lines) — green, 4060 cases
- [x] ✅ `geom.attribute` (920 lines) — green, 2029 cases
- [x] ✅ `geom.tol` (923 lines) — green, 3440 cases
- [ ] 🟡 `geom.tree` (1259 lines) — partial, 618 cases
- [x] ✅ `geom.assert_array` (373 lines) — green, 3734 cases
- [x] ✅ `geom.context` (355 lines) — green, 2013 cases
- [x] ✅ `geom.extent1d` (974 lines) — green, 3091 cases
- [x] ✅ `geom.matrix` (816 lines) — green, 4044 cases
- [x] ✅ `geom.periodic_extent1d` (406 lines) — green, 4054 cases
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
- [x] ✅ `geom.polar_box` (979 lines) — green, 3030 cases
- [ ] 🟡 `geom.solution_array` (1319 lines) — partial, 810 cases
- [ ] 🟡 `geom.global_solver` (1775 lines) — partial, 620 cases
- [x] ✅ `geom.pseudo_box` (1508 lines) — green, 3011 cases
- [ ] 🟡 `geom.tools` (326 lines) — partial, 913 cases
- [x] ✅ `geom.transform` (681 lines) — green, 3042 cases
- [ ] 🟡 `geom.a_object` (935 lines) — partial, 2040 cases
- [x] ✅ `geom.point_set` (694 lines) — green, 2029 cases

### nurbs (48/48)

- [x] ✅ `nlib.knots` (768 lines) — green, 2953 cases
- [x] ✅ `nlib.math` (2971 lines) — green, 3076 cases
- [x] ✅ `nlib.geom` (1244 lines) — green, 3173 cases
- [x] ✅ `nlib.basis_basic` (786 lines) — green, 2596 cases
- [x] ✅ `nlib.srf_basic` (1088 lines) — green, 3220 cases
- [x] ✅ `nlib.basis_adv` (361 lines) — green, 1751 cases
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
- [x] ✅ `nurbs.b_spline_surface_p1` (1790 lines) — green, 2120 cases
- [x] ✅ `nlib.srf_tool_p4` (1005 lines) — green, 2681 cases
- [ ] 🟡 `nurbs.b_spline_curve_p3` (1187 lines) — partial, 2195 cases
- [x] ✅ `nlib.srf_adv_p4` (2228 lines) — green, 3155 cases
- [x] ✅ `nlib.srf_adv_p1` (1188 lines) — green, 2886 cases
- [x] ✅ `nlib.srf_adv_p5` (1970 lines) — green, 3073 cases
- [x] ✅ `nlib.srf_adv_p6` (2214 lines) — green, 1016 cases
- [x] ✅ `nlib.srf_offset` (613 lines) — green, 424 cases
- [x] ✅ `nurbs.b_spline_surface_n_lib_p1` (2033 lines) — green, 665 cases
- [ ] 🟡 `nurbs.b_spline_surface_n_lib_p2` (1932 lines) — partial, 654 cases
- [ ] 🟡 `nurbs.b_spline_surface_p2` (1338 lines) — partial, 764 cases

### curve (25/25)

- [x] ✅ `curve.hermite_curve` (383 lines) — green, 2131 cases
- [x] ✅ `curve.curve_p2` (2302 lines) — green, 3021 cases
- [x] ✅ `curve.conic` (859 lines) — green, 3240 cases
- [ ] 🟡 `curve.crv_in_volume` (949 lines) — partial, 2030 cases
- [x] ✅ `curve.curve_cache` (1474 lines) — green, 638 cases
- [x] ✅ `curve.hyperbola` (468 lines) — green, 2062 cases
- [x] ✅ `curve.iso_curve` (310 lines) — green, 2874 cases
- [x] ✅ `curve.line` (1884 lines) — green, 2103 cases
- [x] ✅ `curve.offset_curve` (492 lines) — green, 2808 cases
- [x] ✅ `curve.parabola` (441 lines) — green, 3044 cases
- [x] ✅ `curve.projected_curve` (569 lines) — green, 2727 cases
- [x] ✅ `curve.tangent_field` (319 lines) — green, 655 cases
- [x] ✅ `curve.ellipse_p2` (1916 lines) — green, 2102 cases
- [ ] 🟡 `curve.curve_p4` (2377 lines) — partial, 3259 cases
- [x] ✅ `curve.curve_p3` (2072 lines) — green, 3076 cases
- [ ] 🟡 `curve.curve_p1` (2097 lines) — partial, 3083 cases
- [x] ✅ `curve.curve_p6` (2376 lines) — green, 3136 cases
- [ ] 🟡 `curve.crv_on_surf` (1728 lines) — partial, 2094 cases
- [x] ✅ `curve.curve_p7` (1666 lines) — green, 3027 cases
- [x] ✅ `curve.ellipse_p1` (1930 lines) — green, 2028 cases
- [x] ✅ `curve.curve_p8` (1670 lines) — green, 3058 cases
- [x] ✅ `curve.composite_curve_p1` (1700 lines) — green, 3093 cases
- [x] ✅ `curve.composite_curve_p2` (1345 lines) — green, 661 cases
- [ ] 🟡 `curve.curve_p5` (2299 lines) — partial, 3097 cases
- [x] ✅ `curve.curve_p9` (1783 lines) — green, 3093 cases

### surface (40/40)

- [x] ✅ `surface.volume_p1` (2014 lines) — green, 2886 cases
- [x] ✅ `surface.b_spline_volume` (1226 lines) — green, 2333 cases
- [x] ✅ `surface.twist_volume` (649 lines) — green, 1572 cases
- [ ] 🟡 `surface.surface_p2` (2398 lines) — partial, 869 cases
- [x] ✅ `surface.curve_bounded_surface` (324 lines) — green, 535 cases
- [x] ✅ `surface.surface_cache_p1` (1944 lines) — green, 872 cases
- [ ] 🟡 `surface.plane_p1` (1934 lines) — partial, 2705 cases
- [x] ✅ `surface.surf_of_revolution_p1` (1522 lines) — green, 858 cases
- [ ] 🟡 `surface.surface_p9` (1252 lines) — partial, 821 cases
- [ ] 🟡 `surface.trim_srf_cache` (966 lines) — partial, 621 cases
- [ ] 🟡 `surface.b_spline_volume_n_lib` (310 lines) — partial, 2420 cases
- [x] ✅ `surface.bend_volume` (945 lines) — green, 2347 cases
- [x] ✅ `surface.cone_p1` (1835 lines) — green, 20702 cases
- [x] ✅ `surface.surf_of_extrusion_p1` (1865 lines) — green, 2055 cases
- [ ] 🟡 `surface.surf_of_revolution_p3` (1620 lines) — partial, 26705 cases
- [ ] 🟡 `surface.surface_cache_p2` (1245 lines) — partial, 905 cases
- [x] ✅ `surface.torus` (1180 lines) — green, 563 cases
- [x] ✅ `surface.unbend_volume` (920 lines) — green, 1874 cases
- [x] ✅ `surface.cone_p2` (1656 lines) — green, 1550 cases
- [ ] 🟡 `surface.surf_of_revolution_p2` (1369 lines) — partial, 1167 cases
- [ ] 🟡 `surface.surface_cache_p3` (1703 lines) — partial, 600 cases
- [ ] 🟡 `surface.volume_p2` (1490 lines) — partial, 11220 cases
- [ ] 🟡 `surface.surface_p7` (2406 lines) — partial, 3660 cases
- [ ] 🟡 `surface.plane_p2` (1627 lines) — partial, 2218 cases
- [x] ✅ `surface.surface_p4` (2270 lines) — green, 3003 cases
- [x] ✅ `surface.sphere` (2861 lines) — green, 800 cases
- [ ] 🟡 `surface.surf_of_extrusion_p2` (1420 lines) — partial, 1373 cases
- [x] ✅ `surface.surface_p5` (2321 lines) — green, 3046 cases
- [ ] 🟡 `surface.cone_p3` (1939 lines) — partial, 1415 cases
- [ ] 🟡 `surface.surface_p1` (2458 lines) — partial, 806 cases
- [x] ✅ `surface.surface_p6` (2150 lines) — green, 2099 cases
- [ ] 🟡 `surface.srf_in_volume` (779 lines) — partial, 2036 cases
- [ ] 🟡 `surface.surface_p10` (2172 lines) — partial, 867 cases
- [x] ✅ `surface.surface_p3` (1794 lines) — green, 2500 cases
- [x] ✅ `surface.offset_surface` (1787 lines) — green, 3077 cases
- [ ] 🟡 `surface.surface_p11` (1789 lines) — partial, 600 cases
- [ ] 🟡 `surface.surface_p8` (2374 lines) — partial, 2054 cases
- [x] ✅ `surface.surface_point_solve_p1` (1771 lines) — green, 1035 cases
- [x] ✅ `surface.surface_point_solve_p2` (1882 lines) — green, 1345 cases
- [x] ✅ `surface.surface_point_solve_p3` (1282 lines) — green, 1365 cases

### topology (26/31)

- [x] ✅ `topology.database_io` (363 lines) — green, 2059 cases
- [x] ✅ `topology.topology` (389 lines) — green, 657 cases
- [ ] 🟡 `topology.brep_data` (1466 lines) — partial, 2379 cases
- [ ] 🟡 `topology.proto_topology_p1` (2746 lines) — partial, 1857 cases
- [ ] 🟡 `topology.edgeuse_p1` (1157 lines) — partial, 4402 cases
- [ ] 🟡 `topology.face_p2` (1827 lines) — partial, 2692 cases
- [ ] 🟡 `topology.edgeuse_p2` (600 lines) — partial, 4583 cases
- [ ] 🟡 `topology.shell` (750 lines) — partial, 911 cases
- [ ] 🟡 `topology.vertexuse` (1429 lines) — partial, 2686 cases
- [ ] 🟡 `topology.vertex` (824 lines) — partial, 5508 cases
- [ ] 🟡 `topology.loopuse` (892 lines) — partial, 8854 cases
- [ ] 🔨 `topology.brep_p8` (1768 lines) — in progress: #72 @bherbruck (ready for review)
- [ ] 🟡 `topology.edge` (2213 lines) — partial, 1787 cases
- [ ] 🟡 `topology.edgeuse_p3` (2273 lines) — partial, 4789 cases
- [ ] 🟡 `topology.trimming_tools` (2878 lines) — partial, 3263 cases
- [ ] 🔨 `topology.face_p4` (1812 lines) — in progress: #134 @bherbruck (draft)
- [ ] 🟡 `topology.brep_p10` (1890 lines) — partial, 1404 cases
- [ ] 🟡 `topology.face_p3` (1881 lines) — partial, 1781 cases
- [ ] 🟡 `topology.topology_traverser` (784 lines) — partial, 909 cases
- [ ] 🟡 `topology.brep_p7` (2503 lines) — partial, 316 cases
- [ ] 🟡 `topology.face_p1` (2171 lines) — partial, 2312 cases
- [ ] 🟡 `topology.brep_p1` (2454 lines) — partial, 2589 cases
- [ ] 🟡 `topology.brep_p6` (1479 lines) — partial, 498 cases
- [ ] 🟡 `topology.brep_cache` (389 lines) — partial, 663 cases
- [ ] 🔨 `topology.brep_p4` (2113 lines) — in progress: #173 @froglemonade2 (draft)
- [ ] 🟡 `topology.brep_p9` (2443 lines) — partial, 684 cases
- [ ] 🔨 `topology.brep_p2` (2443 lines) — in progress: #149 @bluecmd (ready for review)
- [ ] 🟡 `topology.brep_p5` (2193 lines) — partial, 797 cases
- [ ] 🔨 `topology.brep_p3` (2503 lines) — in progress: #204 @bluecmd (draft)
- [ ] 🟡 `topology.proto_topology_p2` (2091 lines) — partial, 1533 cases
- [ ] 🟡 `topology.proto_topology_p3` (1577 lines) — partial, 3013 cases

### intersect (24/25)

- [ ] 🟡 `intersect.surface_tracer` (1900 lines) — partial, 427 cases
- [ ] 🟡 `intersect.point_class` (909 lines) — partial, 2511 cases
- [ ] 🟡 `intersect.curve_class_p1` (2087 lines) — partial, 2125 cases
- [ ] 🟡 `intersect.surface_drop_lib_p1` (1478 lines) — partial, 1862 cases
- [x] ✅ `intersect.surface_silhouette` (979 lines) — green, 709 cases
- [ ] 🟡 `intersect.curve_class_p7` (2248 lines) — partial, 1516 cases
- [ ] 🟡 `intersect.ray_tracer` (1246 lines) — partial, 421 cases
- [ ] 🟡 `intersect.surface_drop_curve` (2014 lines) — partial, 547 cases
- [x] ✅ `intersect.surface_drop_lib_p2` (1080 lines) — green, 1531 cases
- [ ] 🟡 `intersect.surface_intersector_p1` (1816 lines) — partial, 1510 cases
- [ ] 🟡 `intersect.topology_solver_p1` (1838 lines) — partial, 3532 cases
- [ ] 🟡 `intersect.adv_surface_intersector` (1646 lines) — partial, 1931 cases
- [ ] 🟡 `intersect.curve_class_p2` (1928 lines) — partial, 1671 cases
- [ ] 🔨 `intersect.surface_drop_lib_p3` (2062 lines) — in progress: #136 @froglemonade2 (ready for review)
- [ ] 🟡 `intersect.surface_intersector_p2` (1677 lines) — partial, 1660 cases
- [ ] 🟡 `intersect.curve_class_p3` (1986 lines) — partial, 3292 cases
- [ ] 🟡 `intersect.curve_class_p4` (2022 lines) — partial, 3201 cases
- [ ] 🟡 `intersect.surface_intersector_p3` (1932 lines) — partial, 1527 cases
- [ ] 🟡 `intersect.curve_class_p6` (2213 lines) — partial, 4438 cases
- [ ] 🟡 `intersect.surface_intersector_p4` (1570 lines) — partial, 1542 cases
- [ ] 🟡 `intersect.topology_solver_p2` (1391 lines) — partial, 2429 cases
- [ ] 🟡 `intersect.curve_class_p5` (2017 lines) — partial, 1081 cases
- [ ] 🟡 `intersect.surface_intersector_p5` (622 lines) — partial, 1725 cases
- [ ] 🟡 `intersect.curve_class_p8` (1900 lines) — partial, 1127 cases
- [ ] 🟡 `intersect.curve_class_p9` (1220 lines) — partial, 428 cases

### primitive (2/5)

- [x] ✅ `primitive.api_curves` (649 lines) — green, 1629 cases
- [ ] 🔨 `primitive.primitive_creation_p1` (2322 lines) — in progress: #213 @damgru (ready for review)
- [ ] 🔨 `primitive.primitive_creation_p3` (1842 lines) — in progress: #233 @damgru (ready for review)
- [ ] 🔨 `primitive.primitive_creation_p2` (2258 lines) — in progress: #250 @CamWight (ready for review)
- [ ] 🟡 `primitive.api_primitives` (885 lines) — partial, 1981 cases

### query (6/10)

- [ ] 🟡 `query.edge` (1025 lines) — partial, 1368 cases
- [x] ✅ `query.loop` (830 lines) — green, 3040 cases
- [ ] 🟡 `query.face_p2` (1161 lines) — partial, 1344 cases
- [ ] 🔨 `query.face_p3` (1620 lines) — in progress: #132 @bherbruck (ready for review)
- [ ] 🟡 `query.poly` (2119 lines) — partial, 671 cases
- [ ] 🔨 `query.face_p1` (465 lines) — in progress: #238 @denelson1-dot (ready for review)
- [ ] 🔨 `query.face_p4` (1432 lines) — in progress: #129 @bherbruck (ready for review)
- [ ] 🟡 `query.tess` (838 lines) — partial, 676 cases
- [ ] 🔨 `query.brep` (517 lines) — in progress: #101 @0xlillith (draft)
- [ ] 🟡 `query.api_queries` (809 lines) — partial, 1203 cases

### mass (1/1)

- [ ] 🟡 `mass.integrator` (1031 lines) — partial, 1528 cases

### sweep (4/5)

- [ ] 🔨 `sweep.topology_sweep` (2456 lines) — in progress: #222 @damgru (ready for review)
- [ ] 🟡 `sweep.rotational_sweep_geometry` (1542 lines) — partial, 761 cases
- [ ] 🟡 `sweep.trans_sweep_geometry` (817 lines) — partial, 788 cases
- [ ] 🟡 `sweep.api_brep` (529 lines) — partial, 573 cases
- [ ] 🟡 `sweep.api_surfaces` (381 lines) — partial, 882 cases

### heal (7/7)

- [ ] 🟡 `heal.gap` (1475 lines) — partial, 445 cases
- [ ] 🟡 `heal.face_props` (1410 lines) — partial, 1528 cases
- [ ] 🟡 `heal.heal_data_p2` (2160 lines) — partial, 738 cases
- [ ] 🟡 `heal.heal_data_p1` (922 lines) — partial, 493 cases
- [ ] 🟡 `heal.stitch_p1` (2037 lines) — partial, 300 cases
- [ ] 🟡 `heal.heal_data_p3` (1967 lines) — partial, 474 cases
- [ ] 🟡 `heal.stitch_p2` (1906 lines) — partial, 556 cases

### offset (2/3)

- [ ] 🟡 `offset.offset_geometry_creation` (2652 lines) — partial, 861 cases
- [ ] 🟡 `offset.offset_executive_p1` (2107 lines) — partial, 693 cases
- [ ] 🔨 `offset.offset_executive_p2` (1846 lines) — in progress: #201 @altvnk (ready for review)

### boolean (5/9)

- [ ] 🟡 `boolean.topology_intersector_p1` (2491 lines) — partial, 827 cases
- [ ] 🟡 `boolean.topology_intersector_p2` (2195 lines) — partial, 841 cases
- [ ] 🔨 `boolean.topology_intersector_p3` (1849 lines) — in progress: #211 @oursland (draft)
- [ ] 🟡 `boolean.merge_p1` (993 lines) — partial, 248 cases
- [ ] 🔨 `boolean.merge_p2` (1967 lines) — in progress: #229 @Pluto-XI (ready for review)
- [ ] 🟡 `boolean.merge_p5` (475 lines) — partial, 140 cases
- [ ] 🔨 `boolean.merge_p4` (2590 lines) — in progress: #243 @bluecmd (ready for review)
- [ ] 🔨 `boolean.merge_p3` (1152 lines) — in progress: #245 @bluecmd (ready for review)
- [ ] 🟡 `boolean.api_brep` (410 lines) — partial, 284 cases

### blend (5/22)

- [ ] 🟡 `blend.fillet_custom_solver` (1236 lines) — partial, 448 cases
- [ ] 🟡 `blend.fillet_geom_p1` (2113 lines) — partial, 480 cases
- [ ] 🔨 `blend.fillet_solver_p1` (1861 lines) — in progress: #192 @froglemonade2 (ready for review)
- [ ] 🟡 `blend.hcr` (1875 lines) — partial, 434 cases
- [ ] 🟡 `blend.fillet_intersector` (1708 lines) — partial, 1286 cases
- [ ] 🔨 `blend.fillet_solver_p5` (848 lines) — in progress: #208 @damgru (ready for review)
- [ ] 🟡 `blend.fillet_standard_solver_p1` (2336 lines) — partial, 3839 cases
- [ ] 🔨 `blend.fillet_solver_p4` (1404 lines) — in progress: #212 @blackveilprecision (ready for review)
- [ ] 🔨 `blend.fillet_executive_p3` (1608 lines) — in progress: #244 @bluecmd (ready for review)
- [ ] 🔨 `blend.fillet_executive_p1` (2063 lines) — in progress: #260 @H1D (draft)
- [ ] 🔨 `blend.fillet_corner_p1` (1971 lines) — in progress: #266 @bluecmd (draft)
- [ ] 🔨 `blend.fillet_solver_p2` (1939 lines) — in progress: #249 @froglemonade2 (draft)
- [ ] 🔨 `blend.fillet_solver_p3` (1112 lines) — in progress: #236 @denelson1-dot (ready for review)
- [ ] 🔨 `blend.fillet_geom_p3` (1032 lines) — in progress: #242 @denelson1-dot (ready for review)
- [ ] 🔨 `blend.fillet_geom_p4` (2074 lines) — in progress: #252 @bherbruck (ready for review)
- [ ] 🔨 `blend.fillet_geom_p6` (1813 lines) — in progress: #247 @brett19 (ready for review)
- [ ] 🔨 `blend.fillet_standard_solver_p2` (1989 lines) — in progress: #265 @froglemonade2 (draft)
- [ ] 🔨 `blend.fillet_corner_p3` (1697 lines) — in progress: #256 @bherbruck (ready for review)
- [ ] 🔨 `blend.fillet_executive_p2` (1152 lines) — in progress: #267 @bluecmd (draft)
- [ ] 🔨 `blend.fillet_geom_p2` (2077 lines) — in progress: #263 @bherbruck (ready for review)
- [ ] 🔨 `blend.fillet_geom_p5` (1952 lines) — in progress: #264 @froglemonade2 (draft)
- [ ] 🔨 `blend.fillet_corner_p2` (1892 lines) — in progress: #268 @bluecmd (draft)

### (crate-wide) (0/1)

- [ ] 🔨 `audit.recursion` (0 lines) — in progress: #273 @Karl-VonRichter (draft)
