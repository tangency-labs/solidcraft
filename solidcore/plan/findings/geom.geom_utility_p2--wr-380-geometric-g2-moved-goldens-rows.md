# geometric_g2_continuity retired: every moved goldens row

Each row's old value (at the merge base, f760fd4400) and new value, and why: the continuity calls whose G2 verdict changed, measured from both sides' first and second derivatives (Frenet curvature `|P' x P''| / |P'|^3`; the geometric test's gap is `1e-7 * (1 + max curvature)`). Upstream's test compared the raw second-derivative lengths against an absolute 1e-7 and the first normal with itself (`WR-380--evaluate-curve-continuity-g2-normal-and-curvature.md`).

68 rows in 7 units.

## geom.geom_utility_p2 (41)

| case | old | new | why |
|---|---|---|---|
| cont_c1g2 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 2): not G2 |
| cont_g1g2 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 7.5e-01 relative (curvature 2): not G2 |
| cont_kink_wide_angle | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 1.5e-04 relative (curvature 2): not G2 |
| gen0004 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 6.4e-01 relative (curvature 0.1258): not G2 |
| gen0098 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 6.6e-01 relative (curvature 0.02811): not G2 |
| gen0160 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 4.9e-01 relative (curvature 7.178): not G2 |
| gen0203 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 7.2e-01 relative (curvature 0.2302): not G2 |
| gen0294 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 25.9): not G2 |
| gen0300 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.07181): not G2 |
| gen0324 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.1846): not G2 |
| gen0375 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 3.5e-01 relative (curvature 0.5607): not G2 |
| gen0542 | G1G2 | G1 | 1 join(s) with mirrored normals (curvature 0.1512): not G2 |
| gen0661 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 2.379): not G2 |
| gen0721 | C1C2 | C1 | 1 join(s) whose curvatures differ by up to 1.3e-06 relative (curvature 8.849): not G2 |
| gen0845 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 6.0e-01 relative (curvature 202.7): not G2 |
| gen0959 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 2.5e-01 relative (curvature 122.4): not G2 |
| gen1100 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 2.7e-01 relative (curvature 4.58e-06): not G2 |
| gen1142 | C1G2G3 | C1 | 1 join(s) with mirrored normals (curvature 5.714e-06): not G2 |
| gen1182 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 81.47): not G2 |
| gen1190 | C1G2G3 | C1 | 1 join(s) with mirrored normals (curvature 0.002449): not G2 |
| gen1207 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 8.1e-02 relative (curvature 2.692e-05): not G2 |
| gen1318 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 2.0e-05 relative (curvature 0.1015): not G2 |
| gen1417 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.2342): not G2 |
| gen1499 | C1 | C1G2 | 1 join(s) with equal curvature (8.958e-06) and normal: G2 (upstream compared the lengths of P'', which differ) |
| gen1541 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 6.1e-01 relative (curvature 0.04313): not G2 |
| gen1611 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 4.5e-01 relative (curvature 1.025e-05): not G2 |
| gen1684 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 2.8e-01 relative (curvature 0.1295): not G2 |
| gen1775 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 4.5e-04 relative (curvature 0.2163): not G2 |
| gen1868 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.006142): not G2 |
| gen2052 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 1.538e-05): not G2 |
| gen2119 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.8054): not G2 |
| gen2331 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 3.067e-05): not G2 |
| gen2455 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 1.597e-05): not G2 |
| gen2495 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 1.367e-05): not G2 |
| gen2498 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.01725): not G2 |
| gen2635 | G1G2 | G1 | 1 join(s) with mirrored normals (curvature 193.6): not G2 |
| gen2716 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.3787): not G2 |
| gen2749 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.04714): not G2 |
| gen2902 | G1G2 | G1 | 1 join(s) whose curvatures differ by up to 9.1e-03 relative (curvature 1.652e-05): not G2 |
| gen3053 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 0.01783): not G2 |
| gen3142 | C1G2 | C1 | 1 join(s) with mirrored normals (curvature 33.07): not G2 |

## surface.surface_p6 (14)

