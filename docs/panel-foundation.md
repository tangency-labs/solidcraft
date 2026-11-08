# Universal panel foundation

The public contracts are `solidcraft_engine::panels` and
`solidcraft_ui_egui::panel_runtime`. The #11305 host is explicitly enabled and
uses synthetic controllers to exercise shared behaviour. Preferences → Layout →
Use shared panel layout mounts the existing Browser, History, Assistant and Render
Scene Browser content in that host. Apply enables the setting; clearing it returns
to the existing Classic placement. Classic remains the shipping default.
Sketch, feature forms, BOM and the other Render panels remain separate work.
A specimen is not production migration or visual evidence.
Draggable tool windows and the existing native Assistant retain their legacy hosts.

## Initial real-content integration

The shared runtime owns placement and small presentation descriptors. Existing
`SolveApp` document state, Browser semantic rows, History playback and `AgentChat`
continue to own content. A frame-scoped callback invokes the existing renderers
inside the shared host's stable content identity and current focus-report service.
It retains no application reference beyond that paint call. Immutable host
geometry remains visible to content commands throughout the call.

Hidden bodies do not render. History playback and Assistant polling stay in their
existing application paths. Closing an Assistant panel does not stop its backend
or clear its draft. Popped Assistant windows keep the legacy path. Combined
Browser/History uses the existing content preference and suppresses the separate
History body. Render Inspector, Shots and appearances keep their existing hosts
and contribute their actual footprints to navigation and camera fitting.

Visible bodies report their natural scroll content and fixed control bands.
A sole production panel hugs that content until column capacity requires scrolling;
Assistant declares Fill to use the full available side height. A newly visible unmeasured
width starts at capacity until its first real paint supplies its natural extent.
No hidden body is rendered to obtain a measurement.

Bottom History declares a fixed, symmetrically padded icon row. It has no collapse
or height-resize control; commands enforce the same bounds. A new side column
inherits the current source column's width, or the registered standard width when
the source has no column. The standard width matches Classic Browser's default.
Existing columns and explicit resizing keep their width.
The full responsive/native acceptance matrix remains roadmap work. This initial
integration does not close the parent rollout issue.

## Ownership

The engine registry owns bounded kind metadata and capabilities. Layout owns
instance, column, section and tab identities, placement memory, widths, saved
weights, collapse, active tabs and native return destinations. Side columns are
ordered outer to inner; each contains a contiguous vertical stack of sections.
Each section contains ordered tabs. Inactive tabs still constrain composite size
limits. Registrations declare allowed zones, PinOuter, Content/Fill height policy,
text-heavy content, surface preference, header actions and independent
`closable`/`hot_closable` flags. No generic host branches on product panel names.
Only future History product registration allows Bottom; synthetic kinds can allow
all zones. Quick close is an affordance, not the close capability itself.

Layout is application/workspace preference data. It never enters CAD files,
document undo, evaluation or tessellation. `PanelRuntime` retains controllers by
instance and `ContentOwner`, independent of presentation. Services tick while
controllers are collapsed, inactive, unavailable, closed or on inactive documents.
Owner changes retain the appropriate draft/controller rather than redirecting an
old document's model reads to a new document. Explicit close/destroy/reopen and
reparent have side-effect-free lifecycle preflight before commit. Hidden bodies
are not rendered to measure them or animate collapse.

`PanelModels` reads immutable owner models. Explicit content operations use
`ContentCommandSink`. `PanelLocalServices` owns viewport-local keyboard, focus,
clipboard, picking, drops and overlays. Registered header actions must also have a
retained controller binding and pass availability/enabled checks. `panel.action`
returns a truthful committed `LayoutReply`; it cannot invoke an unregistered
binding. A fallible content action cannot be mixed with structural mutations in
one runtime transaction. Synthetic Apply/Finish/Pulse actions never run Session
commands. Real History playback and Assistant New Chat bindings belong to their
content migrations.

## Application preferences

