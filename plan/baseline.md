# C++ reference baseline (upstream SMLib @ c9c5979)

Owner: agent A (harness/cpp). Everything here is reproducible with the commands below.

## Toolchain

| Item | Value |
|---|---|
| Host | WSL2 Linux 6.6.114.1, x86_64, 16 hw threads (shared) |
| Compiler | g++ (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, libstdc++, `_GLIBCXX_USE_CXX11_ABI=1` |
| Build system | CMake 3.28.3 + Ninja 1.11.1 (falls back to make if ninja is missing) |
| Language | C++17 (`CMAKE_CXX_EXTENSIONS=OFF`) |
| Flags | `-O2 -g -DNDEBUG -ffp-contract=off -fno-fast-math -fexceptions -frtti -fPIC -w` |
| Defines (all targets) | `NDEBUG NOMINMAX SM_GRAPHICS_CALLBACKS=1 _GLIBCXX_USE_CXX11_ABI=1` (upstream `premake5.lua`, linux release) |
| Extra defines | SM_API: `SM_API_EXPORTS`; SM_API_test_app: `SM_API_TEST_STANDALONE` (both from upstream premake) |
| TBB | Not used, not linked (see below) |

Upstream sources: `upstream/source/SMLib/src/*.cpp` (224 files), `upstream/source/SM_API/src/*.cpp` (13),
`upstream/tests/SM_API_test/src/*.cpp`, `upstream/tests/prog_test/src/*.cpp`, as listed in their
`premake5.lua` files. Include dirs are exactly those premake files' `includedirs`.

## Build

```sh
nice -n 10 harness/cpp/build.sh            # configure + build + install, -j6
harness/cpp/build.sh --no-tests            # libraries only
harness/cpp/build.sh --clean --jobs 4      # from scratch, other parallelism
CXX=clang++ harness/cpp/build.sh           # different compiler (not the reference)
```

Wall-clock: clean build of everything (SMLib, SM_API, three test apps, 271 ninja steps) took
**111 s** with `-j6` under `nice -n 10` (load ~1.5 from other agents). Rebuild after touching
only test drivers: ~7 s.

Outputs (all under gitignored `bin/cpp/`):

| Path | What |
|---|---|
| `bin/cpp/build/` | CMake/Ninja build tree |
| `bin/cpp/compile_commands.json` | copied **immediately after configure**, before compiling |
| `bin/cpp/ffi.env` | generated settings for `crates/solidcore-ffi/build.rs` (include dirs, defines, flags, `LIBS=SM_API SMLib`, `SYSLIBS=pthread` = no TBB) |
| `bin/cpp/lib/libSMLib.a` (~296 MB with -g), `bin/cpp/lib/libSM_API.a` | static libs; link order `SM_API` then `SMLib`, then `stdc++ m pthread` |
| `bin/cpp/tests/SM_API_test_app` | upstream standalone SM_API test runner |
| `bin/cpp/tests/sm_api_single` | harness driver: run SM_API tests by name |
| `bin/cpp/tests/prog_test_app` | upstream prog_test runner |
| `bin/cpp/run/` | working dirs + private copy of `TestFiles/pt_TestFiles` used when running tests |
| `bin/cpp/logs/` | logs of the baseline runs below |

Files in `harness/cpp/` (owned by agent A; `harness/cpp/shim/` belongs to agent D):

- `CMakeLists.txt` - the build (header comment maps it to the upstream premake files).
- `build.sh` - wrapper; writes everything to `bin/cpp`, emits `compile_commands.json` and `ffi.env`.
- `run_tests.sh` - runs the test apps from the right working directory.
- `stubs/sm_api_test_examples_stub.cpp` - stub, see below.
- `stubs/sm_api_single_main.cpp.in` - template for the per-test driver.

## Stubs, patches and deviations from upstream

No upstream file is modified and `harness/cpp/patches/` is not needed (no patches).
`git -C upstream status` is clean after building and running all tests.

1. **TBB off.** Upstream links TBB (`use_usd_tbb()` in premake) but the TBB code in SMLib
   (`SmTopologyIntersector.cpp`, `SmTess.cpp`, `SmBrep.cpp`, `SmBrep.h`) is only compiled under
   `#ifdef SM_USE_TBB`, and the TBB allocator (`NL_Util.cpp`, `SmMemory.cpp/.h`) only under
   `SM_USE_TBB_SCALABLE`. No upstream build file defines either macro, so upstream's own
   default is the serial path. We define neither and link no TBB. No stub needed.
   (`TBB_SUPPRESS_DEPRECATED_MESSAGES`/`TBB_USE_DEBUG` from `use_usd_tbb()` are only relevant
   to TBB headers and are omitted.)
2. **Static instead of shared libs**, `-fPIC`, so solidcore-ffi can link them into Rust tests.
3. **`-O2 -ffp-contract=off`** (no FMA contraction; x86-64 SSE2 math, no x87), plus `-g` for
   debugging the reference. Upstream "optimize On" is also `-O2` on gcc.
4. **No `-Werror`, warnings silenced with `-w`** (upstream: `FatalCompileWarnings`, `-Wall -Wshadow=local -Wunused-parameter`).
   Upstream test premake also adds `-ansi`; omitted (with `-std=c++17` it is a no-op for C++ besides
   `-std` ordering).
5. **SM_API_test: two tests stubbed** (`stubs/sm_api_test_examples_stub.cpp`).
   `TestSmBooleanBoundsExample.cpp` and `TestSmSTEPDerivativesExample.cpp` compile
   `upstream/Examples/SM_API/boolean_bounds.cpp` and `upstream/Examples/SMLib/step_derivatives.cpp`.
   `Examples/` is outside the port's source allow-list, so those two files are excluded and
   replaced by functions that print `SOLIDCORE-STUB SKIPPED: ...` and return `SM_SUCCESS`
   (the runner counts them as Successful; the counts below subtract them).
6. **sm_api_single** (harness addition, not upstream): the test sources are built once as an
   object library; `SM_API_test.cpp` is compiled twice, with `SM_API_TEST_STANDALONE` (upstream
   `main`) for `SM_API_test_app` and without it for `sm_api_single`, whose `main` is generated
   from the `stat = Test...();` calls in `SM_API_test.cpp` and applies the same global settings as
   `run_sm_api(FALSE)`.
7. **prog_test**: built as one executable from all `tests/prog_test/src/*.cpp` (upstream: a shared
   lib `prog_test` + `prog_test_app.cpp`). Windows-only defines (`SM_CLASSIFY_UPDATE`,
   `SM_TOLERANT_WATCH`) are not set, as on upstream Linux. `APPEND_GIT_BRANCH` is not defined, so the
   generated `_build/generated/SmGitBranch.h` is not needed; `NCLIB` (`machineBrep.h`) is not defined.
8. **Test data / cwd.** Tests open `../../TestFiles/pt_TestFiles/...` and prog_test writes
   `../prog_test/OutputFiles/...`, relative to `tests/<name>/`. `run_tests.sh` rsyncs
   `upstream/TestFiles/pt_TestFiles` (26 MB) into `bin/cpp/run/TestFiles/pt_TestFiles` and runs from
   `bin/cpp/run/tests/<name>/`, so nothing is written into `upstream/`.

## Running tests

```sh
harness/cpp/run_tests.sh sm_api                          # whole SM_API suite (upstream runner)
harness/cpp/run_tests.sh sm_api_single --list            # 114 test names
harness/cpp/run_tests.sh sm_api_single TestSmBoolean     # one (or several) SM_API tests
harness/cpp/run_tests.sh prog                            # all default prog_test suites
harness/cpp/run_tests.sh prog --list-suites
harness/cpp/run_tests.sh prog --suite fillets            # one prog_test suite (repeatable)
```

`sm_api_single` prints `<name>: PASS|FAIL (status N)` and exits non-zero if any selected test failed.
prog_test redirects stdout into `bin/cpp/run/tests/prog_test/OutputFiles/progTest_<M_D_H_MIN>.txt`
(and `_long.txt`) once suites start; `run_tests.sh prog` prints the `Failed: <suite>` lines and the
final `PROG_TEST SUCCESS|FAILED` from the newest log. Exit status 0 = all selected suites passed.
prog_test granularity is the suite: each suite function returns one status. The smallest unit
selectable without editing upstream is a suite.

## Baseline results (2026-10-12, build above)

### SM_API_test_app

`RESULT: PASS`, exit 0, 50 s wall (run concurrently with prog_test).

- Runner counts: **115 Successful, 0 Failed** (115 invocations).
- 115 invocations = 114 distinct tests (`TestSmIntersectBrepWithPlane` is invoked twice by upstream).
- 2 of the distinct tests are stubs (`TestSmBooleanBoundsExample`, `TestSmSTEPDerivativesExample`).
- **Real baseline: 112 distinct upstream tests pass, 0 fail, 2 skipped (stubbed).**
- Each of the 114 names was also run in isolation with `sm_api_single`: all exit 0
  (`bin/cpp/logs/sm_api_individual.txt`), so no test depends on state from an earlier test.
- Failures: none.

### prog_test_app

Default run (16 suites, `stitch` is not run by default): `PROG_TEST SUCCESS`, exit 0, 46 s wall
(internal clock 41.8 s). No `Failed:` lines.

Per suite, each run alone (`--suite <name>`):

| Suite | Result | Wall |
|---|---|---|
| topology | pass | 0.6 s |
| booleans | pass | 1.0 s |
| offset | pass | 12.6 s |
| fillets | pass | 14.8 s |
| local-ops | pass | 0.1 s |
| sweeps | pass | 1.9 s |
| primitives | pass | 0.2 s |
| tessellation | pass | 2.4 s |
| unit | pass | 1.7 s |
| trimmed-surfaces | pass | 1.6 s |
| brep-import | pass | 0.1 s |
| ssi-analytic | pass | 0.1 s |
| ssi-advanced | pass | 0.8 s |
| cci-advanced | pass | 0.1 s |
| section-advanced | pass | 0.1 s |
| silhouette-advanced | pass | 0.9 s |
| stitch (not default) | pass | 0.1 s |

**17/17 suites pass, 0 failures.** The logs contain expected diagnostic text such as
`Fillet Corner Error: ... Failure` from negative-path fillet cases; these do not fail a suite.

## Notes for other agents

- Link order for FFI: `-lSM_API -lSMLib -lstdc++ -lm -lpthread`; `bin/cpp/ffi.env` already says this.
- Shim code should be compiled with the same defines as above (in `ffi.env`). `SM_GRAPHICS_CALLBACKS`
  is defaulted in `SmConfig.h` when unset (1 in debug-code builds, else 0), so shims must keep
  `SM_GRAPHICS_CALLBACKS=1` to see the same configuration as the libraries.
- prog_test log file names contain the local time, and the duration lines use `clock()`; diff logs
  with those lines removed.
