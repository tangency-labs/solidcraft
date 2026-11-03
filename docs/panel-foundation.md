# Universal panel foundation

The public foundation is `solidcraft_engine::panels` and
`solidcraft_ui_egui::panel_runtime`. It establishes contracts and executable
synthetic fixtures for #11302. The current shell and defaults remain in use.
No panel commands, preferences, content migrations, native windows or docking
gestures are enabled by this change. Classic remains a permanent compatibility
preset. Production adapters land in the separate children of #11218.

## Ownership and integration boundaries

| Owner | Public contract | Responsibility |
| --- | --- | --- |
| Foundation / engine | `PanelRegistry`, `PanelSpec`, `Layout`, `LayoutService`, `ActionSink` | Stable identities, bounded wire values, constraints, atomic actions and inspection. No egui or Session dependency. |
| Foundation / UI | `PanelRuntime`, `ControllerBinding`, `PanelController`, `RuntimeActionSink` | Retained state, ownership, lifecycle preflight, live availability, independent service ticks. |
| Host child | `PanelHost`, `PanelContext`, `ResolvedLayout`, `EguiPanelHost` | Geometry, host composition and later docking interactions. Existing `ui_kit::Container` remains the outer boundary. |
| Command child | `LayoutAction`, `LayoutReply`, `PanelError`, `LayoutTarget` | Register schemas and route GUI/agent requests to the live runtime sink. Do not construct a second service from a saved snapshot. |
| Persistence child | `LayoutEnvelope`, `LayoutPersistence`, `decode`, `encode` | Workspace preference key, atomic storage, presets and future migrations. Keep unrelated preferences. |
| Native child | `NativeHostAdapter`, `HostEffect`, `HostCapabilities` | Preflight resources, native host realization, focus and redock. No native implementation is included here. |
| Surface/accessibility children | `InputRegion`, `ControlId`, `Direction`, `Motion`, `ContrastOutcome` | Painted/input bounds, semantic roles, keyboard routing, mirrored edges, reduced motion and validated theme surfaces. |
| Content children | `PanelModels`, `ContentCommandSink`, `PanelLocalServices`, content slots | Model access, explicit content commands, controllers, owner-scoped services and independently scrolling content. |

Panel kind, instance, section, workspace, document, viewport, native host and
semantic control IDs are different Rust types. Their wire values are bounded
lowercase identifiers. Registry specs and serializable layout never contain
callbacks, document state, egui state or live editors. Only `allowed_zones`,
`edge_rule` and `surface` express placement exceptions. Normal capabilities
(close, tabs, collapse, popout, multiple instances) are shared feature flags.

Layout belongs to the application/workspace; it never enters a design file,
Session revision, dirty flag, undo stack, evaluator or tessellation cache.
`ContentOwner` independently chooses application, workspace, document or window
ownership. A native reparent does not change content ownership. Document switching
retains existing controllers and creates only missing owner/instance bindings.
Complete layout application changes the live workspace without replacing the
runtime or service. Application/document/window controllers keep matching instance
identities; workspace controllers remain under their original workspace owner and
continue ticking while hidden. Switching back reuses those controllers and drafts.
All retained controllers receive service updates even when closed, unavailable,
inactive, collapsed or on another document. Providers must expose models for
retained owners, not redirect old-document reads to the current document.

`SolveApp::enable_panel_runtime` accepts a runtime and `AppPanelServices` explicitly.
Its optional field defaults to `None`, remains on the app across document tabs,
and is ticked from `SolveApp::logic` independently of presentation. Refresh receives
only `&Session`. `disable_panel_runtime` preflights cleanup. Tick failures remain
inspectable on `AppPanelRuntime::last_error`; they do not inject untranslated UI.
This is an internal adapter seam, not a preference, environment switch or control
method. `dispatch_panel_layout`, `mount_panels`, `panel_content_action` and
`tick_panel_runtime` forward through the same injected provider. `AppPanelRuntime`
also exposes these operations and `with_services` for scoped integration access;
adapters do not need a shadow provider to borrow runtime and services together.

## Actions and atomicity

`ActionSink::dispatch` accepts `Inspect`, `Apply { expected_revision, mutations }`
or `Replace { expected_revision, layout }`.
GUI, keyboard and agent adapters share this reducer. `RuntimeActionSink` adds
controller/native lifecycle guards around the same engine service. Runtime users
receive immutable layout inspection; there is no mutable bare-service escape.
Replies describe committed layout, revision, availability, interaction, capabilities
and host effects. Layout revision is independent of document revision.

