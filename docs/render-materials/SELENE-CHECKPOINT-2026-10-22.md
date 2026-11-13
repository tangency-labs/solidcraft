# SELENE renderer checkpoint - 2026-10-22

## Result

The selected native studio image is complete: `assets/render/selene/exports/SELENE-studio-4k.png`, 3840 x 2400, 1,024 samples per pixel, maximum path depth 16, ACES-fitted presentation and edge-aware a-trous denoising. Its exact public source/request/resource manifest is `assets/render/selene/scenes/studio-hero.json`. The PNG SHA-256 is `40a492e7979137d0f4027300da3d8af7ca1008c1aab2c309259e65d309a17316` (2,388,114 bytes).

The latest functional master was verified at `cf8e651022deee528b26c303fde58bf3eb8a22a5`, with native-file SHA-256 `3bd2bfa1c6336394c4a4208d30b52c80d06658a3bd7ea57cc93b774411d1bd68`. It evaluates 2,116 bodies with zero timeline errors/warnings. Its original saved visibility contributes 2,094 bodies and 2,144,099 triangles; one passive render-only floor contributes two more triangles. No simplification, source-file mutation, pose change or engineering-material reassignment was used.

Twelve explicit closed glass bodies, 48 exact PBR body bindings across six authored surface families, the original RIG-002 emitters and HDR-002 fill are used. The rig's 1/16 creative radiance gain is disclosed, not confused with the original calibration. Full-frame plus native-pixel detail review accepted the composition. Residual Monte Carlo grain remains visible in some glass/vial pixels; this is not proof of zero noise or measured optical fidelity.

## Source separation

- Native executable generating the image: source `895704a2fdc96dabb9a2afb2e548a8896e4b1e79`, tree `d16a5c4d132e8169f31d24ca9673d6b23e53e1cf`, binary SHA-256 `3421d538c571647c1f24fd74d63487c4ba4ab597980cfeea111a01c0db94e699`.
- GPU envelope regression reconciliation: `edfe6291` changes tests, not production transport.
- Reproducible scene/camera authoring: `56b0005f`.
- Selected native export and manifest: `be930718`.
- Final UI repair/package source: `fb0d7a42`, tree `0bec3bc30e91550709d0494a2f7c5b15a7f57d41`. The closing gate verified that tree unchanged. Later checkpoint documentation does not change the tested renderer/UI code.

The personal branch is `lab/render-scale-resources` in `0xlillith/solidcraft`, draft PR #7. Upstream, main and releases are unchanged.

## Completed validation

| Scope | Result |
|---|---|
| CPU-feature renderer suite | 457 passed, 0 failed |
| Complete hardware/Vulkan GPU suite | 95 passed, 0 failed, 3 explicitly ignored |
| Render-workspace harness | 58 passed, 0 failed; repeated with hardware-only Vulkan policy |
| Whole-workspace/all-target Clippy | PASS with `-D warnings`, repeated on the final code checkpoint |
| Official 12-crate library/web WASM check | PASS, repeated on the final code checkpoint |
| Native public-command resource/persistence workflow | PASS |
| Pure Python camera/publication tests | 7 passed |
| Resource attribution | 760 collected files, all attributed |
| Icon tooling | 17 tests passed |
| Selected PNG and downloadable package | Native frame, source hashes, resource hashes, bounded scanlines and CRCs verified |

The old conditional unsaved-status label shifted Reload and the CPU profile control between pointer events. Its row now has stable geometry. Existing clicked reload/profile tests and the new hit-target regression pass. The material-binding negative fixture now exercises the explicit 128-binding capacity instead of the obsolete limit of 64.

## Deliverable and reproduction

Read `assets/render/selene/RENDER.md` and `tools/render-library/README.md`. The original `selene_studio.py` evaluates the pinned master once and reuses one explicitly installed native GPU host; `selene_recipe.py` supplies the reviewed scene and analytic all-body camera fit. `publish_selene.py` requires an explicitly reviewed completed frame and never resamples it. `package_selene.py` verifies and copies only the PNG, public manifest, README and checksums; it compiles nothing and installs no dependencies.

Successful artifact-only workflow: `39787390664`, artifact `12428754554`, archive SHA-256 `74b94e95eb226a65853c424ec6622da641c765df6c63f48cf7765a315aa94d56`. Package success is not new rendering or whole-CI evidence. Native logs, draft renders, unscaled inspection crops and receipts stay outside the repository under `D:/codex-tools/render-continue-20261022`. The controlled native studio is closed; the editable master hash is unchanged.

## Still open

The full renderer roadmap is not complete. Do not mark quantitative convergence, adaptive sampling, complete CPU/GPU transport parity, EXR/linear-output production workflow, all 600 conceptual materials, all authored scenes, unimplemented advanced optical/physical features, an interactive GUI acceptance session or whole-project CI as finished. The next work must use the actual capability matrix and test each executable subset rather than promoting authoring specifications to runtime claims. This checkpoint delivers a real high-quality full-detail SELENE image and verified renderer/UI improvements, not a blanket release approval.
