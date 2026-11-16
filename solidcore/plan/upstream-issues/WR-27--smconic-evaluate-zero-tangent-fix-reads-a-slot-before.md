#### WR-27. `SmConic::Evaluate` zero-tangent fix reads a slot before writing it
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmConic.cpp:125`, `SmConic::Evaluate`.
- **What happens:** With `bNonZeroTangents` and two derivatives, the fix reads `aPointAndDerivatives[2]` before the second derivative is written there, i.e. the caller's leftover value.
- **Trigger:** A conic with a tiny first derivative, evaluated into a reused array.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reads `Vec3::UNDEF` (matches fresh arrays).
