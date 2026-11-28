# solidcraft-panel-layout

Where an app's panels sit, as plain data. A host registers its panels by id, changes their
placement through one service (docked on the left, right or bottom, floating, or in a window of
their own; stacked, tabbed, collapsed, resized), asks for the rects to draw into, and keeps the
saved layout as bytes wherever it keeps its preferences. The crate knows no UI toolkit, no
document and no app command, and depends only on `serde`, `serde_json`, `schemars` and
`thiserror`. Every size and position is in logical units; the host applies its scale.

Input is hostile: ids, counts, sizes and the saved form are bounded and validated, and every
refusal is a `PanelError` with a machine-readable `Rule`.

## API

| Item | What it is |
|---|---|
| `PanelKind`, `InstanceId`, `SectionId`, `ColumnId`, `DocumentId`, `ViewportId`, `HostId`, `ControlId` | Validated ids (`new`, at most `MAX_ID_BYTES`; `valid_id`) |
| `Zone` | Where a section sits: `Left`, `Right`, `Bottom`, `Floating`, `Window` (the sides are logical, a host mirrors them for right-to-left) |
| `PanelSpec`, `PanelFeatures`, `HeaderActionSpec`, `EdgeRule`, `PanelSurface`, `SideHeightPolicy` | What a host registers for one kind of panel: title key, icon, allowed and default zones, least, default and greatest sizes, header actions |
| `WorkspaceId` | Flat host ids or `plugin:<package>/<workspace>` (`valid_workspace_id`, at most `MAX_WORKSPACE_ID_BYTES` = 136) |
| `PanelRegistry` | The registered kinds: `register`, `get`, `specs`, `set_side_widths`, `is_side_strip` |
| `Layout`, `Section`, `Column`, `PanelInstance`, `SectionLocation`, `PlacementMemory`, `DockReturn`, `ReturnTab` | The arrangement: open panels, the sections (tab groups) they sit in, side columns, and where a panel returns to. `Layout::empty(workspace)` starts one; `validate` checks one |
| `LayoutService` | The one owner of the live layout: `new(registry, layout, capabilities)`, `prepare` and `commit` (a validated candidate, then the change), `set_availability`, and the `ActionSink` trait's `dispatch`, `inspect` and `project` |
| `LayoutAction`, `Mutation`, `Destination`, `RelativePlacement` | What a caller asks for: `Inspect`, `Apply` a list of mutations (open, close, move, activate, collapse, resize, set a floating rect, ...) or `Replace` the whole layout |
| `LayoutReply`, `HostEffect`, `InteractionState`, `HostCapabilities` | What comes back: the layout, its revision, the focused and peeked panels, and the effects the host carries out (focus, a native window to create or release) |
| `Availability`, `AvailabilityMap`, `presented_tab` | Which open panels can be shown right now; live context, never saved |
| `ContextualPanelAction`, `LayoutTarget` | Keyboard actions on the focused panel, and stable names for a command's target |
| `resolve_geometry(service, capacity, direction, chrome)` | The rects of the current layout, from each panel's preferred size: `ResolvedLayout`, `ResolvedPanel` (outer, body, painted and input rects), `UnpresentedSection` |
| `allocate_layout(layout, registry, env)`, `allocation_measure_requests`, `effective_allocation_bounds`, `bottom_resize_height` | The allocator a product host uses with its measured content: `AllocationEnvironment`, `ContentMeasureRequest`, `SideContentMeasure`, `AllocatedGeometry`, `AllocatedSection` |
| `Point`, `Size`, `LogicalRect`, `Direction`, `PhysicalEdge`, `Flow`, `SizeClass`, `responsive`, `intersect_rect`, `InputRole`, `InputRegion` | The geometry values |
| `tab_bounds`, `section_collapsible`, `column_bounds`, `column_widths`, `resolve_column_width`, `normalize_column_widths`, `bottom_height_bounds`, `normalized_pair_weights`, `dock_stack_extent` | Size limits and shares a host reads while the user drags |
| `LayoutEnvelope`, `encode`, `decode`, `LoadOutcome` | The saved form: a layout to bounded JSON bytes (at most `MAX_ENVELOPE_BYTES`) and back. A panel kind the registry does not know is kept aside (`QuarantinedPanel`) and comes back with `restore_quarantine` |
| `decode_retained`, `decode_retained_record`, `project_retained_record`, `with_live_layout`, `preparation_context`, `reserve_envelope_identities`, `merge_retained_priority_context` | Reading and staging a stored record without losing what it holds for panels that are not registered now |
| `LayoutPersistence` | The trait a host implements to `load` and `save` one workspace's bytes |
| `layout_action_schema`, `layout_reply_schema`, `layout_error_schema`, `layout_envelope_schema`, `decode_action` | JSON schemas of the wire forms, and an action read from bytes |
| `PanelError`, `Rule`, `PanelResult`, `error` | The refusal, its rule and the result type |
| `LAYOUT_VERSION`, `PANEL_API_VERSION`, `ENVELOPE_FORMAT`, `ENVELOPE_VERSION`, `MAX_INSTANCES`, `MAX_SECTIONS`, `MAX_TABS`, `MAX_MUTATIONS`, `MAX_HEADER_ACTIONS`, `MAX_LOGICAL`, ... | Versions and limits |

## Using it

```rust
use solidcraft_panel_layout::*;

fn place_a_card(registry: PanelRegistry) -> PanelResult<Vec<u8>> {
    let capabilities = HostCapabilities { native_windows: false, persistence: true, max_native_hosts: 0 };
    let mut service = LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("host.workspace")?), capabilities)?;

    // Open one panel of a registered kind, docked on the right.
    let open = Mutation::Open {
        kind: PanelKind::new("host.card")?,
        instance: InstanceId::new("card.one")?,
        destination: Some(Destination::NewSection {
            section: SectionId::new("card.right")?,
            index: 0,
            location: SectionLocation::Dock { zone: Zone::Right },
            size: Size { width: 280.0, height: 240.0 },
        }),
    };
    service.dispatch(LayoutAction::Apply { expected_revision: None, mutations: vec![open] })?;

    // The rects to draw into, inside the host's free area.
    let capacity = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1000.0, height: 800.0 } };
    let resolved = resolve_geometry(&service, capacity, Direction::Ltr, 24.0)?;
    for panel in &resolved.panels {
        // draw `panel.instance` in `panel.outer`
    }

    // The layout as bytes for the host's preferences; `decode` reads them back.
    encode(&LayoutEnvelope::new(service.inspect().layout), &registry)
}
```

`tests/placement.rs` runs this path as a host outside SolidCraft would: a floating card is
dragged, docked on the right, saved and restored into a fresh service.