`Replace` validates a complete preset/saved layout and atomically changes the
workspace on the same authoritative service/revision. It stages availability,
controller factories and native create/rebind/release effects before committing.
Omitted entries and entries marked closed are noncanceling hides: their retained
controllers keep drafts, conversations and service updates, with no collapsed rail
unless the snapshot actually requests one. Restoring the snapshot restores those
controllers. Instance IDs cannot change kind across workspace switches; the service
retains a bounded identity catalogue until explicit Destroy releases an identity
and all of its retained owners. Unknown kinds fail atomically; persistence
adapters first decode/quarantine unknown data and apply the known layout portion.
Registered kinds with missing bindings or unavailable context remain dormant.
After successful replacement, old local input/overlay ownership is released;
failure leaves environment, revision, controllers and local/native hosts intact.
Use this operation for workspace switching rather than rebuilding a runtime or
synthesizing Close/Open batches. `set_environment` changes document/window/viewport
context within the already committed workspace.

- `Open` targets kind **and** instance. New instances require an explicit destination;
  an existing closed instance can reopen from its remembered placement and size.
  Repeating an open is idempotent unless a new placement is supplied; use `Move`.
- `NewSection` and `JoinTab` are distinct destinations. Section insertion indexes
  address the global ordered section list **after** detachment; tab indexes address
  the destination list after detachment. Both reject invalid indexes. Empty groups
  are removed. Joining retains the group's active tab, dimensions and collapse state.
- `Activate` selects an already-open tab, without requesting keyboard focus or
  uncollapsing it. `Focus` can reopen retained closed state, select its tab,
  uncollapse it and request host focus. `Peek` temporarily presents a collapsed
  tab, without changing persisted size or collapse. Dismiss is transient too.
- `ResizePair` requires adjacent sections of one dock, conserves their split-axis
  extent and changes no cross-axis dimension. `ResetPair` returns the pair to
  the ratio of active members' registered defaults within intersected bounds.
  `SetSize` is a typed explicit outer-size mutation.
- `Close` retains placement/size memory. `Destroy` removes an already-closed
  instance and its retained controllers across workspaces/owners. `NativeClose` redocks the whole section
  to its recorded return destination, optionally rejoining a stable former tab
  group. Missing/reparented groups use the recorded dock fallback; valid joins
  retain target activation. It does not close/cancel the content.

A transaction is prepared on a bounded copy and validated before commit. Rejected
transactions preserve layout, revision, interaction, retained controllers and hosts.
Lifecycle preflight is side-effect-free. It checks explicit Close/Reopen/Reparent/
Destroy events before committing; applying those prepared events is infallible.
State notifications are observational and cannot veto a mutation. Factories stage
fresh controllers without publishing external effects, since staging can be
discarded. Native preflight similarly must establish that commit can finish.
Adapters must reserve/stage fallible work, not discover a failure after commit.
Only explicit content actions may Apply/Finish/Cancel/Stop; hiding, tab changes,
collapse, reparent and native redock never implicitly invoke them. Explicit close
cancels transactional controllers for the affected instance across retained owners;
one cancellation refusal rejects the entire batch.
Close or Destroy combined with any other instance-targeted operation on the same
instance in a batch is rejected with `Lifecycle` before mutations/effects. This
includes Close/Open, Close/Move/Open and Destroy/Open; independent instance and
section mutations can still be batched. Explicit Close cancels a live hidden
transaction even if its saved entry was already marked closed by `Replace`.

`ControllerState` distinguishes requested-open, available, presented, activated
and destroyed. Presented requires usable resolved geometry; activated additionally
requires the selected tab/peek. Keyboard focus is separate in `PanelContext`.
`destroy_owner` performs explicit cleanup without editing layout, leaves a bounded
tombstone, and requires `revive_owner` to create fresh content again. `shutdown`
preflights all owners before clearing any and makes the runtime terminal.

Independent updates attempt every retained controller even when staging or another
controller fails. `update_errors` aggregates at most two bounded typed errors per
controller plus staging/reconciliation errors (258 total). Invalid availability
reports become `Availability` failures. Failing content is unavailable, loses stale
presentation/input ownership and cannot become healthy merely by moving it; a
successful later update restores availability. Reconciliation still runs before
the aggregate failure is returned. Injected-provider refresh failures also do not
skip the independent controller update loop.

