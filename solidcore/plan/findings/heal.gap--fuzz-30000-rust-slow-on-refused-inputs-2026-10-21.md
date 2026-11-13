# heal.gap / offset.offset_geometry_creation: Rust slow, not hung, on fuzz inputs the FFI refuses (2026-10-21)

The fuzz runs the Rust side on inputs the FFI refuses:

- `heal.gap` `fuzz14344` (a Hermite curve point of `1e300`): 52 s in `GapFunction::build_sample_set` -> `Curve::drop_point`. Each drop builds a curve cache (a documented deviation: upstream keeps one on the curve). Its tessellation never reached 1000 leaves; the time is the many cache builds on an overflowing curve. On the unit's fixtures the Rust side takes 0.52x the C++ time.
- `offset.offset_geometry_creation` `fuzz07978` (control-point weights of `+-1e-300`, so Euclidean coordinates near `1e300`): 34 s in `Curve::global_coincidence_checker`, 5 319 076 split nodes against at most 15 139 over every green and partial unit's fixtures. Upstream, at these magnitudes, is HANG-29. (Since #504 on main, `MAX_COINCIDENCE_CALLS = 2^18` per check bounds this, HANG-48.)
- PR #358 added no node or sample bound: the C++ terminates, slowly, on valid inputs of the same shape (HANG-29's `1e15`..`1e150` range, up to 99 s), so a bound would fire where upstream returns.
- Found by raykholo's PR #358, stacked onto main.
