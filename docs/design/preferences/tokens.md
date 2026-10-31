# Preferences colours and theme tokens

Every colour used on the Preferences boards, taken in light and dark pairs (the dark dialog is the
same layout as the light one, so the n-th colour of one is the n-th of the other). Token values are
from `assets/styles/ui/palettes.rs` (`Tokens::light()` and `Tokens::dark()`); field names are in
`crates/ui-egui/src/theme.rs` (`Tokens`).

The tracker says the board colours are the existing token values. That holds for the dialog chrome
(first table). The figure palette and a few badge and accent colours have **no token**; the second
table says what to add. This step adds none: step 2 (#9874) adds the ones it paints with, with both
theme values, and notes them in its pull request. Names are proposals; take a shorter existing name
if one fits.

## Dialog chrome and controls (exact matches)

| Role | Light | Dark | Token |
|---|---|---|---|
| Text, labels, values | `#1B2430` | `#DEE2E8` | `form_ink` |
| Secondary text, icon strokes, section titles, (i) icon | `#4A5868` | `#B4BBC8` | `form_dim` |
| Dialog border | `#D5DBE3` | `#3F4552` | `dialog_border` |
| Rules between header, list, footer and rows | `#E6EAF0` | `#353B47` | `dialog_rule` |
| Footer fill, page-list fill | `#F4F6F9` | `#2F343E` | `dialog_footer` |
| Control borders: field, segmented, button, checkbox, card border | `#C3CBD6` | `#4A5262` | `control_border` |
| Control fill: field, dropdown, button, checkbox box, card face | `#FFFFFF` | `#1C1F26` | `control_fill` |
| Selected row, selected page, selected card fill | `#E8F1FA` | `#2A466E` | `selected_bg` |

## Needing a token, or a mix

| Role | Light | Dark | Token |
|---|---|---|---|
| Dialog surface (97 % opaque) | `rgba(255,255,255,0.97)` | `rgba(39,43,52,0.97)` | dark: `dialog_bg` at 0.97. Light: `dialog_body` (#FFFFFF) at 0.97; `dialog_bg` is #FAFBFC, so the board uses the lighter `dialog_body` |
| Accent: selected borders and text, checks, the figures' accent. Light is `form_primary`; dark `#8CBEF5` is lighter than dark `form_primary` (#3A88EE) | `#236EAD` | `#8CBEF5` | light `form_primary`; dark: **no token: add `pref_accent`** = light `#236EAD` / dark `#8CBEF5` (nearest: `selected_ink` #9CC7F5) |
| Ink on an accent fill (primary button "OK", checked checkbox tick) | `#FFFFFF` | `#0F1620` | light `on_accent`; dark: **no token: add `pref_on_accent`** = light `#FFFFFF` / dark `#0F1620` (dark `on_accent` is white) |
| Segmented control well, tile and card tint behind a figure | `#EEF1F5` | `#2F343E` | dark `dialog_footer`; light: **no token: add `pref_well`** = light `#EEF1F5` / dark `#2F343E` |
| Figure ground (the rectangle under every card figure) | `#F7F8FA` | `#1C1F26` | dark `control_fill`; light: **no token: add `figure_ground`** = light `#F7F8FA` / dark `#1C1F26` |
| Figure outlines (cube edges, geometry strokes) | `#6F7C8F` | `#A3AEC2` | **no token: add `figure_line`** = `#6F7C8F` / `#A3AEC2` |
| Figure cube, left face | `#E4E9F0` | `#515B72` | **no token: add `figure_face_left`** = `#E4E9F0` / `#515B72` |
| Figure cube, right face | `#C9D2DE` | `#394256` | **no token: add `figure_face_right`** = `#C9D2DE` / `#394256` |
| Figure cube, top face and figure paper | `#FFFFFF` | `#6B7790` | **no token: add `figure_face_top`** = `#FFFFFF` / `#6B7790` |
| Warning badge text and stroke ("Restart needed", "not on this GPU") | `#B84A00` | `#F0A27A` | **no token: add `pref_note`** = `#B84A00` / `#F0A27A` (nearest: `surface_ink` #A45000) |
| Warning badge fill | `#FBEBDD` | `#4A3326` | **no token: add `pref_note_fill`** = `#FBEBDD` / `#4A3326` |
| Success badge fill ("Connected", "Up to date", "Signed in") | `#E5F2E6` | `#1F3A31` | light `form_success_fill`; dark: **no token: add `pref_ok_fill`** dark `#1F3A31` (dark `form_success_fill` is #2C4A33) |
| Success badge text and icon | `#276B2C` | `#7FD1AE` | light `form_success`; dark: **no token**, dark `form_success` is #27584E; add `#7FD1AE` |
| Amber reference point (pivot, origin, pointer) in figures | `#8A5300` | `#E8C24A` | **no token: add `figure_reference`** = `#8A5300` / `#E8C24A` |
| Axis Z (blue) in figures | `#2F6FD0` | `#6CA8F7` | **no token: add `figure_axis_z`**; the app `axis_z` is #3C64E6 light and dark |
| Axis X (red) in figures | `#D64545` | `#F27A7A` | **no token: add `figure_axis_x`**; app `axis_x` is #DC3C3C / #C82828 |
| Axis Y (green) in figures | `#2E9E5B` | `#62D48E` | **no token: add `figure_axis_y`**; app `axis_y` is #3CAA3C / #78D250 |

## Board backdrop (not part of the dialog)

The canvas behind each dialog: `#F7F8FA` / `#2E3440` fill (dark = `viewport_top`), with a 24 px grid
in `#E3E7ED` / `#3A414D` (dark = `grid` #3B414C), and a shadow `rgba(20,30,45,0.10)` light /
`rgba(0,0,0,0.30)` dark on the dialog (`Tokens::shell_shadow` derives the app's own). Not built in
the app.

## Theme thumbnails and Gruvbox / imported samples

The theme thumbnails (`theme-*.svg`) paint each theme's own palette, so they use literal colours by
design and are never themed by the dialog's theme: Light (`#F4F6F9` ground, `#FFFFFF` bars,
`#236EAD` accent), Dark (`#2A2F3A`, `#323845`, `#8CBEF5`, bars `#6C7689`), Gruvbox (`#282828`,
`#3C3836`, `#EBDBB2`, `#83A598`), and the imported sample (`#2B2F52`, `#3B4170`, `#B6A6FF`,
`#9CA3DE`). In the app, paint a theme card from the theme's own `Tokens` (built-ins and imported
files are available from `theme_files`), not from literals.

## Colour vision samples

`colour-vision-*.svg` use the colour-blind-safe set the app already applies per mode (Okabe-Ito
values): `#E69F00` (orange), `#56B4E9` (sky), `#0072B2` (blue), `#D55E00` (vermilion), `#CC79A7`
(purple) and `#A1342C` / `#FF9C98` for failed. Map them to the sketch and analysis tokens in
`Tokens` (`sketch_fixed`, `sketch_projected`, `axis_x`..`axis_z`, `bend_up`, `bend_down`,
`sketch_conflict`) so that each colour vision mode changes them. The board draws them with literals
for the Off mode.

## Remaining pairs

All other colours in the boards, with the nearest tokens. Light and dark are listed in the board's
own pairing.

| Light | Dark | Uses | Nearest tokens (light / dark) |
|---|---|---|---|
| `#F7F8FA` | `#2E3440` | 13 boards | `file_background` (#F7F9FB), `toolbar` (#F6F7F9) / `viewport_bottom`, `viewport_top` |
| `#E3E7ED` | `#3A414D` | 13 boards | `file_border` (#E0E5EC), `dialog_rule` (#E6EAF0) / `grid` (#3B414C), `nav_hover` (#3A4150) |
| `#FFFFFF` | `#2F343E` | 3 boards | `control_fill`, `dialog_body` / `dialog_footer`, `nav_field` |
| `#FFFFFF` | `#FFFFFF` | 2 boards | `control_fill`, `dialog_body` / `file_image_tint`, `file_rail_ink` |
| `#1B2430` | `#1B2430` | 1 board | `form_ink`, `nav_tip` / `nav_tip_ink`, `file_background` (#1F232B) |
| `#236EAD` | `#236EAD` | 1 board | `file_rail`, `form_primary` / `sheet_fill` (#2A7FA8), `chip` (#3B65A8) |
| `#C3CBD6` | `#C3CBD6` | 1 board | `control_border`, `grid` (#C4CAD4) / `hover_edge_halo` (#CBCBCB), `icon` (#D0D6E2) |
| `#FFFFFF` | `#DEE2E8` | 1 board | `control_fill`, `dialog_body` / `dialog_label`, `form_ink` |
| `#FFFFFF` | `#272B34` | 1 board | `control_fill`, `dialog_body` / `dialog_bg`, `dialog_body` |
| `#FFFFFF` | `#1B2430` | 1 board | `control_fill`, `dialog_body` / `nav_tip_ink`, `file_background` (#1F232B) |
| `#F4F6F9` | `#F4F6F9` | 1 board | `dialog_footer`, `toolbar` (#F6F7F9) / `nav_tip` (#EEF1F5), `cube_face` (#ECEFF4) |
| `#EEF1F5` | `#EEF1F5` | 1 board | `nav_viewport`, `cube_face` (#ECEFF4) / `nav_tip`, `cube_face` (#ECEFF4) |
| `#EEF1F5` | `#3A4150` | 1 board | `nav_viewport`, `cube_face` (#ECEFF4) / `nav_hover`, `grid` (#3B414C) |
| `#276B2C` | `#8FDB9A` | 1 board | `form_success`, `bend_up` (#1E8C32) / `timeline_marker` (#AAB2C0), `body` (#989CA2) |
| `#D9DFE7` | `#D9DFE7` | 1 board | `nav_press` (#DCE2EA), `dialog_border` (#D5DBE3) / `dialog_label` (#DEE2E8), `form_ink` (#DEE2E8) |
| `#9AA7B6` | `#9AA7B6` | 1 board | `grid_major` (#AAB2BE), `nav_dim` (#8895A7) / `text_dim` (#949BA8), `timeline_marker` (#AAB2C0) |
| `#7B8798` | `#7B8798` | 1 board | `app_bar_separator` (#8891A0), `nav_dim` (#8895A7) / `nav_dim` (#7C879A), `app_bar_separator` (#8891A0) |
| `#A8B3C1` | `#A8B3C1` | 1 board | `grid_major` (#AAB2BE), `body` (#B0BAC6) / `timeline_marker` (#AAB2C0), `file_dim` (#B4BBC8) |
| `#C9D1DB` | `#C9D1DB` | 1 board | `viewport_bottom` (#CBD2DD), `border` (#D0D4DC) / `icon` (#D0D6E2), `origin_point` (#D6D7DA) |
| `#3A4150` | `#3A4150` | 1 board | `dialog_label` (#3A4656), `file_dim` (#3A4656) / `nav_hover`, `grid` (#3B414C) |
| `#252A34` | `#252A34` | 1 board | `cube_text` (#262A32), `text` (#262A32) / `nav_viewport` (#262B35), `cube_text` (#262A32) |
| `#2A2F3A` | `#2A2F3A` | 1 board | `body_edge` (#282C34), `cube_text` (#262A32) / `glyph_bg` (#2C323C), `nav_viewport` (#262B35) |
| `#323845` | `#323845` | 1 board | `app_bar_tab_active` (#353B47), `app_bar` (#343840) / `timeline_item` (#343A46), `app_bar_tab_active` (#353B47) |
| `#6C7689` | `#6C7689` | 1 board | `text_dim` (#6E7480), `cube_border` (#555E6E) / `nav_dim` (#7C879A), `form_accent` (#7B63A6) |
| `#8CBEF5` | `#8CBEF5` | 1 board | `sel_edge_rim` (#7FBFFF), `agent_busy` (#6CB4F0) / `sel_edge_rim` (#7FBFFF), `file_primary_ink` (#9CC7F5) |
| `#99A5BA` | `#99A5BA` | 1 board | `grid_major` (#AAB2BE), `nav_dim` (#8895A7) / `text_dim` (#949BA8), `timeline_marker` (#AAB2C0) |
| `#7E8AA0` | `#7E8AA0` | 1 board | `app_bar_separator` (#8891A0), `nav_dim` (#8895A7) / `nav_dim` (#7C879A), `app_bar_separator` (#8891A0) |
| `#566176` | `#566176` | 1 board | `cube_border` (#555E6E), `form_dim` (#4A5868) / `cube_border` (#555E6E), `control_border` (#4A5262) |
| `#3F485A` | `#3F485A` | 1 board | `dialog_label` (#3A4656), `file_dim` (#3A4656) / `border` (#3F4552), `dialog_border` (#3F4552) |
| `#454D5E` | `#454D5E` | 1 board | `icon` (#464E5C), `timeline_marker` (#464C58) / `nav_press` (#464E60), `grid_major` (#494F5A) |
| `#32302F` | `#32302F` | 1 board | `body_edge` (#282C34), `cube_text` (#262A32) / `dialog_bg` (#272B34), `dialog_body` (#272B34) |
| `#1D2021` | `#1D2021` | 1 board | `form_ink` (#1B2430), `nav_tip` (#1B2430) / `app_bar` (#1C1F26), `control_fill` (#1C1F26) |
| `#282828` | `#282828` | 1 board | `cube_text` (#262A32), `text` (#262A32) / `timeline` (#21252D), `cube_text` (#262A32) |
| `#3C3836` | `#3C3836` | 1 board | `app_bar` (#343840), `app_bar_tab_active` (#353B47) / `dialog_footer` (#2F343E), `nav_field` (#2F343E) |
| `#7C6F64` | `#7C6F64` | 1 board | `text_dim` (#6E7480), `cube_border` (#555E6E) / `cube_border` (#555E6E), `control_border` (#4A5262) |
| `#83A598` | `#83A598` | 1 board | `app_bar_separator` (#8891A0), `nav_dim` (#8895A7) / `app_bar_separator` (#8891A0), `body` (#989CA2) |
| `#EBDBB2` | `#EBDBB2` | 1 board | `file_recovery_well` (#F7E2B8), `file_recovery_border` (#F0D29A) / `timeline_item_error` (#E8C4AA), `hover_edge_halo` (#CBCBCB) |
| `#D5C4A1` | `#D5C4A1` | 1 board | `file_recovery_border` (#F0D29A), `hover_edge_halo` (#CBCBCB) / `timeline_item_error` (#E8C4AA), `file_recovery_ink` (#F5CC89) |
| `#A89984` | `#A89984` | 1 board | `app_bar_separator` (#8891A0), `nav_dim` (#8895A7) / `body` (#989CA2), `glyph_edge` (#C88278) |
| `#504945` | `#504945` | 1 board | `timeline_marker` (#464C58), `icon` (#464E5C) / `border` (#3F4552), `dialog_border` (#3F4552) |
| `#6A5AC8` | `#6A5AC8` | 1 board | `form_accent` (#7349CD), `form_fill` (#7349CD) / `form_fill` (#8A6FCC), `form_accent` (#7B63A6) |
| `#2E5A9E` | `#2E5A9E` | 1 board | `sel_profile` (#1A54A7), `file_rail` (#236EAD) / `file_rail_hover` (#2E6395), `chip` (#3B65A8) |
| `#2B2F52` | `#2B2F52` | 1 board | `app_bar_tab_active` (#353B47), `app_bar` (#343840) / `form_soft` (#372F4F), `timeline_item` (#343A46) |
| `#3B4170` | `#3B4170` | 1 board | `icon` (#464E5C), `dialog_label` (#3A4656) / `accent_soft` (#2A466E), `selected_bg` (#2A466E) |
| `#9CA3DE` | `#9CA3DE` | 1 board | `grid_major` (#AAB2BE), `body` (#B0BAC6) / `form_tab_ink` (#B39BEE), `timeline_marker` (#AAB2C0) |
| `#B6A6FF` | `#B6A6FF` | 1 board | `cube_hover` (#BEDCFC), `control_border` (#C3CBD6) / `form_tab_ink` (#B39BEE), `file_primary_ink` (#9CC7F5) |
| `#E7E1FF` | `#E7E1FF` | 1 board | `form_soft` (#EEE8FA), `hover` (#E2EAF6) / `app_bar_text` (#E6EAF2), `cube_face` (#ECEFF4) |
| `#9C8FE8` | `#9C8FE8` | 1 board | `preview_add` (#70A0E2), `body_selected` (#6EAAEB) / `form_tab_ink` (#B39BEE), `form_fill` (#8A6FCC) |
| `#6E8FE0` | `#6E8FE0` | 1 board | `preview_add` (#70A0E2), `body_selected` (#6EAAEB) / `preview_add` (#70A0E2), `body_selected` (#6EAAEB) |
| `#6E76B8` | `#6E76B8` | 1 board | `sel_face` (#4C7FCB), `sel_vertex` (#466EAA) / `form_accent` (#7B63A6), `form_fill` (#8A6FCC) |
| `#8A94A3` | `#8A94A3` | 1 board | `nav_dim` (#8895A7), `app_bar_separator` (#8891A0) / `app_bar_separator` (#8891A0), `text_dim` (#949BA8) |
| `#7C8899` | `#5A6475` | 1 board | `app_bar_separator` (#8891A0), `nav_dim` (#8895A7) / `cube_border` (#555E6E), `control_border` (#4A5262) |
| `#E69F00` | `#E69F00` | 1 board | `joint_manipulator` (#EB9D00), `mesh_fill` (#E0B200) / `joint_manipulator` (#EB9D00), `mesh_fill` (#C6A800) |
| `#E69F00` | `#F0C060` | 1 board | `joint_manipulator` (#EB9D00), `mesh_fill` (#E0B200) / `preview_new` (#F0E169), `warning` (#ECAA46) |
| `#D55E00` | `#F0A27A` | 1 board | `bend_down` (#C85A14), `surface_accent` (#A45000) / `file_recovery_ink` (#F5CC89), `glyph_edge` (#C88278) |
| `#0072B2` | `#56B4E9` | 1 board | `file_rail` (#236EAD), `form_primary` (#236EAD) / `sheet_ink` (#4FB3D9), `body_selected` (#6EAAEB) |
| `#56B4E9` | `#8CBEF5` | 1 board | `agent_busy` (#6CB4F0), `body_selected` (#6EAAEB) / `sel_edge_rim` (#7FBFFF), `file_primary_ink` (#9CC7F5) |
| `#56B4E9` | `#56B4E9` | 1 board | `agent_busy` (#6CB4F0), `body_selected` (#6EAAEB) / `sheet_ink` (#4FB3D9), `body_selected` (#6EAAEB) |
| `#CC79A7` | `#CC79A7` | 1 board | `glyph_edge` (#BE6E64), `grid_major` (#AAB2BE) / `render_accent` (#E074AE), `render_ink` (#E074AE) |
| `#A1342C` | `#FF9C98` | 1 board | `form_error`, `surface_accent` (#A45000) / `agent_busy` (#E896BE), `timeline_item_error` (#E8C4AA) |
| `#2F7D5B` | `#7FD1AE` | 1 board | `box_crossing` (#28A050), `bend_up` (#1E8C32) / `timeline_marker` (#AAB2C0), `text_dim` (#949BA8) |
| `#C98A00` | `#E8C24A` | 1 board | `warning` (#DE961E), `joint_manipulator` (#EB9D00) / `warning` (#ECAA46), `mesh_ink` (#D4B726) |
