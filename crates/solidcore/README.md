# solidcore

A B-rep solid modeling kernel in safe, deterministic, wasm-compatible Rust: a Rust port
of NVIDIA's SMLib (from [USD BRep](https://github.com/NVIDIA-Omniverse/usd-brep),
Apache-2.0). Not affiliated with or endorsed by NVIDIA.

**Status: experimental (0.0.x).** The planned kernel modules have Rust ports, and
primitive construction is exercised end to end. Integration remains in progress:
partial units, deferred paths and known mismatches still limit coverage, and the API
will change. A unit is green only after differential testing against the C++ original
confirms identical topology and values within 1e-9 relative, with any per-key absolute
floors documented in its report.

Current unit results, blockers and primitive checks:
<https://tangency-labs.github.io/solidcore/>.

Progress, design and how to contribute: <https://github.com/tangency-labs/solidcore>
(see `plan/units/`, `plan/PROGRESS.md` and `AGENTS.md`).

Licensed under Apache-2.0; see `LICENSE`, `NOTICE` and `ATTRIBUTION.md`.
