# nlib.geom: N_DistPtInfLine unset point fixed in v1.0.1 (2026-10-26)

Updates: plan/findings/nlib.geom--green-no-disagreements.md (N_ProjectPtPlane / N_DistPtInfLine unset-point note)

Fixed in v1.0.1 (3fd5b95) for N_DistPtInfLine only (NL_Geom.cpp): the projected point is initialised to the origin, so a missed projection measures the distance to the origin instead of reading an unset point; the port does the same instead of returning InvalidInput. N_ProjectPtPlane is unchanged and the port still returns InvalidInput there.
