# Control protocol

The [universal panel foundation](panel-foundation.md) supplies typed layout actions
and retained runtime guards. The application-layout commands below are registered
engine commands reached through `engine.execute`, `engine.batch`, `engine.script`
and MCP's existing `execute`/`batch` tools. They require an injected application
backend; enabling the internal panel runtime remains an explicit integration step.
Current product defaults and the production shell are unchanged.

### Application panel and layout commands

Every command has input and output schemas through ordinary command discovery.
Inputs are strict at every parameter-check level: unknown fields, including nested
placement fields, are errors. As with other engine calls, omitted/null parameters
mean an empty object. IDs use the foundation's bounded lowercase ID domains;
dimensions are finite positive outer logical units, at most 32768. Requests are
bounded to 16384 encoded bytes before cloning or deserialising. No command opens a
dialog, edits the document, changes dirty/undo/revision, evaluates or tessellates a
model, fits a body, follows a camera request or selects a ribbon/tool/document tab.
Layout revision is independent of document revision.

Canonical existing targets are typed: `{"target":{"type":"instance","id":"log"}}`.
`section`, `workspace` and `native_host` are the other foundation target types;
each command accepts only the domains listed below. A legacy `id` may replace
`target`, but both together are rejected. It first resolves an exact instance ID.
A kind ID resolves only when its registration is single-instance and exactly one
canonical instance exists, including retained closed state. Multi-instance kinds
are rejected even when only one instance currently exists. No first-match guessing
or implicit instance creation occurs. `panel.open` instead requires separate `kind`
and `instance` fields.

All mutations accept optional `expected_revision:u64`. A stale revision produces
the foundation `revision` rule and leaves the command's layout and catalogue
unchanged. Successful mutations return the committed `LayoutReply` (layout,
revision, interaction, effects, capabilities and availability), never an echoed
placement request. `layout` includes all retained instances with `requested_open`,
ordered sections and their active tabs. Missing availability entries mean available.
Query replies contain no new host effects.

| Command | Parameters and behaviour |
|---|---|
| `panel.list` | `{}` → `{state:LayoutReply,kinds:[PanelSpec]}` from the live sink and registry. |
| `panel.open` | `{kind,instance,destination?,expected_revision?}`. New instances require a destination; closed instances can reuse remembered placement. |
| `panel.close` | `{target:instance,expected_revision?}`. Explicit guarded close retains placement and can refuse cancellation. |
| `panel.move` | `{target:instance,destination,activate?:false,expected_revision?}`. Existing open instances only; explicit activation joins atomically. |
| `panel.resize` | `{target:column\|section\|instance,size:{width,height},thickness_only?:bool,expected_revision?}`. Omitted/false keeps full-size behavior. True changes shared Bottom row height or side column width while preserving stack extent preference/rank/weight; Floating/Window refuse. Both size components must remain finite, positive and bounded. All tab and column bounds intersect. |
| `panel.collapse` | `{target:section\|instance,collapsed:bool,expected_revision?}`. Keeps requested dimensions. |
| `panel.float` | Same as move, requiring a `new_section` with `floating` location. |
| `panel.popout` | Same as move, requiring a `new_section` with `window` location and return destination. Desktop/headless without native capability refuse; web explicitly lowers to `floating`. |
| `panel.focus` | `{target:instance,expected_revision?}`. Reopens retained closed state, activates, expands and requests focus. |
| `panel.focus.next`, `panel.focus.previous` | `{expected_revision?}`. Resolve the next or previous eligible panel from the live host's keyboard context. |
| `panel.toggle` | `{expected_revision?}`. Collapse or expand the focused panel's section. |
| `panel.move.next_zone` | `{expected_revision?}`. Move the focused panel to its next permitted zone. |
| `panel.sash.left`, `panel.sash.right`, `panel.sash.up`, `panel.sash.down`, `panel.sash.reset` | `{expected_revision?}`. Resize or reset the focused sash through the same constrained candidate path as pointer resizing. |
| `panel.activate` | `{target:instance,expected_revision?}`. Activates an open tab without expanding or requesting focus. |
| `panel.action` | `{target:instance,action:ControlId,expected_revision?}`. Validates registration, active/open state, availability and owning controller binding, then invokes the real bound action. Returns `LayoutReply`; missing/disabled bindings refuse. |
| `panel.peek` | `{target:instance,expected_revision?}`. Temporary presentation of a collapsed tab, without persistence changes to size/collapse. |
| `panel.dismiss` | `{expected_revision?}`. Dismisses transient peek. |
| `panel.resize_pair` | `{first:SectionId,second:SectionId,first_size:{width,height},second_size:{width,height},expected_revision?}`. Adjacent sections of one dock; conserves split-axis extent and cross-axis dimensions. |
| `panel.reset_pair` | `{first,second,expected_revision?}`. Resets adjacent sections to the registered default ratio within intersected bounds. |
| `panel.native_close` | `{target:{type:"native_host",id:HostId},expected_revision?}`. Native window close redocks its whole section; it does not explicitly close/cancel content. |
| `layout.save` | `{name,overwrite?:false,expected_revision?}` → `{name,overwritten,state}`. Saves the complete current envelope in the injected catalogue. |
| `layout.apply` | `{name,expected_revision?}`. Applies a named preset to the **current** workspace, rebinding its known layout workspace ID. |
| `layout.reset` | `{expected_revision?}`. Applies the selected workspace's registered immutable default. User presets and other workspace histories remain. Missing defaults return a capability error. |
| `layout.get` | `{}` → `{state,workspaces:[{id,title_key,default_preset,has_remembered_layout}]}`. Selection is only `state.layout.workspace`. |
| `layout.workspace.select` | `{workspace:WorkspaceId,expected_revision?}` → `{previous_workspace,source:"remembered"\|"default_preset"\|"unchanged",state}`. Registered descriptors only; no implicit creation. |

The nine contextual commands above return `LayoutReply`. Their strict input accepts
only the optional revision, checked before resolving the host context. A headless
sink without a live context refuses rather than guessing a focused panel or sash.
Command metadata reports the live availability and a localized disabled reason.
An atomic batch may contain one contextual command alone; a mixed atomic batch
refuses before applying any action.

Shortcuts use the existing rebindable keymap. The four sash arrow bindings retain
their existing defaults; the other contextual commands start unbound. Automatic
host shortcuts defer to a focused text editor. Explicit contextual command calls
may target its actually owned panel, but cannot infer a sash from an editor.
Modal, document, viewport and stale-presentation guards still apply.

Destinations are distinct foundation operations, not aliases:

```json
{"type":"new_section","section":"logs","index":0,"location":{"type":"dock","zone":"left"},"size":{"width":280,"height":240}}
{"type":"join_tab","section":"logs","index":1}
{"type":"relative","section":"logs","placement":"after_column","new_section":"new-logs","new_column":"extra-column","size":{"width":280,"height":240}}
```

New-section indices address the global section order after detachment; join indices
address the destination's tab list after detachment. Joining keeps the destination's
activation, collapse and dimensions. Locations are `dock` (left/right/bottom),
`floating` (`origin:{x,y}`), or `window` (`host`, `origin`, `return_to`). Native
`return_to` contains `{zone,index,size,collapsed,join?:{section,index}}`; it never
recursively contains another native host. Pins and placement/size intersections
retain the foundation's structured refusal rules.

The host's explicit title/tab drag is a separate UX intent: a tab landing sends
Move and Activate in the same guarded transaction so the dragged tab is visible.
Generic background joins keep the activation semantics above.

Side sections reference stable columns, ordered outer to inner, and retain saved
stack weights. Relative Before/After column placement is logical; Above/Below
stacks within a target column. Pointer code maps physical edges through dock side
and RTL. App commands and gestures share host allocation and typed `no_space`
admission. Missing host/environment refuses structural app commands. Overflowing
neutral/worsening proposals reject except Close, Collapse (including expansion),
Focus, Activate, or strict extent improvement without growth on the other axis. Independent
commands and refused batches cancel a pending gesture before parsing/preflight.
Standalone `panel.action` validates registration, the available fallback selection,
owner availability, enabled binding and controller preflight. It can execute
without a host environment, during overflow, or when layout storage refuses writes;
it neither allocates placement nor stages layout persistence. It cannot be combined with structural
mutations in one runtime transaction. Published peek/dismiss requests remain
compatibility commands; the amended host exposes no rail/peek/pin-open UI.

Sole side Content height uses bounded immutable controller metrics at assigned
width, including chrome/body/footer/padding. Fill is declarative registration or
responsive snapshot data. An incoming owner cohort captures its own availability and immutable metric
providers before candidate allocation. Projection and commit retain those same
controllers/providers. Changing metrics cancels previews, pending widget intents,
cached proposals and the old frame, requiring fresh paint for widget admission. No hidden render pass is used.

Preset names trim surrounding whitespace, require 1..64 Unicode scalar values and
reject controls anywhere in the supplied name. Raw input is capped at 1024 UTF-8
bytes before trimming; the trimmed name is at most 256 bytes. Names are case-sensitive.
There are at most 64 user presets. Collisions reject unless `overwrite:true` is
explicit; built-ins are immutable. No production kinds, preset contents or workspace
descriptors are invented. Headless starts with an empty registry/catalogue and an
internal current-layout identity, not a registered product workspace. Fixtures and
peer integrations can inject synthetic or real registrations. Missing capabilities
return errors rather than guessed defaults or disk/native success.

Workspace selection stages the outgoing full envelope, destination and selected
workspace together, then uses one guarded `Replace` on the retained runtime.
Same-workspace selection is a true no-op, preserving revision, focus, peek and
effects even if storage staging is unavailable. Applying/resetting a preset never
selects a workspace. Replacements hide omitted content without implicit Close or
Cancel; retained editors, conversations and hidden services survive A→B→A.

Catalogue preparation uses encoded full `LayoutEnvelope` records and explicit
aggregate caps: 64 registered workspaces, 64 built-ins, 64 remembered envelopes,
64 user presets, 16384 metadata bytes, and 4194304 aggregate envelope/recovery bytes.
Each envelope/rejected recovery buffer remains at most 262144 bytes and obeys the
foundation node/depth/collection caps. Quarantine, retained data, shared section
snapshots including projected sections, complete section order and bounded rejected
recovery intent survive save/apply/reset. Conflicting opaque identities, snapshots,
retained data or recovery intent reject atomically. Restoration uses the foundation
decoder and reappearance logic. Storage preparation finishes before live dispatch;
catalogue publication is infallible after successful native/controller guards.
Revision changes during staging reject the staged catalogue. No fallible storage
write after live commit is treated as an atomic success. The application preference adapter stores the bounded workspace catalogue under
`panel_layout` in `SolveApp::prefs`. Native and web use the same representation.
A saved shared layout takes precedence over older placement settings, which are
read only for migration when no shared record exists.

Atomic layout batches contain 1..64 commands, using the same parser/executor and
one final guarded Apply or Replace plus one catalogue publication. Queries and
saves may accompany mutations; save/apply/reset/workspace sequences can stage
together. Replies describe the **final committed** state, not hypothetical
intermediate live states. Every `expected_revision` in an atomic batch refers to
the initial live revision. Explicit Close/Open conflicts remain `lifecycle` errors.
Atomic batches cannot combine a snapshot replacement with explicit panel mutations,
or mix document commands with application layout; they return `capability` before
publishing anything. This preserves the foundation's explicit lifecycle intents.
Use `rollback:false` for sequential per-command transactions across these domains;
failures report `rolled_back:false`, `undone:0` and completed results. Scripts use
the same injected backend sequentially and stop at the first failure. A Session-only
executor (including ordinary CLI design scripts without an injected backend)
returns a structured missing-capability error for layout commands.

The GUI route uses the enabled runtime's actual content-service provider and an
injected `NativeHostAdapter`, never a reconstructed LayoutService. Headless MCP
owns one persistent application backend distinct from Session; Remote forwards to
the app and keeps no mirrored layout. Auto mode does not discard headless layout
work when an app appears. Foundation errors retain `{rule,detail}` with bounded
details: control replies add `panel_error` alongside the existing error string;
MCP tool errors add `structuredContent.panel_error`. The error string also encodes
that object, preserving identity through older string-only backend boundaries.
Rust integrations implement `ActionSink::project(LayoutAction)` as a pure
projection on the authoritative service. Its default returns `capability` so an
older inspect/dispatch-only sink cannot accidentally publish unstaged commands.
`LayoutService` and `RuntimeActionSink` provide it using the foundation's own
preparation path. `with_live_layout` refreshes the envelope's shared section memory
and projections without discarding opaque fields, then encodes against the same
foundation limits before publication.

Storage consumers can enumerate `CatalogueState::user_presets()` read-only and
round-trip a complete validated catalogue with `export()` and
`CatalogueState::hydrate(bytes,registry)` or `InMemoryCatalogue::from_snapshot`.
Hydration is independent of live commands; restored users remain explicitly
overwritable users, while built-ins remain immutable. Workspace descriptors,
remembered full envelopes, and rejected recovery bytes are included. The versioned
snapshot uses `SCPCAT01`, then length-prefixed little-endian u32 counts/UTF-8 names
and raw byte records, in built-ins/users/workspaces/remembered order. Each envelope
record contains length-prefixed envelope JSON, a recovery-presence byte (0/1), and,
when present, length-prefixed exact recovery bytes. Workspace descriptors contain
ID/title-key/default-name strings. The snapshot has an additional 8192-byte framing
allowance above the existing aggregate payload/metadata caps (4218880 bytes total).
Every count/string/record is bounded before allocation, and hydration rejects
duplicate names across users/built-ins, duplicate workspace keys, corrupt framing,
trailing data, invalid envelope identities and all existing aggregate/node/depth
limits before a candidate can be published. This seam performs no disk storage.

With application panel preferences enabled, the existing Layout preferences page
uses these commands for workspace selection, named layout save/apply and reset.
Reset changes only the current workspace and keeps the named layout library.
Preferences Cancel restores the layout and catalogue through runtime admission;
if restoration is refused, the dialog and its saved baseline stay open. Periodic
preference writes retain that baseline until Apply. Settings profiles carry the
selected layout workspace and each workspace's current arrangement, excluding
the named library and machine-specific native window geometry. Page defaults
preserve all arrangements and the library. None of these operations changes the
design or camera. Names compare case-insensitively; `layout.save {name,replace:true}`
explicitly replaces a user layout. The legacy `overwrite` flag remains accepted.