| case | old | new | why |
|---|---|---|---|
| gen0049 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 94 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0119 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 92 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0236 | values.out [0, 3, 4, 1, 3, 11, 1] | [0, 6, 4, 1, 6, 11, 1] | 89 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0437 | values.out [0, 7, 4, 1, 7, 7, 1] | [0, 9, 4, 1, 9, 9, 1] | 94 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0705 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 94 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0725 | values.out [0, 3, 4, 1, 3, 11, 1] | [0, 6, 4, 1, 6, 11, 1] | 89 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0814 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 92 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1019 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 92 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1029 | values.out [0, 3, 4, 1, 3, 11, 1] | [0, 6, 4, 1, 6, 11, 1] | 89 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1453 | values.out [0, 3, 4, 1, 3, 11, 1] | [0, 6, 4, 1, 6, 11, 1] | 89 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1497 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 92 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1520 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 92 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1808 | values.out [0, 3, 4, 1, 3, 7, 1] | [0, 6, 4, 1, 6, 9, 1] | 94 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen1962 | values.out [0, 7, 4, 1, 7, 7, 1] | [0, 9, 4, 1, 9, 9, 1] | 92 straight join(s), curvatures at most 1.8e-10: G2 (upstream compared the lengths of P'', which differ along the tangent) |

## surface.surface_cache_p3 (5)

| case | old | new | why |
|---|---|---|---|
| gen0119 | values.out 1 blocks, first [179.5337, 344.5705, 23088.6121] | 1 blocks, first [187.5337, 352.5705, 23784.6121] | 3 straight join(s), curvatures at most 2.8e-15: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0129 | values.out 4088 blocks, first [332823.9101, 332985.1265, 43900556.8359] | 4088 blocks, first [332839.9101, 333001.1265, 43902604.8359] | 8 straight join(s), curvatures at most 3.6e-15: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0390 | values.out 2 blocks, first [185.0032, 356.9119, 21147.7094] | 2 blocks, first [197.0032, 368.9119, 22683.7094] | 3 straight join(s), curvatures at most 1.5e-14: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0501 | values.out 3 blocks, first [173.4243, 354.4367, 19331.8095] | 3 blocks, first [197.4243, 378.4367, 22403.8095] | 8 straight join(s), curvatures at most 1.8e-16: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0598 | values.out 3 blocks, first [244.1431, 408.0104, 22822.9132] | 3 blocks, first [256.1431, 420.0104, 24360.9132] | 3 straight join(s), curvatures at most 1.4e-15: G2 (upstream compared the lengths of P'', which differ along the tangent) |

## curve.curve_cache (4)

| case | old | new | why |
|---|---|---|---|
| gen0033 | values.out 5 blocks, first [965.7005, 2665.6906, 151395.5229] | 5 blocks, first [971.7005, 2671.6906, 152772.5229] | 3 straight join(s), curvatures at most 1.1e-17: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0080 | values.out 1 blocks, first [425.0837, 742.2305, 49095.8914] | 1 blocks, first [437.0837, 754.2305, 49989.8914] | 2 straight join(s), curvatures at most 2.0e-15: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0119 | values.out 2 blocks, first [-609.9636, 11114.3369, -135561.4685] | 2 blocks, first [-585.9636, 11138.3369, -131725.4685] | 4 straight join(s), curvatures at most 5.2e-17: G2 (upstream compared the lengths of P'', which differ along the tangent) |
| gen0205 | values.out 2 blocks, first [1099.2996, 13071.1314, 12811.4465] | 2 blocks, first [1126.2996, 13098.1314, 16377.4465] | 4 straight join(s), curvatures at most 1.6e-17: G2 (upstream compared the lengths of P'', which differ along the tangent) |

## surface.surface_p1 (1)

| case | old | new | why |
|---|---|---|---|
| domhist0077 | values.out [0, 0.263413, 1, 0.883531] | [0, 0, 1, 1] | 12 straight join(s), curvatures at most 1.6e-14: G2 (upstream compared the lengths of P'', which differ along the tangent) |

## topology.brep_p8 (1)

| case | old | new | why |
|---|---|---|---|
| faces_from_surface_crease_v_7 | values.out 4 blocks, first [401067.0, 401067.0, 54916507.0] | 6 blocks, first [401044.0, 401044.0, 54872675.0] | 3 join(s) with mirrored normals (curvature 9.445): not G2 |

## primitive.primitive_creation_p2 (2)

| case | old | new | why |
|---|---|---|---|
| gen0150 | values.record[0] 1006 | 1001 | 3 join(s) whose curvatures differ by up to 3.3e-06 relative (curvature 1e+06): not G2 |
| ub215_partial_torus_tiny_circle_far_origin | values.record[0] 1006 | 1001 | 3 join(s) whose curvatures differ by up to 3.3e-06 relative (curvature 1e+06): not G2 |

