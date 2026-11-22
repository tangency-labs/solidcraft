//! Amalgamate (#18799): a design whose bodies an Amalgam Tag labels is a tool that other designs
//! place. At each place, the tool's Remove bodies are cut from the bodies named for them, then
//! its Add bodies are joined into the bodies named for those, and its New bodies come in as a
//! part of their own. The tag itself changes no geometry: evaluating it only checks it.
//!
//! A design may hold several tags; each is a tool of its own, named by its feature's name. A
//! tag belongs to the design that declares it: placing a tool brings in its geometry, never its
//! tags, so a design that places tools and tags its own bodies is a tool made of tools.

use crate::{AlignFrame, BodyName, DocError, Document, ModelState, Result};
use serde::{Deserialize, Serialize};

/// The most bodies one tag labels, over all its roles.
pub const MAX_TAGGED: usize = 256;

/// What a tagged body does where the tool is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Cut from the bodies named for Remove, before any Add.
    Remove,
    /// Joined into the bodies named for Add, after every Remove.
    Add,
    /// Brought in as a part of its own.
    New,
}

impl Role {
    pub fn word(self) -> &'static str {
        match self {
            Role::Remove => "Remove",
            Role::Add => "Add",
            Role::New => "New",
        }
    }
}

/// The bodies of a tag with their roles, Remove first, then Add, then New.
pub fn roles<'a>(remove: &'a [BodyName], add: &'a [BodyName], new: &'a [BodyName]) -> impl Iterator<Item = (&'a BodyName, Role)> {
    remove.iter().map(|b| (b, Role::Remove)).chain(add.iter().map(|b| (b, Role::Add))).chain(new.iter().map(|b| (b, Role::New)))
}

/// Checks an Amalgam Tag against the model `st` where it stands in the timeline: it tags at least
/// one body and at most [`MAX_TAGGED`], each an existing body with one role, no mesh body, and
/// only bodies that enclose a volume for Remove and Add (they are cut and joined); its seat, when
/// it has one, is found on the model.
pub fn check_tag(doc: &Document, st: &ModelState, remove: &[BodyName], add: &[BodyName], new: &[BodyName], seat: Option<&AlignFrame>) -> Result<()> {
    let count = remove.len().saturating_add(add.len()).saturating_add(new.len());
    if count == 0 {
        return Err(DocError::Invalid("the Amalgam Tag tags no body: give it Remove, Add or New bodies".into()));
    }
    if count > MAX_TAGGED {
        return Err(DocError::Invalid(format!("the Amalgam Tag tags {count} bodies; it takes at most {MAX_TAGGED}")));
    }
    let mut seen: Vec<(&BodyName, Role)> = Vec::with_capacity(count);
    for (name, role) in roles(remove, add, new) {
        if let Some((_, first)) = seen.iter().find(|(n, _)| *n == name) {
            return Err(DocError::Invalid(if *first == role {
                format!("`{name}` is tagged {} twice", role.word())
            } else {
                format!("`{name}` is tagged both {} and {}: a body has one role", first.word(), role.word())
            }));
        }
        seen.push((name, role));
        let body = st.body(name).ok_or_else(|| DocError::Unknown(format!("body `{name}` to tag")))?;
        if body.body.is_mesh() {
            return Err(DocError::Invalid(format!("`{name}` is a mesh body: an Amalgam Tag takes solid and surface bodies only")));
        }
        if role != Role::New && !body.is_closed() {
            return Err(DocError::Invalid(format!(
                "`{name}` does not enclose a volume: {} bodies are {}, so they must be closed solids",
                role.word(),
                if role == Role::Remove { "cut" } else { "joined" }
            )));
        }
    }
    if let Some(seat) = seat {
        seat.resolve(doc, st).map_err(|e| DocError::Invalid(format!("the Amalgam Tag's seat: {e}")))?;
    }
    Ok(())
}

// ---- Placing a tool (Amalgamate) ----

/// The most seeds one Amalgamate places its tool at.
pub const MAX_SEEDS: usize = 256;

/// Where a tool design was read from, so it can be read again (Update Links).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AmalgamSource {
    /// The tool design's file: relative to this design's folder once this design is saved, else
    /// as given (`/` separates folders).
    pub path: String,
    /// The tool design's configuration row, by stable id. Without one, its saved state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<u64>,
    /// The row's creation identity, so a replaced row is never taken for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_identity: Option<u64>,
    /// SHA-256 of the tool design's bytes as last read, to tell when the file changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_sha256: Option<String>,
}

/// What an Amalgamate took from its tool design: the tagged bodies, moved so the tag's seat is
/// at the origin with its z up, as exact B-rep records kept in this design (it opens without the
/// tool's file), each with its role and the names the tool design gives its faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmalgamTool {
    pub source: AmalgamSource,
    /// The tag the bodies were taken by: its feature's name in the tool design.
    pub tag: String,
    /// The bodies, in the tag's order (Remove, then Add).
    pub records: std::sync::Arc<solidcraft_kernel::ImportRecords>,
    /// Each body's role, in the records' order.
    pub roles: Vec<Role>,
    /// Each body's face names in the tool design, by body then face (empty where not known).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<Vec<String>>,
}