The opt-in preference adapter connects the existing envelope and catalogue to
`SolveApp::prefs` and `load_prefs`, under the application-level `panel_layout` key.
Desktop and web store this same Rust value through their existing preference
writers. The adapter adds no design-file field, filesystem backend or browser
storage path. A runtime must explicitly enable this integration; Classic remains
the production default and ordinary specimens remain ephemeral.

`PanelLayoutPreferences` retains deferred input until a registered runtime is
available. The runtime's layout and injected catalogue remain authoritative once
enabled. The stored workspace identifies what to restore at startup; it is not a
second live workspace selection. Tool, document and ribbon changes do not select
a layout workspace.

The application record uses the existing bounded envelope/catalogue codecs.
Current records take precedence over one-time legacy placement import and
defaults. A corrupt or future current record is present data, so it must remain
recoverable instead of repeatedly triggering legacy import. Layout records keep
columns, saved proportions, tab/collapse state, closed placements and native
return memory. They exclude measured content height, usable-space geometry,
camera state and live controllers. Restored Content/Fill geometry is measured at
the current width through the same host admission path.

The existing Layout page exposes registered workspaces and named layouts only
when this integration is enabled. Its controls use `layout.workspace.select`,
`layout.apply`, `layout.save` and `layout.reset`. Built-ins remain immutable;
replacing a user layout is explicit. Reset affects the current workspace and
preserves the named layout library. Preferences Apply/Cancel and held writes
cover layout changes too. A refused restore keeps the Preferences dialog and its
snapshot available rather than discarding the restore obligation.

Settings profiles carry the selected layout workspace and the current arrangement
of every workspace, excluding the named library and machine-specific native window
geometry. Layout page defaults preserve arrangements and the library. Applying a profile uses
the same validated runtime path and reports a refused layout through the existing
skipped-field result. Product adapters supply their actual preset registrations;
synthetic fixture layouts do not stand in for migrated production content.

## Allocation and presentation

The pure engine allocator is authoritative for both commands and gestures. It
intersects every tab's constraints and each column's width constraints, uses
constrained saved weights for stacks, and preserves minimum dimensions when the
window is tiny. Right and Bottom placement share a bounded scroll extent, avoiding
overlapping docked hosts. The viewport domain stays fixed below the application
bar, including behind the ribbon and panels.

A sole presented side section defaults to Content height. The active tab supplies
bounded preferred outer height at the assigned width, including shared header,
body, content toolbar, footer, padding and spacing. Fill consumes available height
within the same composite limits. Multiple sections use their saved stack weights;
entering/leaving the sole-section state does not rewrite them. Collapse reserves
only the full-width header and leaves genuine uncovered canvas below short or
all-collapsed stacks.

Expanded dock sizing can declare `preferred_extent` on the stack axis. An
omitted preference remains None; saved legacy size does not imply a new override.
After reserving every presented minimum, allocation processes `extent_priority`
tiers highest-first, compressing requested surplus proportionally within a tier
when necessary. Older/common preferences use rank 0; nonexplicit members receive
the weighted remainder. SetSize authors a new highest tier. Ordinary weighted
pair resize/reset clears the pair's overrides/ranks; when weights cannot represent
a feasible actual split, the pair's exact requested extents receive one new tied
highest tier. Repeated overlapping edits therefore favor the latest authored pair
under later compression while preserving older preferences. Unrelated neighbours
keep their rectangles, even when their normalized weights need a common scale;
zero saved surplus remains valid. Sole side Content/Fill ignores latent extent
and rank during presentation and retains them for returning to a stack.

Bottom top-edge dragging uses the typed `SetBottomHeight` mutation. Registered
`panel.resize` with `thickness_only:true` uses the same mutation for Bottom, or
`SetColumnWidth` for a side dock; Floating/Window refuse. Omitted/false retains
full-size behavior. Both size components are validated before ignoring the other
axis. `SetBottomHeight` changes shared row thickness within registered common
bounds while preserving every Bottom width, preferred width, rank and weight. There is no additional public
string command. The side/Bottom separator exists only when side geometry presents;
a Bottom-only 64-point row fits exactly in 64 points of capacity.

