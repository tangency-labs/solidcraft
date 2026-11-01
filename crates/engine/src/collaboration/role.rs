//! Roles as rules, compiled once (#5184, A1a).
//!
//! A rule allows or denies an [`Ability`] or the commands a [`Pattern`] names, optionally only
//! on some targets. Deny wins over allow, and anything not allowed is denied, so a new role
//! starts from nothing.

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::Deserialize;

use crate::EngineError;
use crate::cmd::{CommandSpec, command_specs};

/// A role: a name and its rules.
#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    pub name: String,
    pub rules: Vec<Rule>,
}

/// One rule: allow or deny something, optionally only somewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub effect: Effect,
    pub what: What,
    /// `None`: everywhere.
    pub target: Option<TargetRule>,
    /// Only on "deny see": a plain box is drawn in the hidden part's place (A6).
    pub stand_in: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

/// What a rule is about: an ability, or the commands a pattern names.
#[derive(Clone, Debug, PartialEq)]
pub enum What {
    Ability(Ability),
    Commands(Pattern),
}

/// What a role may receive (`Design`, `Meshes`), see (`See`), or a named group of commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ability {
    Design,
    Meshes,
    See,
    Edit,
    Merge,
    Manage,
    Comment,
    Ping,
    Present,
    Watch,
}

/// Where a rule applies.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetRule {
    pub kind: TargetKind,
    pub pattern: Pattern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Component,
    Feature,
    Body,
    Parameter,
}

/// A persistent id, exact text, or text ending in `*` (`"Bracket*"` is `Prefix("Bracket")`).
/// Nothing else: matching is a string compare, so a hostile pattern can't make it slow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    Id(u64),
    Exact(String),
    Prefix(String),
}

impl Pattern {
    /// Text as a pattern: a trailing `*` makes it a prefix.
    fn parse(text: &str) -> Pattern {
        match text.strip_suffix('*') {
            Some(p) => Pattern::Prefix(p.to_string()),
            None => Pattern::Exact(text.to_string()),
        }
    }

    fn matches(&self, text: &str) -> bool {
        match self {
            Pattern::Id(_) => false,
            Pattern::Exact(s) => s == text,
            Pattern::Prefix(p) => text.starts_with(p.as_str()),
        }
    }
}

impl Ability {
    const ALL: [Ability; 10] = [
        Ability::Design,
        Ability::Meshes,
        Ability::See,
        Ability::Edit,
        Ability::Merge,
        Ability::Manage,
        Ability::Comment,
        Ability::Ping,
        Ability::Present,
        Ability::Watch,
    ];

    /// Whether this ability is a group of commands, and so covered by the pattern `*`.
    fn is_commands(self) -> bool {
        !matches!(self, Ability::Design | Ability::Meshes | Ability::See)
    }

    /// Whether `spec` is one of this ability's commands. The collaboration commands (A2) join
    /// `Merge`, `Manage`, `Comment`, `Ping`, `Present` and `Watch` here. A command missing from
    /// its group is refused to a role that allows only the group, but not to one that allows `*`
    /// (Creator, Worker), so a group must list every command it names.
    fn covers(self, spec: &CommandSpec) -> bool {
        match self {
            Ability::Edit => spec.undoable,
            _ => false,
        }
    }
}

/// The commands that save, export or copy the design (Q3 of the roles design, plus slicing and
/// Printables). A role refused one is told it can't save or export the design.
const SAVES_OR_EXPORTS: &[&str] = &[
    "file.export",
    "file.export_view_dxf",
    "file.open_in_easyprint",
    "file.save",
    "file.save_as",
    "file.save_mesh",
    "file.send_to_slicer",
    "library.export",
    "motion.export",
    "motion.export_animation",
    "parameters.export",
    "render.export",
    "render.result.copy",
    "sheet.export_bend_table",
    "sheet.export_dxf",
    "sketch.copy",
    "sketch.export_dxf",
    "timeline.copy",
    "timeline.export_script",
];

