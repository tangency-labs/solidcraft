# solidcore

A B-rep solid modeling kernel in safe, deterministic, wasm-compatible Rust: a Rust port
of NVIDIA's SMLib (from [USD BRep](https://github.com/NVIDIA-Omniverse/usd-brep),
Apache-2.0). Not affiliated with or endorsed by NVIDIA.

**Status: early work in progress (0.0.x).** The math, NURBS and curve layers are ported
and verified against the C++ original (identical topology, values within 1e-9
relative). Surfaces, topology, primitives, booleans and fillets are still being ported,
so nothing is usable end to end yet and the API will change.

Progress, design and how to contribute: <https://github.com/tangency-labs/solidcore>
(see `plan/PROGRESS.md` and `AGENTS.md`).

Licensed under Apache-2.0; see `LICENSE`, `NOTICE` and `ATTRIBUTION.md`.