`PanelController::measure(&self, owner, models, spec)` captures an immutable
`Arc<dyn HostContentMetrics>` snapshot. Its bounded `measure(request)` answers
assigned-width requests without a renderer or mutable services. `revision()` must
change whenever any answer could change, including responsive policy. The default
reports registered preferred outer height explicitly; it does not claim intrinsic
content measurement. Adapters supply real bounded metrics. `host_measure_requests`
uses the same width allocator as resolution; candidate widths are measured before
a preview is shown. Missing/stale data rejects. Replacing metric data cancels the
old candidate. The host retains the same snapshot identity while its revision is
unchanged, avoiding geometry feedback from an already stretched parent.

`HostLayout` separates intended section slots, scroll extent, actual clipped
paint/input/occlusion footprints and usable-space contour. `ResolvedPanel.body`
and `PanelContext.usable_body` are optional. Collapsed hosts have no content body.
Expanded Wide geometry at or below `theme::PANEL_INLINE_MAX_HEIGHT` (88 logical
points) has one total row: the real shared title/grip, registered actions, dedicated
inline content, menu, chevron and optional quick close, fully mirrored in RTL. It calls
`render_inline`, whose compatibility default delegates to Body. It does not call
Header/Body/Footer again or add a second title band. Controllers can override the
inline slot with a compact representation. This generic capability is demonstrated
by an editable inline specimen, not a substitute History timeline.

Shared chrome uses the existing adaptive `ui_kit::Container`, panel fill, title
fonts, six-dot grip and header separator. Each section has one header row. A single
panel uses a text-only title and grip; a tab group uses one leading section grip
and individual text-only tab headings in that same row, with no duplicate active
title. Grip, tabs and controls have disjoint hitboxes at the same y and height.
The grip drags the active tab; each tab retains its own select, drag and reorder
target. Header title typography is independent of icon metadata, which remains
available to other presentations. Collapsed groups retain their tabs across the
full-width header. Inline groups use their allocated title region for grip/tabs,
preserving the actual inline editor and one total row. Compact tab overflow keeps
the active heading visible whenever its minimum width fits beside the overflow
button; the bounded accessible menu retains every tab. Measurements and allocation
count only the shared header, content toolbar and footer, with no extra tab-row
height. Floating shell spacing comes from
`Container::gap()` and participates in allocation, previews and input geometry;
compact surfaces join without a sizing gap. In LTR, the
trailing controls are custom actions, menu, chevron, optional quick close; RTL
mirrors their geometry. Collapse retains these controls and one click on its
chevron restores the body. There is no rail, hover-peek or pin-open product UI.
Published `panel.peek`/`panel.dismiss` compatibility commands retain their wire
meaning without creating a new host affordance.

Overflow retains actual panels at minimum dimensions. Narrow scroll tracks and
unconsumed wheel input over actual panel paint change presentation offset. The
scroll canvas itself has no full-viewport response; uncovered gaps stay navigable.
Content retains its own scroll area and gets first use of wheel input.

## Exact candidates and admission

`prepare_host_layout` produces a non-Clone `PreparedHostLayout` containing private
engine state, proposed geometry, reference geometry and scope. Both app commands
and pointer actions use this preparation and allocator. Missing host/environment
refuses structural app commands. `NO_SPACE` refuses neutral or worsening overflow
except Close, Collapse (including expansion), Focus, or a strict improvement of one
extent without increasing the other.

Drags start after six logical points. Invisible card-edge regions select adjacent
columns or stacked sections; the centre joins tabs and measured tab halves choose
insertion. Side ordering accounts for both dock side and RTL. Empty space has no
implicit float fallback. Floating is an explicit menu/command action.

Once a Move crosses that threshold, the current moved instance's full presentation
title follows the cursor on the existing preview paint layer, including over
invalid or absent landing targets. Its original header or tab title uses half
its existing ink opacity; other text, grips, actions and panel content keep their
normal appearance. The paint-only badge uses theme tokens, mirrors its cursor
offset in RTL and clamps within the render domain and current clip. It keeps
the full display-ordered title on one line without automatic wrapping or
ellipsis. Physically oversized text clips, aligned left in LTR and right in RTL
so its reading start stays visible. It adds no widget, input, occlusion or
usable-space reservation. Mere presses,
clicks, resize and paired-resize gestures have no title feedback.

