# General

Board: `Prefs2_General` (1920x1436). Heading: none (top of the page list). Subtitle "Profiles,
language, files and updates." Tag: **In profile**, as drawn (a profile holds the language; the other sections are
not in a profile). Not shown on the board: everything in the
"Controls with no board" list of the README.

Page title and tag: "General", "In profile" (hover "Saved in settings profiles").

Layout clarification from the supplied HTML version of the same design: saved profiles are
42 px rounded, bordered cards with 8 px gaps and a selected fill/accent border on the active
card. Export, Rename and Delete are quiet compact icon actions. The save row starts with the
220 px independent name field (placeholder "Name this setup"), then primary Save as new and
Import. Held and This computer use aligned 84 px label columns: Held pills have accent outlines
and page icons; machine pills and removable host pills have neutral outlines, with lock icons
on machine pills. Section heads have no pictograms. Autosave's 260 px timeline sits under its
control column, not across the page. Search sits in the window title bar; the shared footer
retains Apply alongside Cancel and OK for the existing transaction behavior.

## Profiles (3 saved)

Source: today's General > Profiles section (`crates/ui-egui/src/settings_profiles.rs`, #6237). Section
title "Profiles", with the count "3 saved" at the right of the title.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Profile rows (Laptop, Large monitor, Presentation) | List | `app.settings_profiles` (saved list) | none saved | Icon buttons: "Export", "Rename", "Delete" | none (17 px profile icon) | The active row shows badges "Edited" (when changed since saved) and "Active", and no Apply. Other rows show "Apply". Names are sample. |
| Apply | Small button per row | settings profile apply | n/a | none | none | Applying replaces what a profile holds (see README). |
| Profile name | Text field (220 wide) | the name for "Save as new" | empty | none | none | Placeholder "Name this setup"; aria-label "Profile name"; no separate label column. |
| Save as new | Button | settings profile save | n/a | none | none | |
| Import... | Button | settings profile import | n/a | none | none | |
| Held / This computer chips | Two chip rows | none (information) | n/a | none | none | "Held": Appearance, Layout, Navigation, Sketch, Shortcuts, Language. "This computer": 3D mouse, Graphics, Assistant. Matches `settings_profiles::PAGES` plus language, shortcuts, toolbar, S-box favourites and the imported theme; mismatch: the board lists page names, the code lists the Interface, Navigation and Sketch pages (`settings_profiles::PAGES`), so after the page split map Appearance and Layout to the old Interface fields. |

## Language

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Interface language | Dropdown, 280 wide ("English (United Kingdom)") | `Prefs.language` | `"system"` (System) | none | none | The dropdown includes System and `i18n::LOCALES`; the effective language follows the operating system when System is selected. Page: 0 (General). |

## Start and save

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Reopen the designs that were open last time | Checkbox | `Prefs.reopen_designs` | `true` | none | none | Today's label is longer ("Reopen the designs that were open when SolidCraft was last closed"). |
| Autosave every | Number field (64 wide) with "minutes", an (i) and a "-30 min ... now" strip | `app.autosave_minutes` (not a `Prefs` field; `set_autosave`) | session value | "0 turns it off" | none | The strip under the field ("30 min ... now") is today's `settings_autosave_timeline`. aria-label "Minutes". |

## Files

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Largest file to open | Two side-by-side 72 px number fields, each with MB | `Prefs.max_step_file_mb`, `Prefs.max_design_file_mb` | `DEFAULT_STEP_MB`, `DEFAULT_DESIGN_MB` | existing file-size guidance | none | One group label; short captions "STEP and import" and "Design" above the controls. Wrap on narrow pages. Each editor retains its full independent accessible name and search result. |
| Allow network locations in designs | Checkbox | `Prefs.network_locations` | `false` | "For organisations that keep designs on file servers. Off: only servers you allow." | none | Moves from Design (`PAGE_DESIGN`). |
| `\\files.studio`, `\\nas01` | Chip list with a remove icon (11 px) | `Prefs.network_hosts` | empty | none | none | Shown only when network locations is on. Names are sample. |
| Open files in a slicer that is already running | Checkbox | `Prefs.slicer_single_instance` | `true` | "For slicers that load a file into a running window." | none | |
| Print face highlight | Slider, "60 %" readout (44 px column) | `Prefs.print_face_highlight` (0.0 to 1.0) | `0.2` (20 %) | none | none | aria-label "Print face highlight". The sample shows 60 %. Moves from Design. |

## Updates

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Up to date . today 09:12 | Success badge (11 px icon) | `Prefs.update` memo (read only) | n/a | none | none | Sample time. |
| Check now | Button | the update check command | n/a | none | none | |
| Check at start and once a day | Checkbox | `Prefs.check_updates` | `true` | none | none | |

## Crash reports

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| 2 saved | Text count | crash report store | n/a | none | none | Sample count. |
| Open crash reports... | Button | opens the crash reports | n/a | none | none | Programmatic calls must not open a dialog. |

## Footer

"Restore page defaults" (`reset_preferences` for this page), "Cancel", "OK". The page-list profile
card ("Profile", "Laptop", dot with hover "Changed since saved") is the same on every page.

## Mismatches

* No control on the board for `parameters_apply_live` or `personal_sheet_rules`.
* Print face highlight and Largest file come from Design; network locations comes from Design.
* "Profiles" section details (what the Held chips mean) are specified in `settings_profiles.rs`; the
  board is the only authority for the layout.