// The Viewer protects the design's IP and only shows the model (Cameron, 2026-10-27). Every
// command is decided for it, one by one, in one of the two lists below (or is undoable, so an
// edit): `every_command_is_decided_for_the_viewer` fails on a command in neither.

/// What a Viewer may run: commands that only read, by exact id (an area such as `view.*` would
/// let a later writer in it through).
const VIEWER_READS: &[&str] = &[
    "appearance.chain",
    "appearance.copy",
    "appearance.favorite",
    "appearance.library",
    "appearance.list",
    "appearance.pick_up",
    "browser.collapse_all",
    "browser.expand_all",
    "component.copies",
    "component.links",
    "component.list",
    "component.metadata",
    "component.properties",
    "config.preview",
    "config.rules",
    "config.table",
    "contact.check",
    "contact.list",
    "document.inspect",
    "document.metadata",
    "document.revision",
    "engine.commands",
    "engine.describe",
    "explode.list",
    "explode.show",
    "expr.evaluate",
    "frame.profile.list",
    "frame.profile.properties",
    "inspect.accessibility",
    "inspect.center_of_mass",
    "inspect.clearance",
    "inspect.colored_components",
    "inspect.continuity",
    "inspect.curvature_comb",
    "inspect.curvature_map",
    "inspect.draft",
    "inspect.environment_map",
    "inspect.interference",
    "inspect.isocurve",
    "inspect.measure",
    "inspect.minimum_radius",
    "inspect.parameter_sweep",
    "inspect.section",
    "inspect.silhouette",
    "inspect.zebra",
    "joint.inspect",
    "joint.list",
    "library.inspect",
    "material.inspect",
    "material.list",
    "material.properties.directional",
    "material.properties.validate",
    "model.edges",
    "model.faces",
    "model.threads",
    "motion.interference",
    "motion.list",
    "motion.plot",
    "motion.trace",
    "occurrence.properties",
    "parameters.complete",
    "parameters.graph",
    "parameters.list",
    "parameters.users",
    "parts.library",
    "plastic.rules",
    "pointcloud.list",
    "render.aging.describe",
    "render.backends",
    "render.camera.get",
    "render.effects.describe",
    "render.hdr.inspect",
    "render.library.describe",
    "render.library.list",
    "render.library.preview",
    "render.library.targets",
    "render.lights.list",
    "render.lights.mute",
    "render.lights.solo",
    "render.preset.resolve",
    "render.result.close",
    "render.result.view",
    "render.scene",
    "render.settings.get",
    "render.status",
    "select.clear",
    "select.filter",
    "select.same_look",
    "select.set",
    "settings.profile.list",
    "sheet.flat_pattern",
    "sheet.rules",
    "sketch.chain",
    "sketch.drawing_bounds",
    "sketch.glyphs",
    "sketch.inspect",
    "sketch.loose_ends",
    "sketch.snap",
    "timeline.dependents",
    "timeline.inspect",
    "view.ambient_occlusion",
    "view.angle_readout",
    "view.apply",
    "view.back",
    "view.bottom",
    "view.fit",
    "view.front",
    "view.graphics_preset",
    "view.grid",
    "view.ground_shadow",
    "view.history_layout",
    "view.home",
    "view.isometric",
    "view.layout",
    "view.left",
    "view.list",
    "view.navigate",
    "view.navigation_bar",
    "view.next_tab",
    "view.normal_to",
    "view.perspective",
    "view.previous_tab",
    "view.render_browser",
    "view.render_dock",
    "view.render_focus",
    "view.render_frame",
    "view.render_inspector",
    "view.render_select",
    "view.render_stage",
    "view.right",
    "view.shots_strip",
    "view.spin",
    "view.status_items",
    "view.tool",
    "view.top",
    "view.visual_style",
];

