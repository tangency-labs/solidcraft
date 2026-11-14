# blend.fillet_executive_p3: sweep5 mass differs on faces whose edge uses hold no own UV trim (2026-10-27)

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

Inputs: `sweep5-letter_g-e10-r1.0` (`Fillet/letter_g.smb` edge 10, radius 1.0) and
`sweep5-nurb_box1-all-r0.5` (`Fillet/nurb_box1.smb`, every edge, radius 0.5). Circular G1,
thumbweight 1. The probe sweep found the same pattern on `letter_g` e17 (r=1.0), which shows
it once the lazy-extrusion stitch refusal is fixed
(`plan/findings/blend.fillet_executive_p3--sweep5-lazy-extrusion-knots.md`).

Values: topology, every vertex, edge and face sample, the sampled UV trims, and the per-stage
counts of edge uses without a UV trim (CreateFilletBrep through the final trimming) all
agree. Only the mass properties differ:

| case | C++ area | Rust area | rel |
|---|---|---|---|
| letter_g e10 | 855.6511940507597 | 855.6495364737889 | 1.9e-6 |
| nurb_box1 all r0.5 | 1572.3758420480713 | 1572.3758445910357 | 1.6e-9 |

Per-face `SmApiFaceComputeArea` vs `face_area`: only one B-spline face differs in each case.
letter_g e10 face 12 (the fillet face): 58.46440465603618 vs 58.449478062772315. nurb_box1
face 5: 0.34809348253569505 vs 0.34809602550007646. On both sides, before and after the
mass call, some of that face's edge uses have no UV trim curve of their own
(`GetUVTrimCurvePointer() == NULL`): letter_g face 12 edge use 2, all three edge uses of
nurb_box1 face 5. Their curves are held by the mate (`GetUVTrimCurve` returns the mate's).

Suspected cause: the mass-time trim boundary of a face whose edge uses read their UV trim
through the mate (`CreateOrientedTrimBoundary` -> `GetOrCreateUVTrimCurve` returning the
mate's curve, with the edge use's own orientation). This is not localised further yet.
