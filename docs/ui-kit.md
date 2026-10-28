# SolidCraft UI kit

`crates/ui-egui/src/ui_kit/` contains the tokenised controls used by feature dialogs,
preferences, manager cards, prompts, context menus and the sketch palette. Import through
`crate::ui_kit::{...}`. Most controls are crate-visible; the container API is public so platform apps use the same hosts.

## Composition and rules

- Every outer host uses `ui_kit::Container` with a semantic `Surface`. The shell
  publishes the layout once per frame; `Container::floating()` and `frame()` own
  adaptive placement policy and chrome. `Panel`, `Window`, `Confirm` and `Menu`
  compose it. Raw egui hosts and outer shadows are rejected recursively, including
  platform apps. The mandatory `cargo xtask ui-containers` gate runs in fast CI and
  scans new crates/apps automatically, including macro bodies and alias chains. See the [container audit and migration inventory](ui-container-audit.md).

- Colours, typography, dimensions and spacing come from `theme::Tokens`.
- Sliders only via `ui_kit::slider`; `Slider` options change behaviour, never appearance.
  Raw sliders and duplicate track/thumb painters are forbidden in the UI and apps.
- `scope(ui, add)` supplies form styling. `row(ui, label, add)` aligns labels and
  controls at `control_height` (28 pt). Wide manager rows use a 104 pt label basis
  with a 10 pt gutter, derived from the compact 70 pt basis and spacing tokens.
  Compact rows retain their 8 pt gutter. Labels widen from the appropriate basis
  to at most 1.6 times that width and never exceed half the row.
- `expr_row`, `hint_row`, `text_row` and `choice_row` compose `row` with the existing
  expression, hinted expression, plain text and dropdown controls. They preserve row order,
  labels, focus IDs and completion; callers keep Enter/change ownership and explicit dropdown IDs.
  `count_row` composes `row` with `count_field` for pattern quantities. `checked_row` is
  `hint_row` for a value the form checks before it runs: with a warning, the field is outlined in
  the warning colour and the message sits under it (`warning_caption`), so colour is never the
  only cue.
- `Panel` owns bounded windows, feature/palette composition, scrolling bodies,
  pinned actions, compute status and keyboard routing. `wide()` uses three card
  widths. `feature_frame` and `palette_frame` retain docked/floating surfaces.
  `FeatureOptions::apply_label` adds Apply; `Panel::actions` supplies custom labels.
  With `DialogButtons::Top` published (Preferences › Interface › Dialog buttons), `feature`
  puts the actions in the header row through `HeaderActions` instead of a footer: the same
  buttons, enabled states, keyboard and handles, each as wide as its bold label, with Cancel
  standing in for the close button and the compute status in the title's place.
  `body_rows` caps a manager's scrolling body independently of its position; short
  viewports still keep the header and actions visible.
- `Window` composes the shared Panel frame with a 40 pt icon/title/close header,
  padded content and optional 48 pt footer. `show_with_left` pins a left action beside
  the right-aligned buttons. `show_pages` accepts declarative `WindowPage` entries
  (icon, i18n label key, optional group heading, stable handle, scroll policy and draw callback).
  Its 188 pt soft page list has 32 pt icon/label rows, selected tint/ink and a divider.
  Preferences has nine grouped pages; legacy control-channel page names remain aliases.
  `page_rows` opts into a taller body while preserving viewport bounds.
  `actions`, `anchor`/`placed`, `cards`, `fields` and `keep_open_on_escape` retain
  existing window behavior. Empty action labels omit the footer.
- `Confirm` is an input-blocking modal using the same card frame and footer controls.
  It shows a 36 pt icon disc in warning, danger or info tone, a title, caller-supplied
  message and optional monospace `detail`, above a 52 pt footer. Glyphs use the construction
  ramp for warning, the shared shaded danger treatment for danger, and base blue for info;
  the disc keeps its tint. `actions(primary, cancel, left_ghost)`
  places the optional third action at the left. Danger uses `form_error` with a
  contrast-selected on-colour; other primaries use blue. Enter invokes the selected
  answer (primary by default) when valid; Esc cancels. It retains close, answer and
  message scenario handles. The old `Prompt` element is removed.
- Callers supply data and enabled state, then dispatch existing commands. Editors
  leave their established completion, Enter, Tab and commit ownership unchanged.
- Every text input uses the kit's shared `text_input` policy: symmetric horizontal
  `form_inset`, zero vertical margin and centred galley alignment. Font metrics
  therefore centre both mono and proportional text within the allocated field,
  including invalid and attached Library fields, without per-caller offsets.
  Canvas placement editors retain their native line-height plus `space_4` hit
  target so nearby sketch picks are not intercepted; alignment stays shared.
- Scenario handles and accessible labels are API. Consolidated options preserve
  the former geometry, wrapping, native metadata and publication keys.

Preference compositions use `settings_heading`, shared disclosure `settings_section` headings and
`settings_pair` for responsive controls beside one specimen. `option_card` supplies the
shared illustrated radio control. `settings_form` and `settings_block_hint` define aligned label grids; `theme_rail` keeps the import tile fixed beside a scrollbar-free scrolling row. `PreferenceFigure` owns theme swatches, orbit pivots,
quality, anti-aliasing, contact shading and up-axis figures. `layout_preview` follows the
four workspace choices without opening another viewport. Scale, palette, autosave, mouse
and live-axis specimens share tokens and the gallery; live bars show actual calibrated
input and introduce no repaint timer.

## Elements

Module names below describe implementation locations, not caller import paths.
Every inline element/variant has one entry in the light/dark `gallery::ELEMENTS`
table. Panel/Window/Confirm compositions also have dedicated geometry and interaction tests.

