# General: SMLib fork commits not cited by solidcore, and what was measured on them (2026-10-31)

**Unit:** general (the opt-in `Kernel::fixes` flags that port fixes from the SMLib fork).

## Scope

- **Fork:** tangency-labs/smlib, all 28 branches as fetched on 2026-10-31 at 18:30 UTC. The fork's `main` head was 83748ba9876902dd8e91ef72849dedc1a2f1dcb9 (2026-10-28).
- **Commits:** every commit past the 1.0.1 base 3fd5b95 that touches `source/SMLib` or `source/SM_API`. That is 174 on the fork's `main` and 137 more on its other branches, after dropping branch commits whose subject matches one on `main`: 311 commits. Only SMLib and SM_API were read as port sources; `capi/` was read for test names and test inputs only.
- **Solidcore:** checked against main 46b22403cd (2026-10-31) and the 53 PRs open then (titles, bodies and diffs). A fork sha counts as cited when a 7- to 40-hex token anywhere in crates, plan, docs, fixtures, scripts or harness is a prefix of it. The measurements below ran on main 68a6b5a00d.
- **SolidCraft's preset:** `fixes()` in `crates/smlib/src/core.rs` at tangency-labs/solidcraft#8526's head, brett19/solidcraft 0b627d637325c67f07d943d812bf0e58d4647b70 (blob 68d628c3). It sets 188 flags on. Its `HELD_BACK` list (33 flags) and the flags newer than its solidcore pin stay off. Each probe ran this preset, the default kernel, and every flag on (labelled "not the preset").

## Counts

185 of the 311 are cited by sha. The other 126 sort as follows:

