#### `ST_curxcdr` G1 fallback reads a third point of a two-point line (primitive.primitive_creation_p1, PR #213)
- **Severity:** Docs
- **Area:**

- **Location:** `source/SMLib/src/NL_CrvAdv.cpp:999-1006,1077-1084`.
- **Trigger:** a two-control-point line whose endpoint difference cannot be
  normalized by `N_VectorNormalize`, then G1 extension at the affected end.
  Finite coordinates around `1e300` overflow the computed vector magnitude.
  The fallback reads `Pw[2]` at the start or `Pw[n-2]` (index -1) at the end.
- **Found by:** the primitive path helper's 1,000-case FFI fuzz run (cases 427,
  670, 689 and 919), initially reporting history-dependent garbage extension
  intervals instead of Rust's checked error/unchanged curve.
- **Confirmation:** the linked native `N_VectorNormalize` fails for those exact
  two points; source inspection then proves the next read is out of bounds.
  The revised shim prevents that read by preflighting only extension stages
  actually selected by the upstream join and closed/periodic decisions. Huge
  single lines that are not extended remain accepted. Rejected cases are
  excluded from parity. The Rust NLib port already bounds its fallback lookup.
