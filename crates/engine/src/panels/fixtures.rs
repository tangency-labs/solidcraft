//! Executable synthetic contracts for independent host/command/persistence work.
//! These identifiers are fixtures, never production registrations or UI strings.
use std::collections::BTreeMap;

use super::*;

pub const FIXTURE_LAYOUT_V1: &[u8] = include_bytes!("fixtures/layout-v1.json");

pub fn synthetic_registry() -> PanelResult<PanelRegistry> {
    let mut registry = PanelRegistry::default();
    for (kind, multi, pinned) in
        [("fixture.application", false, false), ("fixture.editor", false, false), ("fixture.log", true, false), ("fixture.pin", true, true)]
    {
        let mut allowed_zones: std::collections::BTreeSet<_> =
            [Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating, Zone::Window].into_iter().collect();
        if pinned {
            allowed_zones.remove(&Zone::Bottom);
        }
        registry.register(PanelSpec {
            kind: PanelKind::new(kind)?,
            title_key: "ui.panel".into(),
            icon: "panel".into(),
            allowed_zones,
            default_zone: Zone::Left,
            min_size: Size { width: 160.0, height: 120.0 },
            default_size: Size { width: 280.0, height: 240.0 },
            max_size: Size { width: 1200.0, height: 1200.0 },
            edge_rule: if pinned { EdgeRule::PinOuter } else { EdgeRule::None },
            surface: PanelSurface::FollowZone,
            features: PanelFeatures { multi_instance: multi, ..PanelFeatures::default() },
        })?;
    }
    Ok(registry)
}

pub fn fixture_open(kind: &str, instance: &str, section: &str, zone: Zone, index: usize) -> PanelResult<Mutation> {
    Ok(Mutation::Open {
        kind: PanelKind::new(kind)?,
        instance: InstanceId::new(instance)?,
        destination: Some(Destination::NewSection {
            section: SectionId::new(section)?,
            index,
            location: if zone == Zone::Floating {
                SectionLocation::Floating { origin: Point { x: 40.0, y: 40.0 } }
            } else {
                SectionLocation::Dock { zone }
            },
            size: Size { width: 280.0, height: 240.0 },
        }),
    })
}

pub fn fixture_service(native: bool) -> PanelResult<LayoutService> {
    LayoutService::new(
        synthetic_registry()?,
        Layout::empty(WorkspaceId::new("fixture.workspace")?),
        HostCapabilities { native_windows: native, max_native_hosts: if native { 8 } else { 0 }, persistence: true },
    )
}

pub fn transaction(mutations: Vec<Mutation>) -> LayoutAction {
    LayoutAction::Apply { expected_revision: None, mutations }
}

#[derive(Default)]
pub struct FakePersistence {
    /// Deliberately separate map demonstrates that layout writes own one key.
    pub unrelated_preferences: BTreeMap<String, String>,
    pub layouts: BTreeMap<WorkspaceId, Vec<u8>>,
    pub fail_save: bool,
}
impl LayoutPersistence for FakePersistence {
    fn load(&mut self, workspace: &WorkspaceId) -> PanelResult<Option<Vec<u8>>> {
        valid_id(&workspace.0)?;
        Ok(self.layouts.get(workspace).cloned())
    }
    fn save(&mut self, workspace: &WorkspaceId, envelope: &[u8]) -> PanelResult<()> {
        valid_id(&workspace.0)?;
        if envelope.len() > MAX_ENVELOPE_BYTES || (!self.layouts.contains_key(workspace) && self.layouts.len() >= MAX_SECTIONS) {
            return Err(error(Rule::CollectionLimit, "fake persistence capacity"));
        }
        if self.fail_save {
            return Err(error(Rule::Capability, "synthetic atomic write failure"));
        }
        self.layouts.insert(workspace.clone(), envelope.to_vec());
        Ok(())
    }
}