/// Commands that make no undo step but that a Viewer may not run, beside [`SAVES_OR_EXPORTS`].
const VIEWER_REFUSES: &[&str] = &[
    // Slicing is refused, so the slicer list is too.
    "file.slicers",
    // Reads that also write a file through `path`.
    "assembly.bom",
    // Change the design, its history or what is being edited, without an undo step.
    "component.activate",
    "component.edit_in_place",
    "component.finish_edit",
    "component.revert_position",
    "edit.checkpoint",
    "edit.redo",
    "edit.repeat",
    "edit.restore_checkpoint",
    "edit.undo",
    "sketch.auto_project",
    "sketch.edit",
    "sketch.finish",
    "sketch.solve",
    "timeline.compute_all",
    "timeline.play",
    // Start an editing tool, whose every step is an edit.
    "appearance.paint",
    // Crash reports can hold other designs and file paths (the agent chat's gate refuses them
    // too, `ui-egui/src/agent_chat/gate.rs`).
    "diagnostics.crash_report",
    "diagnostics.crash_reports",
    // Replace the design, or work on this computer's files outside it.
    "diagnostics.crash_reports.clear",
    "diagnostics.crash_test",
    "doc.open",
    "doc.recover",
    "doc.recovery_discard",
    "doc.recovery_list",
    "file.new",
    // Change or move this computer's settings profiles.
    "settings.profile.apply",
    "settings.profile.delete",
    "settings.profile.export",
    "settings.profile.import",
    "settings.profile.rename",
    "settings.profile.save",
    // Render jobs and render settings, until the Render workspace is decided for meshes-only
    // roles (A5).
    "render.camera.fit",
    "render.camera.set",
    "render.cancel",
    "render.batch",
    "render.compare",
    "render.library.physical.prepare",
    "render.library.studio.prepare",
    "render.library.surface.prepare",
    "render.output.size",
    "render.preflight",
    "render.preflight.copy",
    "render.quality.prepare",
    "render.result.pin",
    "render.result.regrade",
    "render.settings.apply",
    "render.start",
];

fn rule(effect: Effect, what: What) -> Rule {
    Rule { effect, what, target: None, stand_in: false }
}

fn allow(a: Ability) -> Rule {
    rule(Effect::Allow, What::Ability(a))
}

fn deny(a: Ability) -> Rule {
    rule(Effect::Deny, What::Ability(a))
}

fn all_commands() -> Rule {
    rule(Effect::Allow, What::Commands(Pattern::Prefix(String::new())))
}

/// The built-in roles, by name: the one place their names and rules are written.
const BUILT_IN: [(&str, fn() -> Vec<Rule>); 3] = [
    // Gets the design and may do anything.
    ("Creator", || vec![allow(Ability::Design), all_commands()]),
    // Gets the design and edits it, but doesn't merge, manage people and roles, or present.
    ("Worker", || vec![allow(Ability::Design), all_commands(), deny(Ability::Merge), deny(Ability::Manage), deny(Ability::Present)]),
    // Gets meshes only and runs the reads; never an edit, a save or an export, nor anything
    // the Viewer refuses.
    ("Viewer", || {
        let mut rules = vec![allow(Ability::Meshes), allow(Ability::See), allow(Ability::Ping), allow(Ability::Watch), allow(Ability::Comment)];
        rules.extend(VIEWER_READS.iter().map(|read| rule(Effect::Allow, What::Commands(Pattern::parse(read)))));
        rules.push(deny(Ability::Edit));
        rules.extend(SAVES_OR_EXPORTS.iter().chain(VIEWER_REFUSES).map(|id| rule(Effect::Deny, What::Commands(Pattern::parse(id)))));
        rules
    }),
];

impl Role {
    /// A built-in role by its name, as a member list names it: `Creator`, `Worker` or `Viewer`.
    pub fn built_in(name: &str) -> Option<Role> {
        BUILT_IN.iter().find(|(n, _)| *n == name).map(|(n, rules)| Role { name: (*n).into(), rules: rules() })
    }
}

/// Why something was refused: the English text the UI shows ("Viewers can't change the design").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason(String);

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the host sends a role: the whole design, or meshes only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receive {
    Design,
    Meshes,
}

