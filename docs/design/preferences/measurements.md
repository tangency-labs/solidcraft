# Preferences measurements

Measured from the rendered boards (light dialog; the dark dialog has the same layout). Sizes are in
CSS pixels, which is the board's unit and equals the app's logical points at 100 % interface scale.
Borders are 1 px (2 px on a selected card or tile); a measured 40.9 means 40 plus a 1 px border.
Where the app already has a token for a dimension, the token is named; otherwise map the value to
the nearest `space_*` / `font_*` token or add one.

Fonts are IBM Plex Sans (text) and IBM Plex Mono (key caps, figure captions), as in the app.

## Dialog frame

| Part | Size |
|---|---|
| Dialog | 880 px wide; height follows the content except Shortcuts (fixed 992 px board with a scrolling list). Outer radius 10, border 1 `dialog_border`, shadow `0 4 16 rgba(20,30,45,.10)` light, `rgba(0,0,0,.30)` dark. |
| Title bar | 40 px, padding 0 8 0 12, gap 8, 1 px bottom rule. Icon 19 px (`form_icon_size`), title "Preferences" 14 px / 600, search field 230x26 radius 6 with a 14 px search icon, placeholder "Search preferences" and the key cap "Ctrl F" (10.5 px mono, radius 4, 17.7 high), close button 26x26 radius 6 (`form_close_size`). |
| Page list | 188 px wide (190 with its 1 px right rule), fill `dialog_footer`, padding 6 8 10, 2 px between items. |
| Page list item | 171 x 32, padding 0 10, gap 9, radius 6, 12.5 px; icon 16 px. Selected: weight 600, fill `selected_bg`, accent text. |
| Page list heading | "Look", "Work", "Input", "Extend": 11 px / 600, tracking 0.07 em, upper-cased, `form_dim`. "General" has none. |
| Profile card | foot of the list: label "Profile" (10.5 px / 600, tracking 0.06 em, upper-cased), then the profile name with a 7x7 amber dot (`#B84A00`, radius 4, hover "Changed since saved") when edited, and a 14 px chevron. |
| Content column | 690 wide (the dialog 880 minus the 188 list minus the border), padding 18 22 10: sections are 646 px wide (the issue's 648 ignores the borders). |
| Footer | 48 px, padding 0 12, gap 8, 1 px top rule, fill `dialog_footer`. "Restore page defaults" at the left (144x28), "Cancel" (60x28) and "OK" (72x28, weight 600, accent fill, `on_accent` ink) at the right. |

## Page head

* Title: 20 px / 600, one line, with the "In profile" tag at the far right on pages that a profile holds.
* Subtitle: 12.5 px `form_dim`, one line (e.g. "Where the parts of the window sit."). Together the head is 46 px tall.
* "In profile" tag: a pill with a 12 px profile icon and the text "In profile", hover "Saved in settings profiles".

## Sections

* **38 px between sections** (flex gap of the content column).
* Section head: a 30 px row, gap 6, a 14 px disclosure chevron (sections fold), then the title (11 px / 600, tracking 0.07 em, upper-cased, `form_dim`), a 1 px `dialog_rule` line fills the rest of the row (left margin 4). The sub-heads inside a section ("When a sketch ends", "Click selects", "History", "Browser") are the same style at 11 px / 600 with 0.06 em tracking, no icon.
* A checkbox row is 16.6 px high (a 15 px checkbox and 12.5 px text, gap 8); label-and-control rows are 28 px high; the label column is about 64 px wide for "Visual style"-length labels (the app uses `form_label_width` 70).

## Controls (all heights are the app's `control_height` 28 unless noted)

| Control | Size and style |
|---|---|
| Checkbox | The native 15x15 checkbox with `accent-color` set to the accent; label 12.5 px, gap 8. In the app use the existing `ui_kit` checkbox. A dimmed (disabled) row uses ink `#8A94A3` (no token, see `tokens.md`). |
| Text and number field | height 28, radius 6, 1 px `control_border`, text 12 px. Widths seen: number 56 to 72, "Port" 80, "Profile name" 220. Unit suffix beside it in 12 px `form_dim`. |
| Dropdown | height 28, radius 6, 12.5 px; widths 180, 200, 220, 240 or 280 as the options need; chevron at the right. Visual style dropdown is 240x30 with an 18 px style icon. |
| Segmented | The existing `ui_kit` segmented control, as the app draws it (the board's accent-filled segment was wrong). A row of segments on a `control_fill` ground, 1 px `control_border` around, radius `control_radius` (6) on the group only, a 1 px `control_border` divider between segments, button padding 0 12, 12.5 px. Height is the control row (about 28). The selected segment fills its whole cell, top to bottom, with `selected_bg` (no gap under it, no inset), its text in `selected_ink` at weight 600; the others have `form_ink` text at weight 400; a hovered unselected segment fills with `hover`. A disabled segment uses `form_dim`. An accent-filled segment with `on_accent` text exists only for a primary choice in a dialog footer, never on a Preferences page. |
| Button | 28 high, radius 6, 1 px `control_border`, 12.5 px; primary: accent fill, weight 600. Small button (Apply, Reset, Test, Show in folder, Remove): 24 high, 12 px. |
| Chip (scale presets) | 22 high, radius 11, 11.5 px, padding 0 8: "75%", "100%", "110%", "125%", "150%", "200%". Selected: 1 px `form_primary` border, `selected_bg` fill, `selected_ink` text, weight 600. |
| Key cap | 17.7 high, radius 4, 1 px `control_border`, IBM Plex Mono 10.5 px, padding 0 5. |
| Badge | 18 high, radius 9, 11 px, padding 0 8: warning `pref_note` on `pref_note_fill` ("Restart needed", "Not on this GPU", "Desktop only"); success badge for "Connected", "Calibrated", "Up to date", "Signed in", "Default". |
| (i) help icon | A 13 px (11 px in dense rows) line icon, `form_dim`; its `title` is the hover text. It sits right after the label. |
| Value readout | e.g. "1.00x", "110 %", "34 degrees": a 44 px column. |

## Cards and tiles

| Piece | Size and style |
|---|---|
| Picture card | A radio button. Radius 9, 1 px `dialog_border`, fill `control_fill`, padding 6 6 7, gap 5, a centred column: the figure at the full inside width (a 130 px card shows a 118 px figure; the figure has radius 5 and keeps its canvas aspect ratio), then the label (12.5 px, weight 400) and an optional sub-label (11 px `form_dim`). Selected: 2 px `form_primary` border (padding drops to 5 5 6 so the card does not grow), `selected_bg` fill, `selected_ink` label at weight 600. "Has options" badge: a 16 px accent disc with a 10 px icon at right 5, top 5, hover "Has options". |
| Card widths (by grid) | Curved surfaces 130 (4 across, 117 high); Anti-aliasing 130; History and Browser 116 (101 high; the figure is 100); Orbit around 196 x 133.5 (3 across); View navigator 190 x 149; Orientation 150 x 146 (2 across); Selecting 150 x 109; "When a sketch ends" 196 x 152.5 (3 across); Theme 88 x 100.8. Grid gap 10. |
| Toggle tile | A checkbox button. Radius 9, same padding and gap as the picture card, the figure on top, the label beneath (12.5 px; selected: `selected_ink`, weight 600), and a 15 px square (radius 4) at the top-right corner, right 6 top 6: accent fill with an 11 px white tick when on. Selected: 2 px `form_primary` border and `selected_bg`. Widths 155.6 (Current view and In the view, 4 across, 112 high) and 208.8 x 140 (Marks in the sketch, Measure and pointer, 3 across). |
| Preset card | A radio button 204 wide x 69.5 high, radius 10, padding 14, 1 px `dialog_border`. Inside, gap 12: a 38x38 icon box (radius 10, fill `pref_well`, `form_dim` icon 21 px), then the name (15 px / 600) over one word (11.5 px `form_dim`). Selected: 2 px `form_primary` border, `selected_bg`; the icon box turns `form_primary` with an `on_accent` icon, the name turns `selected_ink`, and an 18 px `form_primary` disc with a 12 px white tick sits at the right. Three across: Performance "Fastest", Quality "Sharpest", Custom "Your own mix". |
| Theme card | A picture card 88 wide (thumbnail 124x76 shown 76 wide), the name, then one line (11 px) for the source: "Built in", "Follows the OS", "Imported file". The "has options" disc applies. |
| Import card | A button 88 wide, padding 6, gap 5, dashed 1 px `control_border`, transparent fill, centred: a plus icon in the accent, "Import..." (ink) and "theme file" (11 px). |
| Carousel | A row, gap 8: a previous button, the cards, a next button. The arrows are 26 x 64, radius 8, 1 px `dialog_border`, `control_fill`, 16 px chevron; opacity 0.35 when there is nothing further in that direction. |
| Theme options panel | Under the carousel: radius 10, 1 px `dialog_border`, fill `pref_well`, padding 8 16 10. Head 34 high: theme name (13 px / 600), "6 options" (11.5 px `form_dim`), then a ghost "Reset" button (24 high). Below, two columns (gap 2 / 32): each option is a row of at least 30 px, a 92 px label column and the control. Slider: 92 px wide, 4 px track (radius 2, `dialog_rule`), accent fill, a 16 px thumb (white, 2 px accent border); the value is IBM Plex Mono 12 px, min width 40 ("72 %"). Contrast, Corners and Density are segmented controls. An unavailable option stays in place with ink and thumb border `#8A94A3` and a text reason beside it ("Software graphics", "System: Reduce transparency"). |
| Layout schematic | 226x148 viewBox, 262 px wide (`figures/layout-schematic.*`). |
| Colour vision sample | 84 x 44 cell with the label beneath ("Fixed", "Projected", "Axes", "Bends", "Draft", "Failed"); above them four 13 px status icons with names ("Selected", "Warning", "Error", "Done"). |
| Profile row (General) | A 17 px profile icon, the name (weight 600 for the active profile), "Edited" and "Active" badges, then 15 px icon buttons Export, Rename and Delete (each with its hover title), and a 24 px "Apply" button (48.5 wide) on rows that are not active. |

## Type scale in use

| Size / weight | Where |
|---|---|
| 20 / 600 | Page title. |
| 14 / 600 | Dialog title. |
| 12.5 / 400 and 600 | Labels, values, buttons, subtitles (600 for the selected page and primary button). |
| 12 / 400 | Fields, small buttons, units. |
| 11.5 / 400 | Chips. |
| 11 / 600, tracking 0.07 em, upper-case | Section titles; also 11 / 400 for card sub-labels and badges. |
| 10.5 / 600, tracking 0.06 em | Profile card label, nav-bar editor sub-labels. |
| 10.5 mono / 400 | Key caps and keyboard hints. |
| 6.5 to 7 mono | Figure captions. |

## Radii in use

4 (key cap, tile corner box, status dot), 5 (figure), 6 (field, button, segmented group, page list item, search), 8 (carousel arrow),
9 (picture card, tile, badge disc), 10 (preset card, theme options panel, dialog), 11 (chip).
