# general: Issue #448: architecture requirements of the differential reference (2026-10-24)

The pinned C++ converts out-of-range floating-point values to integers in
`SmHashTable.h:1957-1959` (UB-35), `SmTArray.h:2395-2397` (UB-194), and
`SmTwistVolume.cpp:1202`. The Rust ports intentionally emulate the x86-64
conversions on every target. The ARM64 differences and macOS reproduction are
recorded in #448; this change does not alter either kernel or the reference pin.

The five runners named in #448 now declare an x86-64 reference requirement.
On other architectures, comparison and fuzz runs stop before fixture construction
with `skipped(platform)`, separate from passes, deferrals and native rejections.
Serial and parallel summaries exclude these units from their passed counts; JSON
reports have `pass: false`. Rust-only digests remain enabled. Skipped reports are
not accepted as baselines, and an exact-base comparison with an unsupported
reference fails even for a newly selected unit. No plan status, case, tolerance or
coverage waiver changed.

Validation of the skip paths uses an unsupported-reference test double, so it runs
on x86-64 as well. It checks that neither fixture construction nor C++ execution
happens, fuzz workers are not launched, Rust digests still run, skipped reports
cannot replace baselines, and previously passing cases remain ratcheted. The
architecture predicates also cover ARM64 explicitly. No fresh ARM64 native run is
claimed; this machine is Linux x86-64.