/// What is being asked. `Change(&Touched)` and `See(&Target)` come with the slice that builds
/// `Touched` and `Target` in `doc`.
pub enum Ask<'a> {
    /// May this command run (before running: UI greying, `execute`, `run_nested`)?
    Command(&'a CommandSpec),
    /// May the host send this?
    Receive(Receive),
}

/// A role compiled once. Cost: building it matches every rule against every registered command
/// (rules x commands string compares: 0.15 ms for the Viewer, 0.9 ms for 256 rules); then
/// `allows(Ask::Command)` is one hash lookup (all 482 commands in 34 us, enough for every toolbar
/// frame) and `allows(Ask::Receive)` one set lookup. Measured in the `ci` profile on two cores.
pub struct Access {
    role: String,
    /// The answer for every registered command id.
    commands: HashMap<&'static str, Result<(), Reason>>,
    /// The abilities the role has somewhere.
    abilities: HashSet<Ability>,
}

impl Access {
    pub fn new(role: &Role) -> Access {
        let commands = command_specs().into_iter().map(|spec| (spec.id, decide_command(role, spec))).collect();
        let abilities = Ability::ALL.into_iter().filter(|&a| has(role, a)).collect();
        Access { role: role.name.clone(), commands, abilities }
    }

    /// Deny wins; anything not allowed is denied. The error is the reason the UI shows.
    pub fn allows(&self, ask: Ask) -> Result<(), Reason> {
        let people = || who(&self.role);
        match ask {
            // A command outside the registry (made by a test or a plugin) isn't allowed.
            Ask::Command(spec) => {
                self.commands.get(spec.id).cloned().unwrap_or_else(|| Err(Reason(format!("{} can't use {}", people(), spec.label))))
            }
            Ask::Receive(Receive::Design) if !self.abilities.contains(&Ability::Design) => Err(Reason(format!("{} can't get the design", people()))),
            Ask::Receive(Receive::Meshes) if !self.abilities.contains(&Ability::Meshes) => Err(Reason(format!("{} can't get meshes only", people()))),
            Ask::Receive(_) => Ok(()),
        }
    }
}

/// The one reading of a role's rules about something: `Ok` when an allow covers it and no deny
/// without a target does; else the deny's `what`, or `None` when nothing allows it. A rule with a
/// target allows somewhere, but denies only there, so it doesn't refuse the whole thing.
fn verdict(role: &Role, covers: impl Fn(&What) -> bool) -> Result<(), Option<&What>> {
    let mut allowed = false;
    for rule in role.rules.iter().filter(|rule| covers(&rule.what)) {
        match (rule.effect, &rule.target) {
            (Effect::Allow, _) => allowed = true,
            (Effect::Deny, None) => return Err(Some(&rule.what)),
            (Effect::Deny, Some(_)) => {}
        }
    }
    if allowed { Ok(()) } else { Err(None) }
}

/// One command's answer under `role`.
fn decide_command(role: &Role, spec: &CommandSpec) -> Result<(), Reason> {
    let covers = |what: &What| match what {
        What::Ability(a) => a.covers(spec),
        What::Commands(p) => p.matches(spec.id),
    };
    verdict(role, covers).map_err(|why| match why {
        Some(What::Ability(Ability::Edit)) => Reason(format!("{} can't change the design", who(&role.name))),
        _ if SAVES_OR_EXPORTS.contains(&spec.id) => Reason(format!("{} can't save or export the design", who(&role.name))),
        _ => Reason(format!("{} can't use {}", who(&role.name), spec.label)),
    })
}

/// Whether `role` has `ability` somewhere. Getting the design means seeing it, so `Design`
/// covers `See`; the pattern `*` names every command, so it covers every ability that is a
/// group of commands.
fn has(role: &Role, ability: Ability) -> bool {
    let covers = |what: &What| match what {
        What::Ability(a) => *a == ability || (*a == Ability::Design && ability == Ability::See),
        What::Commands(Pattern::Prefix(p)) => p.is_empty() && ability.is_commands(),
        What::Commands(_) => false,
    };
    verdict(role, covers).is_ok()
}

