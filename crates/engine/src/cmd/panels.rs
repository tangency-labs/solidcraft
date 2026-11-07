//! Registry metadata for externally injected application-layout commands.
use super::CommandSpec;
use crate::schema::panels as wire;

fn unavailable(_: &mut crate::Session, _: &serde_json::Value) -> crate::Result<serde_json::Value> {
    Err(crate::EngineError::Panel(crate::panels::error(crate::panels::Rule::Capability, "application layout backend was not injected")))
}

macro_rules! command {
    ($id:literal, $icon:literal, $label:literal, $input:ty, $output:ty, $params:literal) => {
        CommandSpec::new($id, $label, crate::checked!($id, unavailable))
            .icon($icon)
            .noundo()
            .doc_only()
            .contract(&wire::CommandContract::<$input>::new($id))
            .output(wire::strict_output::<$output>)
            .params($params)
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    command!(
        "layout.apply",
        "open",
        "Apply Layout",
        wire::Apply,
        crate::panels::LayoutReply,
        "name, expected_revision?: apply preset to current workspace through retained runtime"
    ),
    command!("layout.get", "list", "Inspect Layout", wire::Empty, wire::GetReply, "{}: committed state and registered workspace summaries"),
    command!(
        "layout.reset",
        "restore",
        "Reset Layout",
        wire::Revision,
        crate::panels::LayoutReply,
        "expected_revision?: restore current workspace's registered default"
    ),
    command!(
        "layout.save",
        "save",
        "Save Layout",
        wire::Save,
        wire::SaveReply,
        "name, replace?: false (overwrite accepted for compatibility), expected_revision?: save complete envelope in injected catalogue"
    ),
    command!(
        "layout.workspace.select",
        "display",
        "Select Layout Workspace",
        wire::WorkspaceSelect,
        wire::WorkspaceReply,
        "workspace, expected_revision?: atomically remember outgoing and replace with destination"
    ),
    command!(
        "panel.action",
        "activate",
        "Run Panel Header Action",
        wire::HeaderAction,
        crate::panels::LayoutReply,
        "target or legacy id, action, expected_revision?: invoke a registered owning-controller action"
    ),
    command!(
        "panel.activate",
        "activate",
        "Activate Panel Tab",
        wire::Select,
        crate::panels::LayoutReply,
        "target or legacy id, expected_revision?: activate open tab without focus"
    ),
    command!(
        "panel.close",
        "close",
        "Close Panel",
        wire::Select,
        crate::panels::LayoutReply,
        "target or legacy id, expected_revision?: explicit guarded close"
    ),
    command!(
        "panel.collapse",
        "fold",
        "Collapse Panel Section",
        wire::Collapse,
        crate::panels::LayoutReply,
        "target or legacy id, collapsed, expected_revision?"
    ),
    command!(
        "panel.dismiss",
        "close",
        "Dismiss Panel Peek",
        wire::Revision,
        crate::panels::LayoutReply,
        "expected_revision?: dismiss temporary collapsed-panel presentation"
    ),
    command!(
        "panel.float",
        "move",
        "Float Panel",
        wire::Move,
        crate::panels::LayoutReply,
        "target or legacy id, destination: new_section with floating location, expected_revision?"
    ),
    command!(
        "panel.focus",
        "fit",
        "Focus Panel",
        wire::Select,
        crate::panels::LayoutReply,
        "target or legacy id, expected_revision?: reopen retained state, activate and request focus"
    ),
    command!(
        "panel.list",
        "list",
        "List Panels",
        wire::Empty,
        wire::ListReply,
        "{}: committed state, active tabs, capabilities, availability and registered kinds"
    ),
    command!(
        "panel.move",
        "move",
        "Move Panel",
        wire::Move,
        crate::panels::LayoutReply,
        "target or legacy id, destination: new_section, join_tab or relative, activate?: false, expected_revision?"
    ),
    command!(
        "panel.native_close",
        "restore",
        "Redock Native Panel Section",
        wire::Select,
        crate::panels::LayoutReply,
        "target: native_host, expected_revision?: native close redocks without cancelling content"
    ),
    command!(
        "panel.open",
        "open",
        "Open Panel",
        wire::Open,
        crate::panels::LayoutReply,
        "kind, instance, destination?: required for new instance, expected_revision?"
    ),
    command!(
        "panel.peek",
        "eye",
        "Peek Collapsed Panel",
        wire::Select,
        crate::panels::LayoutReply,
        "target or legacy id, expected_revision?: transient presentation without resizing"
    ),
    command!(
        "panel.popout",
        "display",
        "Pop Out Panel",
        wire::Move,
        crate::panels::LayoutReply,
        "target or legacy id, destination: new_section with window location, expected_revision?: web lowers to floating"
    ),
    command!(
        "panel.reset_pair",
        "restore",
        "Reset Panel Sash",
        wire::ResetPair,
        crate::panels::LayoutReply,
        "first, second, expected_revision?: reset adjacent dock pair to registered default ratio"
    ),
    command!(
        "panel.resize",
        "scale",
        "Resize Panel Section",
        wire::Resize,
        crate::panels::LayoutReply,
        "target or legacy id, size: {width,height}, thickness_only?: false by default; true changes Bottom height or side column width and ignores the other valid size component, expected_revision?"
    ),
    command!(
        "panel.resize_pair",
        "scale",
        "Resize Panel Sash",
        wire::ResizePair,
        crate::panels::LayoutReply,
        "first, second, first_size, second_size, expected_revision?: conserve adjacent dock pair extent"
    ),
];
