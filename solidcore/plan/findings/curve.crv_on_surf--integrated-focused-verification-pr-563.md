# curve.crv_on_surf: integrated focused verification (PR #563)

After merging authoritative main `9fd1c67c5`, the focused Linux ARM64 run reports:
`curve.crv_on_surf` 2107 pass / 0 deferred / 0 failed; `curve.curve_p2` 3029/0/0;
`query.face_p2` 1326/18/0; `topology.face_p4` 5426/80/0; `heal.face_props`
1431/97/0; `intersect.curve_class_p4` 2918/283/0. All their compared values are
bit-identical. CrvOnSurf sensitivity detects 2107/2107 perturbations and the two
new healer fixtures detect 2/2. Neither gen0125 nor gen0164 is deferred.

Against main's CI reports (run 39923812958), nine focused units convert 649
original deferrals to native comparisons: CrvOnSurf 307, face props 114, curve
class part 4 153, face part 4 50, healer part 1 25. The two new fixtures are
additional passes, not original deferral conversions. Separately, 520 remaining
deferrals change blocker; they are not included in the 649 gains.

This is not an ARM64 all-green claim. The healer units retain native hash-table
conversion differences (#448): part 1 has 15 failures, part 2 260, part 3 28.
Against the earlier local ARM64 run, all 15 + 254 + 28 shared failures have
identical mismatch details. The six additional part-2 cases were previously
deferred at `SmEdge::GetTolerance` before main wired it; that generator never
calls stage two or the new query-cache path. The complete x86_64 CI run is the
remaining full parity gate; no platform exceptions were added to the harness.