## Final product decisions

- Zones are logical Left, Right, Bottom, Floating and Window. Top remains reserved
  for ribbon/application chrome. RTL mirrors the physical side; persisted IDs and
  tab order remain logical.
- `PinOuter` follows whichever logical side the panel occupies, requires its section
  first on that edge and excludes Bottom in registry data. It is inactive in free/
  native hosts. Competing pinned **instances**, including in one group, are rejected;
  no existing panel is silently displaced.
- Tab groups use the intersection of every member's minimum and maximum **width
  and height**. Impossible joins and dimensions outside that intersection fail.
- min/default/max are outer logical dimensions. Geometry reports actual outer and
  usable body bounds separately. egui applies scale once. Free/native flow starts
  Wide at aspect >= 1, otherwise Tall; Tall switches above 1.1 and Wide below 0.9.
  Size classes use the shorter usable edge: Compact < 240, Regular 240..480,
  Wide > 480. Invalid/nonfinite/oversized dimensions are rejected.
- Capacity shortages produce transient unpresented/overflow records, never reduced
  below-minimum content or saved-dimension/collapse rewrites. Collapsed rails are
  chrome. Hosts expose overflow access and retry presentation when capacity returns.
- New Open/Focus/Activate/Peek requests for unavailable content fail with an
  availability reason. Saved requested-open content remains dormant and reappears
  when its owner/context is available. Missing bindings are dormant too.
- Combined History-in-Browser and History Bodies/Origin stay content preferences
  or shared components. They do not merge or depend on panel host identity.
- Opaque surface requests win over Translucent, then zone defaults apply (bottom/
  native opaque, side/free translucent). Contrast resolution can raise background
  alpha only. If full opacity cannot make authored tokens readable, return
  `RetainReadableFallback` with a typed reason and keep the validated fallback/last
  good tokens and raw imported intent. No claim of 4.5:1 or text recolouring is
  permitted to disguise failure. Theme resolution is implemented by the surface child.

## Geometry and local services

`ResolvedPanel` contains outer/body geometry plus explicit paint and input regions.
Validation bounds regions and requires input inside paint; the viewport outside
these regions belongs to the existing camera/navigation path. Semantic controls
carry stable IDs, roles, accessible names and keyboard-focusability. Hosts may use
`consumes_pointer` to compose input ownership without a full-window shield.
The reference `resolve_geometry` is deterministic synthetic mounting geometry,
not production docking. It reports overflow instead of changing desired state.

`PanelContext` carries presentation geometry, owner/scope identities, flow, size
class, direction, focus and motion; services are a separate argument. Content
has Header/Body/Footer slots and chooses scrolling itself. `EguiPanelHost` uses
one kit Container, title/status/attention and semantic content actions; its action
strip can scroll independently of the body. Reduced motion resolves duration to
zero, without delaying identity/focus/lifecycle changes.
The reference egui adapter accepts exactly one full-outer paint rectangle and one
full-outer Surface input region. It rejects sparse, empty-input or partial-input
records with `Capability` before painting or creating widgets. Rich region-aware
surfaces belong to #11307. Every child clip intersects the existing parent clip.

Keyboard ownership, focus, file/pointer drops, overlays, clipboard and picking use
`PanelLocalServices` with explicit workspace/document/viewport scope. Native viewport
identity uses the native host's stable string in the viewport ID domain; platform
adapters map that identity to their actual viewport handle. Reparenting releases
old host-local focus/overlays/drop routing; the controller survives. Never route
a native service request through the root viewport or a different document.

## Persistence and bounds

Envelope format is `solidcraft.panel-layout`, envelope/layout versions are both 1.
`PANEL_API_VERSION` and each live reply's `api_version` are also 1.
Public schema functions describe actions, replies and envelopes. Serde/schema
values are **untrusted wire data**, not validity proofs. Use `decode_action`,
`decode`, `encode` and the reducer. The engine owns versions; persistence adapters
own atomic storage and migration policy, not a second layout model.

Limits: 64-byte IDs, 128 kinds/layout instances/retained instance identities,
64 sections,
32 tabs per section, 64 mutations per transaction, 262144-byte encoded envelope,
32768 logical units per positive size dimension, finite bounded coordinates.
Opaque JSON is limited to 8192 nodes and depth 32; availability reasons to 256
bytes. Region/control, service payload and retained-owner collections are bounded.
Encoding uses a capped writer, not an unbounded serialization followed by a check.
Runtime retention has separate limits: 32 document owners, 128 other owners,
128 controllers per owner, and 20,480 controllers overall. These limits preserve
the application's supported document count without evicting live drafts.

