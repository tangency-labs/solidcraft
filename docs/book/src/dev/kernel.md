# The kernel boundary

`crates/kernel` is SolidCraft's B-rep kernel boundary. The modelling kernel behind it is
`solidcore`, the vendored Rust port of NVIDIA's SMLib (`solidcore/`): `crates/smlib` wraps it in a
safe API, and `crates/kernel` turns that into the operations the app uses. Callers get SolidCraft types (`Body`, face and edge indices, meshes,
measures) and a `KernelError` instead of kernel types and panics.

## `guard`

The vendored kernel can panic on geometry it does not handle. Every call into it goes through `solidcraft_kernel::guard`, which catches the panic and turns it into
`KernelError::Internal`, leaving the model unchanged:

```rust
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T>
```

A failing feature shows an error on the timeline; it never takes the app down.

## What is our own

The B-rep operations are `solidcore`'s. SolidCraft's own code in `crates/kernel` is the boundary
around it: the public API, display meshes, measures, persistent names and STEP exchange. A
failing kernel operation stays failing until the kernel is fixed; there is no workaround on
SolidCraft's side (`tests/kernel-cases/README.md`).

## The vendored kernel

`solidcore` is vendored in the `solidcore/` directory with `git subtree` and is built from
there as the path dependencies `solidcore` and `solidcore-record`. Develop it there, in a normal
commit; `.github/workflows/solidcore-mirror.yml` mirrors changes under it back to
[tangency-labs/solidcore](https://github.com/tangency-labs/solidcore). `solidcore/` and `vendor/`
are excluded from our fmt and clippy rules because they are not our code.
