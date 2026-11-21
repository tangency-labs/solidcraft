//! Amalgamate (#18799): a design whose bodies an Amalgam Tag labels is a tool that other designs
//! place. At each place, the tool's Remove bodies are cut from the bodies named for them, then
//! its Add bodies are joined into the bodies named for those, and its New bodies come in as a
//! part of their own. The tag itself changes no geometry: evaluating it only checks it.
//!
//! A design may hold several tags; each is a tool of its own, named by its feature's name. A
//! tag belongs to the design that declares it: placing a tool brings in its geometry, never its
//! tags, so a design that places tools and tags its own bodies is a tool made of tools.

use crate::{BodyName, DocError, ModelState, Result};

/// The most bodies one tag labels, over all its roles.
pub const MAX_TAGGED: usize = 256;

/// What a tagged body does where the tool is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Checks an Amalgam Tag against the bodies `st` holds where it stands in the timeline: it tags
/// at least one body and at most [`MAX_TAGGED`], each an existing body with one role, no mesh
/// body, and only bodies that enclose a volume for Remove and Add (they are cut and joined).
pub fn check_tag(st: &ModelState, remove: &[BodyName], add: &[BodyName], new: &[BodyName]) -> Result<()> {
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
    Ok(())
}