Unknown panel records and raw future fields are retained in bounded quarantine.
`section_snapshots` stores each raw section once, including future fields, desired
tab order and pending activation; quarantined/retained records refer to its ID.
The bounded `section_order` list ranks **all** saved sections, including dormant
sections and known survivors without snapshots. Restoration uses those ranks, not
insertion indexes clamped into an incomplete list, so instance/quarantine processing
order and staged registry reappearance cannot reorder the saved complete layout.
Known live ordering remains authoritative when edited: surviving visible entries
take their latest relative order while dormant gaps remain; newly added entries
anchor before their next surviving visible neighbour.

A retained snapshot from a fully restored episode is opaque metadata, not stale
layout authority. Before newly unknown members are removed on load, current saved
section semantics refresh its tabs, activation, placement, dimensions, collapse
state and rank. Existing opaque keys absent from typed live data are preserved;
current raw values for the same keys supply the latest saved intent. In an active
partial episode, still-dormant members remain in the complete order. Each snapshot's
optional `projected` section records the last materialized state: changed visible
activation overrides a pending choice, while an unchanged generated fallback does
not replace the desired dormant active member. Newly generated fallback selections
are recorded after quarantine filtering, so they cannot be mistaken for user edits.
The same merge runs before direct restoration after an in-memory live edit.

These v1 fields have serde defaults. Earlier v1 envelopes without `section_order`
derive missing section ranks from sorted snapshot indexes; missing projections
conservatively retain dormant activation until a materialized baseline is recorded.
Both the complete rank list and projected sections obey the existing section/tab/
ID limits, plus the envelope's global byte/node/depth bounds. Known layout starts
independently, and known survivors/extra current tabs remain intact during restores.
`restore_quarantine_with_availability` can defer unavailable entries; invalid bounds
or constraints also leave entries quarantined. Successful restoration preserves
raw instance fields in `retained_data` and shared raw section fields/order in
`section_snapshots`, even across later unregister/register cycles. Adapters consume
these explicitly. Restore stages the whole envelope and applies it only if encoding
fits global byte/node/depth caps; successful decode is guaranteed encodable by this
version. An unrepresentable lossless result returns fallback/recovery, not silent
field loss. Unknown corruption, invalid known layouts or future versions
return a validated preset fallback plus a once-reportable notice and bounded rejected
bytes for recovery. The adapter must preserve unrelated preference keys.

## Executable fixtures

`synthetic_registry`, `fixture_service`, `fixture_open`, `transaction`,
`FakePersistence` and `FIXTURE_LAYOUT_V1` are public engine fixtures. The v1 JSON
lives at `crates/engine/src/panels/fixtures/layout-v1.json`. It is authored contract
data, not a captured production design or a generated schema snapshot.

`PanelMountHarness` combines the actual runtime/action service with synthetic
application-owned content, document-owned transactional editor, workspace-owned multi-instance log/pin
controllers, fake models/command sink/local services/native adapter, fake mount
host and persistence adapter. `FixtureProbe` provides snapshots and bounded draft/
failure controls. `apply_layout` exercises saved-layout/workspace transitions through
the actual sink; `without_content` exercises dormant missing bindings. Fake host mounting invokes all three real controller slots with
an egui context; it is headless and does not establish visual correctness.

```rust
use solidcraft_engine::panels::*;
use solidcraft_ui_egui::panel_runtime::PanelMountHarness;

fn specimen() -> PanelResult<()> {
    let mut h = PanelMountHarness::new(false)?;
    h.dispatch(vec![fixture_open(
        "fixture.editor", "editor", "editor-section", Zone::Left, 0,
    )?])?;
    h.tick()?; // independent services, even with no host mounted
    h.mount(LogicalRect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size { width: 1200.0, height: 800.0 },
    })?;
    h.save()?;
    Ok(())
}
```

Engine and UI domain test modules contain hostile action sequences, bounds and
collection limits, exactly-once constraints, pins, atomic failures, incompatible
tabs, overflow restoration, availability, close/reopen, schema/envelope fixtures,
controller retention, hidden service updates, explicit editor actions, native
preflight/redock, scoped input, RTL/reduced motion and document/cache isolation.