Window docking takes priority over panel targets: Bottom first, then sides,
measured tab insertion, relative edges and centre joins. Side bands are 16 logical
points wide (56 on empty sides); the Bottom band is 64 points high. Only the
innermost presented receiving column's centre-facing edge reaches 64 points into
clear canvas. Other outward column edges retain 18 points. Added reach stops at
painted panels or chrome and preserves the original edge-band tie-breaking.

The Below region extends through clear column space under a short panel, stopping
at the next visible panel, chrome or placement boundary. It adds no idle widget
or canvas obstacle. New stacked sections use the anchor's relative weight scale;
logical pixel heights do not become weights against normalized proportions.

Only the selected successful candidate's complete landing, changed neighbours
and tab marker are painted. Targets are data, never idle widgets. Preview does
not mutate committed layout, preferences, controllers, identities, document or
camera. Release consumes the unchanged shown candidate once. It does not rerun
an action reconstructed from the preview. Runtime performs lifecycle, action and
native preflight before committing that same prepared engine state. Resize uses
the same ownership protocol. Invalid/outside release, Escape, cancellation,
focus/visibility/modal loss, independent commands, changed scope, revision,
availability, metrics or environment discard capture without mutation.
Pointer leave hides the preview and discards its candidate while retaining a
valid held gesture. Re-entry resolves a fresh candidate. A button released
outside cannot resume a drag on re-entry; actual focus loss still cancels it.
The held Move's source text stays dim during pointer absence, but its badge has
no position and is hidden until a fresh inside Move. Cancellation or release
removes the badge and requests a repaint to restore the already-painted source
text. These visual observations derive from capture, never from `HostPreview`
or a second gesture state.

The runtime owns one private proposal containing B's prepared host layout and
one staged incoming controller cohort. B captures once from a pure owner/topology
projection before numeric pair normalization. Controller construction does not
use temporary placement dimensions. Every numeric prefix and final admission use
that same availability/provider snapshot; lifecycle preflight and commit reuse
those exact objects. An explicit Open/Move can change the stack before a pair,
and a newly available floating owner need not match outgoing membership.
Availability freshness compares the captured proposed cohort, including unavailable
or unbound dormant instances, with fresh read-only answers from the same objects.
Mounting, activation and registered actions share active-or-first-available
selection while preserving remembered activation. Existing controllers are retained.

`commands::execute_layout_action` stages the app's injected catalogue before
consuming the candidate once. Its retained-candidate sink returns the exact proposed
layout during projection and never replays a reducer on pointer release. Current
full-envelope preparation context is captured through `current_envelope_snapshot`
and `host_identity_reservations` before generators run. It includes dormant/native/
quarantine identities, effective recovery priority records and completeness.
`ActionSink::project_in_context` and `dispatch_in_context` forward this full context;
a retained proposal refuses any supplied-context mismatch without replay. Incoming
workspace preparation context is retained separately from outgoing paint freshness.
Bare host defaults remain incomplete; admission fixtures read a complete synthetic
envelope rather than claiming empty recovery metadata is authoritative. The engine
command staging boundary must compare the current catalogue's full source context
again after fallible storage preparation, even when service revision is unchanged;
a differing context refuses publication of the retained proposal.
Standalone registered HeaderAction is an owner command without placement/storage
dependency; mixed structural transactions refuse. Preset/workspace replacement
uses the existing command executor with its complete incoming envelope.

Changed metrics/reservations discard pending widget intents, cached proposals,
drag and the old painted frame. Widget admission requires fresh paint. Independent
commands and batches cancel capture before parsing, even if subsequently refused.
Explicit Close/Open intent remains attached when the exact candidate is consumed,
so lifecycle guards cannot disappear during conversion to prepared engine state.

## Application and surface boundaries