impl AmalgamTool {
    pub fn has(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }
}

/// Checks what an Amalgamate is given, before anything is placed: one seed at least and at most
/// [`MAX_SEEDS`], a quarter-turn count of 0 to 3, and targets exactly for the roles the tool has
/// (a tool that cuts names what it cuts, one that adds names what it joins: nothing is skipped).
pub fn check_placement(tool: &AmalgamTool, seeds: usize, turns: u32, remove_from: &[BodyName], add_to: &[BodyName]) -> Result<()> {
    if seeds == 0 {
        return Err(DocError::Invalid("place the tool at one seed at least".into()));
    }
    if seeds > MAX_SEEDS {
        return Err(DocError::Invalid(format!("{seeds} seeds; an Amalgamate places its tool at most {MAX_SEEDS} times")));
    }
    if turns > 3 {
        return Err(DocError::Invalid(format!("{turns} quarter turns: give 0 to 3")));
    }
    if tool.roles.len() != tool.records.bodies.len() || tool.roles.contains(&Role::New) {
        return Err(DocError::Invalid("the tool's bodies and their roles don't match".into()));
    }
    for (role, targets, key, verb) in [(Role::Remove, remove_from, "remove_from", "cut from"), (Role::Add, add_to, "add_to", "joined into")] {
        match (tool.has(role), targets.is_empty()) {
            (true, true) => {
                return Err(DocError::Invalid(format!(
                    "the tool has {} bodies: name the bodies they are {verb} in `{key}` (a tool's roles are never skipped)",
                    role.word()
                )));
            }
            (false, false) => return Err(DocError::Invalid(format!("the tool has no {} bodies: leave `{key}` out", role.word()))),
            _ => {}
        }
    }
    Ok(())
}

/// A seed's name: what it is found by on the model (a sketch point's or curve's id, the name of
/// the face, circle or edge a joint origin snaps to). Copies of the tool are told apart by it,
/// never by the seeds' order, so removing a seed renames nothing at the others. A seed with
/// nothing named to find it by (a bare point or frame) has none, and is refused.
pub fn seed_key(seed: &AlignFrame) -> Result<String> {
    use crate::joints::Snap;
    Ok(match seed {
        AlignFrame::SketchPoint { sketch, point } => format!("S{}.point:{point}", sketch.0),
        AlignFrame::SketchCurve { sketch, curve } => format!("S{}.curve:{curve}", sketch.0),
        AlignFrame::Origin { origin } => {
            let at = match &origin.snap {
                Snap::FaceCenter { face: Some(face), .. } => format!("face:{face}"),
                Snap::CircleCenter { face: Some(face), end, .. } => format!("circle:{face}|{}", end.as_deref().unwrap_or("")),
                Snap::Edge { edge: Some(edge), corner, .. } => format!("edge:{edge}|{}", corner.as_deref().unwrap_or("")),
                _ => {
                    return Err(DocError::Invalid(
                        "not supported yet: a seed at a point or frame with no named geometry; pick a face, a circle, an edge or a sketch point"
                            .into(),
                    ));
                }
            };
            let path: Vec<String> = origin.occurrences().map(|o| o.to_string()).collect();
            format!("o{}/{at}", path.join("/"))
        }
    })
}

/// FNV-1a: the same digest on every build and platform.
fn digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The roles of the faces of the copy of a tool body placed at the seed named `seed`, the body's
/// faces named `carried` in the tool design: `at:<seed>:link:<face>` (digests of the two names),
/// also answering to `at:<seed>:face:<k>`. Each copy's faces are named after its seed, so a
/// reference to one copy's face finds that copy's.
pub fn copy_roles(seed: &str, carried: &[String], faces: usize) -> Vec<Vec<String>> {
    let at = format!("at:{:016x}", digest(seed));
    (0..faces)
        .map(|k| {
            let numbered = format!("{at}:face:{k}");
            match carried.get(k).filter(|n| !n.is_empty()) {
                Some(name) => vec![format!("{at}:link:{:016x}", digest(name)), numbered],
                None => vec![numbered],
            }
        })
        .collect()
}

/// The turn every copy is given on its seed's frame: `turns` quarter turns about z, then, with
/// `flip`, over about x (its z against the seed's).
pub fn turn(turns: u32, flip: bool) -> crate::Mat {
    let (c, s) = match turns % 4 {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    };
    let f = if flip { -1.0 } else { 1.0 };
    // Columns x, y, z, origin: the turn about z after the flip over x (its y and z reversed),
    // Rz(turns) * Rx(flip ? 180° : 0).
    [[c, s, 0.0, 0.0], [-s * f, c * f, 0.0, 0.0], [0.0, 0.0, f, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
