# Analytic mesh reconstruction command plugin

This is an independently compiled Rust/Wasm component for the native command host in PR #8987. It reads an evaluated mesh, computes a certified analytic solid, and records an ordinary embedded STEP Import plus Remove in one host transaction. The original mesh data stays in the timeline. One Undo restores the source mesh; Redo restores the solid.

The supported reconstruction families are certified straight-extrusion parts with circular boundaries, bounded combinations joined at planar shoulders, right circular frusta and coaxial countersunk bores. This is inference from a finite tessellation, not recovery of an original feature tree. Freeform surfaces, arbitrary blends, helical threads, open/nonmanifold input and candidates outside the fitting/work budgets are refused; no faceted fallback is silently substituted.

## Independent build

The standalone Cargo workspace pins its compute SDK to `aab1df183a7b4ca7c3d9e4dd5b52a6b8a2c64e4f` in [the public SDK fork](https://github.com/crono2250/solidcraft/tree/aab1df183a7b4ca7c3d9e4dd5b52a6b8a2c64e4f). The SDK supplies build-time geometry code only. It neither installs nor embeds a second host runtime. The manifest must contain no absolute local dependency paths. Preserve the dependency patch provenance and licences.

Use the project's Rust toolchain with wasm32-unknown-unknown and a compatible wasm-tools. Build the standalone Cargo manifest with --locked --release --target wasm32-unknown-unknown, then use build_component.py to validate and package the core Wasm as a component. Distribution contains a mesh-reconstruct folder with plugin.json and plugin.wasm. Its directory name must match its manifest id. The host needs no Python or Cargo to run that package.

Only the published solidcraft:plugin/types and engine 0.1.0 imports and command export are allowed. There is no WASI, filesystem, socket, process or clock import. The package grants exactly model.mesh and timeline.add. Compilation and the final pure-component import check are required before distribution.

## Explicit execution

The native host must implement the published #8987 command API and its revised explicit resource limits. Inspect a local package first using plugin.inspect {path}. Review its exact code/content identity, two command grants and execution budget. Supply the resulting approval hash to plugin.run:

    {"path":"<package>/mesh-reconstruct", "approval":"<reviewed hash>", "entry":"convert", "params":{"body":"Source", "tolerance":0.05, "name":"Restored"}}

Source is an existing mesh body's exact name, not a file path. Tolerance is finite and between 0.000001 and 1 mm. name is optional and must differ from the input. Unknown parameters, including currently unsupported picks/all_surfaces, are refused. The host transfers component-local millimetres, at most 8192 positions/16384 triangles, additionally bounded by its JSON limit. A changed manifest or component requires a fresh review/hash.

The manifest explicitly requests 100 billion fuel, 128 MiB and 30 seconds. These are maxima, not guaranteed input sizes or completion times. Fuel counts metered operations rather than milliseconds. The host never raises limits or retries automatically. A failed or stopped transaction is not adopted. Stop discards adoption while its worker finishes; it does not forcibly terminate a native kernel call.

After success, ordinary file.export can write STEP without selecting around the old mesh, and a reconstructed planar face accepts an ordinary durable sketch. Save/reopen does not execute or require the plugin. Source mesh data remains in history even though its current body has been retired.

## Recorded identity and limits

The stored STEP identity includes the evaluated mesh, selected source body name, source component, tolerance, immutable SDK revision and reconstruction algorithm/face-order version. Changing those cannot reuse an old face ordinal as a trustworthy attachment. Future geometry-selection parameters must enter this identity before they are exposed.

This host is one-shot. The saved result is frozen native STEP. Editing an upstream source or changing tolerance requires explicit re-execution; it is not an automatically reevaluated custom feature. To run again, first restore the original mesh body with Undo, or suppress the recorded result and its source-removal feature. The component stays outside saved designs and there is no automatic download/approval/installation.

## Validation and review material

Final source, SDK revision, toolchain, package bytes, published host revision and actual executable hashes must match the reviewed results. Keep native regressions, identical-geometry/full-measure parity, actual-host success/STEP/UndoRedo/source-history/sketch/cold-reopen, expected-failure rollback, actual Stop, platform/full registered distributed CI and pure-WIT audits separate. No host-only CI or selected kernel test count substitutes for those gates.

Review media uses only independently generated procedural examples, captured from the actual host. User-supplied STL, screenshots and derived private geometry are never source fixtures, media or attachments. Internal diagnostic archives contain local paths/binaries and are not public attachments.