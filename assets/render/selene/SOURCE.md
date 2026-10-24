# SELENE native source and lineage

The owner supplied **`SELENE-showcase.solidcraft`**, the improved mechanism demonstration published by **SychPL (Mateusz Sych)**. Its bytes match the file added to the author's `media/selene-showcase` branch on 2026-10-21. The unchanged source is retained at [source/SELENE-showcase.solidcraft](source/SELENE-showcase.solidcraft); [source-lock.json](source-lock.json) pins its identity and records the review.

The functional showcase is a new derivative of this file. The older `Rover-v7.solidcraft` remains a historical reference, not the modeling input.

## Exact identity

| Field | Verified value |
|---|---|
| Author repository | `SychPL/solidcraft` |
| Source branch | `media/selene-showcase` |
| Commit | `32ec1720447e3fcc6bb6cc53a63fb5b39cfa21b9` |
| Published timestamp | `2026-10-21T20:00:01Z` |
| Repository path | `SELENE-showcase.solidcraft` |
| Git blob SHA-1 | `a06fee2c8a839f5efccc75ae6de4e68f951cb266` |
| SHA-256 | `1b0df486f30da157db27a33e0b5ed67f13d527d4dfdc6c334717a1df4e5b8677` |
| Length | 314,452 bytes |
| Native container | ZIP with `document.json` and `container.json` |
| Native source format | `solidcraft/23` |
| Units | Millimeters; Z up. Stored angles are radians; joint commands accept degrees. |

The [source commit](https://github.com/SychPL/solidcraft/commit/32ec1720447e3fcc6bb6cc53a63fb5b39cfa21b9) identifies the design as Rover-v7 with closed-loop actuators, links, gears and the SELENE tour study. Both the Git blob hash and SHA-256 were computed independently from the supplied attachment on 2026-10-22.

Read-only container inspection found **3,378 features, 47 component definitions, 47 occurrences, 49 joints, nine motion links and one motion study**. These are source-document counts. They do not establish that every feature evaluates, that every body survives the timeline, or that the assembly is collision-free. Native evaluation and derivative checks are separate evidence.

## Following the original clue

The owner's [exact pinned mechanism reference](https://github.com/tangency-labs/solidcraft/blob/ac1768e08b83ed33212459951f5b50aa7a57cddc/skills/solidcraft-mechanisms/reference.md) describes the SELENE solar-actuator loops, wheel and solar motion links, grounded modules and visibility treatment. It also specifies command units, joint-origin conventions and validation methods. Those descriptions led to the author's showcase history.

| Evidence | What it establishes |
|---|---|
| [Native source at `32ec1720`](https://github.com/SychPL/solidcraft/blob/32ec1720447e3fcc6bb6cc53a63fb5b39cfa21b9/SELENE-showcase.solidcraft) | The exact improved design supplied by the owner. |
| [Parent media commit `df2e7423`](https://github.com/SychPL/solidcraft/commit/df2e74230e6659409d65501b5d9396b55c6503d3) | Three earlier GIF demonstrations, with credit to 0xlillith and Apache-2.0/MIT for the original model. |
| [Author's showcase comment on PR 1540](https://github.com/tangency-labs/solidcraft/pull/1540#issuecomment-6205857321) | The combined demonstration and the motivation for its added mechanisms. |
| [Motion export PR 1680](https://github.com/tangency-labs/solidcraft/pull/1680) | The exact showcase filename and tour/export examples. |
| [Original rover repository](https://github.com/0xlillith/solidcraft-rover/tree/b32437915372d32b71419dc8f5143fe22ff1e869) | The owner's original Rover-v7 model and its attribution. |

The source lock preserves the earlier bounded public search as historical evidence. That search had inspected the media branch before the native file was added. Its former `awaiting_native_source` outcome is superseded by the supplied file and the new author commit; it is not a current blocker.

## Authorship and notices

The [original rover attribution](https://github.com/0xlillith/solidcraft-rover/blob/b32437915372d32b71419dc8f5143fe22ff1e869/ATTRIBUTION.md) offers the original model under **MIT OR Apache-2.0** and credits the SolidCraft emblem to SolidCraft contributors. The original modeled lettering follows **Ubuntu Light** outlines by Canonical Ltd., under the **Ubuntu Font Licence 1.0**. Its exact notice is retained in [licenses/UFL.txt](licenses/UFL.txt). No font binary is added.

SychPL is credited for the improved assembly, helper geometry and showcase study.
On 2026-10-23T22:13:37Z, [SychPL explicitly licensed their additions](https://github.com/tangency-labs/solidcraft/pull/5494#issuecomment-6375788360)
under **MIT OR Apache-2.0**, at the recipient's option. The grant expressly covers
redistribution and derived files, including `SELENE-functional.solidcraft`.
The author media branch itself has no separate licence file; the linked public
author declaration supplies the previously missing permission. The original
rover and Ubuntu Font Licence notices above remain applicable and unchanged.

New procedural geometry, English documentation and command generators in this derivative are original contributions. Raspberry Pi references inform dimensions and component identity; manufacturer CAD, logos, PCB artwork, electrical netlists and photographs are not embedded. No commercial CAD assets or GPL geometry implementations are imported.

## Authoring runtime and format compatibility

The reconstruction workflow selects the official **SolidCraft v0.18.2 Linux x86_64 CLI**. Its [published release](https://github.com/tangency-labs/solidcraft/releases/tag/v0.18.2) identifies source commit `4927214976ff20758fde35af03bd6b2d6cf73e18`. The release archive SHA-256 is `9d0c922ac72204f34ad22ca7a171be78016ab002e304ace3c07d34d187d830ff`; the extracted CLI SHA-256 is `e50fe78292d569c36052b24f46f1732bc4e43d1ecdac6a64092aa1cee3b97086`. The executor is an external development dependency and is not bundled as a model asset.

This runtime writes **`solidcraft/28`**, which the personal render branch's format-29 reader can migrate. The unchanged source remains in format 23. The newer official v0.19.0 writes format 30, beyond that render branch's reader, so its isolated component fixtures remain development evidence rather than the authoritative final build. Final acceptance must name the actual executor and final model hash; success in a different runtime does not replace that run. No native format marker is rewritten by these tools.

## Reproducibility boundaries

- Keep the pinned source unchanged. Author geometry, appearances, components, joints and studies through SolidCraft commands into a separately named derivative.
- Record the executor version and binary hash with each native build. A newer native format is not backwards compatible merely because geometry looks similar. Do not edit a file's format marker to bypass this restriction.
- Bind exact evaluated body names and component identities. Feature declarations alone are not a body inventory.
- Verify joint movement and followers against independent calculations. A successful command without actual movement is not a successful mechanism test.
- Preserve execution failures and incomplete checks explicitly. Do not carry validation claims from a lost intermediate checkpoint into a newly rebuilt model.
- A native model suitable for inspection and render preparation is not a certified manufacturing design, an electrical design, or a physically simulated lunar rover.

The prepared appearance catalog, scene specifications and future dynamics sequence have their own evidence status. Admitting this source does not automatically bind or validate those resources.