`SolveApp::enable_panel_runtime` installs an explicit retained runtime and
`AppPanelServices`; `enable_universal_panel_host` enables workspace composition.
Disabling the host unmounts the root cohort while keeping services alive. Whole
runtime teardown preflights every retained owner. Command, prepare, mount, paint
and finish boundaries refresh read-only Session context without an extra service
tick. Failure suspends stale presentation and remains inspectable in `last_error`.
The app exposes immutable runtime inspection; callers cannot replace its stored
native resource owner while native hosts exist.

The app paints the full-domain scene through its existing navigation path, paints
host chrome, and finalizes host input after later windows/modals. Initial presses
must belong to the actual enabled top-layer widget; exported geometry alone does
not authorize input through another window. Modal/hidden/focus loss cancels capture.
Foreign overlay raises and owner-scoped popup provenance use shared Container
services. Root and each native host mount complete separate cohorts, so unmounting
one does not deactivate another.

Panel operations never change camera, zoom, projection, focus or model screen
position. Explicit Fit uses actual usable space and may zoom. Centre/Look At
preserves zoom/distance and requested orientation, centring only focus if the
current model footprint cannot fit. Navbar/compass use their measured footprints.
Current persistent legacy forms, redefine controls, sketch palette and confirmation
paint remain obstacles after all producer passes. Sizing, closed, stale and Noop-only
hosts must not create obstacles. Transient menus shield input but do not become
framing targets. The existing semantic-content finalization and Browser editors
remain in the application shell.
The opt-in shell calls `browser::editors` once before sketch palette and
confirmation producers, retaining Move/Copy, Capture Position, Canvas, Point Cloud
and Sketch Attachment windows, drafts and command guards. Their existing shared
Window wrappers publish actual per-pass footprints; invisible sizing and closed
windows do not reserve usable space. Sketch Attachment has no second host call.

Suppressing the root Assistant clears only its cached panel, drop and dock-space
geometry, so an undrawn floating card cannot narrow legacy tool bounds. Desired
open state, conversation, backend and layout preferences remain owned by the
existing Assistant. A popped Assistant still uses `agent_chat::show` and its
existing native-window path, which performs the service poll itself; other host
composition passes call `agent_chat::poll` instead. This does not migrate or
reparent the window.

The default adapter truthfully advertises no native windows. Actual Window
placement needs the native sibling's monitor/DPI, focus and close/redock services.
The renderer resolves surface intent through the shared Container, raising its
fill alpha when canonical text needs more contrast. Captured `paint_alpha` and
`contrast` report the effective paint and any readable fallback separately from
declarative intent. Input and occlusion remain reserved regardless of alpha.
Production workspace rollout and real content adapters remain their named peers' work.

### Keyboard, accessibility and motion

The shared host exposes disclosure state, selected tabs and tab lists, action
toggle state, menu items and oriented splitters to accessibility. Disabled actions
retain their reason. Keyboard menu navigation returns focus to its opener on
Escape, and panel focus traversal reveals headers inside bounded scroll extents.

Nine contextual commands use the existing command registry and keymap for panel
focus, collapse, moving to the next permitted zone and sash resize/reset. They
resolve the actual owned host context, then use the existing typed admission and
dispatch path. They do not infer focus from a model or headless session. Automatic
shortcuts defer to focused text editors and foreign or modal UI. Explicit command
calls can resolve an owned editor's panel without borrowing its caret keys.

Controller and egui identities retain editor state. Focus restoration is scoped
to the controller owner, document and viewport; it must not steal deliberate
focus from another control. Hidden bodies are never rendered to restore focus.
Adapters report freshly rendered focusable responses through
`PanelLocalServices::register_focus_response`. The host accepts only eligible
responses from the current content slot, rather than relying on egui's
previous-pass widget lookup. Without a current response, an explicit panel focus
request uses the visible header and leaves retained editor memory intact.
Bookmarks retain a focused text editor's cursor range because egui collapses a
selection during an unfocused render. Eligible restoration replaces only that
range in the existing editor state; undo, IME and scroll state stay in egui.

