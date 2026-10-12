# solidcore

A boundary-representation (B-rep) solid modeling kernel in safe Rust: a port of
NVIDIA's SMLib (from [USD BRep](https://github.com/NVIDIA-Omniverse/usd-brep), Apache-2.0).

Solids are exact: shells, faces, loops, edges and vertices bound to analytic and NURBS
curves and surfaces. solidcore's first consumer is **SolidCraft**.

## Scope

In scope: the `SMLib` kernel and the `SM_API` operation layer of upstream — NURBS
math, curve and surface geometry, B-rep topology, intersections, booleans, fillets and
chamfers, offset/shell, sweeps and skins, primitives, healing/stitching, mass
properties and queries.

Out of scope: upstream's OCCT translators, USD/Hydra integration and Python bindings.
See [ATTRIBUTION.md](ATTRIBUTION.md).

Goals: no `unsafe` in the kernel, no panics on bad input, deterministic results, no
global state, builds for `wasm32-unknown-unknown`, and numerical agreement with the
C++ to `1e-9` relative with identical topology, verified unit by unit.

## Status

Early. The workspace, foundation types (`math`, `error`, `kernel`, `topology`) and the
C++ comparison harness are in place; algorithm modules are being ported unit by unit.
Per-unit state lives in [`plan/modules.toml`](plan/modules.toml) and the summary in
[`plan/STATUS.md`](plan/STATUS.md).

## Layout

| Path                    | Contents                                                     |
|-------------------------|--------------------------------------------------------------|
| `crates/solidcore`      | The kernel library                                           |
| `crates/solidcore-ffi`  | `extern "C"` bindings to the upstream C++ (feature `cpp`)    |
| `crates/difftest`       | Runs Rust and C++ side by side on the same inputs           |
| `harness/cpp`           | Builds the upstream C++ reference into `bin/cpp`; FFI shims  |
| `plan/`                 | Unit list, port order, status, findings                      |
| `upstream/`             | Local clone of usd-brep at `c9c5979` (gitignored)           |
| `bin/`                  | All build output (gitignored)                                |

## Building

Requires the stable toolchain (pinned by `rust-toolchain.toml`, which also installs
`rustfmt`, `clippy` and the `wasm32-unknown-unknown` target). Cargo output goes to
`bin/` (see `.cargo/config.toml`).

```sh
cargo build -p solidcore
cargo test -p solidcore
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p solidcore --target wasm32-unknown-unknown   # wasm check
```

Optional: `export RUSTC_WRAPPER=sccache` to share compilation caches.

## Difftest (Rust vs C++)

```sh
git clone https://github.com/NVIDIA-Omniverse/usd-brep upstream
git -C upstream checkout c9c5979
harness/cpp/build.sh                                  # C++ reference -> bin/cpp
cargo run -p difftest --features cpp -- --green       # all units marked green
cargo run -p difftest --features cpp -- <unit id>     # one unit
```

`solidcore-ffi` and `difftest` compile without the C++ build; only the `cpp` feature
links it.

## Contributing

Read [AGENTS.md](AGENTS.md): rules, code style, and the claim/port/difftest/merge
workflow.

## Licence

Apache-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE) and
[ATTRIBUTION.md](ATTRIBUTION.md). solidcore is an independent project, not affiliated
with or endorsed by NVIDIA.
