//! A host outside SolidCraft places one floating card: drag it by moving its floating rect,
//! dock it on the right, then restore the saved layout value into a fresh service.
use std::collections::BTreeSet;

use solidcraft_panel_layout::*;

type TestResult = PanelResult<()>;

const CARD: &str = "host.card";
const CAPACITY: LogicalRect = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1000.0, height: 800.0 } };
const CHROME: f32 = 24.0;

fn registry() -> PanelResult<PanelRegistry> {
    let mut registry = PanelRegistry::default();
    let allowed_zones: BTreeSet<Zone> = [Zone::Left, Zone::Right, Zone::Floating].into_iter().collect();
    registry.register(PanelSpec {
        kind: PanelKind::new(CARD)?,
        title_key: "host.card.title".into(),
        icon: "card".into(),
        allowed_zones,
        default_zone: Zone::Floating,
        min_size: Size { width: 160.0, height: 120.0 },
        default_size: Size { width: 280.0, height: 240.0 },
        max_size: Size { width: 1200.0, height: 1200.0 },
        edge_rule: EdgeRule::None,
        surface: PanelSurface::FollowZone,
        features: PanelFeatures::default(),
        side_height_policy: SideHeightPolicy::Content,
        fixed_bottom_height: None,
        bottom_always_open: false,
        header_actions: Vec::new(),
        text_heavy: false,
    })?;
    Ok(registry)
}

fn new_service(layout: Layout) -> PanelResult<LayoutService> {
    LayoutService::new(registry()?, layout, HostCapabilities { native_windows: false, persistence: true, max_native_hosts: 0 })
}

fn apply(service: &mut LayoutService, mutations: Vec<Mutation>) -> PanelResult<LayoutReply> {
    service.dispatch(LayoutAction::Apply { expected_revision: None, mutations })
}

/// The card's outer rect and zone in the resolved geometry of the current layout.
fn card_placement(service: &LayoutService) -> PanelResult<(Zone, LogicalRect)> {
    let resolved = resolve_geometry(service, CAPACITY, Direction::Ltr, CHROME)?;
    let card = resolved.panels.iter().find(|panel| panel.instance.0 == "card.one").ok_or_else(|| error(Rule::MissingTarget, "card not presented"))?;
    Ok((card.zone, card.outer))
}

#[test]
fn floating_card_drags_docks_right_and_restores() -> TestResult {
    let mut service = new_service(Layout::empty(WorkspaceId::new("host.workspace")?))?;
    let open = Mutation::Open {
        kind: PanelKind::new(CARD)?,
        instance: InstanceId::new("card.one")?,
        destination: Some(Destination::NewSection {
            section: SectionId::new("card.floating")?,
            index: 0,
            location: SectionLocation::Floating { origin: Point { x: 40.0, y: 40.0 } },
            size: Size { width: 280.0, height: 240.0 },
        }),
    };
    apply(&mut service, vec![open])?;
    let (zone, outer) = card_placement(&service)?;
    assert_eq!((zone, outer.origin), (Zone::Floating, Point { x: 40.0, y: 40.0 }));

    // Drag by the grip: the host reports the card's new floating origin.
    let dragged = Point { x: 100.0, y: 120.0 };
    apply(
        &mut service,
        vec![Mutation::SetFloatingRect { section: SectionId::new("card.floating")?, origin: dragged, size: Size { width: 280.0, height: 240.0 } }],
    )?;
    let (zone, outer) = card_placement(&service)?;
    assert_eq!((zone, outer.origin), (Zone::Floating, dragged));

    // Dock on the right edge.
    let dock = Mutation::Move {
        instance: InstanceId::new("card.one")?,
        destination: Destination::NewSection {
            section: SectionId::new("card.right")?,
            index: 0,
            location: SectionLocation::Dock { zone: Zone::Right },
            size: Size { width: 280.0, height: 240.0 },
        },
    };
    apply(&mut service, vec![dock])?;
    let (zone, outer) = card_placement(&service)?;
    assert_eq!(zone, Zone::Right);
    assert_eq!(outer.origin.x + outer.size.width, CAPACITY.origin.x + CAPACITY.size.width);

    // Save the layout as a value, then restore it into a fresh service.
    let saved = encode(&LayoutEnvelope::new(service.inspect().layout), &registry()?)?;
    let outcome = decode(&saved, &registry()?, &Layout::empty(WorkspaceId::new("host.workspace")?))?;
    assert!(outcome.notice.is_none());
    let restored = new_service(outcome.envelope.layout)?;
    assert_eq!(card_placement(&restored)?, (Zone::Right, outer));
    Ok(())
}