| Section | Count | Meaning |
| --- | --- | --- |
| A | 31 | Unported behavioural fixes: no flag or record port covers them (statuses below). |
| B | 10 | Status hygiene: not ported, by the ruling of 2026-10-29 cited in the WR-260 and WR-562 addenda. The sites that change results are behind `query_statuses_pass_on`, `fillet_statuses_pass_on`, `brep_edit_statuses_pass_on` and `sweep_statuses_pass_on`. |
| C | 26 | C++-only, nothing to port: build fixes, memory and ownership bugs that arena ids rule out, crash guards the port already has, diagnostics, test instrumentation. |
| D | 53 | Covered by an existing flag or record port that does not cite the sha (bit-for-bit results below). |
| E | 6 | Cited only in PRs open at the time: 158cbe7 and 1612a7d (#2061), 1ff9264 (#1913), 5fbffc9 and 6eecf04 (#1828), 8843967 (#2066). #2061 and #2066 have since merged. |

**F: cited, but only partly ported.**
- d6a6d53: subsumed by #1580's branch, so not ported as written.
- 47507b6 and 8088f5e: the rest of each is status hygiene.
- a759622: its `SmBrep::LocalOperation` status change is not ported.
- a96ce9f: covered by WR-615's `offset_faces` API, but whether that covers all of a96ce9f is not checked.
- 0b4de8c: ported as WR-56's flag; not yet on the fork's `main`.

## Section A

**Measured: no port.** Each passes under the preset on main 68a6b5a00d, and the default kernel still shows the C++ defect.

- **c340962** (#429, box minus a cylinder at dx 0, 3e-4, 3e-5 and 3e-3).
  - **Preset:** every dx gives a closed manifold solid with no Level-2 failures.
  - **Default:** dx 3e-4 is not manifold, with 6 Level-2 failures; dx 3e-5 fails in `make_faceuse` with 1001.
  - **Volumes** against the closed form (2.25 minus the disk-square overlap) × 7.1:
    - dx 0, 3e-4 and 3e-3: within 3e-9 relative;
    - dx 3e-5: 3.4279547 against 3.4279462, 2.5e-6 relative. The circle passes 3e-10 from the box corner there.
- **f02ab6e** (#473, the holed cube of `tests/fillet_wr300_320.rs`).
  - **Preset:** chamfers 6 and 9 give closed manifold solids of 3650 and 3350, with no Level-2 failures.
  - **Default:** volume 4000, not manifold.
  - **Chamfer 4:** 3840 in every leg.
- **1b8c9bc** (#422, the block of `tests/issue_422_round_over_through_hole.rs`).
  - **Preset:** R4 gives a closed manifold solid of 7580.29, with no Level-2 failures.
  - **Default:** not manifold.
- **7ee0876** (the fork's own sphere-cap thicken, as built by `tests/issue_1502_z5_cap_thicken.rs`).
  - **Preset:** both rim shapes, one closed circle and two open half circles, give manifold, Level-2-clean solids of 346.6223894574, against 331π/3 = 346.6223894461.
  - **Port:** `offset_closed_curve_trim_arc` (WR-463, the fork's 34eaa6b) covers it. It lacks this commit's own second-trim-point test, but that test is not needed for this case.

**Reconstruction only.**
- **9fd7e7e** (rig booleans/562). The exact operands exist only in the fork. On a reconstruction from the commit's description (a 60 × 40 × 10 box with its vertical edges rounded r3, united with a 60 × 40 × 1 prism of the same rounds), every leg, the default included, gives a closed manifold solid with the closed-form volume. A pass proves nothing about rig booleans/562.

**Unreachable.**
- **e309858** (#390). `SmApiTessellate` (`SmApiBrep.cpp:679`) calls `SmBrep::ConvertToPolyBrep`, which runs `SmTess::DoTessellation` (`SmTess.cpp:12316`), where the fix runs after triangulation. None of the three has a Rust counterpart or a unit in `harness/callgraph/units.json`. `query::tess` ports only `SmTessSrfCache` and the tessellation drivers.

**On side branches, dated 2026-10-29 to 2026-10-31, still moving.**
- blend-end-event: 55bffe0, 922f333, b6f88e4, e881cdb, plus the G2 tangent-end series 6ca3405, 68f6913, 6d73e4f, b34d61b and a87c502, with its revert 0cc48bd.
- smlib-curved-setback: 124e33d, a47424b, b3a33fe.
- blend-exact-classes: 1866b03, 344b305, 808c7f6, c290e61, f473917.
- smlib-blend-refusals: f63fe26.

**Not yet probed.** On the fork's `main`: 3545008, 8c4bda5, 8ea6a20, 9a160ba, f653ac9 and 1279539. f653ac9 is a new API (per-edge second offsets).

## Section D: bit-for-bit

For each of the 53 entries, the commit's SMLib diff was compared with the Rust code at every site that reads its flag. Nothing was run.

**Outside blend and offset** (34 commits: topology, boolean and geometry):
- 19 exact;
- 11 partial;
- 4 fixed differently. These four are 7a6b4ad, f72c79b, 69d8857 and 7a8f4d3, recorded in #2079.

**Blend and offset** (19 commits):
- **15 exact.** Six of these differ only on error paths or leave one point unchecked.
- **2 partial:**
  - 7ee0876: moved to section A (above).
  - 9504880: its "on target" test uses geometry plus `MARK_MOVED`, where the fork uses a surface mark. This deviation is documented in `crates/solidcore/src/offset/offset_faces.rs`.
- **1 superseded:** 2bc03c6, by a15b5a2, which WR-462 cites.
- **1 covered:** 1ccd2af, whose changes are all in e418750, which WR-665 cites.
- 2412a80's fix is in our default path. Its flag `temporary_radius_setup_errors` also fails the constructor, which the fork's does not. That is solidcore's own extension, and SolidCraft holds the flag back.

The probes of the partial entries are pending. No outcome is claimed for them here.

tangency-labs/solidcraft#8526 comment 6548297899 asks Brett whether SolidCraft wants the fork's version of the four fixed-differently commits (7a6b4ad, f72c79b, 69d8857, 7a8f4d3).
