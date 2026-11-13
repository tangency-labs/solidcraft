Updates: UB-6

#### UB-6 (update, 2026-10-26). Fixed in v1.0.1 for the B-spline readers

Fixed in v1.0.1 (3fd5b95), B-spline readers only: SmBSplineCurve::ReadFromDB and SmBSplineSurface::ReadFromDB now check the stream fail state after the header, each control point and each knot, so a short or failed header returns SM_ERR_INVALID_INPUT before n, m, p, q or rat is used. Unchanged in v1.0.1 (no diff in their files): SmAttributeData::ReadIndexedAttributesFromDB, SmLine::ReadFromDB, SmConic::ReadFromDB, SmCrvInVolume::ReadFromDB, SmPlane::ReadFromDB; SmEllipse.cpp changed but not in SmPolarConversion::ReadFromDB. Verification: B-spline reader cases, pending the pin101 difftest run.