/// The people a role names, as the start of a sentence: "Viewers", "The Supplier role".
fn who(role: &str) -> String {
    if BUILT_IN.iter().any(|(name, _)| *name == role) { format!("{role}s") } else { format!("The {role} role") }
}

/// The member list's format tag (the drawing family's pattern).
const FORMAT: &str = "solidcraft-members/1";
const MAX_BYTES: usize = 1 << 20;
const MAX_MEMBERS: usize = 1024;
/// Keys and role names, in bytes.
const MAX_TEXT_BYTES: usize = 256;
/// Member names, in characters.
const MAX_NAME_CHARS: usize = 64;
/// The only key type a member list takes.
const KEY_TYPE: &str = "ssh-ed25519";

/// The repository's `collaboration/members.json`: who has which role. Roles are the built-in
/// ones; custom roles and invites come later (a file with `roles` or `invites` is refused as
/// having an unknown field until then). Signatures and versions against git are checked by the
/// trust slice (A1b), not here.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberList {
    #[serde(rename = "format")]
    _format: String,
    /// +1 each change, which catches replays.
    pub version: u64,
    /// In the order they joined: a new member goes last.
    pub members: Vec<Member>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// The OpenSSH public key line: `ssh-ed25519 <base64> [comment]`.
    pub key: String,
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// A built-in role's name ([`Role::built_in`]).
    pub role: String,
    pub kind: MemberKind,
    /// The key of the member who added this one (the first member adds themself).
    pub added_by: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Person,
    Agent,
}

fn bad(why: impl fmt::Display) -> EngineError {
    EngineError::Other(format!("member list: {why}"))
}

impl MemberList {
    /// Read a member list, refusing the whole file if any part of it is wrong: the size, the
    /// format, an unknown or repeated field, a cap, or a check of what it says. Cost: two passes
    /// over at most 1 MiB.
    pub fn read(bytes: &[u8]) -> crate::Result<MemberList> {
        if bytes.len() > MAX_BYTES {
            return Err(bad("larger than 1 MiB"));
        }
        #[derive(Deserialize)]
        struct Head {
            format: Option<String>,
        }
        match serde_json::from_slice::<Head>(bytes) {
            Ok(Head { format: Some(f) }) if f == FORMAT => {}
            Ok(Head { format: Some(f) }) if fits(&f) => return Err(bad(format!("not a SolidCraft member list (format `{f}`)"))),
            _ => return Err(bad("not a SolidCraft member list")),
        }
        let list: MemberList = serde_json::from_slice(bytes).map_err(bad)?;
        list.check()?;
        Ok(list)
    }

    /// The members who must choose another name, by index: each whose name a member who joined
    /// earlier already has, ignoring case (a ping or a cursor's label shows only the name, and
    /// `alex` reads as `Alex`). The one who joined later is asked to change theirs; the list is
    /// still read, so two people who join at once with one name don't lock everyone out.
    pub fn renames_needed(&self) -> Vec<usize> {
        let mut taken = HashSet::new();
        self.members.iter().enumerate().filter(|(_, m)| !taken.insert(m.name.to_lowercase())).map(|(i, _)| i).collect()
    }

