# mathprobe: does `f64::sin` give the same bits everywhere?

difftest proves solidcore matches the C++ bit for bit on **Linux x86_64 with glibc**.
There, Rust's `f64::sin` and the C++ `sin` both call glibc's `sin`, and so does every
other measured function except `cbrt`. `compiler_builtins` defines its own `cbrt`, and
a pure-Rust Linux binary binds to it unless libm is linked earlier. On other targets,
Rust uses a different libm: Apple's on macOS, and on `wasm32-unknown-unknown` the
pure-Rust libm in `compiler_builtins`. This probe measures how often those differ.

For each std `f64` method solidcore calls (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
`atan2`, `sinh`, `cosh`, `ln`, `log10`, `cbrt`, `powf`, `powi`; `sqrt` is correctly
rounded everywhere, so it is left out), `gen.py` writes 200,000 deterministic inputs.
They are weighted toward what a geometry kernel passes: angles within a few turns,
values near multiples of π/2, near-±1 arguments for `acos`/`asin`, and wide log-uniform
ranges. The probe then evaluates them

* on the host, as a Rust binary (`src/bin/native.rs`);
* as a `wasm32-unknown-unknown` module run by node (`wasmeval.mjs`); node passes `f64`
  values through bit for bit, and the math runs inside the module;
* with docker, through Python's `math` module (a thin wrapper over the C libm) in
  `ubuntu:24.04` on `linux/amd64` and `linux/arm64`. That is glibc 2.39, the same
  base image as CI's hosted runners;

and `compare.py` reports, per function, how many results differ from glibc x86_64
in any bit, and by how many ulp at most. It also compares the host's Rust binary with
Python on the same host (the host's C libm). A nonzero row there means Rust does not
call the platform libm for that function, like `cbrt` in a pure-Rust Linux binary.

```sh
harness/mathprobe/run.sh          # work files in bin/mathprobe/
```

It needs Python 3.11+, node, a Rust toolchain with the `wasm32-unknown-unknown` target
(set `CARGO=`/`RUSTC=` if the `cargo` on PATH is not rustup's) and, for the glibc
columns, docker. Under emulation `linux/amd64` must report `fma` (printed before
its row), or glibc takes different code paths than on CI's hardware.

The results, and what they mean for difftest and wasm, are in `plan/findings.md`
(General, "Math libraries differ by platform").
