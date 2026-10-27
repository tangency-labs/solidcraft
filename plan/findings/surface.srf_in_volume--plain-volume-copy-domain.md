# surface.srf_in_volume: virtual Copy accepts defined shared-volume inputs (2026-10-27)

Unit: `surface.srf_in_volume`. PR: #1354; follows the direct copy-constructor
comparison in #1347 and the kernel Copy work in #1286.

The old FFI guard rejected every virtual `Copy` with a plain `SmVolume`
head. All six cases in `fixtures/surface.srf_in_volume/copy_volume_domain.json`
failed before native execution with `invalid argument: copy of a plain volume`.
Rust could produce the copy, but the comparison admitted no native values.

`SmSrfInVolume::Copy` (`SmSrfInVolume.cpp:185`) invokes its copy constructor.
That constructor can attempt a volume copy even for an owner-1 result;
`SmVolume::Copy` (`SmVolume.h:580`) returns `SM_ERR` without changing the
output pointer. A result with owner flag 0/1 safely shares that pointer;
owner 2/3 would delete the original's volume in `~SmSrfInVolume`. Thus the
existing `Block.owns_volume` predicate identifies the undefined owning set.
The guard now rejects `head_base && owns_volume`. Constructor-copy flags2/3,
other guards, kernel behavior and native object lifetimes are unchanged.

After the fix all six named cases pass, comparing 76-95 actual copied-state
values plus status at zero relative error: ordinary and raw base surfaces,
both non-owning volume result flags, and a non-identity next transform map.
The full native unit passes 2,155 cases: 2,080 value passes, zero failures or
deferrals, maximum relative error zero, and 75 unchanged libc++ `ReadDb`
platform skips. The exact FFI accept/reject matrix passes; sensitivity detects
all six perturbations. Native headers/libraries match upstream1.0.1,
`3fd5b9548d19`, on macOS ARM64; canonical Linux CI remains authoritative.

Existing rejection-sampled generated cases retain their original copyable-head
domain, so admitting these fixtures does not silently redraw existing seeded
inputs. No fixture or comparison was removed, no tolerance or skip changed,
and no new kernel API, native shim or synthetic replay test was added.