Reading a retained envelope does not consume quarantine merely because the registry
has grown. `decode_retained_record` validates an already-retained record exactly;
`decode_retained` additionally checks its workspace. Neither reader generates
section order/projections or performs quarantine migration/reappearance. Hydration
preserves the original validated record bytes and rejected binary data, including
records at the per-envelope or aggregate cap when optional panel kinds disappear
or return. Missing registrations do not invalidate unrelated users, built-ins or
workspace memories. Retained validation checks structural ownership, placement
bounds and all applicable known-kind constraints, without invented descriptors.
An exact retained layout can still contain absent kinds and is not live admission.
`StoredEnvelope::project_envelope` (or foundation `project_retained_record`) stages
missing-kind quarantine before a guarded live use, preserving full section order,
activation and opaque records. This staging does not consume existing quarantine;
ordinary save/live overlays therefore keep reappearance retryable after registry
growth. Foundation `decode` also stages migration and attempts restoration, with
its existing fallback/rejected-input result contract.
Materialization may legitimately exceed a cap and reject its staged candidate;
reading/hydrating the accepted original record does not materialize it.
Apply/reset/workspace replacement
explicitly stage restoration, and consume it only with a successful authoritative
Replace and catalogue publication. A failed action leaves the restore obligation
retryable. `panel.popout` requires the panel's `features.popout` on both web and
native policies before any enabled web fallback; `panel.float` remains separate.

Application command, atomic-batch and GUI action boundaries refresh the current
read-only provider/document environment and availability. A document or contextual
model change earlier in a sequential batch/script is visible to the next panel
step without a tick. Queries and failed actions cannot borrow the previous
document's context. No content tick, document edit, geometry work or camera tail is
added to this boundary. All application GUI and command routes use one stored
native adapter. Command-adapter installation is once-only and precedes native
placements. Disabling the runtime preflights retained controllers and native
release through that same adapter; refusal leaves it installed.
Unsupported atomic intent mixtures are rejected by pure structural preflight
before any context/controller publication as well as before layout/native effects.

At MCP tool level, an atomic batch containing any panel/layout command must reach
the authoritative batch transaction. If the endpoint is unavailable or the encoded
request exceeds its transport margin, a structured zero-effect capability/size
refusal replaces the old sequential fallback. No document checkpoint or individual
layout command is sent. Explicit `rollback:false` keeps the sequential fallback
and reports completed layout changes honestly. Document-only batch fallback keeps
its existing checkpoint/rollback behaviour. Explicit null parameters normalize to
an empty object in single, atomic, sequential and script routes for GUI/headless.

Compatibility: `view.layout` still means Floating/Compact **surface styling**.
History-in-Browser, History Bodies/Origin and Render search/options remain content
preferences. `view.history_layout` and `view.render_dock` use the shared host for
placement. Integrations with explicit identities can issue typed `panel.move`,
`panel.resize` and `panel.collapse` requests directly. No command implicitly
selects a layout workspace on a document, ribbon, tool or context change.

At the Rust app boundary, `AppPanelRuntime::new(runtime, services)` is fallible and
rejects incoming native placements. `SolveApp::dispatch_panel_layout(action)` uses
the stored native adapter. It accepts no
borrowed adapter. Install a future native adapter with `set_panel_native_adapter`
before native creation/restore, or install it with the catalogue through
`AppPanelRuntime::set_command_adapters`. Once command adapters are installed,
neither injection API can replace them. App command/action/render/mount boundaries reconcile the
actual current Session without another service tick. Runtime inspection remains
read-only. The shared host adds no production command registration beyond the
application-layout commands above.

For native host evidence, `ui.panel_fixture {enabled?, preset?, remember?, translucency_percent?, reduced_motion?}` installs bounded
synthetic content in the real `SolveApp` workspace host. The normal connection
authentication applies. It is off by default and ephemeral unless explicitly
enabled with `remember:true`. It adds no design change, production content
registration or native panel window.

| Request | Effect |
| --- | --- |
| `{}` | Read current host/probe/Session observations without ticking services or dispatching actions. |
| `{"enabled":true}` | Enable the `docks` specimen if no runtime exists. |
| `{"preset":"tabs"}` | Enable the named specimen if no runtime exists. |
| `{"preset":"tabs","remember":true}` | Enable the specimen and its application preference catalogue. Restore a saved record after valid host dimensions are available. |
| `{"preset":"surfaces","translucency_percent":50}` | Enable declared surface-policy specimens with a runtime-only translucency request (finite 0–100 percent). Readability may raise effective opacity. |
| `{"translucency_percent":100}` | Change the enabled specimen's request without replacing controllers, drafts or layout. Refuse if no specimen owns the host. |
| `{"reduced_motion":true}` | Disable specimen host animation without replacing controllers or changing saved preferences or layout. False restores theme-controlled motion. Refuse if no specimen owns the host. |
| `{"enabled":false}` | Preflight and tear down only the specimen runtime; refusal leaves it intact. |

Presets are `docks` (Left/Right/Bottom, with two Left sections), `tabs` (two groups
of two), `collapsed` (four full-width collapsed headers), `floating` (four in-app floating
sections), `overflow` (24 instances), and `pinned` (an outer pinned Right section
plus ordinary docks), `columns` (three independent Left columns), `stack_tabs`, `content_fill` (sole Content versus Fill), `inline` (one editable total row), and `surfaces` (mixed tab and explicit surface policies). Only `surfaces` opts out of compatibility opaque presentation. Requested opacity is `1 - translucency_percent / 100`; the override never changes saved preferences. The retired `rails` preset is rejected. Original synthetic content retains editable drafts and
controller state across hiding and moves. Services keep ticking while content is
hidden. Apply/Finish/Cancel are synthetic actions; the Session is never passed to
their command sink. The editor's cancellation guard checkbox can refuse explicit
Close and whole-runtime teardown while its synthetic transaction is live.

Enabling always rejects an existing runtime, including an already enabled specimen.
To change preset, disable successfully first, then enable the next preset. Enable
stages the runtime, first tick and host before publishing any of them; teardown
preflights every retained controller. Wrong types, unknown parameters/presets and
`enabled:false` combined with `preset` fail before mutation.

`remember` is an opt-in enable parameter, not a switch on an existing runtime.
To review restart persistence, save through the normal application preference
writer, restart with the same isolated settings, then explicitly enable the same
specimen with `remember:true`. Loading settings alone never starts specimen
content. A failed restore retains the saved record and reports a recovery notice.
Any Modelling, Sketch, Render, Review or Classic arrangements registered by this
fixture are synthetic demonstrations, not production workspace presets.

Replies include `enabled`, `runtime` when the specimen runtime exists, `host`
(null before its first rendered frame), and `session`. `runtime` contains the
committed `LayoutReply`, retained controller count and synthetic probe snapshots
(typed instance/owner, process-local controller identity tokens, drafts,
service/render/lifecycle/action counts, transaction and guard state).
`host` reports the last actual host frame: full semantic control IDs/typed target
descriptions, accessible names, enabled flags and logical rectangles; section/body
rectangles, collapse/inline flags, declared surface policy and captured frame
`paint_alpha` (null without a renderer snapshot), resolved unpresented content,
bounded scroll extent/offset, paint/input/occlusion
rectangles, render domain, placement capacity, usable-space geometry, viewport,
exact prepared preview, menu bounds, observed host events and committed frame
reply. The top-level `effective_usable` application contour additionally includes
current persistent windows and independent navigation controls; the host's own
usable geometry describes host footprints. Sections and resolved geometry retain intended layout;
paint/input/occlusion arrays and published controls describe actual paint, excluding
invisible Area sizing passes. `reply`, `committed` and `prepared_committed` events describe authoritative post-dispatch state.
Region events include the menu and owned overlays in effective paint/input/
occlusion rectangles. Those overlays do not shrink the render domain or become
automatic camera targets. The viewport fills everything below the application
bar, including behind the ribbon and panels; opacity never makes controls
click-through. Ordinary layout commands and gestures preserve camera pose, zoom,
projection and framing offset. Explicit Fit/Centre use safe usable space and the
model extent. The effective contour combines host footprints with current
persistent surfaces outside the host, such as the active sketch palette; hidden
surfaces and transient menus do not create fitting reservations. Fit may change
zoom. Existing Look At/`view.normal_to` centring
preserves zoom/distance, requested orientation and selected focus: it places the
whole footprint at the current zoom when possible, otherwise only the focus at a
stable usable centre. It never silently zooms out, and generic camera apply/replay
does not acquire a centring offset.

Ordinary title/tab drags accept explicit dock, split and tab targets. A landing
preview describes the exact candidate, affected neighbours and insertion index.
Empty space does not create a floating panel; use `panel.float` or the explicit
menu action. Invalid/no target, cancellation and stale captured geometry
commit nothing. A valid unchanged candidate commits once, consuming its owned prepared state without redispatch. Only its landing, changed neighbours and tab marker are painted. There are no idle target widgets. Full-width collapsed headers retain declared custom actions, menu, chevron and optional quick close; one click expands. Tiny windows retain panels at minima in a bounded scroll extent; narrow scroll tracks do not shield uncovered canvas.

Public Rust `SolveApp::universal_panel_frame()` also exposes
`HostFrame::next_layout`, the resolved state pending repaint, and
`owned_overlays`. The fixture JSON does not serialize those two as separate
fields. Never infer painted controls from pending layout geometry. Compare the
painted frame's revision with committed replies and runtime revisions. An empty
fixture query neither reconciles context nor ticks: the current Session may have
changed since the retained runtime/frame was reconciled. Compare document scope
as well as revision. Target descriptions are diagnostics, never command addresses
or strings to parse.

`session` observes actual document ID/UID/revision/dirty/undo/redo, document/model/
cache Arc identities, recomputed feature counts, existing cached mesh identities
(first 128 bodies, with total count), and camera. Pointer strings are opaque
process-local equality tokens. Inspection uses only already cached meshes;
`evaluation_count` counts live-model evaluations after Session initialization.
It excludes construction, other Sessions and trial evaluations of cloned models;
it does not count all geometry work or tessellation. These values permit
before/after comparisons, not a claim that any invariant or visual result has
already been verified.

In the real `cad_ui`, host drawing defers input finalization until after all
foreground windows/modal layers. Finalization checks the current Session and the
actual enabled/top-layer egui target before accepting an initial press, then uses
the stored native adapter for dispatch. Exported rectangles alone do not authorize
clicking through another window. The root host and each native-host presentation
use complete separate mount sets; mounting or unmounting one does not deactivate
the others. These are native-adapter seams, not an implementation of OS panel
windows. The specimen/default adapter advertises no native windows, and the
current opaque renderer rejects unsupported translucent policy requests.

Scenarios accept `{"panel_fixture":{"enabled":true,"preset":"docks"}}`,
`{"panel_fixture":{}}` and `{"panel_fixture":{"enabled":false}}`, using this
same control path and normal `fail:true` handling. Use the returned control
rectangles with existing pointer/key, resize and screenshot requests. Capture real
native `ui.screenshot` output and inspect it for chrome evidence; a model-only
`ui.render` or headless scenario does not show a native panel host.

For UI-kit review, `ui.status_fixture {"preset":"mode"}` shows bounded status
specimens in the real viewport. Presets are `off`, `mode`, `result`, `attention`,
`row`, `kinds`, `many` and `key`; the reply carries the preset and item count. This
scenario fixture does not change engine state or open a dialog. `off` clears it.
The published `viewport_status:<id>:item`, `:setting`, `:key` and `:close` handles
support the ordinary `ui.at`, `ui.move`, `ui.click` and keyboard controls.
With `viewport_status_summary` enabled, `kinds` (three) and `many` (four) exercise
the summary component, using explicit specimen newest metadata. The
`viewport_status:summary` and `menu:viewport_status_clear_all` handles open its
native menu and activate its footer. These specimens are not persistent engine
results; actual production sources currently supply at most three Mode items.

