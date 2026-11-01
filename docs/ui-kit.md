# SolidCraft UI kit

`viewport_status_item` and `viewport_status_row` draw persistent viewport pills on
`Surface::ViewportStatus`. Callers supply translated names, formatted settings,
kind, stable identity and key availability, and dispatch returned clear/edit actions.
The row keeps supplied order and anchors to the top centre of the free viewport.
Its presentation state remembers only open cards, focus, scroll and the selected diagnostic
record, scoped to the current document. The `ui.status_fixture`
control method provides named specimens for UI scenarios and live visual review.

`details` renders a `Details` description in the shared host: optional caption, live settings,
key, findings or readout, then at most two text actions. Tools supply data and handle
`DetailEvent` requests through their normal commands. The caption and actions stay fixed
around a body bounded to 60% of the free viewport height. Findings/readouts preview six rows;
Show all expands the same scrolling body. `patterned_swatch` supports an optional second
semantic colour, so a grouped key can honestly represent both existing scene colours.

`viewport_status_details` defaults to live cards. Its false value restores read-only keys
and the Section dialog independently of the analysis/Section HUD and Summary preferences.
Continuity edits explicitly select one retained body/edge record; its own successful limits
and sample count accompany every live command. Stale geometry disables editing and targets.
`view.status_details` reads current effect settings/capabilities and the retained per-seam
diagnostics without preparing or evaluating geometry. `details_findings` and
`details_readout` fixture presets are labelled component specimens, not production results.

| Component | Use |
| --- | --- |
| `method_section` | Section with a GPU/CPU tag; unsupported contents stay faded with a readable reason. |
| `read_only_text` | Bounded, selectable code box for read-only diagnostic JSON. |

`crates/ui-egui/src/ui_kit/` contains the tokenised controls used by feature dialogs,
preferences, manager cards, prompts, context menus and the sketch palette. Import through
`crate::ui_kit::{...}`. Most controls are crate-visible; the container API is public so platform apps use the same hosts.

## Stage bars

`StageBar` composes two clipped, horizontally scrollable rows inside the shell's
shared floating `Container` (`Surface::Chrome`). Its 36 pt first row contains numbered
stage chips with the current workspace accent, shared primary/normal buttons and
the existing command-search editor. The second row fills the remaining card height.
The feature supplies labels, enabled reasons and command dispatch; the kit owns
chip geometry, row spacing, typography and colour. Stage selection is navigation;
the kit never invents completion badges. The shared compact workspace card is 104 × 88 pt; its icon, label and caption are
centred vertically. The stage bar is 88 pt high with an 8 pt gap from that card.
Its 36 pt header and 52 pt tools band share a 1 pt divider. Tools use 8 pt side
padding, icon-text segments, icon-label actions and an information chip, all with
`theme::Tokens` colours in both themes. Render retains its Inspector controls
outside the bar.

`Panel::workspace` gives paired floating docks the same bounded vertical span and
limits each dock to half the available width with an 8 pt gap. Content clips inside
the allocated host, so a small window cannot grow a dock into the stage bar or Shots
strip. Docks without usable space are omitted. Render's default outer widths are 266 pt
for Browser and 305 pt for Inspector, retaining the shared minimum/maximum resize
rules. Its compass is 110 pt; the 48 pt Shots strip clears the docks by 8 pt.

## Composition and rules

| Component | Contract |
| --- | --- |
| `ReviewAndApply` | Shared resource card, wrapping Replaces/Kept rows, variant content, acknowledgement, Cancel and a gated apply action. Physical plans offer Copy plan. Composes `Confirm`. Gallery: `review_and_apply`. |
| `SphereTile::badge` | Optional availability text on a three-column library tile, alongside the user-count badge. Uses theme tokens. |
| `catalog_footer`, `catalog_footer_with_tip`, `catalog_footer_height` | One-line information note with ellipsis and full guidance on hover; optional separate tooltip text. |
| `CatalogLayout` | Fills the remaining panel height with a scroll body above a measured bottom footer. Optional minimum search-row count keeps lists usable while a long detail card scrolls independently. |

- Every outer host uses `ui_kit::Container` with a semantic `Surface`. The shell
  publishes the layout once per frame; `Container::floating()` and `frame()` own
  adaptive placement policy and chrome. `Panel`, `Window`, `Confirm` and `Menu`
  compose it. Raw egui hosts and outer shadows are rejected recursively, including
  platform apps. The mandatory `cargo xtask ui-containers` gate runs in fast CI and
  scans new crates/apps automatically, including macro bodies and alias chains. See the [container audit and migration inventory](ui-container-audit.md).

- Colours, typography, dimensions and spacing come from `theme::Tokens`.
- `Container::selection_card` extends the navigation host with a bounded, rounded
  selection pill. `selection_actions` composes its name chip and quiet icon actions;
  callers supply command data and dispatch the chosen action. `Container::tooltip`
  shares the library samples' tooltip frame and padding.
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
  `count_row` composes `row` with `count_field` for pattern quantities.
- `Panel::workspace` owns grip-based left/right placement and bounded width resizing for
  floating workspace cards, including in Compact ribbon layouts. It reuses the 40 pt
  Panel/Window header and padded body, with no second top inset below the ribbon.
  Persistent workspace cards omit the close action. `Panel::workspace_with_header` lets tree cards
  compose the shared `tree_header` (title, compact lens and quiet search action). Its tree policy
  caps the card at ten 28 pt rows and lets rows own their 8 pt side insets; header navigation
  retains a separate title grip. Optional header actions share the same title band.
  `stack_first_bounds` reserves a bounded Browser above its companion;
  `stack_next_bounds` places that companion with the shared gap. `Panel::bottom_strip`
  uses `Container::bottom_strip_rect`, the same placement as Design History.
  `Container::navigation_card` supplies the shared Navigation surface for anchored HUDs.
  Register measured card rectangles and reserved edges with `shell::register_panel` so
  fitting and pointer shielding agree. Feature modules provide content, never outer chrome.