    fn check(&self) -> crate::Result<()> {
        if self.members.len() > MAX_MEMBERS {
            return Err(bad(format!("more than {MAX_MEMBERS} members")));
        }
        // Whether each built-in role may manage, worked out once, not once per member.
        let manages = BUILT_IN.map(|(n, _)| Role::built_in(n).is_some_and(|r| has(&r, Ability::Manage)));
        // One key is one member, however its text is written (another comment, say).
        let mut keys = HashSet::new();
        let mut someone_manages = false;
        for (i, m) in self.members.iter().enumerate() {
            let at = |why: String| bad(format!("member {}: {why}", i + 1));
            name(&m.name).map_err(at)?;
            for (field, text) in [("key", &m.key), ("role", &m.role), ("added_by", &m.added_by)] {
                if !fits(text) {
                    return Err(at(format!("{field} must be 1 to {MAX_TEXT_BYTES} bytes with no control characters")));
                }
            }
            if !is_colour(&m.colour) {
                return Err(at("colour must be #rrggbb".into()));
            }
            let key = ed25519_key(&m.key).ok_or_else(|| at(format!("key isn't an {KEY_TYPE} public key")))?;
            if !keys.insert(key) {
                return Err(bad(format!("two members have the key `{}`", m.key)));
            }
            let role = BUILT_IN
                .iter()
                .position(|(n, _)| *n == m.role)
                .ok_or_else(|| at(format!("`{}` isn't a role (one of {})", m.role, BUILT_IN.map(|(n, _)| n).join(", "))))?;
            someone_manages |= manages.get(role).is_some_and(|&m| m);
        }
        for (i, m) in self.members.iter().enumerate() {
            if !ed25519_key(&m.added_by).is_some_and(|k| keys.contains(&k)) {
                return Err(bad(format!("member {}: added_by isn't a member's key", i + 1)));
            }
        }
        if !someone_manages {
            return Err(bad("nobody in it can manage people and roles"));
        }
        Ok(())
    }
}

/// 1 to 256 bytes, with no control characters.
fn fits(text: &str) -> bool {
    (1..=MAX_TEXT_BYTES).contains(&text.len()) && !text.chars().any(char::is_control)
}

/// A name shown to others: trimmed and not empty (as render setup names are), at most 64
/// characters, and nothing that shows as nothing, breaks the line or turns the text around:
/// control characters and the line and paragraph separators, the invisible format characters (zero-width spaces and joiners, bidi marks, embeddings and
/// isolates, word joiners, tags) and the blank fillers. With any of them, `Alex` could be shown
/// for someone who isn't Alex. Variation selectors stay: emoji need them.
fn name(text: &str) -> Result<(), String> {
    let hidden = |c: char| {
        c.is_control()
            || matches!(
                c,
                '\u{AD}'
                    | '\u{34F}'
                    | '\u{61C}'
                    | '\u{115F}'
                    | '\u{1160}'
                    | '\u{17B4}'
                    | '\u{17B5}'
                    | '\u{180B}'..='\u{180F}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{206F}'
                    | '\u{3164}'
                    | '\u{FEFF}'
                    | '\u{FFA0}'
                    | '\u{FFF9}'..='\u{FFFB}'
                    | '\u{1D173}'..='\u{1D17A}'
                    | '\u{E0000}'..='\u{E007F}'
            )
    };
    if text.trim().is_empty() || text != text.trim() || text.chars().count() > MAX_NAME_CHARS || text.chars().any(hidden) {
        Err(format!("a name must be trimmed, 1 to {MAX_NAME_CHARS} characters, with no control, bidi or invisible characters"))
    } else {
        Ok(())
    }
}

/// `#rrggbb`, as the design writes colours (`Look::hex`).
fn is_colour(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The 32 bytes of an OpenSSH `ssh-ed25519` public key line (`ssh-ed25519 <base64> [comment]`):
/// the base64 blob is the key type and the key, each as a 4-byte big-endian length and its bytes.
fn ed25519_key(line: &str) -> Option<[u8; 32]> {
    let mut parts = line.split_whitespace();
    if parts.next()? != KEY_TYPE {
        return None;
    }
    let blob = solidcraft_doc::canvas::base64_decode(parts.next()?)?;
    let (kind, rest) = ssh_string(&blob)?;
    let (key, rest) = ssh_string(rest)?;
    if kind != KEY_TYPE.as_bytes() || !rest.is_empty() {
        return None;
    }
    key.try_into().ok()
}

/// An SSH wire-format string: a 4-byte big-endian length, then that many bytes.
fn ssh_string(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, rest) = b.split_first_chunk::<4>()?;
    let len = usize::try_from(u32::from_be_bytes(*len)).ok()?;
    (len <= rest.len()).then(|| rest.split_at(len))
}

#[cfg(test)]
#[path = "role_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
