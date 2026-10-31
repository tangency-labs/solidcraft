# #483: remaining ignored helper statuses at v1.0.1

- Severity: wrong result / false success.
- Source: `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`.
- Locations: `SmPrimitiveCreation.cpp:3295,5528,5596,5693` (pivot, extension,
  profile placement, sweep helper); `SmFilletCorner.cpp` (G1 tangent evaluations
  and `CheckAndFixDegeneracy` UV refinement); `SmFilletSolver.cpp:374`
  (`sm_CheckEdgeCurveDomains`); `SmOffsetGeometryCreation.cpp:2175`
  (`HaveTangentGaps`); `SmAdvSurfaceIntersector.cpp:131` (alignment macro).
- Trigger: a geometry operation returns failure before assigning its output.
  For example a collapsed edge domain `[0.5,0.5]` refuses Trim; the helper still
  returns the untrimmed copy as a successfully trimmed curve. A corner UV
  projection failure still installs the undefined default and marks completion.
- How found: independent permitted-source audit for issue #483 and Rust
  regressions in PR #1775. The original Boolean scope is recorded separately as
  WR-586, profile placement as WR-632, and fillet geometry Trim as WR-604.
- Native confirmation: the listed drops are explicit in the pinned C++ source.
  Existing native fixtures exercise defined default behavior; failure-injection
  regressions run on Rust service boundaries. This record does not claim a new
  native reproduction of the synthetic adapter failures or unsafe null cases.
- Rust policy: targeted default-off flags; first mandatory error returned before
  reading its output; successful prefix mutations retained. Optional mitre
  searches retain their independent cutting-line fallback. A path plane normal
  is computed only when a mitre needs one, so the corrected sweep still accepts
  a single straight path. Sweep-owned temporary bodies are cleaned on every
  checked failure. Full call inventory and evidence:
  `plan/findings/general--status-audit-483.md`.
