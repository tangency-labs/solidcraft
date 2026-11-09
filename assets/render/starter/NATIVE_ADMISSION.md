# Native resource admission and continuous validation

This gate connects the authored resource checks to the **real public Rust APIs**
in `solidcraft-render`. It is a development conformance tool, not a new application
importer, persistent texture format, renderer feature or GUI control. The fixed
resource branch remains isolated from the renderer implementation checkpoint.

## One command

From the repository root, use Python 3.11+ in an isolated development environment:

```sh
python -m pip install -r assets/render/starter/tools/requirements.txt
python -B assets/render/starter/tools/check_all.py
```

The command checks the original catalog, prepared SELENE contracts, resource
manifests, original Python regression tests and exact material/HDR reproduction.
It then runs repository formatting, attribution and icon checks, all-target
Clippy for `solidcraft-render`, its library tests, the new native checker tests
and full native resource admission. `--locked` preserves the Rust dependency
resolution. No Rust dependency, application command or saved design is changed.

The runner writes uniquely named logs and a machine-readable `report.json` under
`target/render-starter-checks/`. It fingerprints its source inputs before and after
checking: a concurrent change invalidates the result instead of producing mixed
revision evidence. Native success additionally requires the complete expected
receipt inventory, correct source/decoded digest syntax and explicit non-runtime
status. A failed stage stops the gate and retains its diagnostic output.

`--authoring-only` explicitly omits the native/repository stages and records that
limited scope; it is never reported as a native pass. `--independent-decode`
additionally requires an installed FFmpeg executable and fails rather than
silently skipping a missing decoder. Neither option installs application features.

The separate **Render resource gate** GitHub workflow runs on relevant pull
requests, including drafts, using Ubuntu 24.04 and Windows Server 2025. It keeps
receipts/logs as workflow artifacts, not tracked assets. This targeted gate does
**not** replace complete `cargo xtask ci` or integrated application acceptance.

## What the native checker exercises

The executable is an example target, outside the compiled application library:

```sh
cargo test --locked --profile ci -p solidcraft-render --example validate_starter
cargo run --locked --profile ci -p solidcraft-render --example validate_starter
```

Its standard output is JSON evidence; progress and failures go to standard error.
It accepts no user paths and reads only the known repository-relative collection.
These checks cover the fixed starter v1 inventory; changes to that inventory need
an explicit update and review rather than an automatic acceptance-hash refresh.

| Resource | Native checks |
| --- | --- |
| 192 eight-bit maps | Exact encoded SHA-256/size, full `TextureSource` decode and equality with the authoring tool's decoded-pixel digest |
| 192 normalized inputs | `NormalizationSource` with an explicit source-sRGB declaration, independent transfer/data expectations for every channel, no unexpected clipping or orientation operation |
| 1,920 mip levels including base levels | `TextureView` and `MipChain`, ten levels per map, final 1x1 mean conservation and periodic bilinear/trilinear sampling |
| 48 height sources | Full native grayscale-16 PNG decode, little-endian sample digest and range, plus deliberate refusal by the strict eight-bit input route; no invented height role |
| 24 HDR probes | `HdrSource` and the cooperative `HdrDecodeTask`, bounded completion, full float pixel digest, primaries/exposure/correction, radiance range, spherical mean and six directional cosine integrals |
| Cancellation and hostile inputs | Source reuse after texture cancellation, consumed HDR task after cancellation, no partial mip chain, altered hashes, bounded reads, ambiguous paths, duplicate known fields and unsupported map roles |

The native HDR directional checks use pixel-center directions and exact cell
solid angles. They compare against the authored receipts with their documented
rounded precision, not an arbitrary visual similarity threshold. The uniform
comparison field must remain exactly unit radiance. The black-vacuum field has
energy owner `none` and must remain exactly zero: an environment-source fallback
must not accidentally add light.

## Coordinate and interpretation boundaries

The authoring normal convention is OpenGL +Y, with authored V pointing up.
The existing native sampler addresses image rows with V pointing down. The
checker maps `(u, v)` to `(u, 1-v)` **once**, preserves the green normal component,
and renormalizes a filtered tangent-space normal. A directional synthetic fixture
checks both halves of this mapping. This is not an object tangent-frame binding:
face/definition coordinates, handedness on mirrored instances, UV ownership and
mapping onto actual CAD surfaces still require renderer acceptance.

Base color is linearized exactly once; roughness, metallic and normal channels
remain linear data. The 1x1 mean check uses normalized values, not gamma-encoded
averages. Generic mip filtering does not establish normal-variance compensation,
anisotropic texture filtering, BRDF energy accuracy or CPU/GPU parity.

## Scope and remaining work

No source texture, HDR probe, recipe or generator receipt is rewritten. Files are
admitted one at a time with a 16 MiB encoded limit and fixed v1 dimensions; existing
native APIs retain their own allocation/work limits. These are bounded input
checks, not a whole-process RSS ceiling or a general-purpose sandbox. Run from a
trusted checkout; do not race filesystem mutations against the checker.

Source metadata is checked only as needed for native conformance; the original
Python validators remain mandatory for the full authoring schema and provenance.
Reports identify bytes and operations, not publisher authenticity or material
measurement. The light-rig generator/calibration remains covered by the Python
suite; no native rig installation or measured LED output is inferred.

Remaining integration work includes persistent body/face resource ownership,
texture/height adapters, native save/reopen, actual CPU/GPU reference scenes,
capability-aware library UI, blocked LED/aperture calibration and reviewed SELENE
source admission. Every native report retains `runtime_ready: false`.