| Module / element | Use and options | Main tokens |
| --- | --- | --- |
| assistant: `assistant_header`, `assistant_controls`, `assistant_badge`, `assistant_bubble`, `assistant_prose`, `assistant_tool_frame/row/detail/status`, `AssistantToolState`, `assistant_approval/actions`, `assistant_composer/footer`, `assistant_input`, `assistant_send`, `assistant_empty`, `assistant_banner`, `AssistantTone` | Shared Assistant presentation: 40 pt header, 22 pt state pill, right-aligned user messages limited to 86% width, unboxed assistant prose, bordered tool groups and command detail, tinted approval heading, rounded focused composer with one unframed growing editor and pinned footer, centred setup copy and semantic notices. Closures preserve existing permission choices, protocol routing, queue actions and scenario handles. | `form_header_height/close_size`, `font_label/small/body`, `dialog_body/footer/rule`, `control_fill/border/radius`, `form_ink/dim/primary/success/error`, `accent_soft/selected_ink`, `warning`, radii and spacing |
| history: `HistoryChip`, `HistoryStyle`, `history_drop_bar`, `history_marker`, `HistorySeam`, `history_seam` | One painter family for bottom chips, 38 pt vertical icon chips and single-line 26 pt name rows (the browser's shared `TREE_ROW_HEIGHT`). Selected items use the active tab tint/ink/border; all variants retain multi-tone feature artwork using one shared kind-to-accent rule (Sketch green, Surface orange, Mesh yellow, Form purple, solids base blue); rolled-back items have 42% opacity; a reference row (a sketch listed under another feature, #6489) has its name in secondary text. Error/warning badges, suppression strikes, horizontal/vertical drop bars and rollback line/hollow circle are shared. No connector lines. | `control_height`, `space_2/3/4/6/8/10/12/14/16/24`, radii, typography, `TabAccent`, `timeline_item`, `control_border`, `text/dim`, `form_error/fill` |
| search: `CommandSearch`, `MenuItem::search_row` | Ribbon search composition: one persistent ribbon editor expands from 28 to 40 pt and to 520 pt wide with its right edge anchored, magnifier and S/Esc keycap, 38 pt two-line menu row with 26 pt icon tile, accent bold substring, path, shortcut/kind keycap, counts, JSON detail box and footer. The ordinary 30 pt menu row cannot fit a second line; this variant keeps its enabled/selected states and scenario IDs. Ranking and dispatch stay with the caller. | `form_card_width × 2 − space_8` (520 pt), `control_height`, `space_2/3/4/6/8/10/12/14/16/24`, `radius_xl/control_radius`, `control_fill`, `dialog_body/footer/rule/border`, `form_ink/dim/primary/error/error_fill/success/success_fill`, `selected_ink`, `accent_soft`, typography |
| menu: `Menu`, `MenuItem` | Shared popup, 30 pt rows, 28 pt icon column, clipped labels, right-aligned mono shortcuts, submenu chevrons, checked/unchecked and document swatches, inert 40% disabled rows and danger ink; headings and separators; ribbon drag/pin targets retained | `dialog_body/border/rule`, `radius_xl`, `control_height/radius`, `form_ink/dim/primary/error`, `accent_soft`, typography and spacing |
| radial: `radial_menu` | Eight rounded annular sectors, parallel 4 pt gaps, 9 pt circular corner joins, tab-accent artwork and hover, centre label/key; existing hold/flick/release dispatch | `dialog_body/border`, `form_ink/dim`, `TabAccent`, icon palette, font/spacing tokens |
| panel: `Window`, `WindowPage` | Shared card frame, header, optional page list, bounded content and footer with a left action | `form_header/footer_height`, `form_card_width`, `form_label_width`, `control_height`, `dialog_footer/rule`, `selected_bg/ink`, spacing |
| confirm: `Confirm`, `Tone` | Modal warning/danger/info, title/message, mono detail and ghost/cancel/primary actions | `warning`, `form_error/fill`, `accent_soft`, `form_primary`, `on_accent`, `tab_on_fill_dark`, `font_subtitle`, spacing |
| metrics: `row_height`, `table_row_height`, `header_height` | Shared control/header heights | `control_height`, `space_8`, `form_header_height` |
| layout: `scope`, `row` | Form styling and aligned label/control columns | `form_label_width`, `form_gap_y`, `control_*`, `font_label`, `dialog_label`, `space_4/8` |
| `caption`, `warning_caption` | Dim caption aligned beneath a control; the warning one says what is wrong with its value | `font_small`, `text_dim`, `warning`, label-column tokens |
| `figure_rows`, `FigureSlot` | A caller-painted figure in the label column beside a stack of captioned expression fields (the Hole dialog's section); the caller gets each field's rect and hover/focus state to draw leaders and light the part it sizes. Fields keep their `dialog:<label>` handles; the block publishes `dialog:figure:<id>` and its picture `dialog:figure_picture:<id>` | `control_height`, `form_label_width`, `font_small`, `form_dim`, `space_2/4`, label gap |
| `section`, `section_count`, `divider` | Persisted small-caps disclosure with chevron, optional count badge and trailing hairline; full-width rule | `control_height`, `dialog_rule`, `font_caption`, tracking, spacing |
| `list_row` | Picked item with a remove target | `control_height`, `font_label`, icon/spacing tokens |
| `text`, `Text` | Hint, note, report, inline or cell text; see variants below | `font_small/label`, `text/dim/error`, `control_height`, `space_4` |
| `text_lines` | Virtualized read-only licence/report lines, horizontal overflow | `font_small`, `text`, `control_height` |
| fields: `field`, `hint_field`, `focused_field`, `checked_field` | Expression editors, optional empty hint or retained focus, or outlined in the warning colour when flagged; returns accepted Enter | `control_height/fill`, `form_inset`, `font_label`, `warning` |
| `count_field`, `Count` | Expression editor for a whole-number count (a pattern's quantity) with up/down step arrows at its right edge. `Count` gives the limits and the evaluator (the design's parameters); an arrow steps the evaluated count by one within them, replacing a plain number or adjusting an expression's trailing `+ n`, and is dimmed when it can't step. Handles `dialog:<row> up` / `down`; returns accepted Enter | field tokens, `control_border`, `control_radius`, `selected_bg`, `form_ink/dim`, `space_4` |
| `text_field`, `focused_text_field` | Plain name editor, optionally retaining focus | field tokens |
| `multiline_text_field` | Three-line plain-text row with stable focus ID | field/row tokens |
| `password_field` | Stable secret editor; excludes the secret from scenario/accessibility metadata | field tokens |
| `message_field` | Growing message editor; owner retains scroll and Enter routing | field tokens |
| `cell_editor`, `bounded_cell_editor`, `search_field`, `library_cell` | Stable-ID inline/search editors; bounded variant caps Unicode characters; `library_cell` attaches a Library dropdown inside the editor border | field tokens, `text/error` |
| `text_area` | Bounded multiline JSON/data editor | field tokens |
| `dropdown`, `dropdown_menu` | Indexed choices or caller-supplied menu controls | `control_height`, `text_dim`, form tokens |
| `searchable_dropdown`, `SearchOption` | Filtered choices, custom-value action and pending message | dropdown tokens |
| `selection_field`, `Selection` | Selection chip with explicit `active` and `filled` state, in three looks: the active input (where the next pick goes) has a `form_primary` border and `selected_bg` tint; a filled inactive one a quiet neutral tint, a stronger neutral border and its count in `form_ink`; an empty inactive one the plain control with a dim prompt | `selected_bg/ink`, `form_primary/ink/dim`, `control_*`, `form_inset`, `space_2/8/24` |
| `value_pair` | Paired captions/expressions; optional independent field names | row/field tokens, `space_8` |
| `fraction_field`, `scalar_field` | Locale-aware bounded dimensionless values; scalar fields fill their control column | form control/font tokens |
| `number_field` | Integer/decimal field with caller range, drag speed and suffix | form control/font tokens |
| `length_field`, `angle_field` | Length in design units, angle in degrees | form control/font tokens |
| `slider` + `Slider` options | One token-painted track/thumb over native slider/value interaction; optional step, caller formatting, whole-step manipulation and hidden value editor | `control_height`, `space_24/8/4/2`, form control/font tokens |
| `placement_field` | Stable focus ID; drawing tool owns Enter/Tab validation and completion | `control_fill`, `font_body`, `text`, `control_height`, spacing |
| `color_field`, `color_popover` | Opaque document RGB swatch and kit popover; global U8/F segments, copy, gamma-byte or linear RGB fields, painter-drawn HSV square/hue and current strip; outside click dismisses | `control_*`, `form_*`, `border`, `on_accent`, `space_*` |
| choice: `segmented`, `Segments` | Equal text choices; `states` supplies mixed selection, `compact` clips instead of falling back to a dropdown, `primary` uses accent/on-accent for the selected segment | `control_*`, `selected_bg/ink`, `font_label`, `space_4/8` |
| `segmented_icons`, `IconSegment` | Equal icon choices and mixed states | segment tokens, icon palette |
| `checkbox`, `checkbox_indeterminate`, `checkbox_row`, `checkbox_grid`, `options` | One box/tick painter for inline, mixed, full-row, grid and menu choices | `space_14/3/2`, `radius_sm`, `control_fill/border`, `form_primary/ink`, `hover`, `on_accent` |
| `text_emphasis` | Bold/italic with translated accessible names | form control/font tokens |
| `tool_choices`, `ToolChoice`, `toggle_icon` | Wrapped command modes or independent icon toggles | sketch mode/icon tokens, `accent` |
| `list_picker` | Selectable row with caller identity and native checked state | `control_height`, form selection tokens |
| `popup_list`, `PopupOption` | Focus-neutral suggestions; owner retains keyboard routing | `space_2`, `form_label_width`, `control_height`, form tokens |
| `color_swatch`, `swatch_grid`, `color_thumbnail` | 30 pt draggable samples, 6 pt grid step gap, selected outer ring and checker pattern under transparent colours; caller payloads retained; local-library thumbnails use 16 pt | `control_height/radius`, `border`, `form_primary`, `control_fill`, `space_2/6/8` |
| `assistant_entry`, `AssistantEntryState` | Application-bar chat icon with quiet, ready, busy, auth, stopped and approval count marks; caller owns state and target | `app_bar`, `agent_ok/busy`, `warning/error`, `font_badge` |
| actions: `button`, `Button` | Secondary action: `Normal`, `Small`, or `Enabled(bool)` | native sizing, form font/control tokens |
| `primary_button` | Accent action outside a footer | `accent`, `on_accent`, form tokens |
| `icon_button_at` | Icon action inside a supplied rectangle | `form_icon_inset`, `text/dim`, icon palette |
| `button_pair`, `PairButton` | Equal secondary actions, optional icons and selected state | `form_button_height`, control/selection/icon tokens |
| `footer`, `Footer`, `Action`, `HeaderActions` | OK/Cancel with optional Apply, compute status or selected action; `HeaderActions` puts the same buttons in a header row | `form_footer/button_height`, `dialog_footer/rule`, `control_*`, `accent`, spacing |
| `keyboard` | Esc cancels, unowned Enter submits when valid | none |
| `action_menu`, `command_menu_button` | Nested dense action menu or ribbon overflow action retaining its icon, shortcut and enabled state | form control/font tokens, `space_16`, icon palette |
| table: `table`, `table_custom`, `Column`, `Table::headers`, `Table::row`, `TableRow::cell`, `Table::add_prompt` | Left-aligned headers/cells, horizontal overflow, selected/hover rows and empty state. `table_custom` supplies header actions through `headers`. `Column::flex` shares remaining width by weight, freezing columns at their control minimums before redistributing the remaining space; `Column::icon_only` hides the heading while retaining its automation identity. `add_prompt` is the final slim dashed line and requests a regular draft row from its owner; `Column::new(label, width)` uses label-column multiples, `min_content_width(points)` reserves measured control content; `ContentWidth::name` measures strong configuration names | label-column tokens, `control_height`, `dialog_rule/footer`, `selected_bg`, spacing |
| `removable_header`, `plus_header`, `bin_button`, `active_pill` | Hover-only column removal, anchored Add Column menu with insertion brackets, icon-only immediate deletion and active-row status. Bins preserve `dialog:button:Actions:Delete` and `:×`; header removal preserves `dialog:button:LABEL:×`. | `control_*`, `form_*`, `selected_bg`, spacing |
| `compact_button`, `summary_row` | Borderless stars and chevrons with enabled/selected states; named two-line rule summary beside an enable checkbox and compact actions. | `form_close_size`, `control_*`, `form_*`, typography and spacing |
| `label_actions` | Clipped row name beside compact actions | `control_height`, form font/spacing tokens |
| status: `status_header`, `StatusHeader` | Supplied icon/name/status and constraint meter | `font_label`, `text/dim`, `accent`, sketch/header tokens |
| `progress` | Clamped determinate progress | `space_3/6`, `dialog_rule`, `accent` |
| `steps` | A running operation's steps: ticked when done, a text-high spinner on the current one with how far it is, a ring for those to come | `font_label`, `control_height`, `form_ink/dim/primary`, `space_2/8` |

History contents are selected from measured panel width. A 56 pt thin strip shows icons
and four vertical transport controls: beginning, back, forward and end. The minimum named
width is 130 pt; it adds horizontal transport and the same six-dot window grip as Assistant.
When the remaining header space fits its measured text, **History** appears between grip
and transport. No names-mode flag
or command selects these layouts. The seam snaps between 56 and 130 pt; widths from 130
through 624 pt remain continuous and persist as `timeline_left_width`.

Drag the grip or title to dock along the bottom, at the full-height left edge, or beneath
the browser. Either card's right edge resizes their shared width. This position divides
available column height equally. Folding either card leaves its header visible and gives the remainder to
the other. If both are folded, their headers sit together at the top of the column.
Expansion and collapse share a smooth cubic transition capped at 120 ms, respecting
the interface’s instant-animation setting. Their collapse controls share the same active-tab blue hover tint. Browser resizing also resizes History.
The `view.history_layout` command validates width, docking and collapse requests without
altering the design or opening dialogs. Floating mode preserves viewport projection.

The seam handle overlays the strip's right edge without taking layout space. Its grip is
visible at rest; the arrow appears only on hover. Click, double-click the edge, or use
Enter/Space on the focused handle to resize between thin and minimum named widths. Drag
freely above the named minimum; resizing draws no dotted guide or name label. Header/title dragging
and the edge width gesture are separate from feature/folder and rollback dragging.
The rollback marker is 12 pt high.
Its 2 pt line uses the active tab fill and ends in a 10 pt hollow circle with a 2 pt accent
border and the card/panel surface inside. The circle overlaps the line end by 1 pt. The bottom
strip uses the same painter, rotated. Folder selection, range selection, context menus,
sketch-pattern children, drag reordering and command JSON retain their existing behavior and handles.
`history_icons`, `history_names`, `history_marker` and the `history_seam_rest/hover/drag` states
have light/dark gallery references. The CPU
board test captures both strip widths with expanded and collapsed browsers
(`SOLIDCRAFT_HISTORY_EVIDENCE`).

`text(ui, text, options)` supports the existing text behaviors and the new error band:

| Option | Layout and scenario publication |
| --- | --- |
| `Text::Hint(key)` | Wrapping 12 pt hint; rect and handle `dialog:hint:KEY` |
| `Text::Note(key)`, `Text::MonoNote(key)` | Wrapping small dim text (optional monospace) with vertical padding; rect `dialog:note:KEY` |
| `Text::Error(key)` | Wrapping full-width error band with tinted background and padding; retains `dialog:report:KEY` |
| `Text::Report(key, error)` | Native wrapping label, optional error colour; rect/handle `dialog:report:KEY` |
| `Text::Heading(key)` | Semibold body heading; retains `dialog:report:KEY` |
| `Text::NameCell(strong)` | Medium/semibold clipped clickable name; retains `dialog:cell:TEXT` |
| `Text::MonoCell`, `Text::Value(key, error)` | Monospace unit/value cells; clipped to one row with full tooltips. Value/error cells retain `dialog:report:KEY`. |
| `Text::Inline` | Native truncated label; rect/handle `dialog:inline:TEXT` |
| `Text::Cell` | Fixed-height clipped clickable cell with tooltip; rect/handle `dialog:cell:TEXT` |

Examples:

```rust
segmented(ui, &mut mode, &["One", "Two"], Segments::default());
selection_field(ui, "Select", Selection { active: true, filled: false });
value_pair(ui, "Position", [&mut x, &mut y], ["X", "Y"], Some(["Distance X", "Distance Y"]));
button(ui, "Add", Button::Enabled(valid));
footer(ui, "OK", "Cancel", valid, Footer { apply: Some("Apply"), ..Default::default() });
text(ui, report, Text::Report("validation", invalid));
```

Icon surfaces that explicitly choose a palette ramp use `icons::paint_accented`.
`None` selects base blue, while unknown names retain the icon's semantic accent.
Only active roles change: ink/line outlines, passive faces, guides and knockouts keep
normal artwork colours. Existing icon painters retain their semantic-accent behaviour.
Danger menu items and danger confirmation glyphs both use `icons::paint_danger`.
It additionally maps passive-1/2/3 faces to constraint active-1/2/3, retaining the
shading steps, ink outlines, handle and transparent slat details. The cache distinguishes
this whole-glyph treatment from ordinary accent selection and monochrome ink masks.
Radial icons follow the active tab (Sketch, Surface, Mesh, Form; other tabs use base blue)
in both normal and hovered sectors; the hovered background retains its tab tint.

## Extending and checking the kit

Add one implementation, document its tokens/options here, re-export it through
`mod.rs`, and add one table entry in `gallery::ELEMENTS`. Keep implementation-only
helpers private. Add interaction tests when the element introduces an action.

The gallery checks geometry, label alignment, handles and effective paint bounds in
both themes. Field/footer/panel tests retain focus, completion and action coverage.
`ui_kit_guard_tests` scans every UI feature module recursively, rejects raw control constructors
and methods (including aliases), and rejects removed legacy helpers. The raw-control
allowance table is empty and guarded against additions. Standard colour/font guards,
Clippy, wasm, i18n, assets, icons and layers still apply.

See [remaining coverage boundaries](manage-ui-kit-audit.md) for specialised surfaces
that remain outside the kit and why.

## Popup presentation and draft rows

Panel and Window use 28 pt controls, a 40 pt title band (14 pt semibold title,
19 pt command icon and 26 pt close/fold targets), and a 48 pt footer with
28 pt actions. Table rows are `control_height + space_8` (36 pt), independent
of control height. Bodies use 12 pt insets. Width limits and scroll boundaries
remain owned by Panel/Window/Confirm. Titles retain the supplied case.

The `form_ink`, `form_dim`, `form_primary`, `form_error` and `form_error_fill`
colour keys match the approved light cards. Dark values follow existing text,
accent and error palette roles; every role is exported by theme files and measured
by the colour-vision checks. Invalid fields include both error text and a tinted
surface. Long variable-height lists can use `Window::placed` with a top-left pivot to
keep their painted controls and scenario targets aligned as they grow.
Completion lists use the approved 230 pt minimum width (derived from card and
spacing tokens), show kind labels on the right, selected details beneath
the list, and the existing keyboard hint.

Parameters and Configuration Table use this one table grid, including Favorites
and each model group. The grid has `space_8` horizontal insets within the row
highlight, shared by headers and cells, plus `space_8` column gaps. Flexible widths
are measured inside the scroll viewport and rounded by cumulative column edges;
fitting content does not acquire a horizontal scrollbar from independent rounding
or inherited scroll margins. Explicit minimum widths still allow genuine overflow.
The viewport and measured content width publish `dialog:table:TABLE:viewport` and
`dialog:table:TABLE:content_width` for geometry checks. Hover uses the clipped row
rectangle, including interactive children, while preserving child click ownership.
Their add prompts publish `dialog:table:TABLE:add`, use `control_height - space_4`
height, a dashed `control_radius` outline and a pointing-hand cursor.
Clicking creates a regular unsaved row, focuses its name and exposes the existing
field controls and bin. Enter or valid focus loss commits through the existing
commands; abandoning a nameless row drops it. Configuration Enter with an empty
name retains ConfigurationN naming and copies the active row. Parameter validity
is published as `param:new:valid`; invalid expressions cannot commit on focus loss.
The natural scrolling body height is published as `dialog:body:content_height`,
so geometry tests distinguish content size from the viewport cap.
After a successful parameter add, a fresh empty draft retains Name focus for
continuous keyboard entry; leaving that empty row drops it.
Parameter draft fields retain `param:new:name/unit/expression/comment`.
Configuration drafts publish `config:new:name`; existing `config:cell:ROW:COLUMN`,
activation, rule and preview handles remain. The header plus retains
`dialog:menu:Add Column`, including the existing menu nesting. The old separate
Add forms and column-removal band have been removed.


Crash Reports uses `report_list` (bordered bounded scroll), `report_header` (kind,
count pill and mono UTC time), `path_chip` (clipped mono path and adjacent action),
`empty_box` (dashed empty state), and `icon_label_button` (secondary icon/text,
optional danger ink). These use `dialog_rule/footer`, `control_*`, `form_*`,
`font_label/small` and spacing tokens. The report header retains the combined-title
cell alias; bins retain the previous Delete handle. Report actions remain immediate.
`list_frame` supplies the same border to virtualised library rows.
The list frame owns its border and horizontal padding; the report viewport adds
no inherited scroll margin. Report list measurements publish
`dialog:list:KEY:viewport` and `dialog:list:KEY:content_width`. Reports use vertical
scrolling; the horizontal rule beneath each report remains a divider.
Appearance uses a 340 pt card derived from card/label/spacing tokens, with the
104 pt label basis where space permits, 10 pt body gaps, and a count on In This Design.

Colour popovers retain `dialog:color:ROW` and add `color:format:U8/F`,
`color:copy`, `color:channel:R/G/B`, `color:sv`, `color:hue`, `color:current`
and the `color:popover` rectangle. Format changes update egui's global numeric
colour presentation without changing RGB. Linear fields edit linear RGB, not
normalised gamma bytes. HSV state preserves hue at zero saturation/value and
sub-byte precision during drags. Gradient colours are document colour data;
all interface chrome uses existing paired light/dark tokens.

Sliders only via `ui_kit::slider`, everywhere in `crates/ui-egui/src` and `apps`.
The element owns its form styling even when called from a viewport menu.
`Slider` options select behaviour, never appearance: range and values remain in caller
units, `step` preserves stepping, `format` preserves suffixes/display mapping,
`integer` rounds manipulation and `show_value: false` hides the editor. Disabled
sliders use `ui.add_enabled_ui`; tooltips attach to the returned response. Preferences
UI scale still applies only after the pointer is released. There are no alternative
slider helpers, raw `egui::Slider` callers, or custom track/thumb painters outside
the kit. The source guard scans both trees without an allowance table; the gallery
covers continuous, percent/stepped, integer/no-editor and disabled states in both themes.
Numeric fields use the existing kit `scalar_field`, `length_field`, `angle_field`
or `number_field` rather than constructing a raw `egui::DragValue` at a caller.

Parameters retains its bounded header drag offset while the manager is open,
including frames without a regular feature dialog. The shared Panel bound reserves
header and footer space at the bottom edge. Import and both export actions use
`icon_label_button` with the existing `import`/`export` arrow icons and unchanged
command routing and scenario identities. The gallery includes all three actions.
The Add Column target is `form_close_size - space_4` (22 pt) square: idle paint is
only `form_dim` ink; hover adds the selected accent fill and insertion guides.
HSV gradients use a rounded mesh silhouette sampled along `control_radius` arcs;
the current-colour strip uses the same rounded radius. Render tests sample all
four corner pixels and opaque interiors of all three surfaces at 1× and 2× in
both themes.

Checkboxes only via `ui_kit::checkbox`, everywhere in `crates/ui-egui/src` and
`apps`. `checkbox_row`, `checkbox_grid` and menu checkbox rows share its single
box painter and `paint_check_glyph` path. The box is `space_14` square with
`radius_sm` corners, a `control_border` outline and `control_fill` background.
Checked/mixed boxes use `form_primary` and `on_accent`; hover uses `hover` and
an accent border, focus adds a `space_2` outset ring, and disabled controls inherit
egui's disabled opacity. Geometry never depends on caller font or native icon size.
`checkbox_indeterminate` represents a mixed selection with a bar and activates to
checked. Space/Enter, accessible checked state, tooltips and scenario handles are
retained. Exclusive choices use existing kit segments/dropdowns/list pickers;
there are no radio controls. The recursive guard rejects native checkbox/radio
constructors (including aliases), raw toggle methods and duplicate check glyph
painters across both trees without allowances. Offscreen checks compare actual
box and tick geometry/pixels across form, grid, palette and menu call sites at
1×/2× in both themes.

Disclosure chevrons point **down when open** (content visible) and **right when
closed** (content hidden). Only `ui_kit::disclosure` draws them, using the original
`chevron` / `chevron_right` SVGs at `form_section_icon_size`, without animation
(as in kit section headers). Sections, nested groups, Browser trees, card headers,
window folds and the Assistant strip all follow this rule. Keep existing scenario
handles, including `dialog:collapse`, and translated Collapse/Expand tooltips.
Dropdown arrows always point down; submenu arrows point right. Playback, reorder,
window management and overflow controls are not disclosures. The source guard scans
UI, apps and compiled asset glyphs without caller allowances; render tests inspect
the actual icon textures used by sections, card folds and window folds.

## Per-tab accent tokens

`TabAccent::for_tab` supplies three independent theme colours: `color` (fill) for
pills, active-tool borders and the history marker; `ink` for small text and selected
rows; `soft` for tinted selection/tool backgrounds. These are palette entries, not
blends. Browser rows and selected ribbon labels use `ink` directly.

| Tab id | Fill token | Ink token | Soft token |
| --- | --- | --- | --- |
| `MODEL` / default | `accent` | `selected_ink` | `accent_soft` |
| `SKETCH` | `sketch_accent` | `sketch_ink` | `sketch_soft` |
| `SURFACE` | `surface_fill` | `surface_ink` | `surface_soft` |
| `MESH` | `mesh_fill` | `mesh_ink` | `mesh_soft` |
| `FORM` | `form_fill` | `form_tab_ink` | `form_soft` |
| `SHEET METAL` | `sheet_fill` | `sheet_ink` | `sheet_soft` |
| `RENDER` | `render_fill` | `render_ink` | `render_soft` |

`form_tab_ink` is separate from the existing `form_ink` used for form controls.
Sketch reuses its existing fill in both themes. Surface and Form fills match their
existing accents in light mode but need independent dark values; Mesh and Render
fills also differ from their existing `mesh_accent` and `render_accent` values.
All existing tokens, including Model's fill, ink and tint, retain their values.
The Model/default mapping preserves its existing appearance rather than adopting
the proposed Model colours from the board.

The new palette entries (fill / ink / soft) are:

| Tab | Light | Dark |
| --- | --- | --- |
| Sketch | `#057F45` / `#057F45` / `#EDF7F1` | `#128D45` / `#46BE78` / `#1F3B2B` |
| Surface | `#A45000` / `#A45000` / `#FBEBD8` | `#B26A14` / `#E0914A` / `#45331E` |
| Mesh | `#E0B200` / `#8A6D00` / `#FFF8DC` | `#C6A800` / `#D4B726` / `#41391A` |
| Form | `#7349CD` / `#7349CD` / `#EEE8FA` | `#8A6FCC` / `#B39BEE` / `#372F4F` |
| Sheet Metal | `#1E6F96` / `#1E6F96` / `#E1F0F7` | `#2A7FA8` / `#4FB3D9` / `#1F3A47` |
| Render | `#A8326E` / `#A8326E` / `#F8E6EF` | `#C8588F` / `#E074AE` / `#402336` |

Each key is supported by `solidcraft-theme/1` import/export, including
`tab_on_fill_dark` (`#000000` in both themes). `TabAccent::on_fill` uses `on_accent`
(white in the built-in palettes) when it reaches 4.5:1, otherwise that dark ink.
The fallback is black because the board's dark ink does not reach 4.5:1 on several
of the approved fills. It only styles tab pills; global primary actions still use
`on_accent`. Built-in palette tests require WCAG contrast of at least 4.5:1 for
ink on the card and panel, ink on soft, and pill text on fill in both themes.
Only Mesh separates ink from fill in the light theme.

Context menu adapters retain item IDs, enabled logic and command parameters. `Menu`
owns popup elevation (10 pt corners, 1 pt border), headings, separators, adjacent
submenus and bounded arrow navigation. Enter opens a submenu or chooses its row;
Right opens, Left returns, and Esc dismisses the menu. Appearance includes document
swatches and Remove override through `appearance.assign`; its original action still
opens the full editor. Workspace retains its descriptive cards and scenario handles
on the shared surface. Ribbon rows reuse `MenuItem::paint` while owning drag, focus
and pin interactions. History context menus and ribbon overflow use kit popup chrome.
Rename uses the shared caption, field policy and Cancel/OK actions with its stable
editor ID. No additional translation keys or colour roles are needed.

The menu gallery includes normal, hovered, open, disabled, checked/unchecked,
swatch and danger rows, a submenu and rename. Radial geometry tests measure the
actual side endpoints at both radii and sample all four rounded corners. Optional
CPU-rendered menu evidence can be produced without a window with
`SOLIDCRAFT_MENU_EVIDENCE=/tmp/menus cargo test -p solidcraft-ui-egui --lib menu_board_in_both_themes`.
Desktop interaction and visual acceptance still require a reviewer capture.

Command search uses the ribbon field itself (`sc_sbox_text`); `ribbon:search`,
`shortcut:search` and `palette:search` all identify that same field. Opening gives
it the accent focus ring, cursor and Esc keycap. The field and the attached results
sheet unfold in 120 ms with egui cubic ease-out, capped by egui's animation time
so instant settings remain instant. Esc, click away and command dispatch reverse
the animation. Only the visible sheet height participates in layout and hit testing;
result content is clipped while unfolding. The sheet's `ribbon:search_popup` /
`palette:outer` handles exclude the field. Its footer (`ribbon:search_footer`)
fills the bottom strip edge to edge, with rounded bottom corners and a 1 pt top
rule. Ranking, command mode and keyboard routing remain with the caller.

Generate CPU-rendered evidence of all five search states and the actual ribbon's
closed/opening/open/closing transition in both themes with
`SOLIDCRAFT_SEARCH_EVIDENCE=/tmp/search cargo test -p solidcraft-ui-egui --lib command_search`.

### Compact workspace composition

`Container::gap` and `Container::frame` supply the layout policy for every adaptive
surface. Compact has no gaps or shadows between edge-bound panels; floating uses
the shared card spacing and frame. `Panel` bounds draggable compact sheets to the workspace. The shared frame rounds
all corners away from the right dock and only the left corners while right-docked;
the footer inherits that frame's bottom corners. Both layouts retain drag offsets. Browser uses the same content renderer and width in both modes,
with a square, flush ribbon join in Compact. Non-workspace surfaces keep their roles.
`DockEdges` declares touching edges. The container measures content-hugging cards
before rounding their exposed corners, and aligns touching outlines onto a single
border strip without changing logical panel allocations. The application bar has
an independent surface role.
The saved `Prefs::layout` is changed through `view.layout`; `ui.set` may still make a
transient automation override.
## File/start presentation

`FileView` owns File surfaces and controls. It accepts translated strings, existing
icon ids and cached thumbnail textures; callers retain commands, document state,
persistence, thumbnail jobs and recovery decisions.

- `page`, `content` and `rail` own the page background, responsive insets and
  full-height rail fill/clip. `navigation` also supports the narrow navigation menu.
  Callers supply layout rectangles, scrolling and entry order.
- `nav_row` accepts a stable id, label, shortcut, selection and enabled state.
  `back_to` accepts a translated `{name}` template, paints the document name
  semibold and reuses the mirrored chevron and outlined keycap. `separator`
  supplies grouped rules. Disabled rows cannot activate.
- `title`, `subtitle`, `detail` and `section` own typography. `section` accepts a
  token-based heading/body gap: `space_12` for New, `space_10` for Recent/Samples.
  Callers retain semantic section order and the larger `file_section_gap`.
- `grid` provides responsive columns with 14 pt gutters and no trailing row gap.
  `action_tile` supplies an 84 pt card with 44 pt original artwork and vertically
  centered label/subtitle. `tile_min_width` informs responsive columns.
- `recent_list` owns the rounded border and zero row gap. `recent_row` accepts a
  `RecentRow`, paints internal separators, medium names and workspace dots, and
  retains title/path at narrow widths. Callers handle unavailable-file actions.
- `sample_grid(ui, id, &[SampleCard])` measures each row's tallest content and
  stretches cards to equal bottoms without clipping descriptions. Responses stay
  in sample order, with stable `{id}:{index}` ids. Artwork inputs remain unchanged.
- `recovery_banner` accepts `RecoveryMessage` segments for a bold title, normal
  message, monospace filename and normal suffix. Actions align right or wrap below
  at narrow widths. Discard is optional for summaries; the return value is
  `(recover_response, optional_discard_response)`. `recovery_review` uses the same
  geometry with per-design Discard. Callers provide truthful metadata.

The exported `file_*` colour tokens provide readable light/dark foreground and
background pairs, including File-specific secondary text. Thumbnails retain the
neutral `file_image_tint`; icon and text colours do not change their pixels.
Spacing, dimensions, fonts and radii come from `theme::Tokens`.

Gallery and geometry tests cover these components. To capture the actual File
view using public temporary fixtures, set `SOLIDCRAFT_FILE_EVIDENCE` to an output
directory and run `file_view_application_evidence_in_both_themes_and_widths` with
`--ignored`. It produces desktop and 420 pt narrow PNGs in both themes.

`button_at(ui, id, rect, label, Button::Primary/Normal)` places the same shared
native button used by `button` inside a bounded, centred layout. It preserves
stable identity, enabled state and exact caller-owned geometry. Recovery actions
use this component rather than custom button painters. The recovery well renders
the existing restore artwork through its construction accent ramp.


### Theme corner radii

Theme files may override `radius_sm`, `radius_md`, `radius_lg`, `radius_xl`,
`control_radius` and `sketch_mode_radius` in an optional `radii` object (integer
UI points, 0–32). Missing entries retain the base palette's shape. Light and dark
variants of a family should use identical values. Import/export and preferences
retain these overrides; colour vision modes leave them alone.

Use `Tokens` for decorative UI corners, including hover, selection, focus,
image thumbnails, popups and nested surfaces. Small legacy accents derive their
radius from the small token so the default appearance stays unchanged. Expanded
outlines use `Tokens::outset_radius` so square corners stay square. `Container`
continues to own outer surface chrome and square dock contacts. Functional
geometry (circles, model selection boxes, plot clipping, manipulator handles
and the artwork inside icons) retains its own shape.

## Theme border hierarchy

Three structural levels distinguish outer chrome, major section rules and minor dividers.
Separate solid container and control roles keep enclosing boxes independent of divider patterns.

| Role | UI audit / assignment |
| --- | --- |
| Primary | Ribbon/workspace cards, Browser/History, feature and palette sheets, system windows, menus/popups, navigation/status surfaces |
| Secondary | Ribbon tab/content rule, Browser/History heading rule, feature/window header and footer, Preferences page/navigation division, disclosure section rules, Assistant header/composer division, File navigation sections |
| Tertiary | Ribbon category dividers, menu groups, report/table rows, neutral History items, File cards/thumbnails, ordinary inherited egui separators |
| Container | Settings previews, information cards, list/detail wells, Assistant content cards, shortcut keycaps |
| Control | Neutral fields, buttons, checkboxes, segmented choices and search inputs |

Focus, selection, active commands, errors, recovery warnings and status keep their existing
semantic strokes. Icon artwork, example mouse diagrams, plots, model edges, manipulator
geometry and the compass are content; structural border roles do not recolour them.

`solidcraft-theme/1` optionally accepts `borders`. Each role has a `width` (finite 0–4
logical UI points; zero hides the stroke) and a `color` (`#rrggbb` or `#rrggbbaa`).
`primary_inset` also requires `gap` (finite 0–12 points of clear distance inside the
primary border). This companion is painted inside panel/ribbon/window/menu chrome,
without extra input targets or layout space; tiny surfaces omit it. Compact contacts
remain square and collapse adjoining primary strokes. Other ornament, such as the
circular compass and native widget-owned tooltip content, does not acquire an inset.

```json
"borders": {
  "primary": {"width": 1, "color": "#ffffff"},
  "secondary": {"width": 1, "color": "#e5efff", "pattern": "dotted"},
  "tertiary": {"width": 1, "color": "#102748"},
  "container": {"width": 1, "color": "#a8c8ed"},
  "control": {"width": 1, "color": "#a8c8ed"}
}
```

Those are Blueprint's settings: a single primary outline and dotted secondary rules. Secondary roles optionally accept `pattern`: `solid` (default) or `dotted`; this pattern follows horizontal/vertical section rules. Container outlines remain solid, as do the other roles. Themes without border settings retain their previous appearance.
SolidCraft 95 uses silver surfaces and directional two-band bevels: light top/left
edges and dark bottom/right edges for raised panels and buttons, reversed for
recessed fields and pressed buttons. Inputs use the `field` colour independently
of the neutral button face. Disabled controls fade their bevel; focus, selection
and validation keep their semantic indicators. Solid grey rules divide sections.

Optional `borders.bevel` enables this shared container/control treatment. It accepts
`width` (finite, greater than 0 and at most 2 points per band), `highlight`, `light`,
`shadow` and `dark_shadow` (hex colours). Missing bevel settings preserve flat
rendering exactly. The bands paint inside existing rounded or square geometry
without changing hit targets or allocation. Ghost controls remain borderless.
Missing roles preserve each surface's existing width and colour, including overrides in
`colors`; exporting keeps omitted roles absent so reimport cannot flatten those legacy
surface differences. Dark/Light and the Gruvbox families share border geometry.
Imports and direct preference updates reject unknown roles/settings, invalid colours,
non-finite/out-of-range numbers and missing required settings. Invalid hand-edited saved
border settings safely fall back to legacy styling. The optional field requires no format
version change. There is no general border editor in Preferences; theme file import/export
and the existing preference/control workflow own this extension.

## Theme icon and tab colours

Theme files optionally supply `icon_palette` SVG roles and semantic ramps. The
shared renderer snapshots that palette into the icon look and cache identity;
callers continue to request semantic icon IDs and surface-specific monochrome ink.
See [the shipped-theme colour audit](theme-colour-audit.md) for the schema,
workspace assignments, accessibility rules and intentionally monochrome themes.
