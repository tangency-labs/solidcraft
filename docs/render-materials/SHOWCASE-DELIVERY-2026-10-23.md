> Historical evidence from the personal donor, not validation of this upstream integration.
> Production source deliberately omits generated review media and distribution ZIPs.
> [Immutable complete delivery](https://github.com/0xlillith/solidcraft/tree/aea20b0115c402209fb2149558830e95f406ac0e/assets/render/showcase/aging-effects-20261023)
> and [package](https://github.com/0xlillith/solidcraft/tree/aea20b0115c402209fb2149558830e95f406ac0e/distribution).

# Completed collaborator showcase delivery - 2026-10-23

## Public downloads verified without authentication

- [MP4: 1080p, 30 fps, 2 minutes 2 seconds](https://raw.githubusercontent.com/0xlillith/solidcraft/970e11b26181bdc3c755c154b7db45acf088c9aa/assets/render/showcase/aging-effects-20261023/SolidCraft-Aging-Effects-Showcase.mp4)
- [Complete ZIP: video, renders, screenshots, editable scene and thirteen shots](https://raw.githubusercontent.com/0xlillith/solidcraft/970e11b26181bdc3c755c154b7db45acf088c9aa/distribution/SolidCraft-Render-Showcase.zip)
- [Actual new Backgrounds-page screenshot](https://raw.githubusercontent.com/0xlillith/solidcraft/970e11b26181bdc3c755c154b7db45acf088c9aa/assets/render/showcase/aging-effects-20261023/screenshots/ui-backgrounds-final.png)
- [Source showcase guide](https://github.com/0xlillith/solidcraft/blob/aea20b0115c402209fb2149558830e95f406ac0e/assets/render/showcase/aging-effects-20261023/README.md)
- [Pull request #11](https://github.com/0xlillith/solidcraft/pull/11)

The immutable media commit is `970e11b26181bdc3c755c154b7db45acf088c9aa`. The personal branch is `lab/render-aging-showcase`; later documentation-only commits do not change these deliverable bytes. The local worktree and remote branch matched after publication. No main/upstream branch, release, branch protection or Actions configuration was changed.

The published MP4, ZIP and Backgrounds screenshot were downloaded again over public HTTPS without a GitHub token. All three returned HTTP 200 and matched their exact accepted local byte lengths and SHA-256 values. The downloaded ZIP passed its complete CRC check with 78 members. Before publication, every member had also been checked against the canonical staged Git source blobs, including LF-normalized text; the package is not dependent on the producer workstation's line endings.

| Deliverable | Bytes | SHA-256 |
|---|---:|---|
| MP4 | 11467697 | `227ecf8d5dc15ef9b7bbafb1d56ff239ce88b5b01e9488d6a9d96c25fcf401e4` |
| ZIP | 54107781 | `1183f4c1be9cef9942f64eca56688a8ae13a0758948f357d1742c72b4f5943ff` |
| Backgrounds screenshot | 1003257 | `cd28e2a7aa2753d58d7fc0727dcf158b806f1fa09783352c51e98b1f2a9cdcf9` |

## What is closed

The selected video is complete: 16 chapters, 122 seconds, 1920x1080, 30 fps, 3,660 frames, H.264/AAC and fast-start layout. It uses actual native renders and actual application screenshots with English collaborator captions, image-only crossfades/comparison wipes and original synthesized background music. It is an edited showcase, not real-time rendering or physical animation. An older verified SELENE 4K reference is explicitly labelled rather than presented as a newly aged rover.

Every frame decoded successfully. Actual encoded midpoint frames from all sixteen chapters, transition samples and the final fade were inspected. Decoded AAC was finite, correctly timed and below clipping; this is numerical audio integrity, not a claim of subjective full-length audition. The first draft encoding's overlapping transition captions was corrected before the selected final encoding. Intermediate drafts are not distributed.

All nine original production shots and four added background shots reopened with identical canonical saved settings in both the older render-producing application and the newly built Backgrounds-UI application. Both isolated native processes exited normally with code zero, without OS termination. One absent named Cyanotype shot was restored from its retained explicit recipe before the roundtrip; native defaults and optional-null normalization are documented. The old window's preference-hash baseline follows its disclosed control reconnection; the new isolated profile also preserved personal preferences.

The new Backgrounds page groups camera solid/gradient presentation, physical floor/wall/cyclorama and a separate HDR-lighting link. Native UI actions rendered a real image, changed the gradient by exact-master regrading without changing the raw EXR, applied the 26-panel cyclorama and cleared it while preserving the camera background. The fixed Clear Stage implementation forwards the entire unsaved numerical draft instead of loading unrelated older saved settings.

The video contains three explicit background chapters. Solid and radial-gradient presentations share exactly the same EXR. Physical floor/wall and cyclorama change transport, shadows and reflections. The cyclorama is the declared twenty-four-strip cove approximation, not an analytic smooth infinite surface. The UI explains why a large physical stage hides a camera background, why Live Preview is not final finishing and why an HDR is not automatically a visible panoramic camera backdrop.

## Tested source and evidence boundaries

- Original render-producing application: `9f8c3d3031737b19a5bb1bf8d3802f3af6915c1e`.
- Backgrounds UI: `9bc71b298719bbe62e1bdbc49eba45f6863e55d0`, exact app SHA-256 `2acc780b33a6fe7e75e306cfe51de5881eaf93496a2c85fe1828c80386e13ebf`.
- New Render-workspace harness: 93 passed, 0 failed. Ribbon 27, localization 29 and kit guards 16 passed. Strict all-workspace/all-target Clippy, a fresh native build and the official 12-crate WASM check passed on the frozen source.
- Earlier renderer core: 482 CPU-feature tests and 271 default-feature tests passed. Corrected full hardware GPU suite: 115 passed, 0 failed, 3 explicit ignores. These are distinct suites, not an aggregate project CI certificate.
- Final shipped asset gate: 836 collected files, all attributed. Icon tooling: 17 tests passed. No font files, application/encoder binaries, control credentials, personal settings or raw intermediate EXR pile are bundled.

The user-facing studio guide, source recipes, manifests, exact export hashes and selected screenshots are published alongside the editable sample. `tools/render-showcase/package.py` verifies source identities before packaging. `distribution/SolidCraft-Render-Showcase.json` describes the verified ZIP.

## Remaining external state, not hidden by this delivery

GitHub Actions dispatch still returned HTTP 422, `Actions has been disabled for this repository`, while the permission endpoint returned `enabled: true`. The observed discrepancy is documented without changing account or repository settings. Local executed acceptance is not represented as a new remote CI pass. PR #11 remains draft; this completes the requested video/background/roundtrip delivery, not every advanced-renderer roadmap item or an upstream release.
