// solidcore harness stub (not upstream code).
//
// Replaces, in the SM_API_test_app build only:
//   upstream/tests/SM_API_test/src/TestSmBooleanBoundsExample.cpp
//   upstream/tests/SM_API_test/src/TestSmSTEPDerivativesExample.cpp
// Both compile code from upstream/Examples/ (boolean_bounds.cpp,
// step_derivatives.cpp), which is outside the source allow-list of the
// solidcore port, so they are not built. Each stub prints a SKIPPED marker
// and returns SM_SUCCESS; the runner therefore counts them as "Successful",
// and plan/baseline.md subtracts them from the pass count.

#include "StdAfx.h"
#include "SM_API_test.h"

#include <cstdio>

SmStatus TestSmBooleanBoundsExample()
{
    std::printf("\nSOLIDCORE-STUB SKIPPED: TestSmBooleanBoundsExample (needs upstream/Examples)\n");
    return SM_SUCCESS;
}

SmStatus TestSmSTEPDerivativesExample()
{
    std::printf("\nSOLIDCORE-STUB SKIPPED: TestSmSTEPDerivativesExample (needs upstream/Examples)\n");
    return SM_SUCCESS;
}
