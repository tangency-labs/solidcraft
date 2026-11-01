---
name: solidcraft-performance
description: "Use when a SolidCraft change is slow, blocks the window, runs long work, or must build for the browser. Covers keeping the window thread free, measuring a speed-up, the wasm build and what may be JavaScript or Python."
---

# Performance, threads and wasm

## Do not block the window
- Work that can take long (a feature computation, a save, a file load, reading a picture) runs beside the window and is applied when done. Existing examples: `crates/ui-egui/src/save_job.rs`, `crates/ui-egui/src/file_load.rs`, and the agent path in `docs/control-protocol.md` ("Slow commands and the window").
- With the preference "Keep the window responsive while agents run commands" (`agent_off_thread`) on, a design change that takes longer than about 150 ms is evaluated ahead on a copy of the design, then run for real reusing that result. A person's edit made meanwhile goes first; Stop drops the computation and nothing is applied. Keep that contract when you add a long-running command.
- `file.export` and `file.save_mesh` change nothing in the design: they run on a copy and nothing waits behind them.
- Test it: `crates/ui-egui/src/apply_off_thread_tests.rs` drives a long feature through the harness; never depend on wall-clock timing in a test.

## Measure before you claim a speed-up
- Profile first. Time with `solidcraft-cli` from `main` and from your branch, alternating runs, after the load is low. On a shared machine check `cut -d' ' -f1 /proc/loadavg` and wait while it is over 24; build with `nice`.
- Report medians with the range, the machine, and the cases. The pattern to follow is `docs/perf.md`: what was slow, why, the result, and proof that nothing changed (same bodies, volumes, areas and triangle counts).
- Do not rewrite kernel code for speed: the kernel is being replaced, and kernel work is paused. Report a slow case with its file under the `kernel-perf` label (CONTRIBUTING.md, "Where help is wanted most").
- Pull requests run only the tests for what they touch; geometry-heavy tests run in the merge queue. Use `cargo xtask test` with the `ci` profile locally (`skills/solidcraft-tests/SKILL.md`).

## The browser build
- Everything below the apps must build for `wasm32-unknown-unknown`: `cargo xtask wasm` checks every library crate and the web app, and is part of `cargo xtask ci`. Do not add a dependency that cannot.
- Application code is Rust only. The browser requires two pieces of JavaScript: the service worker `apps/solidcraft-web/sw.js` and the small inline scripts in `apps/solidcraft-web/index.html`. Keep them minimal; `cargo xtask web-offline-test` covers the service worker.
- Python 3.11+ is allowed only for asset authoring, lint and review tools (`assets/AGENTS.md`), never as an application dependency.
- Design notes for the browser runtime: `docs/web-runtime.md`, `docs/browser-api.md`.
