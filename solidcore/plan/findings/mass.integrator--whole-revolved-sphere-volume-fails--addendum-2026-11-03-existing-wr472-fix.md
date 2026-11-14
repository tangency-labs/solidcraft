# Whole revolved sphere: the existing WR-472 error scale resolves the ignored case

Updates `mass.integrator--whole-revolved-sphere-volume-fails.md` and its
`addendum-2026-11-01-shared-at-1e-8` record. The default-path C++ comparison in
PR #2259 remains valid. This follow-up is Rust-only numerical evidence.

The ignored `sweep::api_brep::tests::wr471_whole_sphere_volume` enabled the two
WR-471 sweep flags but omitted `precise_properties_body_length_scale`. WR-472
already corrects the dimensional error scales used by face integration: volume
gets one body-length factor and higher moments get their corresponding powers.
The old scale can demand errors below roundoff, causing adaptive stepping to fail.
No new integrator or production-code change is needed for this fixture.

A 24-case probe varied radii 3 and 5, axis offsets 0 and 3e-7, requested relative
accuracies 1e-6, 1e-8 and 1e-10, and the WR-472 flag. Both sweep flags were on.
Every flag-off case at 1e-8 and 1e-10 returned `mass::integrator` error 1001.
Every flag-on case converged with measured relative volume error below its requested
accuracy, checked against `4/3 * pi * radius^3`.

For the exact ignored fixture (radius 5, offset 3e-7):

| Requested accuracy | WR-472 off | WR-472 on: volume | Relative error |
| --- | --- | --- | --- |
| 1e-8 | Error 1001 | 523.5987767442521 | 2.1886094e-9 |
| 1e-10 | Error 1001 | 523.5987756148963 | 3.1698857e-11 |

The test is now active. It retains the default error contract and checks the
flag-on volume against each requested accuracy, using a fresh body for each run.
The successful assertion is tighter than the former ignored test's 1e-6 bound.
This does not remove the default limitation or establish a general error guarantee
for the integrator. The probe ran on Linux with the difftest profile at opt-level 1.
