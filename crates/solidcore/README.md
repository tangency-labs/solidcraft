# solidcore

A B-rep solid modeling kernel in safe, deterministic, wasm-compatible Rust: a Rust port
of NVIDIA's SMLib (from [USD BRep](https://github.com/NVIDIA-Omniverse/usd-brep),
Apache-2.0). Not affiliated with or endorsed by NVIDIA.

**Status: 1.0.1.** The version tracks the upstream release it is ported from and
tested against, usd-brep v1.0.1. Everything SMLib's own test suites exercise is ported:
- primitives, booleans, fillets and chamfers, offsets and shells, sweeps, healing, and
  mass properties, plus most of the public SM_API (a few large entry points, such as
  polygon-mesh booleans, tessellation, point-cloud surface fitting and fillet removal,
  are not ported yet);
- a unit is green only after differential testing against the C++ original confirms
  identical topology and values within 1e-9 relative, with any per-key absolute floors
  documented in its report.

The public API mirrors SMLib's.

## Versioning

1.0.1 matches SMLib 1.0.1. Later releases keep their own semver and name the SMLib
release they match in build metadata, for example `1.1.0+smlib.1.0.1`:

- **Minor** (`1.1.0`, `1.2.0`): new opt-in fixes for upstream defects, as fields of
  `Kernel::fixes` (`UpstreamFixes`, all off by default). Without them the default path
  still matches the named SMLib release bit for bit.
- **Patch** (`1.1.1`): a fix where the port did not match SMLib.
- **Major**: a breaking API change.
- A new SMLib release moves the `+smlib.…` metadata, with a minor or major bump.

Current unit results, blockers and primitive checks:
<https://tangency-labs.github.io/solidcore/>.

Progress, design and how to contribute: <https://github.com/tangency-labs/solidcore>
(see `plan/units/`, `plan/PROGRESS.md` and `AGENTS.md`).

Licensed under Apache-2.0; see `LICENSE`, `NOTICE` and `ATTRIBUTION.md`.