- `Panel` owns bounded windows, feature/palette composition, scrolling bodies,
  pinned actions, compute status and keyboard routing. `wide()` uses three card
  widths. `feature_frame` and `palette_frame` retain docked/floating surfaces.
  `FeatureOptions::apply_label` adds Apply; `Panel::actions` supplies custom labels.
  With `DialogButtons::Top` published (Preferences › Interface › Dialog buttons), `feature`
  puts the actions in the header row through `HeaderActions` instead of a footer: the same
  buttons, enabled states, keyboard and handles, each as wide as its bold label, with Cancel
  standing in for the close button and the compute status in the title's place.
  `palette` (the Sketch panel) follows the same preference: its footer row moves under the
  header, above the options.
  `body_rows` caps a manager's scrolling body independently of its position; short
  viewports still keep the header and actions visible.
- `Window` composes the shared Panel frame with a 40 pt icon/title/close header,
  padded content and optional 48 pt footer. `show_with_left` pins a left action beside
  the right-aligned buttons. `show_pages` accepts declarative `WindowPage` entries
  (icon, i18n label key, optional group heading, stable handle, scroll policy and draw callback).
  Its 188 pt soft page list (150 pt below a 700 pt window width) has 32 pt icon/label rows,
  selected tint/ink and a divider. Page windows use the navigation board's border/radius
  and the shared menu shadow; other windows retain their shared card surface.
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
- Every kit text field has one right-click menu, `text_menu::menu` (#8875): Cut,
  Copy, Paste and Select All, disabled when there is nothing selected, the text is
  read-only or a password, or the clipboard holds no text (always in the web app,
  where Ctrl+V pastes). A kit helper calls it right after adding its `TextEdit`;
  the edit marks the response changed and gives the focus back, so owners that
  commit on lost focus keep the field. Filters inside menus and drop-downs have no
  menu of their own. Rows publish `menu:text_cut`, `menu:text_copy`,
  `menu:text_paste` and `menu:text_select_all`.
- Scenario handles and accessible labels are API. Consolidated options preserve
  the former geometry, wrapping, native metadata and publication keys.

Preference compositions use `settings_heading`, shared disclosure `settings_section` headings and
`settings_pair` for responsive controls beside one specimen. `option_card` supplies the
shared illustrated radio control. `settings_form` and `settings_block_hint` define aligned label grids; `theme_rail` keeps the import tile fixed beside a scrollbar-free scrolling row. `PreferenceFigure` owns theme swatches, orbit pivots,
quality, anti-aliasing, contact shading and up-axis figures. `layout_preview` follows the
four workspace choices without opening another viewport. Scale, palette, autosave, mouse
and live-axis specimens share tokens and the gallery; live bars show actual calibrated
input and introduce no repaint timer.

`theme_options_list` renders generic `ThemeOptionRow` definitions without standard-option
branches. Its pure `preferences_layout::layout` packs measured narrow/composite/wide entries
in order, with ten non-advanced top-level cells before More. A composite retains its child in
one cell; advanced descendants move the entire cell under More. Packing and allocation share
measured localized label/readout widths, the form gutter and a compact slider minimum; short
composites can share a row with narrow entries. A composite takes a full row when needed and
stacks its controls only if they exceed that entire row's width. `theme_options_badge` paints
inside an existing theme card and is called only for offered options. The small registry in
`theme_options.rs` supplies standard label/help catalog ids, availability and behaviour;
only opacity is registered today. Missing providers are absent; unavailable registered
providers keep their controls and show their translated reason. Callers submit list edits
and Reset through the existing atomic preference merge/control path.
The scoped `i18n::resolved_labels` adapter keeps theme language-map text and fallback labels
verbatim through shared controls, while stable catalog messages continue to translate.

Review fixtures are in `crates/ui-egui/tests/fixtures/theme-options.json`, keyed by
`light-0/1/3/6/12` and `dark-0/1/3/6/12`. Each value is a complete importable theme file:
pass it as `ui.prefs` `theme_import` together with `open: "Interface"`. The 12-entry files
include a four-choice wide row, Shadow/Softness composite, six-choice dropdown, an advanced
entry and overflow. More contains two top-level entries. Set
`theme_options: {"options-dark-12": {"opacity": 60}}` (or the selected fixture's id) to
compare with 100 on a busy model. Reset uses `prefs:theme_options_reset`; More uses
`prefs:theme_options_more`; controls and cells publish `prefs:theme_option:<id>` and
`prefs:theme_option_cell:<id>`. Unavailable-provider specimens are internal to the existing
`prefs::tests::theme_options_provider_registry_filters_unknown_and_retains_unavailable` test;
they are never installed in the live registry or exposed as preferences. The manager captures
and reviews live-app screenshot evidence separately from the headless harness tests.

## Elements

`aspect_grid` composes three-column `AspectTile` choices with token-sized aspect glyphs,
selection tint and stable targets. `frame_guides` paints a bounded viewport mask, outline,
thirds, a 90% safe area and a centre mark from `FrameGuides`. These are viewport decorations;
callers exclude them from exported images and result canvases.

Module names below describe implementation locations, not caller import paths.
Every inline element/variant has one entry in the light/dark `gallery::ELEMENTS`
table. Panel/Window/Confirm compositions also have dedicated geometry and interaction tests.

| Module / element | Use and options | Main tokens |
| --- | --- | --- |
| assistant: `assistant_header`, `assistant_controls`, `assistant_badge`, `assistant_bubble`, `assistant_prose`, `assistant_tool_frame/row/detail/status`, `AssistantToolState`, `assistant_approval/actions`, `assistant_composer/footer`, `assistant_input`, `assistant_send`, `assistant_empty`, `assistant_banner`/`banner`, `AssistantTone` | Shared Assistant presentation: 40 pt header, 22 pt state pill, right-aligned user messages limited to 86% width, unboxed assistant prose, bordered tool groups and command detail, tinted approval heading, rounded focused composer with one unframed growing editor and pinned footer, centred setup copy and semantic notices. Closures preserve existing permission choices, protocol routing, queue actions and scenario handles. | `form_header_height/close_size`, `font_label/small/body`, `dialog_body/footer/rule`, `control_fill/border/radius`, `form_ink/dim/primary/success/error`, `accent_soft/selected_ink`, `warning`, radii and spacing |
| history: `StripHeading`, `HistoryChip`, `HistoryStyle`, `history_drop_bar`, `history_marker`, `HistorySeam`, `history_seam` | Shared strip headings use semibold `font_heading`, `text`, a centred baseline and `space_12` trailing gap. One painter family for bottom chips, 38 pt vertical icon chips and single-line 26 pt name rows (the browser's shared `TREE_ROW_HEIGHT`). Selected items use the active tab tint/ink/border; all variants retain multi-tone feature artwork using one shared kind-to-accent rule (Sketch green, Surface orange, Mesh yellow, Form purple, solids base blue); rolled-back items have 42% opacity; a reference row (a sketch listed under another feature, #6489) has its name in secondary text. Error/warning badges, suppression strikes, horizontal/vertical drop bars and rollback line/hollow circle are shared. No connector lines. | `control_height`, `space_2/3/4/6/8/10/12/14/16/24`, radii, typography, `TabAccent`, `timeline_item`, `control_border`, `text/dim`, `form_error/fill` |
| tree: `tree_row`, `Row`, `TreeAction` | Shared Browser rows with disclosure, visibility (an eye column at the row's end, clear of a floating scroll bar, with a gap between the disclosure and the activation radio; or beside the disclosure with `eye_left`), activation, selection, appearance swatches, optional subtitle and trailing actions. Compact scene rows are 28 pt, with inline right-aligned summaries, circular swatches, trailing eyes and full-width selection tint. Actions use quiet icons and can appear on hover. Disabled actions retain their reasons and cannot select or fold the row. Browser automation records the same row geometry. | `TREE_ROW_HEIGHT`, `font_label/small`, spacing, control radius/border, `text/dim`, workspace selection tint |
| search: `CommandSearch`, `MenuItem::search_row` | Ribbon search composition: one persistent ribbon editor expands from 28 to 40 pt and to 520 pt wide with its right edge anchored, magnifier and S/Esc keycap, 38 pt two-line menu row with 26 pt icon tile, accent bold substring, path, shortcut/kind keycap, counts, JSON detail box and footer. The ordinary 30 pt menu row cannot fit a second line; this variant keeps its enabled/selected states and scenario IDs. Ranking and dispatch stay with the caller. | `form_card_width × 2 − space_8` (520 pt), `control_height`, `space_2/3/4/6/8/10/12/14/16/24`, `radius_xl/control_radius`, `control_fill`, `dialog_body/footer/rule/border`, `form_ink/dim/primary/error/error_fill/success/success_fill`, `selected_ink`, `accent_soft`, typography |
| menu: `Menu`, `MenuItem` | Shared popup, 30 pt rows, 28 pt icon column, clipped labels, right-aligned mono shortcuts, submenu chevrons, checked/unchecked and document swatches, inert 40% disabled rows and danger ink; headings and separators; ribbon drag/pin targets retained | `dialog_body/border/rule`, `radius_xl`, `control_height/radius`, `form_ink/dim/primary/error`, `accent_soft`, typography and spacing |
| radial: `radial_menu` | Eight rounded annular sectors, parallel 4 pt gaps, 9 pt circular corner joins, tab-accent artwork and hover, centre label/key; existing hold/flick/release dispatch | `dialog_body/border`, `form_ink/dim`, `TabAccent`, icon palette, font/spacing tokens |
| panel: `Window`, `WindowPage` | Shared card frame, header, optional page list, bounded content and footer with a left action | `form_header/footer_height`, `form_card_width`, `form_label_width`, `control_height`, `dialog_footer/rule`, `selected_bg/ink`, spacing |
| confirm: `Confirm`, `Tone` | Modal warning/danger/info, title/message, mono detail and ghost/cancel/primary actions | `warning`, `form_error/fill`, `accent_soft`, `form_primary`, `on_accent`, `tab_on_fill_dark`, `font_subtitle`, spacing |
| status: `completion_text` | Compact strip readout with an optional green completion tick and caller-supplied result identity | `font_small`, `control_height`, `form_ink/success/dim`, `space_4/14` |
| metrics: `row_height`, `table_row_height`, `header_height` | Shared control/header heights | `control_height`, `space_8`, `form_header_height` |
| layout: `scope`, `row` | Form styling and aligned label/control columns | `form_label_width`, `form_gap_y`, `control_*`, `font_label`, `dialog_label`, `space_4/8` |
| `status_line`, `StatusTone` | Full-width, wrapping status or refusal text in dim, warning or error ink. Translates source labels; already translated messages also work. Place directly in the form, outside `row`, so its height grows with the whole message. Rect: `dialog:status:TEXT`. | `font_label`, `text_dim`, `warning`, `error` |
| `caption` | Dim caption aligned beneath a control | `font_small`, `text_dim`, label-column tokens |
| `figure_rows`, `FigureSlot` | A caller-painted figure in the label column beside a stack of captioned expression fields (the Hole dialog's section); the caller gets each field's rect and hover/focus state to draw leaders and light the part it sizes. Fields keep their `dialog:<label>` handles; the block publishes `dialog:figure:<id>` and its picture `dialog:figure_picture:<id>` | `control_height`, `form_label_width`, `font_small`, `form_dim`, `space_2/4`, label gap |
| `linear_color_field` | Linear RGB swatch with precise channel fields; clips only the preview to display gamut and preserves HDR values until edited | `control_height`, `space_4`, shared colour popover |
| `section`, `section_count`, `section_method`, `search_fold`, `divider` | Persisted small-caps disclosure with chevron, optional count or method tag and trailing hairline; `search_fold` is that row for shortcut matches during Preferences search, closed until opened; full-width rule | `control_height`, `dialog_rule`, `font_caption`, tracking, spacing |
| `icon_action` | Icon-only action with an accessible name, tooltip, enabled state and danger variant | Shared icon-label action geometry and ink tokens |
| `list_row` | Picked item with a remove target | `control_height`, `font_label`, icon/spacing tokens |
| `text`, `Text` | Hint, note, report, inline or cell text; see variants below | `font_small/label`, `text/dim/error`, `control_height`, `space_4` |
| `text_lines` | Virtualized read-only licence/report lines, horizontal overflow | `font_small`, `text`, `control_height` |
| fields: `field`, `hint_field`, `focused_field` | Expression editors, optional empty hint or retained focus; returns accepted Enter | `control_height/fill`, `form_inset`, `font_label` |
| `count_field`, `Count` | Expression editor for a whole-number count (a pattern's quantity) with up/down step arrows at its right edge. `Count` gives the limits and the evaluator (the design's parameters); an arrow steps the evaluated count by one within them, replacing a plain number or adjusting an expression's trailing `+ n`, and is dimmed when it can't step. Handles `dialog:<row> up` / `down`; returns accepted Enter | field tokens, `control_border`, `control_radius`, `selected_bg`, `form_ink/dim`, `space_4` |
| `text_field`, `focused_text_field` | Plain name editor, optionally retaining focus | field tokens |
| `multiline_text_field` | Three-line plain-text row with stable focus ID | field/row tokens |
| `password_field` | Stable secret editor; excludes the secret from scenario/accessibility metadata | field tokens |
| `message_field` | Growing message editor; owner retains scroll and Enter routing | field tokens |
| `cell_editor`, `bounded_cell_editor`, `search_field`, `library_cell` | Stable-ID inline/search editors; bounded variant caps Unicode characters; `library_cell` attaches a Library dropdown inside the editor border | field tokens, `text/error` |
| `wide_cell_editor` | A table cell editor that, while focused and too narrow for its text, opens over the cells to its right and wraps, with the live value or error under it (#9358) | field tokens, popup frame, `text/error` |
| `menu_filter` | The filter at the top of a long menu: search glyph, frameless editor and a rule under them, so it does not read as an entry; the caller filters and owns focus | `space_8/16`, `font_label`, `form_dim/ink`, `dialog_rule` |
| `text_area` | Bounded multiline JSON/data editor | field tokens |
| `dropdown`, `dropdown_menu`, `compact_dropdown_menu` | Indexed choices or caller-supplied menu controls | `control_height`, `text_dim`, form tokens |
| `searchable_dropdown`, `SearchOption` | Filtered choices, custom-value action and pending message | dropdown tokens |
| `selection_field`, `Selection` | Selection chip with explicit `active` and `filled` state, in three looks: the active input (where the next pick goes) has a `form_primary` border and `selected_bg` tint; a filled inactive one a quiet neutral tint, a stronger neutral border and its count in `form_ink`; an empty inactive one the plain control with a dim prompt | `selected_bg/ink`, `form_primary/ink/dim`, `control_*`, `form_inset`, `space_2/8/24` |
| `value_pair` | Paired captions/expressions; optional independent field names | row/field tokens, `space_8` |
| `fraction_field`, `scalar_field`, `mixed_scalar_field` | Locale-aware bounded dimensionless values; scalar fields fill their control column; the mixed variant accepts a translated placeholder | form control/font tokens |
| `number_field` | Integer/decimal field with caller range, drag speed and suffix | form control/font tokens |
| `number_fields` | N numeric cells sharing a row's remaining width equally, including gaps; returns each field response in order | `control_height`, inherited item spacing, number-field tokens |
| `length_field`, `angle_field` | Length in design units, angle in degrees | form control/font tokens |
| `slider` + `Slider` options | One token-painted track/thumb over native slider/value interaction; optional step, caller formatting, whole-step manipulation and hidden value editor | `control_height`, `space_24/8/4/2`, form control/font tokens |
| `placement_field` | Stable focus ID; drawing tool owns Enter/Tab validation and completion | `control_fill`, `font_body`, `text`, `control_height`, spacing |
| `color_field`, `color_field_with_opacity`, `mixed_color_field`, `color_popover` | Document RGB swatch and kit popover; global U8/F/Hex segments, active-format copy, gamma-byte or linear RGB fields or one Hex editor, painter-drawn HSV square/hue and current strip; outside click dismisses. The opacity-aware field edits the caller's separate `f64` opacity, never premultiplied RGB. Six Hex digits preserve exact opacity; eight set alpha to `aa / 255`. RGB-only callers reject non-FF alpha without applying RGB. Incomplete text remains editable without changing the last valid colour. The mixed-aware wrapper adds a non-editable label and reports accepting the first colour as an edit, so the caller can set all selected targets. | `control_*`, `form_*`, `border`, `on_accent`, `space_*` |
| choice: `segmented`, `Segments` | Equal text choices; `states` supplies mixed selection, `enabled` keeps unsupported choices faded and inert, `disabled_reasons` also disables choices and supplies hover explanations, `compact` clips instead of falling back to a dropdown, `primary` uses accent/on-accent for the selected segment | `control_*`, `selected_bg/ink`, `font_label`, `space_4/8` |
| `segmented_icons`, `IconSegment` | Equal icon choices and mixed states | segment tokens, icon palette |
| `check_button` | Small action shown by the shared tick alone, accepting what is on the canvas | `control_height`, `space_3/4/14/24`, `form_primary` |
| `checkbox`, `checkbox_indeterminate`, `checkbox_row`, `checkbox_grid`, `options` | One box/tick painter for inline, mixed, full-row, grid and menu choices | `space_14/3/2`, `radius_sm`, `control_fill/border`, `form_primary/ink`, `hover`, `on_accent` |
| `text_emphasis` | Bold/italic with translated accessible names | form control/font tokens |
| `tool_choices`, `ToolChoice`, `toggle_icon` | Wrapped command modes or independent icon toggles | sketch mode/icon tokens, `accent` |
| `list_picker` | Selectable row with caller identity and native checked state | `control_height`, form selection tokens |
| `thumbnail_choice`, `ThumbnailChoice`, `add_choice` | Shared selection-chip variant with 52 × 30 preview, selected tab accent, modified dot, progress ring and dashed add action for bottom strips; `Panel::strip_contents` reserves right actions and scrolls choices | `control_height`, spacing, radii, `TabAccent`, control/semantic tokens |
| `popup_list`, `PopupOption` | Focus-neutral suggestions; owner retains keyboard routing | `space_2`, `form_label_width`, `control_height`, form tokens |
| sphere: `sphere_thumbnail`, `SphereTile`, `sphere_grid`, `sphere_preview`, `sphere_preview_with_source` | Deterministic lit material samples; matte/satin/gloss/metal/glass finishes. Catalog tiles centre a token-sized sphere above two full lines of text with an optional source badge, active-tab selection ring, user-count badge and shared Container tooltip. The grid owns three columns and token gaps. The Inspector preview composes a sphere, name, informational chips and replacement action in the shared list card; the source variant supports an interactive inheritance menu. | `control_height`, `radius_xl`, `space_2/3/4/6/8/16`, `font_small/badge`, `control_fill`, `dialog_rule`, active-tab accent, `on_accent` |
| `color_swatch`, `swatch_grid`, `color_thumbnail` | 30 pt draggable samples, 6 pt grid step gap, selected outer ring and checker pattern under transparent colours; caller payloads retained; local-library thumbnails use 16 pt | `control_height/radius`, `border`, `form_primary`, `control_fill`, `space_2/6/8` |
| `assistant_entry`, `AssistantEntryState` | Application-bar chat icon with quiet, ready, busy, auth, stopped and approval count marks; caller owns state and target | `app_bar`, `agent_ok/busy`, `warning/error`, `font_badge` |
| actions: `button`, `Button` | Secondary action: `Normal`, `Small`, or `Enabled(bool)` | native sizing, form font/control tokens |
| `primary_button` | Accent action outside a footer | `accent`, `on_accent`, form tokens |
| navigation: `NavigationMetrics`, `navigation_surface`, `navigation_button`, `navigation_separator` | Shared live/editor navigation painter, button states, upward menu chevron and separator; approved board dimensions below | `dialog_body/border`, `form_ink/dim/primary`, `control_border`, `selected_bg`, `nav_hover/press` |
| `ReorderPayload<T>`, `insertion_marker`, `drop_index` | Typed drag origin, insertion chrome and horizontal gap selection shared by the ribbon and navigation editor | `accent` |
| `navigation_button_editable`, `navigation_edit_outline` | The live navigation button with drag interaction, selection outline and dashed hidden-slot chrome | Navigation control tokens, `border/accent` |
| navigation_menu: `navigation_menu`, `NavigationMenuRow`, `navigation_context`, `navigation_tooltip` | Upward menus, checked rows, headings, separators and the delayed shortcut tooltip; context menus retain kit popup hosts and Container's shared elevation | `dialog_body/border/rule`, `form_ink/dim/primary`, `selected_bg`, `nav_hover/tip/tip_ink/tip_key_border` |
| navigation_settings: `navigation_stage`, `navigation_tile`, `navigation_drag_ghost` | Custom preview stage, draggable slot tiles and drag feedback. Ordinary preferences reuse `settings_section`, `settings_form`, `row`, default `segmented`, `checkbox`, `button`, `divider` and text hints | `dialog_border`, `control_border`, `form_ink/dim`, `nav_field/hover/viewport` |
| `settings_card` | Full-width settings detail group; uses Container chrome and preserves its width with empty or changing content | `dialog_footer`, `border`, `radius_md`, `space_12/10` |
| `mirrored_icon_button_at` | Directional counterpart of an existing icon master, sharing icon-button presentation and accessible label | Shared icon-button tokens |
| `icon_button_at` | Icon action inside a supplied rectangle | `form_icon_inset`, `text/dim`, icon palette |
| `button_pair`, `PairButton` | Equal secondary actions, optional icons and selected state | `form_button_height`, control/selection/icon tokens |
| `footer`, `Footer`, `Action`, `HeaderActions` | OK/Cancel with optional Apply, compute status or selected action; `HeaderActions` puts the same buttons in a header row | `form_footer/button_height`, `dialog_footer/rule`, `control_*`, `accent`, spacing |
| `keyboard` | Esc cancels, unowned Enter submits when valid | none |
| `action_menu`, `command_menu_button` | Nested dense action menu or ribbon overflow action retaining its icon, shortcut and enabled state | form control/font tokens, `space_16`, icon palette |
| table: `table`, `table_custom`, `Column`, `Table::headers`, `Table::row`, `TableRow::cell`, `Table::add_prompt` | Left-aligned headers/cells, horizontal overflow, selected/hover rows and empty state. `table_custom` supplies header actions through `headers`. `Column::flex` shares remaining width by weight, freezing columns at their control minimums before redistributing the remaining space; `Column::icon_only` hides the heading while retaining its automation identity. `add_prompt` is the final slim dashed line and requests a regular draft row from its owner; `Column::new(label, width)` uses label-column multiples, `min_content_width(points)` reserves measured control content; `ContentWidth::name` measures strong configuration names | label-column tokens, `control_height`, `dialog_rule/footer`, `selected_bg`, spacing |
| `name_list`, `NameList::row`, `NameRow::name`, `NameRow::cell` | A list of named rows on the table grid (the web app's stored files): the name column takes the width the other columns leave; the other columns are as wide as their widest cell, measured as drawn (a changed measurement discards the frame). A name is cut with an ellipsis, and given its full text on hover, only when it does not fit; `dialog:name:NAME:cut` reports it | `form_label_width`, `font_label`, `form_ink`, `space_8`, `table_row_height` |
| `removable_header`, `plus_header`, `bin_button`, `active_pill` | Hover-only column removal, anchored Add Column menu with insertion brackets, icon-only immediate deletion and active-row status. Bins preserve `dialog:button:Actions:Delete` and `:×`; header removal preserves `dialog:button:LABEL:×`. | `control_*`, `form_*`, `selected_bg`, spacing |
| `compact_button`, `summary_row` | Borderless stars and chevrons with enabled/selected states; named two-line rule summary beside an enable checkbox and compact actions. | `form_close_size`, `control_*`, `form_*`, typography and spacing |
| `label_actions` | Clipped row name beside compact actions | `control_height`, form font/spacing tokens |
| status: `status_header`, `StatusHeader` | Supplied icon/name/status and constraint meter | `font_label`, `text/dim`, `accent`, sketch/header tokens |
| `progress` | Clamped determinate progress | `space_3/6`, `dialog_rule`, `accent` |

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

`name_cell_with_hint(ui, name, strong, hint)` keeps the name cell's allocation and
scenario identity, but owns its single tooltip: the action hint alone when the
name fits, or the full name followed by the hint when it is clipped. Use it instead
of adding a second hover tooltip to `Text::NameCell`.

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

## Navigation board dimensions

`Navbar_Bar` / `Navbar_Settings` and their dark boards own this composition.
`navigation_bar_view::paint_row` paints both callers; the editor supplies selection,
canvas availability and drag interactions. Its stage publishes `prefs:navbar:stage`;
`prefs:navbar:bar` exposes the positioned bar inside it. Existing `nav:*` and editor
slot/tile/add/appearance targets retain their identities.

| Board specimen | Logical points / state | Authority |
| --- | --- | --- |
| Bar 1, 6 | Regular: 32 button, 18 icon, 7 button radius, 40 outer height, 10 bar radius. Compact: 26 / 16 / 6 / 34 / 9 | `NavigationMetrics::for_size`, `height` |
| Bar 1 | Border 1, padding 3, gap 2, bottom offset 7; separator width 1 + two 4 margins, height 18 | `NAV_BORDER/PADDING/GAP/BOTTOM/SEPARATOR/SEPARATOR_HEIGHT` |
| Bar 2 | Menu padding left 7 / right 6, gap 5, chevron 12; medium text 12.5 | `NAV_MENU_LEFT/RIGHT/GAP`, `NAV_CHEVRON/TEXT` |
| Bar 3 | Hover / press and open / latched; focus outline 2 with offset 1 | `navigation_visual`, `navigation_fill`, `NAV_FOCUS_WIDTH/OFFSET`; `nav_hover`, `nav_press`, `selected_bg`, `form_primary` |
| Bar 5 | Width 244, padding 5, radius 10; shared menu shadow (0,14), blur 36; row 30, radius 6, inset 8, icon 17, gap 9; heading 22, text 11, tracking .07em | `NAV_POPUP_*`, `NAV_MENU_*`, `NAV_ROW_GAP`, `NAV_HEADING_*`, `navigation_popup_frame`, `Container::menu_frame` |
| Bar 6 | Left / Centre / Right, 12 edge inset | `Position`, `NAV_EDGE`; legacy bottom / bottom_left / bottom_right deserialize as centre / left / right |
| Bar 7 | More button follows the selected size; popup width 200; value text drops before labels and overflow | `NavigationMetrics`, `NAV_MORE_WIDTH`, `navigation_bar_view::measure` |
| Bar 8 | Idle fade 45%, hidden 0%, restored 100%; hide-mode fade in 120ms | `navigation_opacity`, `NAV_IDLE_OPACITY/FADE_SECONDS` |
| Bar 9 | Tooltip delay 800ms, gap 6, padding (8,5), radius 6; 12 medium text, 11 mono key, 15 key line height | `NAV_TOOLTIP_DELAY/POPUP_GAP`, `navigation_tooltip`; `nav_tip/tip_ink/tip_key_border` |
| Bar 10 | Render hides Look at and Visual style; flat canvases hide Orbit, Spin, Look at, Projection and Visual style, retaining Home and Display | `Slot::available`, `canvas_caps` |
| Bar 11 | Context width 232; bar has Customise / Hide / Reset, slot has Remove | `NAV_CONTEXT_WIDTH`, `navigation_bar_view::context_menu` |
| Settings 1, 8 | Stage height 58, radius 8, padding 12 inside its 1 border; position follows preference live | `NAV_STAGE_HEIGHT/RADIUS/INSET`, `editor_layout` |
| Settings 1–3 | Standard settings section and full-width detail card; actions stay on one line, with name truncation before icon-only actions | `settings_section`, `settings_card`, `inspector_header_layout` |
| Settings 4, 5 | Marker 2×24 radius 1; ghost 70%, dragged tile 55%, hidden slot 40% and dashed border | `insertion_marker_with_ink`, `NAV_MARKER_HEIGHT/GHOST_OPACITY/TILE_DRAG_OPACITY/HIDDEN_OPACITY` |
| Settings 6 | Enabled Add buttons answer with “Remove a slot to add another.” at the 16-slot cap | Pure list operations in `navigation_bar.rs`; `warning` |
| Settings 7 | Standard form label column, control height, segment typography and fills; kit dropdown fallback at narrow widths | `settings_form`, `row`, `segmented`, default `Segments` |
| Settings controls | Standard checkbox, Normal buttons, icon buttons, divider and hints. Custom tile content height 30 + two 1 borders = 32 outer; Add 22 | `checkbox`, `button`, `icon_button_at`, `divider`, `Text::Hint`, `NAV_TILE_HEIGHT/ADD` |

The custom tiles use CSS content-box sizing. Their rendered outer height is
32 points, including the two border pixels around their declared height of 30.
Editor menu slots also include a transparent one-point border, unlike live menu
buttons. Their geometry and icon padding include it while reusing the same painter.
Ordinary choice rows use the app's standard segmented control and its narrow-width dropdown fallback.

Colours shared with the boards use `dialog_body` (window/popup), `dialog_footer`
(inspector), `dialog_border`, `dialog_rule`, `control_border`, `form_ink`, `form_dim`
and `selected_bg`. Additional `nav_*` roles above, plus `nav_field`,
`nav_viewport` and `nav_dim`, have light/dark defaults in
`assets/styles/ui/palettes.rs`. They are ordinary `solidcraft-theme/1` colour keys:
import, export and palette cache identity all include them through `theme_files.rs`.
Files omitting them retain their base palette, including older exported themes.
Semantic selection/action ink reuses measured `form_primary`, and the slot-cap notice
reuses measured `warning`; neither has a navigation-specific duplicate. The colour-vision
groups measure them together against the dialog ground. Remaining navigation colours
are neutral surfaces/ink, with explicit reasons in `NO_MEANING`. Popup elevation comes
from `Container::menu_frame`, not a navigation-owned shadow: its (0,14) offset, 36 blur
and app-bar colour at 18% replace the board's (0,4), 16 blur and 10%/30% shadow.

The default is Regular/Centre; saved Compact values stay Compact. `slots` remains
a plain list of IDs; optional `slot_options: {"visual_style":{"shows":"icon_and_value"}}`
stores menu presentation independently of global button labels. Missing Visual style
options mean Icon only; Display has no current value and remains Icon only.

Board regression checks live in the existing test modules:

- `navigation_bar_tests.rs`: `navigation_board_metrics_and_state_colors_are_shared_in_both_themes`,
  `navigation_board_popup_width_includes_its_border_and_padding`,
  `navigation_board_position_aliases_migrate_and_editor_alignment_follows_position`,
  `navigation_board_drops_menu_value_text_before_folding_slots`.
- `theme_files.rs`: `navigation_board_colours_are_themeable_and_roundtrip_on_both_bases`.
- `prefs.rs`: `navigation_bar_preview_stage_follows_position_live`; the existing narrow-width
  regression also checks unelided text bounds at 1600, 880, 640, 600 and 480 points.
  The existing drag regression covers removal into empty stage space as well as outside the stage.
- The existing pointer/state, shared-painter, context-menu and canvas-availability tests
  follow the board's changed states, editing borders, menu split and hidden slots.
  The tool-cursor regression uses its Navigation preference because Display no longer lists it.
  `tool_cursor.rs::navigation_preference_restores_tool_badge_on_cancel_and_defaults` covers
  the relocated setting's snapshot and page defaults.
  Runtime icon discovery visits More at narrow width, retaining Preferences page coverage.

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
For a numeric tuple, call `number_fields(ui, &mut values, range, speed, suffix)`
inside `row`. It divides the remaining control width equally after reserving the
inter-cell gaps, and keeps each cell's editor width and native input minimum local
to that cell. An empty slice draws nothing; responses follow the input order.

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
Dropdown arrows always point down; submenu arrows point right. Bottom-strip hide/restore
actions use `strip_toggle` with the same down chevron and its vertical flip for up.
Playback, reorder, window management and overflow controls are not disclosures. The source guard scans
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

`Panel::workspace` accepts `WorkspaceOptions::compact` to display the minimum
dock width while retaining the remembered width. Compact widths do not resize
the saved preference; returning to a wider floating layout restores it.

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

Preference page scrollers keep their content width separate from the scrollbar host's
gutter extension. Child layout overflow contributes height only, so it cannot widen
the window and leave earlier sections clipped at their previous right edge. The
Navigation editor follows the board's composition and uses standard kit controls
for ordinary settings, rather than a second control skin.

## Material previews and look chains

`sphere_thumbnail(ui, id, Sphere { color, kind, opacity }, size)` paints a lit
sphere using a fixed CPU mesh and the egui painter, without a texture or GPU.
`FinishKind` covers Matte, Satin, Gloss, Metal and Glass. Colour is document data;
highlights, shadows, checker and UI chrome use `Tokens`. Glass shows a four-by-four
checker and defaults to 35% opacity; other finishes default to opaque. Explicit
opacity is clamped to 0–1; non-finite opacity uses the finish default. Sizes are
rounded and bounded to 1–96 logical points (non-finite sizes become 1).

A shared per-egui-context FIFO cache retains at most 64 meshes, keyed by RGBA,
finish, byte opacity and bounded size. Hits reuse shading without changing eviction
order. Every mesh has 637 vertices and 1,152 triangles, independently of input.
Theme highlight/shadow changes clear the cache. No file assets are added.

`look_chain(ui, id, labels, looks, active)` draws Face → Body → Component → Material
as four connected steps. Pass translated labels, `[Option<Sphere>; 4]` and one
`LookStep`; empty slots use a dashed circle and plus, while filled slots reuse the
sphere cache. Exactly one step has a `Tokens::render_accent` ring. It returns
`Option<LookStep>` on activation; callers own commands and selection. Disabled
controls do not activate. Handles/rects are `{id}:face`, `:body`, `:component`,
`:material`; `{id}` is the full allocation. Long labels clip within their step.
These are content widgets; their enclosing surface still belongs to `Container`.

Gallery entries show all five sphere finishes, a mixed filled/empty look chain
with Body active, and an empty chain with Face active in both themes.

For CPU-rendered review images, run
`SOLIDCRAFT_MATERIAL_WIDGET_EVIDENCE=/tmp/material-widgets cargo test -p solidcraft-ui-egui ui_kit::gallery::material_widgets_board_in_both_themes`.

`catalog_scope` supplies compact catalog metrics to the shared search field, segments,
icon segments and category chips. `catalog_footer` draws the quiet information hint.
`CatalogLayout::new(ui, id, min_rows)` reserves the measured footer height and gives
`body` all remaining space without shrinking to its items. Call `footer` after
`body`; changes to the footer's natural height request another layout pass.
Use `min_rows = 3` for a search list with selected-item details below it; the
detail card scrolls when necessary. The kit publishes `{id}:content`, `:body`
and `:footer` rectangles for layout checks.
`SphereTile` reserves two full font rows below the sphere. Names wrap at word
boundaries with an ellipsis after the second line; `{id}:label` and `:galley`
expose the caption allocation and laid-out text bounds.
Catalog title marks use the active workspace accent; tile captions, usage badges and
selection outlines share the sphere widgets. Feature code supplies data and commands,
while the kit owns every dimension, colour and inset.

The Appearance Inspector uses the shared `inspector_header` (selection icon/name,
workspace chip, Menu and close action) and `Panel::editor_footer` (48 pt pinned
band). `mixed_color_field` uses the one shared colour popover with a non-editable
mixed-value label, not an outer Hex editor. Non-PBR Appearance passes exact opacity;
PBR passes no opacity and remains opaque. All Hex parsing and draft state stay in the kit.
`mixed_slider` pairs the standard slider with the mixed-aware scalar editor.
`editor_section` uses a 24 pt heading and 6 pt field gaps.
`workspace_chip`, `text_button`, `section_heading`, `pending_section`,
`searchable_icon_dropdown` and `Menu::text` retain
shared token paint and hit targets. All are exercised in the kit gallery.

| Shared editor component | Behaviour |
| --- | --- |
| method_section | Compact, initially collapsed editor section with a method tag; unsupported contents remain inspectable and disabled, with a translated reason visible even when collapsed. Uses editor_section typography and spacing. |
| vector3_field | Three bounded number fields divide the row's control column evenly, retaining the shared parser, focus and bevels. Used for RGB absorption and world-space origin/repeat vectors. |
| sphere_preview_with_source | Sample card with an interactive source tag, retaining the sphere, name, usage badge and trailing action. Appearance uses the tag to inspect its inheritance chain. |

The compact `settings_choice_row` variant reuses the shared settings radio-card painter
for preset lists: 40 pt rows, right-aligned summaries and the active workspace accent.
`progress_action` keeps a status caption, bounded progress track and Cancel action on
one row inside a Navigation card. `Container::below_navigation_row` reserves that card
and its shared gap above an inline result toolbar.

`number_pair` extends the shared value-pair row to bounded numeric fields (such as
image width and height), preserving label alignment, clipping and number-field behavior.
