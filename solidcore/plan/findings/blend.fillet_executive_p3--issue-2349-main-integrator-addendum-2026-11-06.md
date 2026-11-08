# Closed-angle full-fixture mass differences after main's integration changes

Unit: `blend.fillet_executive_p3`. Addendum to
`blend.fillet_executive_p3--issue-2349-teardrop-full-symmetric-zeros.md`.
The earlier record's two-field measurement is historical.

## Reproduction and measured revision

Ubuntu WSL, pinned C++ reference `v1-3fd5b9548d19-726643fb17a52b7d`:

```sh
CARGO_BUILD_JOBS=2 nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit blend.fillet_executive_p3
```

Measured on the combined allocation/closed-Turn stack at
`ad2ffcd7dcf0505bfaa44cd2465229d5079e8e5d`, based on main
`a39659e3d798b56f99266cd2e522360dd99f7cf7`. The subsequent main commit
`513e6fcfcd049d41e9487599880479de4add63f2` changes tests and records only.
The input is `fixtures/blend.fillet_executive_p3/issue_2349_closed_angle.json`.
No comparison field, relative tolerance or absolute floor changes.

## Differences

`issue-2349-teardrop-full-r0.5` now differs in all twelve
`values.mass_properties[0]` through `[11]`; topology and the remaining compared
values agree. Representative values, copied from the native report:

| Field | C++ | Rust | Relative difference |
| --- | --- | --- | --- |
| `mass_properties[0]` | `4.4965252984564165e2` | `4.496526318586798e2` | `2.2687076849334211e-7` |
| `mass_properties[4]` | `-1.420937672371026e-6` | `-9.29908076387405e-8` | `0.9345567300755896` |
| `mass_properties[9]` | `-6.38053826782007e-3` | `-4.3551871074616686e-4` | `0.9317426379303007` |

`issue-2349-teardrop-full-r3` has exactly one disagreement; every other field
agrees within its unchanged comparison contract:

| Field | C++ | Rust | Relative difference |
| --- | --- | --- | --- |
| `mass_properties[11]` | `-5.74729606867793e-4` | `-5.747296057537034e-4` | `1.9384585257132095e-9` |

The eight intermediate teardrop checkpoints have zero numerical relative error.
Their only differences on the closed-Turn stack are the intended corner-type
integers, C++ 7 versus Rust 9, in `ints.prefix_state[5]` and, where present,
`ints.before_direct_topology[5]`. In particular, neither `solver_geometry` nor
`corner_edges` has a vertex-tolerance or ownership disagreement.

## Cause and disposition

These measurements follow main's changes to precise-property integration
(`mass.integrator--trim-base-v-default-moves-cases-2026-11-05.md` and
`mass.integrator--split-at-surface-knots-default-moves-cases-2026-11-05.md`).
The individual arithmetic operation responsible for the radius-3 last-digit
difference has not been isolated. This is not evidence of a split-allocation
regression or proof that either complete rounded body is geometrically correct.

Both full cases remain ordinary known mismatches. Neither is parked with
`cpp_defect`, and the unit remains partial. The strict `1e-9` relative comparison
is retained. This record changes no kernel result, approximation tolerance or
fit-size contract; the known closed-round and named-fit-size defects remain open.
