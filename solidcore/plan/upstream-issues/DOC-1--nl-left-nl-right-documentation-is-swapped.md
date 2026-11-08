#### DOC-1. `NL_LEFT` / `NL_RIGHT` documentation is swapped
- **Severity:** Docs
- **Area:**
- **Location:** `NL_BasisBasic.cpp:1433`-`1434` (parameter comment of `N_BasisFindSpan`), `:1506`, `:1522` (case comments).
- **What happens:** The comments say `NL_LEFT` means `U[spn] < u <= U[spn+1]`. The code (`while( u LT U[mid] OR u GE U[mid + 1] )`) does the opposite: `NL_LEFT` gives `U[spn] <= u < U[spn+1]`. `NL_RIGHT` is likewise swapped.
- **Trigger:** `u` equal to an interior knot.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** `Side` documents what the code does.