Toasts the user may close have a close (×) at their top right instead of a Close button, published
as `toast:<name>:close`: `toast:status:close`, `toast:space_mouse_offer:close` and
`toast:stand_up:close` (#7971). Those that go by themselves draw a thin ring around it that
shrinks over their time; the ring is full again while the pointer is on a toast that waits for it.

Live details publish `details:field:<id>`, `details:show_all` and `details:action:copy`
handles. `details_findings` and `details_readout` are explicitly labelled component-only
`ui.status_fixture` presets. Real card settings execute the normal analysis/Section commands;
agents can read the same structured settings and retained Continuity records with the pure
`view.status_details` engine command. Its `current` flag guards geometry targets, and its
per-record limits prevent mixed measurements from being mistaken for one global tolerance.

The control channel is **off by default**: Preferences > Assistant > Agents through MCP > "Let AI
agents connect" turns it on, and `--control PORT` opens it for one run. Once on, SolidCraft listens on
`127.0.0.1` only, for JSON lines, so agents can drive the app without a launch flag. Every connection
still needs this launch's token (below); nothing else on the machine, and nothing off it, can send a
command.

Internal rendering harness: `ui.blur {radius_px?, surface_alpha?, fixture?}` queries or sets
session-only panel blur. Radius is finite physical pixels in 0..60; the optional
capture tint is 0..1 (`null` restores ordinary fills). This is not a saved
preference. Theme-authored blur is edited with `ui.prefs {theme_options:{theme_id:{blur:18}}}`;
the theme must declare the option. Finite fractional values remain valid, although
the UI slider edits in steps of one physical pixel. The `ui.prefs` option definition's
`available`/translated `reason` describes current app/device/OS availability; its
value remains the saved/default intent while unavailable.

The `ui.blur` reply contains requested radius, renderer capability, app-aware
`available`/`reason`, `guard` (tier cap, streak, notice, session opt-out), and cumulative
actual effect counters, including live resources and encoded passes. Unsupported
software/web/source contexts stay on ordinary painting. Use `ui.screenshot` for
panel evidence; `ui.render` remains a model-only capture. Activation, full egui GPU
fixture and manager-run evidence commands are in [blur-behind.md](blur-behind.md).
Supplying `radius_px` selects the legacy session-only rendering harness override;
`fixture:"clear"` removes that override and any injected availability reason,
returning to theme intent. It does not revoke a quality-guard Undo opt-out.

Explicit evidence-only fixtures are `gpu-software`, `reduce-transparency`, and
`web-unsupported` (force an unavailable reason), plus `guard-60` (inject sixty
eligible 23 ms samples into the transient guard at the current positive radius).
The reply labels the active injection as `evidence_fixture`; `null` means none.
These are not end-user settings, real adapter/OS readings or performance measurements.
There is no fixture that makes an unsupported renderer available. They do not
change saved options, host OS settings or document history. All supplied fields
are validated before mutation. Capture fixture screenshots with that label in the
evidence description, and clear the fixture before checking real capabilities.

| Launch | What listens |
|---|---|
| `solidcraft --control PORT`, or `SOLIDCRAFT_CONTROL_PORT=PORT` | exactly `127.0.0.1:PORT` (the flag wins over the variable), whatever the setting |
| neither, and Preferences > Assistant > Agents through MCP > "Let AI agents connect" on (the default) | the setting's port: the default **27878**, or the next free one up to 27893 when it is taken (a second window, another program); any other port exactly, or nothing when it is taken (the panel says so) |
| neither, and the setting off | nothing |

Turning the setting on or off, or changing its port, takes effect at once: the channel closes (with its connections, token file and discovery file) and opens again where the setting asks. A launch with `--control` or the variable keeps its port for that run, and the setting waits for the next launch. Old preference files, which don't have it, load with it on. A
launch with `--control` or the variable behaves as it always did (the start page stays hidden,
the window doesn't ask for the foreground), because a scripted launch expects that; the default
channel changes neither for a normal launch.

### Finding the port

Each app that opens its channel writes the port to a discovery file beside the token files,
`control/current.port` (same folders and permissions as below), and removes it at exit if no later
app has replaced it: it names another port, or another app now holds that port's token file (two
Flatpak windows, each with its own loopback, can listen on the same port number). It holds only
the port, never the token. `solidcraft-cli mcp` with no
arguments reads it, checks that something listens there and bridges to that app; with no app
running, or with `--headless`, it runs a session of its own. `--connect HOST:PORT` and the
other arguments still decide first. With several windows open, the most recently started one is
the one found; use `--connect` for another.

Requests are one line `{"id": …, "method": "…", "params": {…}}`; replies are described below. Each request is one line `{"id": …, "method": "…", "params": {…}}`; each reply is one
line `{"id": …, "ok": true, "result": …}` or `{"id": …, "ok": false, "error": "…"}`. Requests are
handled on the UI thread between frames; pointer and keyboard input is injected as real egui
events, one pointer event per frame. A connection that sent `control.subscribe` is also sent
event lines `{"event": "…", "params": {…}}`, which have no `id`
([Following the design](#following-the-design)).

### Token

Every connection must authenticate. At start the app makes a random 256-bit token for this
launch and writes it, as one line of hex, to a file only the user can read:

| System | Token file |
|---|---|
| Linux | `$XDG_DATA_HOME/solidcraft/control/PORT.token` (`~/.local/share/solidcraft/control/PORT.token`) |
| macOS | `~/Library/Application Support/SolidCraft/control/PORT.token` |
| Windows | `%LOCALAPPDATA%\SolidCraft\control\PORT.token` |

On Unix the folder is `0700` and the file `0600`; on Windows the file's inherited permissions are
replaced by a single entry for the user. `SOLIDCRAFT_CONTROL_DIR` moves the folder. If Windows
doesn't finish restricting the file within five seconds, the default folder (private to the user)
keeps it, but a custom folder gets no token file, and clients then need
`SOLIDCRAFT_CONTROL_TOKEN`. The file is
removed when the app exits, unless another launch has written its own token there since, and the
next launch writes a new token. The token file's path is
printed to stderr at start-up. Beside it, `<port>.pid` holds the app's process id (and, on Linux,
its start time).

An app that is force-quit can't remove its files, so a later client may send that launch's token.
When the app refuses it, `solidcraft-cli mcp` names the token file it read and when it was
written, and says the file is stale when the process in `<port>.pid` has exited. Delete the file
or restart SolidCraft. On Windows, an app or client started from inside an MSIX-packaged app (and
the tools it launches) has its AppData redirected: it writes and reads
`%LOCALAPPDATA%\Packages\<package>\LocalCache\Local\SolidCraft\control\` before the folder above.

The first request on a connection must carry the token as a top-level `"token"` member
(`{"id": 1, "method": "ui.inspect", "params": {}, "token": "…"}`); later requests on the same
connection may leave it out. A first line without the right token is answered with
`{"ok": false, "error": "unauthorized: …"}` and the connection is closed, without running
anything. Tokens are compared in constant time. A connection that hasn't sent that first request
within five seconds is closed without a reply, and blank lines don't extend the time. So connect
when you have something to send; once authenticated, a connection may stay idle.

`solidcraft-cli mcp --connect 127.0.0.1:PORT` reads the token file for that port by itself
(`--token T` or `SOLIDCRAFT_CONTROL_TOKEN` give one explicitly). For a test harness, or a
sandboxed package whose data folder the client can't see, start the app with
`SOLIDCRAFT_CONTROL_TOKEN` set to at least 32 hex characters (for example
`openssl rand -hex 32`) and the app uses that token instead of making one; the file is still
written. A one-line client in Python:

```python
token = (Path.home() / ".local/share/solidcraft/control/7878.token").read_text().strip()
sock.sendall((json.dumps({"id": 1, "method": "ui.inspect", "params": {}, "token": token}) + "\n").encode())
```

### Limits

The channel is untrusted input. A request line may be at most 16 MiB; a longer one is answered
with `{"ok": false, "error": "request too large"}` and the connection is closed, as are
connections that send eight lines that are not JSON. At most 16 connections are served at once
(further ones get `{"ok": false, "error": "too many connections"}`), at most 64 queued requests
run per frame so a flood cannot starve drawing, and a request that the UI thread has not
answered within 60 seconds (3600 seconds for `engine.batch`) is replied to with
`{"ok": false, "running": true, "error": "timeout: … is still running …"}`. The same 16 MiB line
limit applies to the MCP server.

A timed-out request has not failed: it is still running in the app, and a change still applies
when it finishes (a long fillet, say), so don't simply send it again. The timeout's text says
what it still does: a request that only reads (`ui.inspect`, `document.inspect`, an inspection
command) "only reads: nothing changes"; one that only writes a file (`file.export`,
`file.save`, `file.save_as`, `file.save_mesh`, another export given a `path`, `ui.render`, or
`ui.screenshot` with a `path`) "will still write the file when it finishes" and leaves the design
as it is; any other request "will still apply when it finishes". The connection keeps its reply (#1618). Ask for it
with `control.pending`: `{"pending": <id>, "method": …, "done": false}` while it runs, then
`{"pending": <id>, "method": …, "done": true, "reply": {…}}` (its own reply, once), and
`{"pending": null}` when nothing is outstanding. Until it lands, any other request on that
connection is refused at once with `{"ok": false, "busy": true, "error": "busy: …"}` and not
run, so nothing is queued to apply unseen behind it. `control.pending` belongs to the
connection: a new connection has nothing pending. The bridged MCP server does this itself
([mcp.md](mcp.md#long-running-requests)).

`control.pending` answers only for its own connection. `control.running` answers for the whole
app, the same on every connection, at once (it never waits behind a running request, and a
connection may ask it while it is `busy`): every request the app has been sent and has not
answered yet, oldest first, from any connection, including a timed-out one whose connection has
since closed:

```text
{"id":1,"method":"control.running","token":"…"}
→ {"id":1,"ok":true,"result":{"running":[
    {"connection":3,"id":17,"method":"engine.execute","command":"file.export","running_s":312.4,"queued":false},
    {"connection":5,"id":2,"method":"engine.batch","steps":12,"running_s":4.1,"queued":true,"self":true}]}}
```

`running_s` counts seconds since the request arrived; `queued` is true while it still waits its
turn behind another. `command` is the command id of `engine.execute` (a known one only) and
`steps` the length of `engine.batch` or `engine.script`; parameters are never shown, so file
paths and expressions stay with the connection that sent them. `self` marks the asking
connection's own request. `{"running": []}` when the app is idle.

`control.capabilities` says what this app's control channel does beyond the methods every
version has, so a client can find out before it relies on it: `{"param_check": ["off", "warn",
"strict"]}`, the levels a request's own `param_check` takes (below). It is answered at once, like
`control.running`, also on a `busy` connection. An app older than it answers ``unknown method
`control.capabilities` ``; a feature added later is listed here too.

Any request's `params` may carry `allow_paths: ["/abs/folder", …]`: every file that request
reads, writes or deletes (command paths such as `file.export`'s `path` or `doc.recovery_*`'s
`dir`, `ui.render`, `ui.screenshot`) must then lie inside one of those folders after `..` and
symlinks are resolved, or the request fails with `… outside the folders this agent may use …`.
The app adds its own recovery folder and, when it lies safely inside the home folder and not in
a hidden folder, the open design's folder (whose hidden files stay out of reach). With
`allow_paths`, `design_folders: ["/abs/folder", …]` (at most 16) names folders the agent opened
or saved a design in; each counts on the open design folder's terms. The bridged
MCP server sends its folders this way on every request ([mcp.md](mcp.md)). Without
`allow_paths` file paths are not limited: a client holding the token can do what the user can.

`engine.execute`, `engine.batch` and `engine.script` may carry `param_check: "off" | "warn" |
"strict"` in their `params`: the commands of that request are checked against their typed
contracts at that level, whatever the app's own (which is `off`), as a headless session's
`--param-check` checks them (#9184). `warn` runs a call that breaks its contract and adds
`param_warnings` to its result; `strict` refuses it (a batch before its first step), and the
design is left as it was. Every reply to such a request says the level it was checked at as
`param_check`, beside `ok`. A command that switches to another design (`component.edit_in_place`,
`component.finish_edit`, `view.next_tab`, `view.previous_tab`), or one that goes to the file
loader (`doc.open`, `file.insert_step`, `file.insert_mesh`, `file.export_view_dxf`,
`pointcloud.insert`, `pointcloud.reload`), has its parameters checked at that level before it
runs, and then runs at the app's own level, as each design keeps its own.
Any other word is refused; other methods ignore the field. An app older than this ignores the field and its reply has no
`param_check`, so a client that needs the check asks `control.capabilities` first and treats a
reply without `param_check` as unchecked. The bridged MCP server asks once per connection
([mcp.md](mcp.md)).

### Cancelling a request

`control.cancel {id?}` cancels the request the connection still waits for: the timed-out one
`control.pending` reports (`id`, when given, must name it). A connection can only cancel its own
requests; another id is refused with `nothing to cancel: request … is not running on this
connection`. What happens depends on where the request is:

- **Waiting its turn** (behind a design change that computes, a file opening, the agent cursor):
  it is removed and never runs. `{"cancelled": <id>, "method": …, "outcome": "removed", "note":
  "cancelled: …"}`; its own reply, if anyone still reads it, is `{"ok": false, "cancelled": true,
  "error": "cancelled: …; nothing was applied"}`.
- **Computing off the window thread** (a design change or an export, see below, or one the
  agent cursor is acting out): it is stopped as **Stop** stops it, and nothing of it is applied:
  `"outcome": "stopped"`. Its worker is told to stop as well: the evaluation skips the features it
  has not reached, the kernel starts no further boolean, and nothing it computed after that is
  kept. A boolean or tessellation already under way still runs to its end in the
  background. An export's worker may still write its file; the design is unchanged.
- **Running on the window thread** (with the preference off, while it applies after computing, or
  a file being opened or imported, which the app's own Cancel stops): it can't be cancelled.
  `{"ok": false, "cancellable": false, "error": "cannot be cancelled: …"}`; it will still finish,
  and `control.pending` gets its result.
- **Finished**: its reply comes back as `control.pending` gives it (`done: true, reply`), with
  `cancelled: null`.

With nothing outstanding the reply is `{"cancelled": null, "note": "nothing to cancel: …"}`.
After a request is cancelled or stopped the connection is free at once (`control.pending` says
`{"pending": null}`). Since a connection sends one request at a time, only a request that
outlasted its wait can be cancelled from the connection that sent it: 60 s, or 3600 s for
`engine.batch` and for every request from the agent chat's own connection.

### Params

A field may be left out where a method marks it optional (`?`), but a field that is present must
have the documented type: a wrong type, or `null` for a field with a default (such as `shift` or
`delta`), is refused with an error naming the field, and nothing runs. An optional field without a
default takes `null` as left out (not `ui.scroll`'s `unit` or `ui.home`'s `sample`, which refuse
it); `engine.execute` takes `params: null` as `{}`, and `ui.start` as no params (so
`solid.extrude` starts on `null` but refuses `{}`).
`ui.set`, `ui.blur`, `ui.editFolder`, `ui.resize`, `ui.palette`, `ui.agent_cursor` and
`ui.icon_audit` still read their fields one by one: the first four refuse a wrong type, the other
three pass over it. Screen points (`x`, `y`)
must lie within ±1 000 000. In particular, `engine.execute` takes `command` or its alias `id`,
not both; `ui.help` refuses an item it doesn't know; and `ui.menu` needs both `x` and `y` to open
a menu (neither describes the open one).

## Engine

| Method | Params | Result |
|---|---|---|
| `engine.execute` | `command`: command id, `params`: JSON object | the command's result |
| `engine.script` | `commands`: `[{command, params}, …]` | results. Each step runs as `engine.execute` runs it, so the window follows it as it does a batch step. The first error stops the script, and the steps before it stay |
| `engine.batch` | `commands`: `[{command, params}, …]` (at most 10 000), `rollback?`: bool (default true), `timings?`, `profile?`, `coalesce_parameters?`: bool (default false) | `{completed, results, undone}`, plus `failed: {index, command, error}` when a step fails, and with `timings` `timings: [{index, command, ms}]` (the failing step included) and `total_ms`; `profile` adds each step's `profile: {boolean_ms, tessellate_ms, other_ms}`. Runs the steps in one request, so within one frame, each as `engine.execute` runs it. The first failure stops the batch, and with `rollback` the design goes back to how it was before it (`undone`: the steps taken back, `rolled_back`: `true` only when a rollback was asked for and the design is where the batch started, `false` when `rollback: false` or when the checkpoint could not restore it). Refused before anything runs, with an error starting `engine.batch unavailable`, when a step loads a file (`doc.open`, `file.insert_step`, `file.insert_mesh`, `pointcloud.insert`, `pointcloud.reload`) or when it goes to another design's tab (`component.edit_in_place`, `component.finish_edit`, which `engine.script` refuses too). Send such steps one by one. A step the batch would certainly refuse at its turn is found before the first step runs (#5853): a step without a command, an unknown command id (with the closest ids) or, with strict parameter checking, a call that breaks its command's contract. The reply is then that step's failure with `completed: 0`, `undone: 0`, `rolled_back` as after a rollback that went back (`true`, or `false` when `rollback: false`: the design is where the batch started), `not_run` listing every step and a `note`; nothing has run. With `coalesce_parameters`, each run of consecutive plain parameter edits (`parameters.change` with only a name and an expression, or with `changes`) is set as one `parameters.change {changes}`, so the design rebuilds once for the run. Every step still gets its own result (with `coalesced`: the run's length), and the run is one undo step. The run skips the states between its edits: an edit that would be refused on its own, but not once the run's later edits are in, goes through. A run whose combined edit is refused (a new parameter, a bad expression) runs step by step, so the refused step is named. Answered when done, for up to 3600 s |
| `engine.commands` | — | every command: id, label, tab, panel, icon, shortcut (the user's current first key, or null when cleared), params doc, enabled |
| `document.revision` | — | `{document_id, revision}`: the active design's id and a number that moves whenever the design changes, for programs that follow it ([Following the design](#following-the-design)) |
| `control.subscribe` | — | `{document_id, revision}`, as `document.revision`; the connection is then sent a `design.changed` line whenever they change ([Following the design](#following-the-design)) |
| `document.inspect` | `measure?`: bool, `sketch_counts?`: bool (default false), `timeline?`: `true` (default), `"summary"` (each feature as `id`, `name`, `type`, with `error`, `warning`, `suppressed` and `rolled_back` only when set) or `false` (left out), `bodies?`: `true` (default), `"names"` (only the body names, nothing measured) or `false` (left out) | parameters, timeline (errors, timings), bodies, sketches, selection (assembly joints: `engine.execute` `joint.inspect {joint?}`, which reads only: each joint's two origins with the occurrence, the snap as saved, the feature, face, edge or sketch curve its names refer to, and whether it resolves or why not; kind, offset, angle, flip, values, limits, expressions and motion links); with `sketch_counts` each sketch also gives its `plane` and its number of `points` and `constraints` |
| `mcp.options` | — | `{"compact": bool}`: the output the MCP server bridged to this app should give its agents (Preferences › AI › Agents through MCP, `ui.prefs` `mcp_compact`, default false; [MCP](mcp.md#compact-output)) |
| `document.export` | — | `{name, revision, dirty, format, text}`: the active design as its file holds it, for a host that keeps the files itself ([browser command API](browser-api.md)). `text` is the design's JSON, what a `.json` design file holds; `format` is `"json"`; `revision` is the one `document.revision` answers for this state. Only reads |
| `document.import` | `text`: a design's JSON; `name?`: the name it takes | `{name, features, errors, revision}`: replaces the active design with `text`, timeline and parameters as saved; it opens saved, with a new `document_id` and nothing to undo. It does not ask about unsaved changes in the design it replaces: that is the caller's to decide. A text that does not read changes nothing. Waits behind a file loading or a design change, and evaluates on the window thread in the frame it runs |
| `document.saved` | `revision`: as `document.export` gave it | `{revision, dirty}`: the host saved the design at that revision, so it counts as saved from there. Refused when the design has changed since |

The selection alone is the engine command `selection.get` (run it with `engine.execute`; read-only, no undo step, headless too): `{count, items}`, each item as `select.set` takes it (`type`, `body`, `index`, `point`, `id`, `sketch`, `path`…) with `kind` and, unless `detail: false`, what it resolves to: a face's persistent `name`, `surface` (`plane` with `normal`, `cylinder` with `origin`, `axis` and `radius`, `cone`, `sphere`, `torus`, or `other`) and `area_mm2`; an edge's persistent `name`, `curve` (`line`, `circle`, `arc`, `other`), `length_mm`, `start`, `end` and, when round, `center`, `axis` and `radius`; a body's `volume_mm3`, `feature` and `visible`; a body item's `component` and `occurrence_path` in an assembly; a sketch item's `sketch` (its id, so the item can go back to `select.set`), `sketch_name` and `entity` (a line's `sketch_midpoint` gives `midpoint` and its `position`). `ui.selection` (below) reports the window's view of it with the open dialog's inputs and the hovered item.

Commands never open dialogs when run this way. `solidcraft-cli commands` prints the same registry.

`document.export`, `document.import` and `document.saved` are control methods, not commands: they
act on the app's open design rather than on an engine session, and they are not among the methods
the agent chat may use.

### Following the design

A program that follows the open design (a renderer kept in step, a live link) asks
`document.revision`, which only reads and does not wait behind design changes (a slow command
computing, a file loading, the agent cursor acting one out), and exports only when the answer
changes (#6646):

```text
{"id":1,"method":"document.revision","token":"…"}
→ {"id":1,"ok":true,"result":{"document_id":"18f3a0c2d4e5b6a7-1","revision":3}}
```

- `document_id` names the active design (its tab) while it stays open, through edits, saves and
  renames. Two open designs have different ids even with the same name, and each design started
  or opened gets a new one, also one opened or started in the same tab. Ids of one run never
  match those of another. Treat it as an opaque string.
- `revision` moves whenever the design changes: an edit, a parameter, undo or redo, a design
  opened or started, what is hidden or shown (saved with the design), a component moved and not
  captured yet. It stays the same for reads, a selection, a camera move, a section or an analysis
  view. Revisions come from one counter for the whole run, so a revision never names two
  states, and it never goes back: changes undone before the next ask that end where the design
  was need no new one (its revision still names that design).

Switching tabs changes `document_id`; back on a tab its revision is the one it had, if nothing
changed it meanwhile. Compare both values, and export when either differs from the last
answer. The same answer comes from `engine.execute` `document.revision` (and so from scripts and
MCP `execute`), which waits its turn behind design changes like any command.

`engine.execute` with `view.status_items` and `{}` reads the persistent viewport effects without
evaluating the model or changing the design, undo history or view revision. It returns
`{items: [...]}` in oldest activation order. Each item contains a stable slot `id`
(`surface_analysis`, `section`, or the legacy independent `continuity`), `kind`, source `command`, structured `setting`, `has_key`
and a `clear: {command, params}` request that can be executed unchanged. Settings retain numbers:
lengths use millimetres, Draft's `angle_deg` uses degrees, directions are normalised, and
Continuity's `edge_count` counts the seams currently kept for shading. Labels and units are
formatted by clients. `engine.describe {command: "view.status_items"}` exposes both schemas.

By default, surface analyses, including Continuity, share one slot; successful replacement keeps that
slot's position and ends the previous analysis. Section stacks independently. Clearing and
re-enabling a slot makes it newest. Ordering metadata belongs only to the running session;
effects and ordering are not saved in the design. Existing analysis clear semantics apply:
any surface-shading command with `{clear: true}` clears the active surface shading, whereas
Continuity clears its seam records. Section clear never clears an analysis.

The standard **Graphics → Viewport → Use a single surface analysis** preference is
`single_surface_analysis` (default `true`). `ui.prefs {"single_surface_analysis": false}` restores
the legacy behavior: surface shading and Continuity remain independently active and clearable,
and `view.status_items` faithfully lists both, using a separate stable `continuity` ID. Together
with Section there are at most three slots. Turning the preference back on keeps the last
successfully started or changed analysis at the oldest analysis-group position beside Section.
Queries never normalize state. Graphics Restore page defaults restores `true`; other page resets
leave it alone. It applies immediately to all open designs without document edits or undo.
Cancel restores the opening preference value, but does not restart an analysis already stopped
by switching to single mode. Run that analysis again to restore coexistence after cancelling.
Successful Continuity measurements with no seams create no item: legacy mode keeps existing
surface shading; single mode ends the previous analysis as for other successful replacements.

Headless SDK clients use `Session::set_single_surface_analysis(bool)` and
`Session::single_surface_analysis()`. Configuration survives scratch/new/open session lifecycle;
effects and chronology still follow their existing transient lifecycle. The policy is an
application preference/session setting, never saved design-format data. Policy changes update
the view revision only when visible effects or their declarations change.

A client need not ask again and again: after `control.subscribe` on its connection, the app
sends it a line whenever the active design changes (#6985):

```text
{"id":1,"method":"control.subscribe","token":"…"}
→ {"id":1,"ok":true,"result":{"document_id":"18f3a0c2d4e5b6a7-1","revision":3}}
{"event":"design.changed","params":{"document_id":"18f3a0c2d4e5b6a7-1","revision":4}}
{"event":"design.changed","params":{"document_id":"18f3a0c2d4e5b6a7-2","revision":5}}
```

- The reply is what `document.revision` answers then (and, like it, does not wait behind design
  changes); no event comes before it. From there on, each `design.changed` line carries what
  `document.revision` would answer, sent when its `document_id` or `revision` differs from the
  last one this connection was told: an edit, undo or redo, a design opened or started, a tab
  switched.
- Lines are coalesced, not sent per frame: at most about ten a second, and changes made faster
  (a parameter dragged) are followed by one line with the latest. Once the design stops
  changing, the last line names it as it is.
- An event has `event` and `params` and no `id`; every reply still has its request's `id`. Each
  event is a whole line of its own, between replies, never inside one. Ignore events you don't
  know.
- Only connections that subscribed get events, until they close (or shut their sending side). A
  second `control.subscribe` on a subscribed connection is refused with `already subscribed`;
  ask `document.revision` for the design now. A `control.subscribe` that timed out or failed did
  not subscribe the connection: send it again.

### Slow commands and the window (#1989)

With the preference **Preferences > AI > Keep the window responsive while agents run commands**
on (the default; `ui.prefs {"agent_off_thread": false}` turns it off), a design change in the desktop app
that takes longer than about 150 ms (an `engine.execute` of a
modelling command, an `engine.batch` of them) computes off the window thread, so the window keeps
drawing and answering while it runs:
- The command is evaluated ahead on a copy of the design, which also makes the display meshes of
  the bodies it ends with. Then it runs for real on the window thread, reusing every feature the
  evaluation computed (kept for it, whatever the bounded evaluation cache dropped meanwhile), so
  the window waits only for the bookkeeping. Its reply, undo step, autosave and crash journal are
  the same as before. With the agent cursor shown, the evaluation runs while the pointer glides
  to the button, and the pointer waits there until it is done.
- Meanwhile `ui.*` requests, `document.inspect` and `engine.commands` are answered at once
  (inspection shows the design as it was), also while the agent cursor acts a command out. Later
  design changes, from any connection, wait their turn and run in the order they arrived.
- A person's edit made meanwhile goes first. The agent's command is then computed again for the
  design as it is and lands after that edit, never in the middle of it. If the design was replaced
  (another tab made active, a new file opened, its tab closed), the request fails at once with
  `… the design was replaced … nothing was applied`, and the requests behind it go on.
- A row of the operations panel in the view's bottom-right corner names the command, with how
  many of a batch's steps are done (hover it for the steps still to come) and a red X (with
  **Preferences > Layout > Show operations in the view** off, a chip with **Stop** instead;
  `ui.operations` lists and cancels the rows). Each design change waiting behind it is a row of
  its own, numbered by its place in the queue (1 runs next), whose X cancels just that one; the
  panel shows the first 4 and one "N more waiting" row whose X cancels the rest, while
  `ui.operations` lists every one of them, each cancellable by its own `key`.
  The X answers `{"ok": false, "stopped": true, "error": "stopped: …; nothing was applied"}` and
  the requests waiting behind it go on. The computation stops before its next feature or boolean,
  in the background. The agent can do the same for its own request with `control.cancel` (above).
- `file.export` and `file.save_mesh` change nothing in the design. They run on a copy of the
  design as it was when they were asked for, and nothing waits behind them.
- Batches with `timings` or `profile` still run on the window thread, so the times they report
  measure the real work.

A file being opened or inserted (from the window or the control channel) holds the agent's
design changes the same way until it is in, so an agent's `doc.open` and the edits after it go
in that order. The person at the window can go on working meanwhile: the file is a row of the
operations panel, and its result lands after their edits (`ui.prefs {"file_operations_wait":
true}` brings back the card over the design that makes everything wait for the file).

Timeouts and `control.pending` work as described above. A slow command is still answered when it
is done, from a later frame. With the preference off, commands run on the window thread, and the
window waits for each one.

Product images use `render.preflight/start/status/cancel/export` through `engine.execute`.
Keep the returned session-scoped job ID for subsequent calls; status does not block or
render. See [render jobs](render-jobs.md) for capture, cancellation and export semantics.

While an interactive native file picker is open, `ui.inspect` reports `file_picker_busy: true`.
Engine commands, `ui.file` and `ui.home` requests wait for its answer; window resizing,
inspection and screenshots remain available.

Every `sketch.*` command that changes a sketch adds `created: {points, curves, constraints,
dimensions}` to its result (and `wires` for 3D curves): the ids of what it added, in creation
order, next to whatever else the command returns (`curves`, `sides`, `corners`…). Sketch ids are
stable and never reused, even after a delete (`l1`, `l2`, then `l3` once `l2` is gone; only undo
rolls the numbering back with the design), so they can go straight into later commands:

```text
{"id":7,"method":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[40,0]]}}}
→ {"curves":["l1"],"created":{"points":["l1.start","l1.end"],"curves":["l1"],"constraints":["k1"],"dimensions":[]},…}
{"id":8,"method":"engine.execute","params":{"command":"sketch.dimension","params":{"entities":["l1"],"value":50}}}
```

A bare numeric `value` given to `sketch.dimension` (from the control channel, scripts or MCP) is
read in the design's units, exactly as if it were typed into the dimension box: in an inch design
`"value": 2` makes the line 2 in (50.8 mm) long. Point coordinates stay in millimetres. To pin a
unit regardless of the design, pass an expression with one: `"value": "50 mm"` or
`"value": "0.5 in"`. Angles are degrees unless the expression says otherwise.

Any expression parameter of any command can also define a user parameter as it sets the value:
`"distance": "depth = 15"` adds `depth` (15, read as the bare value would be) and stores `depth`
in the input, in the same undo step. The reply names it in `defined` and lists every definition
in `definitions: [{name, expression, unit}]`; a taken or reserved name fails the command and
changes nothing. `parameters.change` with `"expression": "height = 12"` renames the parameter it
changes instead (see the parameters guide).

The viewport also shows an X/Y/Z-aligned dimension chain between the endpoints of the
currently shown distance (centre, minimum or maximum), with each world-axis distance
labelled separately as an absolute value (including zero).
It uses the design's units and length precision, independently of the view or sketch plane.
Preferences → Graphics → **Show X/Y/Z distances** controls these extra lines and
labels (default on, saved between runs). Set `ui.prefs {"measure_axis_distances": false}`
to hide them while keeping the total distance and the dialog's ΔX/ΔY/ΔZ values. These
overlays remain visible until Measure closes.
Overlay handles `measure:label:x`, `measure:label:y` and `measure:label:z` identify the extra
labels; `prefs:measure_axis_distances` identifies the checkbox.
A `ui.drag` from `measure:label` or one of the axis labels moves that label (screen offset, kept
while the measured items stay the same; a double-click puts it back). It picks nothing and starts
no box selection.

## UI

| Method | Params | Notes |
|---|---|---|
| `ui.inspect` | — | UI state, pending file picker (`file_picker_busy`; compatibility alias `file_dialog_open`, no path), what is hidden (`visibility`: bodies, origin and construction items, sketches, joints (`hidden_joints`, joint ids); set it with the `browser.visibility` command), viewport rect, camera, the active tool (its command id), the open dialog (`dialog`, below, or else the "Recover unsaved design?" prompt shown after an unclean exit as `{kind: "recover_unsaved", title, designs: [{id, name, path, features, saved_at, autosaved}], actions: ["recover", "discard", "later"]}`, answered with `ui.recover`), the windows open beside the model (`windows`, each named by what opens it: the `ui.start` command `parameters.change`, `document.metadata`, `sketch.inspect`, `assembly.bom` or `material.properties.validate`, or the `ui.help` item `about`, `updates`, `shortcuts`, `crashes`, `report`, or `messages`), what is under the pointer (`hover`, in the form of a selection item: `{"type": "edge", "body", "index", "mid", "point"}`, or null), the status line (`status`: text and whether it is an error), renderer, timings, what the GPU drew last frame (`draw`: `draw_calls`, `triangles`, `line_segments`, `supersampled`; null without a GPU) and the graphics `adapter`, live preview (`active`, `busy`, `error`, `ms`, bodies it stands in for) |
| `ui.set` | any `UiState` field (`tab`, `perspective`, `showGrid`, `dark`, `layout`, …), and the browser's view of the design, saved with it: `showOrigin` (the Origin folder's eye), `showSketches` (the design's Sketches folder, `c0/sketches` in `hiddenFolders`), `hiddenBodies`, `hiddenOrigin`, `hiddenSketches`, `shownSketches`, `hiddenJoints`, `hiddenFolders`, `hiddenProfiles`, `shownDims`, `lockedBodies` (whole lists; `browser.visibility` changes single items) | `dark: false` switches to the light theme; `layout: "floating"` (default) or `"compact"` (`"docked"` is also accepted) selects the transient shell layout without saving a preference. Scenarios use `{"call":"ui.set","params":{"layout":"docked"}}` |
| `ui.view` | `view`: `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`, `home`, `fit`; `animate` (bool, default false: snap) | |
| `ui.camera` | `target?`: `[x, y, z]` (mm), `yaw?`, `pitch?`, `roll?` (radians), `distance?` (mm), `fov?`, `up?`; `fit?`: `true`, `false` or `[occurrence id or name…]` (`root`: the bodies outside every component) | the viewport's orbit camera: it looks at `target` from `distance` away; `yaw` 0 looks along +Y (from the front), `pitch` is the elevation (±π/2: straight down or up), `roll` turns the image. Fields given are set first (the others keep their values), then `fit` frames the listed occurrences (or everything), keeping the direction. The reply, passed back, sets the same view again, so a framing can be stored and replayed. `fov` is the angle the view is drawn with: 0 (parallel) with Perspective view off or while a sketch is open, else `perspective_fov`; in Render, the Render camera's own. `fov` and `up` follow the perspective setting (`ui.set`), its angle (`ui.prefs {perspective_fov}`) and the up axis preference (`ui.prefs`) and can't change here; a refused field changes nothing |
| `ui.start` | `command`, `params?` | like a toolbar click: starts the sketch tool or dialog for the command. `params` sets what the dialog opens on, and nothing runs until OK: `joint.drive` takes `joint` (id or name; without it, the joint picked in the browser, else the first joint that moves; a rigid joint opens to say why it has nothing to drive, with an Edit Joint button, handle `drive:edit_joint`) and `values` (numbers in degrees or millimetres, or expressions); `motion.study` takes `study` (id or name), which opens that study to edit and chart. A command that takes no params is refused before it starts; a param the dialog doesn't take is refused and the dialog closes. Replies what it opened: `tool`, `dialog` and `windows` as `ui.inspect` gives them, `status` (where a command that starts nothing says why), and `pending: true` when the start waits for the history to compute and opens on a later frame |
| `ui.click` | `x`, `y` (screen points), `button?`: left/right/middle, `shift?`, `ctrl?`, `double?` | real pointer input |
| `ui.move` | `x`, `y`, `shift?`, `alt?`, `ctrl?`, `cmd?` | pointer movement without a button; modifiers apply until the next synthetic move/click; a move with omitted modifiers releases them. With `nav: "touchpad"`, Shift pans and Alt/Option orbits. |
| `ui.drag` | `x0`, `y0`, `x1`, `y1`, `via?`, `button?`, `shift?`, `ctrl?`, `steps?`, `hold?`, `hold_end?` | press, move, release; `via` is a list of `[x, y]` points passed through on the way (`steps` moves to each); a left drag on the model is a box selection (left to right: window, right to left: crossing), or with `lassoSelect` on (`ui.set`, the SELECT panel's Lasso) a freehand lasso that takes what is wholly inside it; with OpenSCAD navigation, plain left-drag on empty space orbits, Shift+left-drag box-selects, and drags starting on sketch geometry edit it; `hold` keeps the button down that many frames first (a right press-and-hold opens the marking menu, releasing over a direction picks it), `hold_end` that many frames at the end (a sketch drag shows its snap) |
| `ui.menu` | `x?`, `y?`, `target?`, `close?` | with a point: open a context menu there as a right-click would (the viewport's marking menu, a browser menu with `target: {"type": "body", "name"} \| {"type": "sketch", "id"} \| {"type": "component", "id"}`, or a History bar feature's with `{"type": "timeline", "feature": id}`); a target the design doesn't have is refused (`no body \`Name\``); without: the open menu (`radial` and `items`: id, label, shortcut, enabled, icon, placeholder, and `children` for a row that opens a flyout; blank icons on headings/separators have `placeholder: false`) |
| `ui.icon_audit` | `reset?`, `stop?` | `reset: true` enables collection on the UI thread and clears previous requests; returns `{requested: [{name, master, placeholder}]}` sorted by requested id. Each read drains names collected since the previous read/reset. Collection stays enabled until `stop: true` (which returns pending names before disabling it). Includes aliases, variable/table ids and cached paints. |
| `ui.menuPick` | `item`: id or label | runs an item of the open menu or of any of its flyouts, as clicking it does; a row that only opens a flyout is refused |
| `ui.selectOther` | `x?`, `y?`, `pick?`, `add?`, `hover?`, `close?` | Select Other: with a point, list compatible hidden and visible geometry near the pointer by default (as holding the left button does; disable `select_other_hidden` for the original visible-only list); the popup fits the available window space and scrolls only for overflow; `pick`: select row `pick` (`add`: as Shift-click; with a dialog open, into its active input); `hover`: light a row up; without: the open list (`open`, `at`, `total`: everything under the cursor, `rows`: label, icon, sel, at most 20 per kind, nearest first) |
| `ui.rename` | `text?`, `commit?` (default true) | finishes the rename box a Rename item opened |
| `ui.selection` | | the selection, the open dialog's inputs, the hovered item, and the active sketch's drawn dimensions (`dimensions`: id and text centre, `dimension_selected`, `dimension_editing`), and finished sketches' dimensions shown in the model view (`shown_dimensions`: sketch, id and text centre; a double-click there edits the value) |
| `ui.worldToScreen` | `points`: `[[x, y, z]…]` world points | their screen points |
| `ui.triad` | | screen points of the 3D sketch move triad's handles (`x`, `y`, `z`, `plane`) |
| `ui.sketchToScreen` | `points`: `[[x, y]…]` in active-sketch coordinates | their screen points (null when behind the camera) |
| `ui.dialogInput` | `index` | what clicking an input's Select chip does: picks go to that input |
| `ui.at` | `world: [x,y,z]` \| `sketch: [x,y]` \| `plane: "XY"` \| `axis: "Z"` \| `dimension: "d1"` (its value text) \| `handle: "arrow"` (a manipulator handle, see `scenario::publish_handle`) | the screen point (a scenario step's target; see `crates/ui-egui/src/scenario.rs`) |
| `ui.pick` | `pixels?: [[x,y]…]`, `rays?: [{origin, dir}…]`, `frame?: "window"\|"model"`, `width?`, `height?`, `edges?` (default true), `edge_px?` (default 6), `all?`, `include_ghosted?` | read-only geometry under each pixel or ray (#6304): one entry per input, `null` where nothing is hit — body, face index, exact world `point` and outward `normal`, `depth`, `occurrence`/`component` for placed parts, `edge`/`vertex` within `edge_px` of a pixel, `section_cap: true` on a cut face. `window` pixels are physical pixels of a window screenshot; `model` pixels are pixels of a `ui.render` of `width`×`height` (default 960×600), without needing a window screenshot. Both frames and ray picking require the app's control channel; they are unavailable through `solidcraft-cli mcp --headless`. As `view.pick` through `engine.execute`/`engine.batch` when connected to the app; absent from the engine command registry (`list_commands` and `describe_command`). See `crates/ui-egui/src/view_pick.rs` |
| `ui.handles` | `prefix?` | the handles drawn last frame, `[{name, x, y}]`: toolbar buttons and drop-down items `toolbar:<command id>` (the button alone: `button:<command id>`; a drop-down row: `dropdown:<command id>`, highlighted `dropdown_hot:<command id>`, its pin `dropdown_pin:<command id>`, its ⋯ menu button `dropdown_more:<command id>`; Customize Toolbar's check boxes `customize:<command id>`), ribbon tabs `tab:<name>` (MODEL, SURFACE, ASSEMBLE, INSERT, INSPECT, MANAGE, VIEW, SHEET METAL, WELDMENTS; contextual SKETCH; workspace RENDER), the toolbar menu button `ribbon:add_tab`, the hint when every tab is hidden `ribbon:tabs_hidden`, Preferences tab check boxes `prefs:toolbar_tab:<NAME>`, panel labels `panel:<tab>:<panel>` (also `ribbon:section:<TAB>:<SECTION>`), document overflow list `tab:overflow`, workspace column `ribbon:workspace`, command-search launcher `ribbon:search`, browser rows `row:<label>`, where each row's depth puts it `indent:<label>`, and fold arrows `fold:<label>`, manipulator handles; signed-axis tips `axis_view:z`, `axis_view:-y`; optional cube facets `navigation_cube:top`, `navigation_cube:top_front_right`, controls `navigation_cube:control:home` (and other actions), plus signed-axis aliases for visible cube faces; Preferences navigator picker `prefs:navigator`, options `prefs:navigator:Signed axes` and `prefs:navigator:Cube`; in the Motion Study dialog, where the chart puts its first and last step `motion:chart_start` and `motion:chart_end`, the Check collisions button `motion:check` and each collision found `motion:collision:<n>`; with Placement handles on, the move marker of an open placement dialog `<tool>:position` and its ring `<tool>:rotate` (`rest`, `boss`, `snap_fit`; `primitive:position` for Box, Cylinder, Cone, Sphere, Torus and Coil, which have no ring); with `hole_moves` on, a Single hole's markers `hole:<n>` |
| `ui.editFolder` | `folder` (id or name) | Edit Holes on a History folder that holds holes only: one Hole dialog, filled in from its first hole, whose OK gives every hole in the folder the fields changed there. Replies as `ui.start` does |
| `ui.editFeature` | `feature` (id or name) | what double-clicking a timeline item does: rolls back to it and opens its dialog filled in (sketches open in sketch mode). Replies as `ui.start` does, with the history `marker` |
| `ui.scroll` | `x`, `y`, `delta?`, `ctrl?`, `unit?`: `point` \| `line` | wheel zoom at the cursor. `ctrl: true` replays a Ctrl+wheel pinch (Ctrl held and the wheel in points); `unit` selects the wheel unit and defaults to points with `ctrl` and lines without it. `delta` is in points either way. Invalid units or coordinates outside the finite screen-coordinate range are rejected before queuing input |
| `ui.key` | `key` (egui key name, `Enter`, `Escape`…), `cmd?`, `ctrl?`, `shift?`, `alt?`, `pressed?` | By default, press and release, then release modifiers. With `pressed: true` or `false`, send only that edge; modifiers remain as given. `cmd` retains the legacy primary-modifier alias unless `ctrl` is explicit: `cmd:true, ctrl:false` sends physical Command, and both true sends Control+Command. `ui.move` can change modifiers between edges. |
| `ui.text` | `text` | typed text |
| `ui.screenshot` | `path?` | PNG of the window (needs a presented frame: a minimised or fully covered window answers at once with `hidden: true` and an error; use `ui.render`) |
| `ui.render` | `path`, `width?`, `height?`, `grid?`, `axes?` | Render of the model with the current camera (no window needed), drawn on the app's GPU device with the viewport's pipelines (the reply's `renderer`: `gpu`, or `gpu-software` on a software adapter), in the window's theme (light or dark; `solidcraft-cli snapshot` keeps the light colours); cut by the section the viewport shows (`inspect.section`, an open Section Analysis dialog's preview, a sketch's Slice), with the cut face capped. The ground grid and the origin's axes are drawn as the window shows them (`ui.set` `showGrid` and `showOrigin`, `hiddenOrigin`); `grid` and `axes` (booleans) say otherwise for this render |
| `ui.confirm` | `accept?`: bool | answers the Delete confirmation (shown when a delete takes other features with it or makes some fail); without `accept`, what it lists |
| `ui.messages` | `clear?`: `true` (empty the list), `open?`: bool (show or hide the Messages window) | every status message of this run, oldest first (the last 200; a message repeated in a row is one entry with a `count`): `messages` `[{text, error, at, count}]` (`at` in ms since the Unix epoch), and whether the window is `open` (#2763) |
| `ui.documents` | `action?`: `new`, `switch`, `close`, `close_all`, `close_others`, `move`, `quit`; `index?`; `from?`, `to?` (move); `save_indices?`: array of tab indices to tick in the grouped save prompt; `save_row?`, `discard_row?`: tab index to save or discard and close in that prompt; `force?` (close without asking); `answer?`: `save`, `dont_save`, `cancel` (answers "save changes?"); `open_elsewhere?`: `read_only`, `anyway`, `cancel` (answers "open elsewhere" for the first Open waiting); `changed_on_disk?`: `save_copy`, `overwrite`, `cancel` (answers "changed on disk" before a Save) | the open designs (tabs): names, unsaved flags, the active one, a pending "save changes?" prompt, `save_prompt` (null or rows with `index`, `name`, `path` (null if never saved), `selected`), whether a quit is asking, whether the active design is `read_only`, the Opens waiting for "open elsewhere" (`open_elsewhere`) and a waiting "changed on disk" (`changed_on_disk`) |
| `ui.operations` | `cancel?`: an operation's `key` | the operations under way, newest first, as the view's operations panel shows them: `key`, `label`, `steps` (`{done, total}` when the work counts them; `percent`: they are bytes, a file read or written), `queued`, `cancelling`, `cancellable`, `next` (on the row of an agent batch: the names of its steps still to come, at most 12, which the row shows as a tooltip); each design change held behind the agent's running one is a queued row of its own, which the X cancels (it is answered as cancelled and never runs); those that ended in the last minute (`finished`, newest first, at most 16): `key`, `label`, `outcome` (`done`, `failed` with its `error`, or `cancelled`) and `ms_ago` (one done within the app's wait was never under way, and isn't listed); and whether the panel is `shown`. With `cancel`: cancels that operation as its X does (its work stops at its next check and nothing of it is applied; operations queued behind it go on); a save stops unless it is already replacing its file, and leaves the file, its backup and the design's unsaved changes as they were (it then ends `cancelled`); an error when no such operation is under way or it can't be cancelled (an opened design's bodies being meshed) |
| `ui.file` | `path`; `insert?`: bool | starts an interactive background Open or Insert; poll `ui.inspect.file_busy` for completion; the window stays responsive |
| `ui.home` | `open?`: bool; `sample?`: index | the start page: show or hide it, open a built-in sample (once built); returns `open` and the recent designs |
| `ui.shortcut` | `command`, `key?`, `slot?`, `replace?` | a command's keys (`key`: the first, shown in menus; `keys`: all), or rebinds it: `key` becomes its only key, or with `slot` replaces its key there (one past the last adds a key, `""` there removes that key); `""` without `slot` clears them; key names are case-insensitive and modifiers are normalized (`shift+ctrl+t` becomes `Ctrl+Shift+T`); unknown and reserved keys are refused; a key another command uses is refused unless `replace` |
| `ui.help` | `item?`: `about`, `updates` (checks for updates and shows the result), `shortcuts` (read only), `report` (opens a report draft for review), `crashes` (the Crash Reports window), `docs` (the documentation in the browser), `messages` (the Messages window), `install_cli` (macOS: links `solidcraft-cli` into `/usr/local/bin`); another item is an error | the version, commit, build date and diagnostic text |
| `ui.prefs` | any of the [`ui.prefs` settings](#uiprefs-settings) below; also `open`, `button` and `stand_up` (listed there) | sets preferences (kept between runs), returns them all plus `dialog` (the open page or null), `apply_enabled` (whether the dialog's Apply has a change to keep), `dark`, `viewport_dark` (whether the 3D view is dark), `stand_up_offer` (the offered bodies, or null), `stand_up_note` (whether that offer is the note), `theme_followed`, `theme_follow_error`, `theme_option_defs`, `theme_option_values` and `theme_option_notices` |
| `ui.capture` | `action?`: `capture`, `revert`, `cancel` | answers the Capture Position question (asked when a command starts or the design is saved while components were moved without capturing); without `action`, what waits |
| `ui.recover` | `action?`: `recover`, `discard`, `later`; `id?` (a design's `id`; default: the first one listed) | answers the "Recover unsaved design?" prompt as its buttons do: `recover` opens that design's last autosave (in the background; poll `ui.inspect.file_busy`; refused while the open design has unsaved changes), `discard` deletes it (`doc.recovery_discard`), `later` closes the prompt and keeps them all for the next launch; a non-string `id` is refused, and so is `recover` while 32 designs are open and the active design is not blank (the entry stays offered); returns the prompt as `ui.inspect` reports it (`dialog`, null when no designs remain) and `file_busy`. Without `action`, returns only the current `dialog` |
| `ui.window` | `action`: minimize, maximize, restore, toggle, close | what the title bar's caption buttons do (on Windows and Linux the application bar is the title bar); close asks about unsaved designs first |
| `ui.agent_cursor` | `show` (bool), `speed`: a number from 1 to 5 (times the normal speed) or `"instant"` (no animation: the command runs at once and the pointer jumps to where it acted), `follow_camera` (bool); all optional | the settings. While the cursor is shown, `engine.execute` first glides to the command's toolbar button and to each pick, then runs and replies; later requests wait their turn. The pointer shows the command's localized name, including commands inside a closed toolbar group; an animated batch names each command it visits. It also follows the pointer input sent with `ui.move`, `ui.click`, `ui.drag` and `ui.scroll`, as each event reaches the app, and a press leaves a ripple. `"animate": false` on a call skips it |
| `engine.execute` → `view.layout` | `mode`: `floating` or `compact` | Apply and save the workspace layout without opening a dialog, changing the design, resetting the camera or cancelling its active tool. General preferences expose the same choice; `ui.prefs {"layout":"compact"}` also saves it. |
| `engine.execute` → `browser.expand_all` | `path?`: occurrence ids; `component?`: id; omit both for the whole tree | Open every Browser row under that scope, including folders that start closed (Origin, Named Views, Features). Display only: not saved in the design, not an undo step. |
| `engine.execute` → `browser.collapse_all` | same | Close those rows. The design row stays open when the whole tree is collapsed. |
| `engine.execute` → `view.history_layout` | `width?`, `dock?`: `bottom`, `left`, `under_browser`, `browser`, `collapsed?` | Update the History panel in the shared layout without changing the design. `browser` shows features in the Browser in place of a separate panel. |
| `ui.agent_chat` | `features?`, `toolbar_button?`, `connect_at_start?` and `open_at_start?` (connect the agent started last, and open the panel, when SolidCraft starts; both off by default, never with `--control`), `instructions?` (the custom instructions every new chat's system note carries, Preferences → AI; at most 4000 characters are kept), `permissions?` (`ask`, `edits`, `all`), `add_agent?` ({`name`, `kind`: `acp`, `api` or `anthropic` (`api` is OpenAI-compatible, `anthropic` Anthropic-compatible), `command` or `base_url` + `model` + `request_timeout_s?` (seconds one model request may take, 300 by default, rounded and clamped to 30 to 3600), `allow_insecure_http?` (OpenAI-compatible endpoints only; false by default; explicitly permits sending the key over unencrypted HTTP to this endpoint, without opening a confirmation dialog)}; no keys), `add_dialog?` (`installed`, `custom`, `api`, `anthropic`, or `false` to close), `edit_dialog?` (the id of an agent you added: the same dialog filled with what it holds, as its **Edit** button opens it; the key is never shown), `default_agent?`, `agent?` (the conversation's agent, by id), `mode?` (the agent's own mode), `provider?` (which provider's group the model picker opens first, when the models come from several), `model?` (one of its models, or see below), `open?`, `toggle?` (true: show or hide the panel as its button does, unfolding a folded one), `side?` (`left` or `right`), `minimized?`, `width?` (placement of the open shared Assistant), `popped?` (in its own window), `picker?`, `refresh?`, `connect?`, `stop?`, `attach?`: a file's path (a picture, a text file or a CAD file, for the next message; CAD files are imported at once), `remove_file?` (index of a file on the next message), `send?`: text (files alone make a message too), `send_input?` (true: send what is in the message box, as Enter does), `attach_view?`, `attach_selection?`, `pick?` (true: open the context picker), `pick_at?` (with `pick`: the character of the message box its pill goes to; the end by default), `permission?`: an option id or `"cancel"` (the newest question of the agent's own), `approve?`: true, false or `"always"` (the oldest request held for Allow/Deny), `cancel?` (Stop: the queue stays), `send_now?` (stop the running step, then send the queued messages), `remove_pending?` (index), `new_chat?` (clear the chat, new session), `undo?` (the last message's changes) | the chat's state: `features`, `agents` (found and added), `currentAgent`, `state` (off, starting, ready, busy, auth, exited, failed: the agent never got ready, and Connect reads Retry), `connectAtStart`, `openAtStart`, `instructions`, `lastAgent` (the agent started last), `popped`, `window` (its place and size), `agent`, `modes`, `mode`, `models`, `model`, `modelWarning` (the model capability warning text, or `null` when unknown or tool support is reported; checked asynchronously and never blocks sending), `turnWarning` (the latest completed turn's no-tool notice, otherwise `null`; cleared on the next turn or new chat), `providers` (`name` and `count` of each, empty with one provider), `provider` (the one listed), `authMethods`, `toolsPort`, `pid`, `waiting`, `turn`, `pending` (queued messages, with their `files`), `input` (the message box's text, picks as `«n: …»` marks), `pills` (what the marks in the box stand for), `picked` (the same items as one list of `label` and `detail`, as before the pills), `attachments` (the files on the next message, by name), `loading` (pictures still being read for it, by name), `stopped`, `detection` and the conversation (`items`). With `features` off only the settings apply. Desktop only; see [Agent chat](agent-chat.md) |
| `ui.resize` | `width`, `height` | |
| `app.quit` | `force?` (quit without asking about unsaved designs) | `quitting` (the window closes), `asking` (a "save changes?" prompt is open) |

`dialog` (in `ui.inspect`, `ui.start` and `ui.editFeature`) is `null`, the recovery prompt
described above, or the open feature dialog as an object that doesn't change with the form's internals:

```json
{"command": "solid.extrude", "title": "EXTRUDE", "editing": null,
 "inputs": [{"label": "Profiles", "items": [{"type": "profile", "sketch": 10, "index": 0}], "active": true}],
 "error": null,
 "commands": [{"command": "solid.extrude", "params": {"sketch": 10, "profiles": [0], "distance": "10 mm",
               "direction": "positive", "operation": "join", "exact_regions": true}}],
 "waiting": null}
```

`command` is the command the dialog runs (null for a few forms that run none), `title` its
header, `editing` the feature id an edit rebuilds, `inputs` its selection inputs (`active`: the
one picks go to), `error` the refusal it shows. `commands` is what OK runs now, with the
parameters as that command takes them (an edit runs `timeline.redefine` with the feature's command and
parameters inside); when OK can't run yet, `commands` is null and `waiting`
says what it waits for (`select profiles first`).

For an ACP agent advertising configuration options, `ui.agent_chat` also accepts
`{"config_option": {"id": "effort", "value": "high"}}`. Use an option ID and choice value
from its reported `configOptions`; IDs and available values belong to the agent. The reply's
`configOptions` is `null` for a legacy agent, or the current list of supported select options
(including an empty list when none remain). `configPending` is true while a selection waits for
the agent's reply. Read the confirmed state after that reply; submitting a change is not its
confirmation. The existing `model` and `mode` inputs use the corresponding configuration option
when advertised, and retain their existing behavior for agents using legacy lists or API endpoints.

`model` may also name a model the agent did not list, as the picker's custom row does: it goes to an
API endpoint, to an agent with a model option, and to a legacy agent that listed models; the reply's
`customModels` says whether this agent takes one. Otherwise it is ignored. A model an agent refuses
leaves its previous model and adds a notice. `modelsNote` says why an endpoint's models could not be
listed, or is `null`.

### `ui.prefs` settings

`render_previous_geometry_budget` (Graphics, default false) restores the previous fixed Render preview geometry budget: 512 MiB on 64-bit desktop, 128 MiB elsewhere. With it off, desktop uses the device-based budget.


One setting per line, sorted by name. A change that adds a setting adds its own line in order, so two such changes only meet when their settings are neighbours.

- `angle_decimals`
- `assembly_move_handles` (default true): explicit Assembly Move uses the permitted joint-motion handles; false keeps the previous Move panel.
- `auto_operation` (default true: feature windows choose Join, New Body or Cut from what their material touches until one is picked; false: each keeps the operation it opens with, Extrude its own Join or Cut)
- `background_image` (string, default empty: a picture's file, shown behind the model and covering the window from its centre).
- `browser_bodies_folders` (`several` (default): a component with one body and no groups lists it straight under the component in the browser, without a Bodies folder; `always`; `never`: components list their bodies and groups straight under them, and the folder's menu items are on the component's menu; the design's own Bodies folder, and one hidden with its eye, always show; #4217)
- `browser_components_open` (default false: each component placement in the browser starts closed until it is opened, by its fold arrow, Expand All or Find in Browser; true: every placement starts open, as before #9274)
- `browser_eye_left` (default false: each browser and History tree row's eye, handle `eye:<name>`, sits in a column at the rows' right end, clear of the fold arrow and the radio; true: between them, as before #8718)
- `check_updates` (default true; [updates](book/src/guide/updates.md))
- `color_vision` (`off` (default), `protan`, `deutan`, `tritan`: colour vision modes, #778)
- `constraint_glyph_scale` (1…3, default 1: how large the open sketch's constraint glyphs are drawn, a multiple of their 15-pixel badge, letter and icon included; Preferences › Sketch › Constraint glyph size, #10794)
- `default_units`
- `dialog_buttons` (`bottom` (default), `top`: a feature dialog's OK, Cancel and Apply sit in its title row instead of under its inputs, #6708; the buttons keep their handles, and `dialog:close` is then Cancel's; the Sketch panel's Finish Sketch row moves above its options, #9860)
- `dimension_colour` (`null` (default) or `[r, g, b]` numbers 0…255: the colour a sketch dimension draws its lines, arrows and value in; null uses the theme's dimension colours. Selection, hover and conflicts keep their own colours, #14121)
- `dimension_input` (`view` (default): a sketch dimension's value is typed in a box on the dimension; `window`: placing a dimension with the Dimension tool, or double-clicking one, opens a small window at it with the value field (handle `dimwin:value`), arrows that step the value (`dimwin:up`, `dimwin:down`), a check (`dimwin:ok`, also Enter) and a cross (`dimwin:cancel`, also Esc), #7246)
- `dimension_step_deg` (default 1, 0.001…180: what the dimension window's arrows, Up/Down and the wheel over its field add to an angle, in degrees)
- `dimension_step_mm` (default 1, 0.001…1000: what the window's arrows, Up/Down and the wheel over its field add to a length, in millimetres, written in the unit the value is written in)
- `dimension_text_scale` (default 1, 0.5…2.5: how far a sketch dimension's text is scaled; it applies in the open sketch and to Show Dimensions, #14121)
- `escape_twice_finishes_sketch` (default true: in a sketch, two quick presses of Esc (0.4 s apart or less) with no tool or dialog left to cancel finish it, as before; false: Esc only cancels the tool, dialog or selection and the sketch stays open until Finish Sketch, #12069)
- `export_name` (string, default empty: each export offers the design's name, or a mesh its body's; otherwise the name File › Export and Save As Mesh offer, with `{design}`, `{body}` (the exported body's name) and the design properties (`{project}`, `{part_number}`, `{revision}`, `{author}`, `{organization}`, `{description}`, `{license}`) replaced; [export file names](book/src/guide/files.md#export-file-names))
- `extrude_arrow_face_stops` (default true: dragging Extrude's distance arrow stops at flat faces parallel to the profile that lie over it, takes their exact distance and outlines them, #2398; false: the drag moves in round steps only, as before)
- `extrude_auto_to_object` (default true: in the Extrude dialog, with a profile picked, a face, vertex or parallel plane picked off its plane switches the extent to To Object, unless the Profiles box was clicked into; it applies to Extrude dialogs opened afterwards)
- `fillet_legacy_radius` (default false: a new Solid Fillet opens at the radius the tool ships with, 2 mm, shown in the design's units ("0.07874 in" in an inch design); true: it opens at the number 2 read in the design's units ("2 in"), as before, #12418)
- `forget_stand_up: true` (action: forget all remembered stand-up answers; also available beside the memory checkbox in Preferences › Design)
- `frame_rate_details` (default false: with the frame rate, a frame-time graph of the frames drawn with the triangles over it, a frame-time histogram and what the GPU drew; clicking the readout switches it)
- `graphics_adapter` (exact card name, or empty for Automatic; applies after restart; `WGPU_ADAPTER_NAME` takes precedence)
- `graphics_backend` (native Graphics API: `auto` by default; Windows: `dx12`, `vulkan`, `gl`; macOS: `metal`; Linux: `vulkan`, `gl`. Saved before graphics initialization and applied on restart. Existing manual comma lists are preserved until an explicit API change. `{"button":"restart_graphics"}` asks about unsaved designs, then restarts with the saved API; unavailable in the browser or under an explicit launch override.)
- `ground_shadow_follows_model` (Graphics, default false): raised models cast their ground shadow on the origin ground plane; true restores the shadow below the lowest visible body. Both modes keep the shadow below bodies extending beneath the origin, and follow the selected up axis.
- `hidden_toolbar_tabs` (default `[]`: all existing tabs, including Sheet Metal and Weldments, stay visible; lists tabs to hide, each once, e.g. `["SURFACE", "SHEET METAL"]`; hidden tabs stay hidden regardless of the design's features; [ribbon layout](ribbon-layout.md))
- `history_in_browser` (default false): list each component’s features in the Browser and hide the separate History panel.
- `history_sketches` (`in_order` (default), `under_feature`: vertical History lists a sketch under the first feature built from it, #6489. The bottom strip keeps creation order.)
- `hole_group` (default false: the holes one Hole command makes, two or more, go into a feature folder in History, closed at first)
- `hole_moves` (default false: a placed Single hole has a drag handle (`hole:<n>`), and a pick on a face that has a hole moves it there)
- `hole_several` (default false: a face takes several Single holes; a pick elsewhere on a face that has a hole adds another, and a pick on a hole takes it out)
- `hole_snaps` (default false: a Single hole snaps to its face's middle and round edges' centres)
- `home_fit_size` (0.1…1, default 1: how much of the view the model fills when Home, Fit, an opened design or a navigator view change frames it, with `navigator_fit` on or off; 1 fills the room between the panels, the compass and the navigation bar, as before; #10091)
- `joint_drag_trackball` (default true: dragging a part that a ball joint holds to a part fixed in place turns it by the smallest turn about the joint, like a trackball the size of the part; false: the grabbed point follows the pointer and the part may spin about it, as before #13262)
- `joint_rotation_coarse` (default 10°): coarse Assembly Move rotation step, in 0.001…180°.
- `joint_rotation_fine` (default 0.1°): fine Assembly Move rotation step, in 0.001…180°.
- `keep_hover_while_navigating` (default true: the face under the pointer keeps its highlight while the view moves under it; off: the highlight goes off while the view moves, as before, #2130)
- `keep_unsaved_also_asks` (default true; web app, with `keep_unsaved_in_browser` on: the browser's "leave site?" box still appears when the tab closes with unsaved work; off, it appears only while a copy isn't current yet, #10220)
- `keep_unsaved_in_browser` (default false; web app: each open design's unsaved changes are kept current in the browser's private storage, and the next visit offers them back in a "Restore unsaved design?" window with Restore, Open saved version and Discard; off, nothing is kept, #10220)
- `keep_work_on_tab_switch` (default false: each design tab keeps its open dialog or tool, with what was picked and typed in it, and its toolbar tab while another design is shown, and has them back when it is shown again; off, a switch cancels them and shows MODEL, #9360)
- `language` (`en`, `ar`, `ja`, `de`, `fr`, `he`, `ko`, `ms`, `ru`, `uk`, `zh`; see [localization](localization.md))
- `length_decimals`
- `line_drag_draws_one` (default false: in the Line tool, a press, a drag and the release draw one line from the press to the release and end there, snapping as clicks do; clicks still chain, and a drag under 24 points is a click; off, a drag places a point where it is let go, #7247)
- `line_hover_tangent_arc` (default false: in the Line tool, after a segment, moving away from the chain's last point and resting back on it for about a third of a second makes the next click a tangent arc, and the same again goes back to lines; off, resting there does nothing, #9489)
- `logo_accent` (bool, default false: the application bar's logo in shades of the theme's accent instead of the theme's logo colours)
- `look_at_anim`
- `look_at_speed`
- `max_design_file_mb` (default 256): the largest design file opened, in MB, from the default up to 16384 (16 GB) (Preferences › General › Largest file to open, #4086; a `--max-file-size` launch flag overrides both for that run; the web app keeps the defaults and refuses other values; [large files](book/src/guide/files.md#large-files))
- `max_step_file_mb` (default 512): the largest STEP or import file opened, in MB, from the default up to 16384 (16 GB) (Preferences › General › Largest file to open, #4086; a `--max-file-size` launch flag overrides both for that run; the web app keeps the defaults and refuses other values; [large files](book/src/guide/files.md#large-files))
- `maya_horizontal_zoom`: Maya Alt+right drag zooms horizontally (right in, left out), default `true`; `false` restores vertical drag zoom. Other presets and the fixed Zoom tool are unchanged.
- `mcp_compact` (default false): compact tool output for agents bridged to this app through the MCP server (Preferences › AI › Agents through MCP; the server reads it with `mcp.options`; [MCP compact output](mcp.md#compact-output))
- `middle_double_click` (`home` (default), `fit`, `off`)
- `msaa`
- `nav` (`fusion`, `solidworks`, `creo`, `inventor`, `alias`, `maya`, `blender`, `tinkercad`, `openscad`, `touchpad`)
- `nav_bridge_enabled` (default false) and `nav_bridge_port` (default 8765: a device's own app on this computer moves the view, [the navigation bridge](3d-mouse.md#device-apps-the-navigation-bridge))
- `navigator_arrows` (default true: the custom view navigator has the cube's orbit and roll arrows around its body; also `view.navigator {arrows}`)
- `navigator_fit` (default false: a view chosen on the view cube or the axis gizmo also looks at the middle of the model from the one distance that fits its bounding sphere in the free part of the view, at `home_fit_size`; Home and an opened design use the same zoom and centre while it is on, so views chosen after them only turn; #10091)
- `navigator_home` (default true: the custom view navigator has the cube's Home button beside it; also `view.navigator {home}`)
- `navigator_style` (`axes` (default), `cube`, `custom`: the body chosen with `view.navigator`, the cube until one is chosen)
- `navigator_texture` (`themed` (default): the custom view navigator in the cube's colours; `modeled`: in its body's own appearance and edge colour)
- `offer_stand_up`
- `orbit_around` (`view_centre` (default: what Pan moves and Fit frames), `origin` (the world origin stays still on screen), `cursor` (the model point under the pointer when the drag began); the older `orbit_cursor` boolean still sets `cursor` when true)
- `orbit_cursor_wrap` (default true: an orbit drag that reaches the edge of the view moves the cursor to the opposite edge and keeps turning)
- `orbit_reverse` (default false: Shift with two-finger scrolling orbits the other way)
- `origin_axes_across_view` (default false: short axes as before; opt in through Preferences → Design → Origin or the bottom bar's Display menu. The command `view.origin_axes_across_view {enabled: bool}` sets the same preference. When true, the X, Y and Z origin axes run through the origin to the edges of the view, both ways, #11123; away from the origin they are picked only in an open sketch or by a dialog input that takes axes, after anything else under the pointer, and not where a body hides them; false: short axes from the origin, solid then dashed, as before)
- `origin_plane_names` (`xyz` (default) or `top_front_right`: the browser, dialogs and lists call the origin planes Top, Front and Right; their ids, commands and saved designs keep XY, XZ and YZ)
- `pan_reverse_x` (default false: horizontal scrolling pans the other way)
- `pan_reverse_y` (default false: vertical scrolling pans the other way)
- `parameters_apply_live` (default true: the Parameters dialog applies an expression when its field is left; false: edits wait for Apply or OK, which set them together in one rebuild and one undo step)
- `perspective_fov` (Perspective view's vertical field of view in radians, 0.2618…1.5708 (15°…90°), default 0.6; when it changes, the camera's distance scales so the view keeps its size)
- `placement_handles` (default false: handles on the preview of a Rest, Boss, Snap Fit, Box, Cylinder, Cone, Sphere, Torus or Coil that move it across the model and turn it where it can turn)
- `print_face_highlight` (0…1, default 0.2: how strongly a print face is tinted and outlined in the view, #8429; 0 draws no highlight)
- `quick_measure` (default false: the selection's length, area or distance in the view's bottom right corner, the rect `measure:quick`; #7000)
- `remember_stand_up` (default true: reuse the stand-up or Leave it answer per SHA-256 of the original file bytes, at most 500 answers with oldest-first eviction; false restores asking on every open)
- `render_legacy_memory_admission` (Graphics, default false): Use previous render memory checks. True skips native available-RAM/cgroup fit refusals for future CPU/GPU admissions; shared/host software limits, device limits, checked allocation and all held reservations remain enforced. It is a machine preference, not saved image quality. The CLI equivalent is `solidcraft-cli render --legacy-memory-admission`.
- `reuse_imports` (default true: reopen unchanged STEP and IGES files using the local saved-model cache; false imports each time). `doc.open {reuse_import: false}` bypasses reading and writing this cache for one open; its reply includes `import_reused` and `content_sha256`. `file.import_cache_clear {}` removes cached imports and returns `{removed}`, keeping stand-up answers. `SOLIDCRAFT_IMPORT_CACHE_DIR` overrides the default `imports` folder under SolidCraft's per-user data folder; `SOLIDCRAFT_NO_MODEL_CACHE` also bypasses import reuse.
- `ribbon_menus_follow_hover` (default false). Click a ribbon panel label to open its menu. Hover another panel label to switch, or move outside the ribbon and open menu to close it.
- `ribbon_reveal` (default false: hovering a result in Search commands switches the ribbon to the command's tab and marks its button, or opens the panel drop-down that holds it on its row, handle `ribbon:reveal`; leaving the results goes back to the tab before; a command started from the search blinks there, and a running command from a drop-down marks its panel label, `ribbon:running_section:<TAB>:<SECTION>`, #7287)
- `right_drag_orbits` (default true; false: a right drag the mouse preset would orbit leaves the camera alone, #6492)
- `save_prompt_one_at_a_time` (default false: closing several tabs or quitting with more than one unsaved design asks once, in one "Save changes to N designs?" list; true restores one "Save changes?" prompt per design, #11680)
- `select_hidden_edges` (default false: v0.30.1 visible-only edge picking; true allows ordinary edge picks through faces in Shaded with hidden edges, including Fillet inputs; hidden bodies and section-clipped edges stay excluded)
- `select_other_hidden` (default true: Select Other includes hidden geometry near the pointer; false restores the original visible-only list under the pointer)
- `show_frame_rate` (default false: frames per second in the view's bottom-right corner, counting the frames the view draws anyway; it reads 0 FPS once the view is idle)
- `sign_saved_designs` (default false: every save from the app adds a GnuPG OpenPGP signature to the design's provenance; `file.save`'s `sign` is accepted only from the app's own Save, because a script or agent must not make GnuPG sign with the user's keyring; the key may be on a smartcard, where GnuPG uses it and asks for its PIN; [signing](formats/cose-provenance.md))
- `signing_key` (string, default empty: the GnuPG key a signed save uses, a fingerprint or user id; empty asks GnuPG for its default signing key)
- `sketch_auto_dimensions` (default false: each shape drawn in a sketch gets all its dimensions at the sizes drawn, as `sketch.draw.dimensioned` with `as_drawn`; typed values win)
- `sketch_auto_locate` (default false; with `sketch_auto_dimensions`, new shapes are also placed from the sketch origin until fully constrained, as `sketch.draw.dimensioned` with `locate`)
- `sketch_conflict_colour` (default true: while the open sketch's constraints conflict, the curves and points a failing constraint holds are drawn in the theme's `sketch_conflict` colour too, not only its glyph or dimension; false: only the glyphs and dimensions are red, in `error`, as before, #7244)
- `sketch_construction_lines` (default true: show the Construction Line tool family in Sketch CREATE; false restores the earlier menu)
- `sketch_exit_view` (`keep` (default: the view stays where it is when a sketch ends), `previous` (back to the view from before the sketch), `unless_rotated` (back unless the sketch view was turned))
- `sketch_mirror_auto_line` (default true: opening Sketch Mirror uses the only preselected straight construction line of the active sketch as its reference; unselected lines do not count; false keeps manual reference selection)
- `sketch_open_look_at` (default true: creating or editing a sketch turns the view to face it; false leaves the view as it is, and Look At faces the sketch when asked)
- `sketch_recent_snap` (default false: while drawing, the cursor snaps where the horizontal guide of one recent circle or arc centre or straight-edge midpoint meets the vertical guide of another; a free coordinate, with no constraint and no construction geometry. Off: those guides stay visual and the cursor does not move. The references come from `sketch_tracking_guides`, #8694)
- `sketch_snap_to_visible_grid` (default true: a point in a sketch snaps to the crossings of the lines the grid draws when the pointer means one, and to the finer lattice between them otherwise; false: the point snaps to the finer lattice only, as before, #7114)
- `sketch_state_icons` (default true: a sketch's icon in History and the browser takes the colour of its solve state, `sketch` under constrained, `sketch_fixed` fully constrained, `error` while its constraints conflict, and its tooltip names the state; false: every sketch row's icon and tooltip as before, #7245)
- `sketch_tab_always` (default false: the SKETCH tab stays in the ribbon right after MODEL while no sketch is open; a tool in its CREATE or REFERENCE section then opens Create Sketch, on the selected plane or planar face or else on the plane picked next, and starts in the new sketch, #7904)
- `smooth_wheel_zoom` (default true)
- `space_mouse_model_pivot` (default false: Classic orbit around the view's centre; true opts into Traced orbit around the model for the 3D mouse and navigation bridge)
- `space_mouse_sketch_orbit` (default false: the 3D mouse and navigation bridge pan and zoom a sketch seen square on and turn it only after the view is tilted; true lets them turn it too, as the mouse can)
- `stand_up_unknown` (`note` (default: a file whose header names no program known to save Y up and whose bodies seem to lie on their back (their flat area across Y outweighs their flat area facing down Z) gets a small note over the view, "Did this part open the wrong way up?", with Stand it up and Don't ask again as the handles `stand_up:stand` and `stand_up:never`, and its close (×) as `toast:stand_up:close` (also `stand_up:dismiss`); it goes by itself after 10 seconds and never blocks input), `ask` (the "Stand this part up?" dialog for every file from another program, SolidWorks too, without looking at how it rests, as before #7717), `off` (no offer); otherwise a SolidWorks file gets the dialog, or the note when its bodies clearly stand on Z already)
- `status_bar` (default false: a thin bar along the window's bottom edge, below History, handle `status:bar`: what is being edited (`status:editing`), the open sketch's state in its colour (`status:sketch`) and the design's units (`status:units`, a click opens the Units menu, whose rows run `document.units`); the sketch's state then leaves the view's top-left corner, #7243)
- `step_values` (default false: Up/Down and the wheel step a plain number in a focused value field)
- `tab_double_click_renames` (default true: a double-click on the selected design's tab edits its name in the tab, handle `tab:rename`; Enter or a click elsewhere keeps the name through `component.rename` with `component: 0`, Esc cancels, and a name left as it was or blank changes nothing; false: a double-click on a tab maximizes the window, as the bar's empty space does, as before)
- `theme` (`dark`, `light`, `system` or a theme file's id: a built-in one such as `gruvbox-dark`, or an imported one; the reply's `theme_ids` lists them)
- `theme_export: true` (the reply's `theme_file` is the active theme, including a followed file; themes with options retain their authored definitions, defaults and overlays, while older themes keep the complete snapshot shape; saved option values are excluded; see [colour themes](book/src/guide/ui-tour.md#colour-themes))
- `theme_follow` (desktop only, default `""`: a `solidcraft-theme/1` file's path to follow as the theme, reloaded within about a second whenever it changes and when the window gains the focus; the file is read at once, and a file that doesn't read keeps the last good theme; `""` stops following, as does setting `theme`; in the browser it is an error; the launch flag `--theme-file PATH` follows a file for one run without saving it; the reply's `theme_followed` is the path followed now or null, and `theme_follow_error` why its last read was not applied, or null)
- `theme_import` (a `solidcraft-theme/1` file's JSON or text: adds the theme and turns it on; the reply's `theme_imported` is its id)
- `theme_options` (default `{}`): a map of theme id to option-id/value map, merged per theme; `{"theme_options":{"my-theme":{"enabled":true,"amount":5}}}` changes those options, `null` resets one option, and an empty map resets that theme. Unknown themes or option ids, wrong types, unknown choices and out-of-range numbers refuse the entire call and name the key, before imports, removals, dialogs or other preference effects occur.
- `theme_remove` (an imported theme's id; also removes its saved option values)
- `timeline_origin_bodies` (default false: the History on the left or under the browser starts with the active component's Bodies folder and the Origin, the browser's rows with their eyes, folds and menus; the bottom History is unchanged)
- `toast_place` (`corner` (default: notices and offers stack in one column in the view's bottom-right corner, each 360 points wide, buttons right-aligned with the main action right-most, #7893), `centre` (at the bottom centre above the view toolbar, each as wide as its text, as before))
- `trackpad_pan` (default true: two-finger scroll pans, except Touchpad always zooms; pinch zooms)
- `trackpad_zoom_modifier` (`off` default, `cmd`, `alt`, or `ctrl`: hold this key alone for vertical scroll to zoom)
- `trusted_signing_keys` (string, default empty: fingerprints of signing keys whose signatures the app shows as verified, separated by commas; spaces inside a fingerprint, as `gpg --fingerprint` prints them, are ignored; any other valid signature is shown in grey with its fingerprint and names no user id; a user id inside a file is not proof)
- `ui_font` (an installed font family or a font file for the interface; empty for the built-in font; a `Family-Weight.ttf` file also takes its heavier sibling files for bold text, see [interface fonts](fonts.md))
- `ui_scale`
- `up_axis` (`z`, `y`)
- `verify_opened_designs` (default false: opening a design verifies its signature and reports it, `doc.open {verify: true}`; off: opening does not read every payload again, and `file.verify` checks on request)
- `view_confirm_corner` (default false: while a dialog or a sketch is open, a check and a cross at the view's top right, handles `view:confirm:ok` and `view:confirm:cancel`, do what the dialog's OK and Cancel do, or Finish Sketch and Cancel Sketch; the axis view or cube moves down below them, #6709)
- `viewport_theme` (`theme` (default: the theme's own), `light`, `dark`: the 3D view in the Light or Dark theme's colours whatever the window uses, #4146)
- `wheel_zoom_speed` (0.25 to 4, default 1: how far each wheel step zooms; the 3D mouse keeps its own speed)
- `window_system` (Linux: `auto` (default: XWayland whenever there is an X display, native Wayland on niri and Hyprland), `wayland`, `x11`; saved in `graphics.json`, applies at the next start; `SOLIDCRAFT_WAYLAND` overrides it)
- `zoom_reverse`
- `zoom_stop_at_model` (default false: the 3D mouse and navigation bridge stop zoom at the surface in the middle of the view; drag-zoom holds its press point; independent of `zoom_to_cursor`)
- `zoom_to_cursor`

The built-in `gruvbox-dark` and `gruvbox-light` themes each expose the `contrast`
choice (`soft`, `standard` default, `hard`). For example:
`{"theme":"gruvbox-dark","theme_options":{"gruvbox-dark":{"contrast":"soft"}}}`.
The former `gruvbox-dark-soft`, `gruvbox-dark-hard`, `gruvbox-light-soft` and
`gruvbox-light-hard` ids remain accepted for compatibility and select the matching
family and contrast; replies and `theme_ids` expose only the two canonical families.
An explicit canonical option change or reset in the same call wins over the legacy
selection. Legacy option-map keys are also accepted; canonical keys take precedence
when both occur in a call. Imported themes cannot claim the retired built-in ids.

Theme option values persist per theme, including a followed file's own id, through the same
preference store in the browser and on desktop. Switching themes restores their saved values;
Interface's Restore page defaults clears them. Preference exports and settings profiles carry values;
`theme_export` carries the author's defaults. Saved entries that no longer fit a definition remain
stored and use the default with a notice; unknown ids have a notice and no effective value.

For the active theme, `theme_option_defs` lists the declared options in `ordered_options` order:
`id`, `type`, `default`, type-specific `min`, `max`, `step`, `unit` or choice `values` (each with
`id` and `label`), `parent`, `advanced`, `section`, `label`, `help`, `standard` and `available`.
Labels, help and choice labels use the current language with English fallback. Unavailable
definitions include `reason`; standard options currently have `available: false` and
`reason: "not implemented"`. `theme_option_values` contains the effective values, including
defaults and values of inactive children. `theme_option_notices` contains `{id, kind}` entries,
where `kind` is `unknown_option`, `wrong_type`, `unknown_choice`, `non_finite` or `out_of_range`.
Without an active theme file these fields are `[]`, `{}` and `[]`. Portable input
and output schema types live in `solidcraft_engine::schema::ui_prefs` (`PrefsParams`, `PrefsReply`).

Other `ui.prefs` parameters:

- `open` opens Preferences at General; Look: Appearance, Layout, Graphics; Work: Design, Sketch, Tools; Input: Navigation, 3D mouse, Shortcuts; or Extend: Assistant, Plugins. Names are case-insensitive. Legacy aliases remain accepted: Interface → Appearance, Interface/navigation bar → Layout, Keyboard Shortcuts → Shortcuts, AI → Assistant, and Selection, Sketch & selection or Sketch and selection → Sketch. Replies use the canonical page name. The corresponding legacy `prefs:page:<name>` scenario handles also remain aliases of the canonical page rows.
- `button`: `ok`, `cancel`, `apply`, `defaults`
- `stand_up: true` stands up a just-opened Y-up file, `stand_up: false` leaves it as it is ("Leave it": the offer, dialog or note, closes and `offer_stand_up` stays on)
- the dialog's buttons are the handles `stand_up:stand`, `stand_up:leave` and `stand_up:never`

`ui.workspace {workspace?: "design" | "render"}` reads or changes the active workspace.
Entering Render requires finishing the current modelling operation; returning restores
the Design camera. See [the integrated Render workspace](render-workspace.md).

`ui.inspect.render.live` reads automatic image feedback without launching or pumping
work. It includes enabled/paused, pending/running, capture generation, isolated job,
completed passes, target, sequence, current/stale image and error. These job
identities belong to the live scratch session and cannot be exported or selected as
completed results through the original document session. `view.render_live {paused:
true|false}` pauses/resumes feedback through the registered command; its headless
form validates and echoes the request without changing a document.

GPU adaptive checkpoints include optional `preview.adaptive` metadata in
`render.status` and `ui.inspect.render.live.adaptive`: `stage` is `pilot` or
`production`, `completed` and `target` count that phase's iterations, and
`pilot_samples`, `minimum_production_samples`, `maximum_production_samples` retain
the captured allocation bounds. The compatibility field `samples_per_pixel` is an
absolute completed pilot-plus-production iteration when this metadata is present;
it represents uniform SPP only when `adaptive` is absent. The checkpoint target is
the validated pilot count plus maximum production count. Pilot images never enter
the final image or raw master, and production iteration counts are not uniform
per-pixel sample counts.

### Sketch alignment guides

`ui.prefs {sketch_tracking_guides: true}` enables temporary horizontal and vertical dotted
alignment guides from hovered circle/arc centres and straight-edge midpoints (default true).
The guide spans the reference and cursor projection, including positions inside a circle.
The two most recent references are kept, and both guides can be drawn. A single guide does
not move the cursor. Set the preference to `false` to hide the guides. Snap suppression
(Alt, unless rebound) does the same for that gesture. Snapping the cursor to where those
guides cross is separate, and off unless `sketch_recent_snap` is true. While a drawing tool
waits for a point, `sketch_tracking_points` (default true) makes every shown sketch point
other than the origin a reference too (line and arc ends, rectangle corners, free points);
`false` keeps centres and midpoints only.

### Continuous camera motion

Continuous camera motion is also available through `engine.execute` with command
`view.navigate`: pass `camera` (from `ui.inspect`) and `motion: "pan" | "orbit" | "zoom"`.
Pan and orbit take `delta: [dx,dy]` in screen points (each within ±10000); pan also requires
`viewport: [width,height]` (each 1..1000000). Orbit accepts a world-space `pivot` with the
viewport size. Zoom takes `factor` (0.05..20, below 1 zooms in) and an optional world-space
`anchor`. The returned `camera` is applied immediately by the UI, without animation,
document edits, undo steps or a success-log entry per movement.

## MCP

`solidcraft-cli mcp --connect 127.0.0.1:PORT` bridges an MCP client (an AI agent) to this
channel; see [mcp.md](mcp.md).

## Example

```text
{"id":1,"method":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}}
{"id":2,"method":"engine.execute","params":{"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[40,30]}}}
{"id":3,"method":"engine.execute","params":{"command":"solid.extrude","params":{"distance":20}}}
{"id":4,"method":"engine.execute","params":{"command":"solid.fillet","params":{"edges":[[0,0,10]],"radius":3}}}
{"id":5,"method":"ui.screenshot","params":{"path":"/tmp/part.png"}}
```

Preferences → Navigation → View navigator offers **Signed axes** (the original default),
**Cube** and **Custom**; a right-click anywhere on the navigator offers the same three. This choice is
saved independently of the mouse preset. The control channel selects it with
`ui.prefs {"navigator_style":"axes"}`, `"cube"` or `"custom"`. Existing settings without this field
keep signed axes. Signed-axis tips choose principal views; clicking an aligned tip again reverses
the view.

Custom draws a body in place of the cube, at the cube's size; a click on one of its faces looks
straight at it, and with `navigator_fit` on it also frames the model, as the cube does. Picking
Custom from the navigator's menu opens the Custom View Navigator panel, with OK and Cancel: pick
the body in the view, then its texture (`themed`: the cube's colours; `modeled`: the body's own
appearance and the view's edge colour) and whether the cube's rotation arrows and Home button go
around it. The navigator previews the panel's choices while it is open. OK runs
`view.navigator`, which scripts and `engine.execute` run too without opening anything:
`{"style": "custom", "body": "Body1", "texture": "modeled", "arrows": true, "home": false}` (any
subset; `"body": null` goes back to the cube) returns the whole navigator. A body over 10,000
triangles is refused, and so is a long or flat one: measured across the axes and the diagonals,
its thinnest width must be at least a fifth of its widest. The body is kept as a snapshot with the
preferences and in settings profiles (`settings.profile.*`, as `view_gizmo`), so it stays across
runs and designs and travels with an exported profile.

Hold Shift during an orbit gesture to snap to the nearest standard or diagonal view. This applies to
right-drag, the Orbit navigation tool, and dragging either view navigator. Under the default preset,
Shift+middle also orbits with snapping; middle alone pans. Cube faces choose principal views,
and corners choose isometric views. The middle of each cube edge belongs to its face. Face names
follow the up-axis preference; the small X/Y/Z triad shows world axes. Clicking the same face keeps that view.
The inward-pointing arrows orbit by 90 degrees; the curved arrows at the upper right roll the
view by 90 degrees. The Home button at the upper left returns to isometric and the lower-right circle
fits the model.

The cube is subdued while idle. Hovering its area brightens the cube and navigation bar and
reveals the surrounding controls; the picked face or corner gets a pale blue fill and
blue outline. Moving away restores the subdued state. Navigation-bar controls also brighten
when hovered or when a navigation tool is active.

The cube has six face targets and eight corner targets, with no edge targets. Corner picking
regions lie on the six flat faces, preserving sharp corners with no permanent subdivision
lines. Hovering a corner highlights its parts on the adjacent visible faces together.

The cube widget occupies a compact 112-point square in the viewport corner. A straight-on
cube face is approximately 55 points wide; painting and picking scale together. Face labels
are stretched vertically. View clicks ease through intermediate orientations when Navigation
animated view changes is enabled (the default), at the configured animation speed.

The cube runs `view.navigate` through the command engine. Programmatic callers pass `camera`
and either `direction: [x,y,z]` in the camera's up-axis frame or `action` (`left`, `right`, `up`,
`down`, `clockwise`, `counterclockwise`, `home`, `fit`). `fit: true` with a `direction` or a
turn asks for the whole model to be framed from the new direction as well, as `home` does. The
result contains `camera` and `fit`; UI callers apply the camera with their animation
preference, without editing the design. The Navigation preference `navigator_fit` (off by
default) makes every view chosen on the cube or on the axis gizmo frame the model this way:
looking at its middle from the one distance that fits its bounding sphere, which Home then shares.

Shift-orbit snapping includes 45-degree edge views and isometric corner views, with a
lightly damped spring transition and a small boundary margin to prevent flicker.

### Command search

`ui.palette {text?, open?}` opens command search under the ribbon search field; `open: false`
closes it. `ui.start {command: "ui.palette"}`, clicking `ribbon:search`, and S open the same
popup. This is UI routing: `engine.execute` continues to run commands without opening dialogs.
The query is limited to 4096 Unicode characters, with at most 80 visible results.

`shortcut:search` and `palette:search` identify the editor; `ribbon:search_popup` and
`palette:outer` identify the popup. Command rows retain `menu:<command id>`; design rows use
`menu:search:design:<index>`. Up/Down choose an enabled row, Enter runs it, Tab fills its id or
name, and Esc closes. Typing `command.id {"parameter": value}` validates JSON live and runs
through the same engine entry as `engine.execute`. Shift+Enter runs JSON and keeps the query
open. Invalid JSON stays editable and reports the parser's error. Feature results open their
existing edit tools; parameter results open Change Parameters filtered to that name.

### Saving several designs before closing

`ui.documents {"action":"close_all"}` (or `close_others` with the tab `index` to keep,
or `quit`) returns one `save_prompt` list when more than one target design has unsaved
changes. Only tabs in the addressed window participate. Each row starts selected.
Read with `ui.documents {}`; change all ticks and answer in one call:
`ui.documents {"save_indices":[0,2],"answer":"save"}`. Indices must belong to the
pending list; invalid lists return an error without answering. An empty list saves
nothing and still closes every target, discarding their unsaved changes. Without `save_indices`, `answer:"save"` saves the current ticks.

Answer an individual row with `ui.documents {"save_row":0}` or
`ui.documents {"discard_row":0}`, using its current tab `index` from `save_prompt`.
Save asks for a location if needed and closes only that tab after a successful write;
a cancelled picker or failed save leaves the row and prompt unchanged. Discard closes
only that tab without saving. Remaining tab indices shift after a tab closes, so read
the returned list before answering another row. The grouped prompt stays open even
with one row left; answering the last row completes the pending close or quit.

Selected designs save in tab order through the existing interactive save flow;
never-saved designs ask for a location. Cancelling a picker or failing a save stops
the close and keeps every remaining tab open, including tabs already saved successfully.
`answer:"dont_save"` discards all target changes and completes the close;
`answer:"cancel"` keeps every remaining tab open. This is an explicit UI workflow;
`engine.execute` save commands continue to require paths and never open pickers.
With `ui.prefs {"save_prompt_one_at_a_time":true}`, the previous individual prompts
are used instead (default: false). One unsaved target always uses the original prompt.