The native host follows the effective locale's logical direction, including
Arabic and Hebrew, through the existing edge mapping. Saved layout coordinates
and physical floating/native window positions remain unchanged. Theme motion
uses a 120–160 ms cubic-out paint transition. Reduced motion or disabled theme
animation completes immediately; layout, semantics and focus never wait for it.

## Specimens and verification boundary

`ui.panel_fixture` is authenticated, off by default, and refuses to replace another
runtime. Presets: `docks`, `tabs`, `collapsed`, `floating`, `overflow` (24 instances),
`pinned`, `columns` (three independent Left columns), `stack_tabs`, `content_fill`
(short Content versus Fill), `inline` (one editable total row), and `surfaces`
(declared surface preferences with the existing synthetic bodies). The old `rails`
preset is replaced by `collapsed`; it is not an alias to the retired experience.
The optional `remember:true` enable parameter attaches the application preference
catalogue for isolated restart review. A restart must explicitly enable the same
specimen with `remember:true`; loading preferences alone never starts it. Saved
restoration waits for valid host dimensions and uses normal runtime admission.
Fixture workspace/preset registrations remain synthetic evidence, not shipping
production registrations.
The optional `reduced_motion` boolean updates the enabled specimen in place.
It changes no saved preference, controller identity, draft or layout and refuses
without a specimen owner. This review seam adds no operating-system motion service.

### Surface review specimen

The `surfaces` preset alone opts out of the host's compatibility opaque default.
It contains an inactive text-heavy editor tab beside an active log tab, explicit
Opaque and Translucent side members, and FollowZone Bottom and Floating members.
The same existing content, commands and translated captions remain in use.
Other presets retain their previous opaque presentation.

`ui.panel_fixture {"preset":"surfaces","translucency_percent":50}` requests
50% translucency for this explicitly enabled specimen. The optional value is a
finite number from 0 to 100; opacity is `1 - translucency_percent / 100`.
Zero requests an opaque surface and 100 requests a fully transparent surface
before readability protection. The override is not a saved preference.
`ui.panel_fixture {"translucency_percent":100}` updates the already enabled
specimen host in place, preserving retained controllers, drafts and layout.
It refuses when no specimen host is enabled or another provider owns the runtime.
The parameter is also accepted with other explicitly enabled specimen presets;
their compatibility opaque policy still applies. Omitting the
override uses current theme/preferences rather than a specimen colour palette.

Surface resolution uses every registered tab's metadata, including inactive tabs.
Explicit Opaque wins, then explicit Translucent. Otherwise FollowZone is opaque
for Bottom/Window or a multi-tab group containing a text-heavy member, and
translucent elsewhere. Effective opacity may increase to protect both primary
and muted text at 4.5:1 over possible viewport backgrounds. When the authored
token pair cannot meet that guarantee even at full opacity, the renderer uses a
known accessible opaque fallback pair and reports the failure, rather than
describing the authored pair as compliant. Existing blur infrastructure remains
independent of opacity and input capture.

Section inspection reports declarative `surface`, actual `paint_alpha`, and
optional `contrast`. The latter preserves `ContrastOutcome`, including the
fallback reason; fallback is not a successful proof for the original pair.
Actual clipped surface footprints continue to own input even when translucent.
Uncovered short-panel gaps remain navigable, while content scrolling and actual
controls shield the model. Temporary popup shielding is not a Fit/Centre obstacle.

This is shared surface policy and an opt-in review seam, not a production content
migration or a new appearance preset. Classic and shipping defaults remain.
Future approved docked adapters are Browser, History, Assistant and their allowed
RenderSceneBrowser counterpart; other content conversions require a separate
scope decision. Persistent legacy forms retain their existing hosts and continue
to contribute their actual footprints until their own migrations.
The original synthetic controllers expose draft, transaction, action, lifecycle,
render and service counters plus process-local equality tokens. They do not expose
fake production actions or pretend to perform CAD work.

The existing engine, panel-runtime, kit, shell and viewport test files and JSON
scenarios are the locations for authored regression cases. This source describes
contracts and review fixtures; it does not claim tests, native screenshots,
performance budgets or visual acceptance have been verified. Root owns runnable
milestone builds, screenshots, user visual approval and the final checks.
