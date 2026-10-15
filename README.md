<p align="center"><img src="assets/logo.svg" width="140" alt="solidcore logo: an isometric solid with one filleted edge and a NURBS curve on its top face"></p>

# solidcore

**📊 Live progress: [bherbruck.github.io/solidcore](https://bherbruck.github.io/solidcore/)**: every unit's port status and difftest results, the upstream-issues log, and the surface/surface intersection corpus scores, regenerated on every merge.

solidcore is a Rust port of NVIDIA's SMLib: a boundary-representation (B-rep) solid
modeling kernel in safe Rust (SMLib is from [USD BRep](https://github.com/NVIDIA-Omniverse/usd-brep), Apache-2.0).

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

About a third ported and verified against the C++: the math, NURBS and curve layers,
with surfaces and topology under way. **Checklist: [`plan/PROGRESS.md`](plan/PROGRESS.md)**
(updated automatically). Notes and handoff: [`plan/STATUS.md`](plan/STATUS.md).
The same data, browsable, is on the progress site built by
[`site/build.py`](site/README.md) (`python3 site/build.py` -> `bin/site/`).
Nothing is usable end to end yet (no primitives, booleans or fillets).

## Layout

| Path                    | Contents                                                     |
|-------------------------|--------------------------------------------------------------|
| `crates/solidcore`      | The kernel library                                           |
| `crates/solidcore-ffi`  | `extern "C"` bindings to the upstream C++ (feature `cpp`)    |
| `crates/difftest`       | Runs Rust and C++ side by side on the same inputs           |
| `harness/cpp`           | Builds the upstream C++ reference into `bin/cpp`; FFI shims  |
| `plan/`                 | Unit list, port order, status, findings                      |
| `site/`                  | Generator for the GitHub Pages progress site                 |
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

### WebAssembly stack size

On `wasm32-unknown-unknown` the Rust stack lives in linear memory **without a guard
page**, so an overflow corrupts memory instead of aborting. solidcore bounds
input-driven recursion, but kernel algorithms (subdivision, intersection, filleting)
still recurse legitimately. Link wasm consumers with a larger stack than the ~1 MB
default, e.g. in the consumer's `.cargo/config.toml`:

```toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "link-arg=-zstack-size=8388608"]   # 8 MiB, as a native main thread
```

## Difftest (Rust vs C++)

```sh
git clone https://github.com/NVIDIA-Omniverse/usd-brep upstream
git -C upstream checkout c9c5979
harness/cpp/build.sh                                  # C++ reference -> bin/cpp
cargo run -p difftest --features cpp -- --green       # all units marked green
cargo run -p difftest --features cpp -- --unit <unit id>   # one unit
```

`solidcore-ffi` and `difftest` compile without the C++ build; only the `cpp` feature
links it.

## Contributing

Contributions are pull requests, one per port unit, and AI agents are welcome:

1. Pick any unclaimed unit in [`plan/PROGRESS.md`](plan/PROGRESS.md). "Ready" ones are
   easiest; the others need stubs or co-porting of missing dependencies.
2. Claim it by opening a draft PR titled `port: <unit id>` (lowest-numbered PR wins).
3. Port it, prove it with difftest against the C++ reference, mark the PR ready.

The full rules, style and step-by-step workflow are in [AGENTS.md](AGENTS.md) (point your
agent at it).

## Releasing

`solidcore` is published to crates.io from CI: bump `version` in
`crates/solidcore/Cargo.toml` in a PR, merge it, then push a tag
`solidcore-v<version>` (`.github/workflows/release.yml`, needs the
`CARGO_REGISTRY_TOKEN` repository secret). Only `solidcore` is published;
`solidcore-ffi` and `difftest` are test-only. It stays at 0.0.x until it is usable end
to end.

## Licence

Apache-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE) and
[ATTRIBUTION.md](ATTRIBUTION.md). solidcore is an independent project, not affiliated
with or endorsed by NVIDIA.
