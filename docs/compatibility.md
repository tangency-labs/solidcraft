# Compatibility with earlier names

SolidCraft was first called SolveCraft. That name is gone: there is no `solvecraft-cli`, no
`SOLVECRAFT_*` variable, no `.solvecraft` file association and no pre-rename settings-folder
copy. What stays is what an existing design or install still needs:

- Designs saved by any earlier release open, whatever their file name, because opening goes by
  the file's contents (section 2).
- The `.solidcraft` extension (saved before designs moved to `.sc`) is still a design extension
  (section 1).
- The app-ID move in the packages (`ai.storyteller.solidcraft` to `io.tangencylabs.solidcraft`,
  [Linux app ID](#linux-app-id)).

`git grep -i solvecraft` should list only: the content tags in section 2, the tests that pin that
a file under the old name opens by contents, this file, and changelog / release notes. When you
add or remove a shim, update this file in the same PR.

## 1. `.solidcraft` design files

New designs are saved as `.sc`. Files saved before that as `.solidcraft` still open everywhere.
One constant names the extension new designs get, `DESIGN_EXT` (`sc`), and one list names every
extension a design may have, `DESIGN_EXTS` (`sc`, then `LEGACY_DESIGN_EXTS`: `solidcraft`); code
uses those, never its own string.

| Path in | Where |
|---|---|
| Extension → design format | `crates/io/src/lib.rs` (`DESIGN_EXTS`, `is_design_ext`, `is_design_path`, format detection) |
| Open dialog filters (desktop and browser) | `crates/io/src/formats.rs` (the design row lists every entry of `DESIGN_EXTS`), shown by `apps/solidcraft/src/main.rs` (`services`) |
| Drag-drop, recent files, `doc.open`, MCP `open`, CLI `eval`/`run`, Insert/Link/Relink design | go through the same format detection in `solidcraft_io` (`is_design_container_path` for links) |
| Browser file picker | `apps/solidcraft-web/src/disk.rs` (`open_options`, `opens_design`) |
| Windows | `assets/platforms/windows/solidcraft.wxs`: `.sc` is the design ProgId's extension; `.solidcraft` points at the same ProgId and is in `SupportedTypes` |
| macOS | `assets/platforms/macos/Info.plist.in`: `sc` and `solidcraft` in the design document type and its `io.tangencylabs.solidcraft.design` UTI |
| Linux / FreeBSD / Flatpak / Snap | `assets/platforms/linux/io.tangencylabs.solidcraft.mime.xml`: `*.sc` and `*.solidcraft` globs under `application/x-solidcraft`; the `.desktop` file names that type |

Opening a `.solidcraft` file and pressing **Save** writes that same file: nothing is renamed behind
the user's back. **Save As** (and every new design, autosave, crash report and component
extracted to its own design) offers `.sc`, and leaves the old file as it was.

Recent-files entries that point at an older file keep working: they are plain paths and
open through the same detection. Autosaves and crash reports that earlier releases wrote
(`<id>.solidcraft`, `design.solidcraft`) are still listed and recovered
(`solidcraft_engine::recovery`, `solidcraft_engine::crash::read`), and the browser app still
recovers `recovery.solidcraft` from its storage. The designs in
`apps/solidcraft-cli/tests/corpus` keep their `.solidcraft` names on purpose: they are files saved
by earlier releases. Tests: `crates/io` (`Format::from_name`, `is_design_path`, the import filter
groups), `saving_an_opened_legacy_design_keeps_its_name` (engine),
`autosave_from_a_legacy_folder_is_listed_and_loads`, `autosaves_are_written_as_sc` and
`a_report_from_an_earlier_release_still_finds_its_design`.

## 2. Old files open by contents

A design saved under the name SolveCraft, with the `.solvecraft` extension, is not associated
with the app any more, but it opens like any other design: the loader looks at the file's
contents (the ZIP signature of a container, else JSON), not at its extension, so opening it by
path, dragging it in, or `doc.open` / MCP `open` / `solidcraft-cli eval` all work. The container
format is the same as `.sc`. Tests: `legacy_extension_and_format_tag_still_open` (CLI,
`run_results.rs`, a JSON and a container under `.solvecraft`) and
`legacy_design_files_open_by_contents` (MCP `open`).

Designs record their format as `"format": "solidcraft/N"`. Files saved by builds up to 0.5 say
`"format": "solvecraft/N"`; `crates/doc/src/format.rs` (`version_of`) reads both prefixes and
upgrades them as usual (the file is re-tagged `solidcraft/N` when saved). Not
backward-compatible: builds up to 0.5 don't know the new tag and refuse files saved by later
builds. Test: `version_of` accepts both prefixes; legacy-tagged designs load and are re-tagged on
save. The container marker `solvecraft-container/1` of the #303 draft is accepted the same way
(`crates/io/src/design.rs`; test `draft_container_marker_remains_readable`).

## 3. Environment variables and folders

Every runtime variable is read through one helper, `solidcraft_geom::env` (`var`, `var_os`):
`SOLIDCRAFT_<NAME>` only. The shell entry points read `SOLIDCRAFT_*` only too
(`packaging/linux/AppRun`, `packaging/env.sh`).

Settings and recovery use the SolidCraft name:

| | Linux | macOS | Windows |
|---|---|---|---|
| Preferences, graphics settings | `~/.local/share/solidcraft` | `~/Library/Application Support/SolidCraft` | `%APPDATA%\SolidCraft\data` |
| Recovery (autosave) | `…/solidcraft/recovery` | `…/SolidCraft/recovery` | `%LOCALAPPDATA%\SolidCraft\recovery` |

Folders of releases before the rename are not read or copied. In the Flatpak sandbox these folders
live under `~/.var/app/<app id>/data`; for data kept under the app IDs of earlier releases, see
[Linux app ID](#linux-app-id). The copy helper is `solidcraft_io::adopt_legacy_dir`, used for
those (`adopt_legacy_settings`, `solidcraft_engine::recovery::default_dir`). Tests: `legacy_dir`
(copy once, never overwrite, old folder intact), `first_launch_adopts_the_previous_release_settings`,
`autosave_from_a_legacy_folder_is_listed_and_loads`, `recovery_dirs_are_named_per_platform`,
`stored_prefs_come_from_the_current_key_only`.

## 4. CLI and MCP

The CLI is `solidcraft-cli`; there is no forwarding binary under the old name. MCP
`serverInfo.name` is `solidcraft`, and clients set up with:

```sh
claude mcp add solidcraft -- solidcraft-cli mcp
```

Resources are listed as `solidcraft://design` and `solidcraft://commands`.

## Package IDs

| Package | Decision | Why |
|---|---|---|
| Flatpak app ID / Linux desktop ID | changed in 0.42 from `ai.storyteller.solidcraft` to `io.tangencylabs.solidcraft` | The project's own reverse-DNS domain. The `.desktop`, metainfo and MIME file names, the icon name, and the Wayland `app_id` / X11 class follow it. Existing installs are moved over; see [Linux app ID](#linux-app-id). |
| macOS bundle ID | changed to `ai.storyteller.solidcraft`, then to `io.tangencylabs.solidcraft`; the CLI's signing identifier is `io.tangencylabs.solidcraft.cli` (was `ai.storyteller.solidcraft-cli`) | No updater depends on it; the app is drag-installed and replaces the old `SolidCraft.app`. Settings live in folders named after the product, not the bundle ID, so they stay. macOS ties privacy permissions (such as access to Documents or Downloads) to the bundle ID, so it asks for them once more after the update. |
| MSI `UpgradeCode` | **kept** (also through the `io.tangencylabs` move; the Windows build sets no AppUserModelID and its ProgIDs `Solidcraft.Design` / `Solidcraft.Model` don't carry the app ID, so nothing there changes) | So the new MSI upgrades the old install in place. Product name is SolidCraft; the default folder becomes `Program Files\SolidCraft` (the major upgrade removes the old folder). A custom install folder isn't remembered across this one upgrade. |

## Linux app ID

Releases before the move to Tangency Labs used `ai.storyteller.solidcraft`; releases since use `io.tangencylabs.solidcraft` everywhere (Flatpak,
deb, rpm, AppImage, Snap, FreeBSD, Nix, and the macOS bundle). The settings and autosave folders
(`solidcraft` / `SolidCraft`) are named after the product, not the app ID, so outside the Flatpak
sandbox they don't move. The shims cover the earlier ID:

| Shim | Where | What it does |
|---|---|---|
| End-of-life rebase in the Flatpak repository | `packaging/flatpak-site.sh` | Each release's Pages deploy keeps the old `app/ai.storyteller.solidcraft/<arch>/stable` ref (mirrored from the previous deploy) and marks each with `flatpak build-commit-from --end-of-life-rebase=<old id>=io.tangencylabs.solidcraft` (once the repository has the new ref). `flatpak update` then says the old app is end-of-life and offers to replace it with the new one; accepting installs `io.tangencylabs.solidcraft`, uninstalls the old app, and Flatpak moves `~/.var/app/<old id>` to the new id's folder (leaving a symlink at the old path) on the first launch, so settings and autosaves come along unchanged. |
| AppStream `<replaces>` / `<provides>` | `assets/platforms/linux/io.tangencylabs.solidcraft.metainfo.xml.in` | Software centres treat the new id as the successor of `ai.storyteller.solidcraft` and of its `.desktop` ids. |
| `X-Flatpak-RenamedFrom` | `assets/platforms/linux/io.tangencylabs.solidcraft.desktop` (both old `.desktop` ids) | Desktops that honour it (GNOME Shell) keep favourites and pinned launchers pointing at the renamed entry. |
| Read-only access to the old Flatpak data folders | `--filesystem=~/.var/app/ai.storyteller.solidcraft:ro` in `packaging/linux/flatpak/io.tangencylabs.solidcraft.bundle.yml`; `solidcraft_io::legacy_flatpak_paths` (`PREVIOUS_APP_IDS`, newest first), used by `adopt_legacy_settings` and `recovery::default_dir` | When the new app was installed next to an old one (for example from a `.flatpak` bundle, which has no update feed) instead of through the rebase, its first launch copies settings and autosaves from the newest old app's folder that exists, as in section 3. The old folders are not changed. |
| Old AppImage menu entries | `packaging/linux/AppRun` | An AppImage's first run removes the `ai.storyteller.solidcraft.desktop` entry and icons that an earlier AppImage installed in `~/.local/share` (only entries it wrote itself), so the menu doesn't show the app twice. Test: `packaging/tests/apprun_test.py`. |

deb and rpm need no shim: the package (`solidcraft`) is unchanged, and an upgrade removes the old
`/usr/share/applications/ai.storyteller.*.desktop`, metainfo, MIME and icon files
because the new version doesn't ship them; the post-install script refreshes the desktop, MIME
and icon caches, so no stale menu entry is left. The Snap is unaffected apart from its
`common-id`: snapd names its menu entry after the snap.

Flatpak installed from a downloaded `.flatpak` bundle (no update feed): install the new bundle,
launch it once (settings and autosaves are copied), then
`flatpak uninstall --user ai.storyteller.solidcraft`.

**Removal:** keep the end-of-life refs in the Flatpak repository for at least six months after
the release that moved to `io.tangencylabs.solidcraft` and at least two minor releases; removing
them strands any install that hasn't run `flatpak update` since. The read-only filesystem grants
and `legacy_flatpak_paths` can go at the same time. `<replaces>`, `<provides>` and `X-Flatpak-RenamedFrom` are harmless and can stay.
Tests: `flatpak_twins_map_our_sandbox_data_to_the_old_app_ids`,
`first_launch_adopts_the_previous_release_settings`.

## Flatpak remotes added from the former website address

The feed has moved twice: from the old address, then from `bherbruck.github.io/solidcraft`
when the project moved to `docs.solidcraft.io` (#1920). A remote added from either address no
longer finds updates. Point it at the current one:
`flatpak remote-modify --user --url=https://docs.solidcraft.io/flatpak/repo/ solidcraft`
(use your remote's name), then `flatpak update --user`.

## Split pieces in designs saved before format 14

Builds before format 14 named the pieces a cut splits a body into in the old kernel's shell
order, and the design's later features name the piece they work on by that name (`Body1`,
`Body1 (1)`). SMLib orders pieces otherwise. Such a feature (`shell_piece_order` in the file)
names its pieces so its later features find them: each name goes to the piece the points those
features picked lie on. A name no later feature uses keeps SMLib's order. Designs saved since
format 14 name pieces after their faces and are not affected.

## Edge references after the kernel swap

Designs saved before the SMLib kernel replay their timeline on the new kernel. A face, edge or
vertex reference that no longer finds what it named is reported on its feature ("no longer
exists as it was picked; reselect it"). It is never picked again silently. The new kernel names
some faces differently. A fillet or chamfer edge reference is found at its saved pick point in
two such cases, and only where that point decides exactly one edge:

- One of the edge's two faces is gone, but its feature still made faces of the body (renamed,
  not removed). The edge is the one of the remaining face that the pick lies on, clear of its
  ends (#14959).
- A face is named after the faces around it (`F44:top#[F43:side:l7]`) and that neighbour list
  changed (the face still has one, a different one), so the name finds that face only by its
  stem or finds several faces. For example,
  joining one extrude over another's footprint names the walls it joins differently, and it can
  leave two faces of that stem. The candidates are the edges between faces answering to the
  name's two faces, at least one of them by a neighbour list that is not its own. The edge is
  the one candidate whose exact curve passes through the saved point, within the pick tolerance.
  If no candidate passes through the point, or several do, the reference asks to be picked again.
  A named face that was split into pieces is not renamed: an edge that split with it still asks.
- A face named as a piece (`F1:-y#[F1:+x,F2:+x]`) is whole again, after an edit upstream took
  away what split it, and the piece's neighbour list names the edge's other face, which the name
  finds exactly. No other piece touched that face, so the edge was never split, and the same
  candidate rule decides (#21555). A piece whose list does not name the other face may have
  bordered it only in part: its edge has grown along the whole face, and it still asks (#11731).

Tests: `a_join_that_makes_the_body_bigger_leaves_round_walls_it_never_touched_alone`,
`a_chamfer_saved_under_a_top_faces_old_neighbours_is_found_at_its_pick`,
`a_chamfer_whose_old_name_and_pick_fit_two_edges_asks_again`,
`a_round_beside_a_piece_whole_again_is_found_when_its_qualifier_names_the_other_face`,
`a_round_on_a_piece_of_an_edge_whole_again_still_asks_for_it` (`crates/engine/src/face_name_tests.rs`).

Numbered round faces (#16602): 0.30.1 split a cylinder's or cone's round side into numbered pieces
(`F1:side#2`) and built a round tool's wall in several pieces (`F4:bore:side:1`, now
`F4:bore:side:0`), so a thread saved then names a face that is whole now. A cosmetic or modelled
thread finds the whole face when it is the only face of that stem, is a cylinder or cone, and the
saved pick lies on it (exact geometry, not the mesh). A pick off the face, or a stem that still has
several faces, asks to be picked again. The recovery applies to threads only: any other feature
(a projection, a shell, a hole, a plane) saved under such a piece would take in material the old
piece never covered, so it asks to be picked again, as does a flat numbered face unless its original
construction feature made it. Tests:
`a_thread_saved_under_a_numbered_side_piece_of_a_cylinder_is_found_at_its_pick`,
`a_modelled_thread_saved_under_a_numbered_bore_wall_is_found_at_its_pick`,
`a_shell_saved_under_a_numbered_side_piece_of_a_cylinder_still_asks_to_reselect`
(`crates/engine/src/face_name_tests.rs`).

Numbered pieces (#16535): 0.30.1 split a round face into numbered pieces (`F302:side#1`), and a
design saved then can still name a fillet or chamfer edge that way, though the kernel now builds
the face whole. The edge is found when the name without its piece numbers names exactly one edge
whose faces are each whole, and the saved pick lies on that edge. A pick on another edge, or a
face still in several pieces, asks to be picked again. Test:
`a_chamfer_saved_under_numbered_side_pieces_of_a_peg_is_found_at_its_pick`
(`crates/engine/src/face_name_tests.rs`).

Round edge numbers (#14479): where a fillet or chamfer rounded one of several edges between the
same two faces (`F2:side:c1|F2:side:l1~2`), 0.30.1 wrote an edge of that round in the older form,
with the round's edge number at the end (`F2:top|F3:blend:F2:side:c1|F2:side:l1~2`). That form
reads a trailing number as the whole edge's, so such a name finds no edge as read. A fillet, a
chamfer or a Sketch Project link then reads the number as the round's edge, and takes the one
edge between the other face and a face answering to that round whose curve the saved pick lies
on. A blend of several edges between the same two faces names each of its faces after all of
them, so the pick, not the number, tells those faces apart. A pick on an edge between other faces
asks to be picked again, and so do other references (holes, joints, construction geometry) with
such a name. Test:
`a_chamfer_saved_on_a_rounds_edge_with_its_number_at_the_end_is_found_at_its_pick`
(`crates/engine/src/face_name_tests.rs`).
