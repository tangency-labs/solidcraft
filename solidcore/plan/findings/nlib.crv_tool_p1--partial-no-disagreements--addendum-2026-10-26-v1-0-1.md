# nlib.crv_tool_p1: N_CrvRemoveKnot error flag fixed in v1.0.1 (2026-10-26)

Updates: plan/findings/nlib.crv_tool_p1--partial-no-disagreements.md ("N_CrvRemoveKnot error flag" bullet)

Fixed in v1.0.1 (3fd5b95): N_CrvRemoveKnot (NL_CrvTool.cpp), a failing N_BasisFindGlobalMax still assumes a maximum of 1 but no longer leaves the error flag set, so the routine returns success after completing the removal. The port now returns the result in that case. Other bullets unchanged.
