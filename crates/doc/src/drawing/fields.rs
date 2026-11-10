//! A title block's fields (#3875): the keys each source has, and what they read. Stateless: the
//! drawing gives its own fields, its sheets' and its newest revision's, and the caller hands in the
//! component a sheet documents with what the model says of it (its material and mass, worked out
//! by the bill of materials). A key its source doesn't have is refused when a block is given or
//! read; a value that isn't there, or is empty, shows [`MISSING`]. Each field is shown, empty or
//! lost ([`Values::state`]): lost when the sheet documents a component the design no longer has,
//! so a reader can tell the two apart while the paper shows neither as stale text. A note's field its
//! source doesn't have is refused when it is given ([`unknown_field`]), but a drawing saved with one
//! still opens: the field reads unknown. Sheet tables'
//! cells (B3) and notes read the same fields.
//!
//! - **component:** `name`, `part_number`, `description`, `revision` (its part properties),
//!   `material`, `mass`, and `properties.<name>` (its free-text properties, such as a finish);
//! - **drawing:** `name`, `number` (its `part_number` property: the drawing's identification
//!   number; without one, the name of the component the sheet documents), `organization`, `author`,
//!   `project`, and `properties.<key>` (any of its properties);
//! - **sheet:** `name`, `number`, `count`: sheets left out of the count are neither counted nor
//!   numbered, and such a sheet shows neither; `scale`: the scale its base views share (neither a
//!   drawing nor a sheet sets a scale of its own), written `1:2`; a sheet with no base view, such as
//!   one of projected views only, shows the scale its views draw at. A sheet whose base views (or,
//!   without any, its views) differ in scale shows none, as each view then carries its own scale label;
//! - **revision:** the newest revision row's fields ([`Field`]);
//! - **prompt:** any key; answers come with templates, so a prompt shows [`MISSING`] until then.

use solidcraft_geom::cell::{Content, MISSING, Part, Source};
use solidcraft_geom::drawing_view::Scale;
use solidcraft_geom::revision::{Field, RevisionRow};
use solidcraft_geom::title_block::{CellKind, TitleBlock};

use super::Drawing;
use super::view::ViewKind;
use crate::{DocError, Metadata, Result};

const COMPONENT: [&str; 6] = ["name", "part_number", "description", "revision", "material", "mass"];
const DRAWING: [&str; 5] = ["name", "number", "organization", "author", "project"];
const SHEET: [&str; 4] = ["name", "number", "count", "scale"];
/// Before a property's name: the field reads that property.
const PROPERTIES: &str = "properties.";

/// The component a sheet documents, as its fields read it.
#[derive(Clone, Copy, Debug)]
pub struct Component<'a> {
    pub name: &'a str,
    /// Its part properties.
    pub metadata: &'a Metadata,
    /// Its free-text properties (the design's own: none).
    pub properties: Option<&'a std::collections::BTreeMap<String, String>>,
    /// What it is made of, as its bill of materials row would say: none when nothing is assigned.
    pub material: Option<&'a str>,
    /// What one of it weighs, written for the sheet: none when it can't be weighed.
    pub mass: Option<&'a str>,
}

/// The component a sheet documents, as its fields find it.
#[derive(Clone, Copy, Debug)]
pub enum Documented<'a> {
    /// The sheet documents none.
    Nothing,
    /// The sheet documents a component the design no longer has: its fields are lost.
    Gone,
    Present(Component<'a>),
}

impl<'a> Documented<'a> {
    fn component(self) -> Option<Component<'a>> {
        match self {
            Documented::Present(component) => Some(component),
            Documented::Nothing | Documented::Gone => None,
        }
    }
}

/// What a field reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldState {
    Shown(String),
    /// Its source has no value for it, or an empty one.
    Empty,
    /// Its source is gone: the sheet documents a component the design no longer has.
    Lost,
    /// Its source has no such field ([`no_field`] says why).
    Unknown,
}

/// Whether `source` has the field `key`.
pub fn has_key(source: Source, key: &str) -> bool {
    let property = key.strip_prefix(PROPERTIES).is_some_and(|name| !name.is_empty());
    match source {
        Source::Component => COMPONENT.contains(&key) || property,
        Source::Drawing => DRAWING.contains(&key) || property,
        Source::Sheet => SHEET.contains(&key),
        Source::Revision => Field::ALL.iter().any(|f| f.key() == key),
        Source::Prompt => !key.is_empty(),
        Source::Row => false,
    }
}

/// Why `source` can't have the field `key`, when it can't.
pub fn no_field(source: Source, key: &str) -> Option<String> {
    (!has_key(source, key)).then(|| format!("the {} has no field `{key}`", source_name(source)))
}

/// Why `content` can't be read, when it can't: the first of its fields its source doesn't have.
pub fn unknown_field(content: &Content) -> Option<String> {
    let unknown = |part: &Part| match part {
        Part::Field { source, key, .. } => no_field(*source, key),
        Part::Text(_) => None,
    };
    content.0.iter().find_map(unknown)
}

/// Whether `block` has a text cell `name`: what a sheet's override may name.
pub fn overridable(block: Option<&TitleBlock>, name: &str) -> bool {
    block.is_some_and(|block| block.cells.iter().any(|cell| cell.name == name && matches!(cell.kind, CellKind::Text(_))))
}

/// Each field of `block` names a key its source has: what a sheet holds its block to.
pub fn check_block(block: &TitleBlock) -> Result<()> {
    for cell in &block.cells {
        let CellKind::Text(text) = &cell.kind else { continue };
        if let Some(why) = unknown_field(&text.content) {
            return Err(DocError::Invalid(format!("cell `{}`: {why}", cell.name)));
        }
    }
    Ok(())
}

/// A field's source as a block names it: `component`, `drawing`, `sheet` and so on.
fn source_name(source: Source) -> String {
    serde_json::to_value(source).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// What a sheet's fields read: its drawing, its place among the drawing's sheets, and the component
/// it documents.
#[derive(Clone, Copy, Debug)]
pub struct Values<'a> {
    pub drawing: &'a Drawing,
    pub sheet: usize,
    pub component: Documented<'a>,
}

impl Values<'_> {
    /// `content` as its cell shows it: text as written, each field its value or [`MISSING`].
    pub fn text(&self, content: &Content) -> String {
        let part = |part: &Part| match part {
            Part::Text(text) => text.clone(),
            Part::Field { source, key, .. } => match self.state(*source, key) {
                FieldState::Shown(value) => value,
                FieldState::Empty | FieldState::Lost | FieldState::Unknown => MISSING.into(),
            },
        };
        content.0.iter().map(part).collect()
    }

    /// What the field `key` of `source` reads.
    pub fn state(&self, source: Source, key: &str) -> FieldState {
        if !has_key(source, key) {
            return FieldState::Unknown;
        }
        match self.field(source, key) {
            Some(value) if !value.is_empty() => FieldState::Shown(value),
            _ if source == Source::Component && matches!(self.component, Documented::Gone) => FieldState::Lost,
            _ => FieldState::Empty,
        }
    }

    /// The fields of `content` that read no value ([`state`](Values::state): empty, lost or unknown),
    /// each as `source.key` (`component.part_number`): what [`text`](Values::text) shows as [`MISSING`].
    /// A revision field before the sheet's first revision row reads empty but isn't one: the drawing
    /// has no revision to show yet.
    pub fn empty_fields(&self, content: &Content) -> Vec<String> {
        let unrevised = self.newest().is_none();
        let empty = |part: &Part| match part {
            Part::Field { source: Source::Revision, .. } if unrevised => None,
            Part::Field { source, key, .. } => {
                (!matches!(self.state(*source, key), FieldState::Shown(_))).then(|| format!("{}.{key}", source_name(*source)))
            }
            Part::Text(_) => None,
        };
        content.0.iter().filter_map(empty).collect()
    }

    /// The newest revision row in the sheet's scope: the drawing's, or the sheet's own once rows are
    /// per sheet.
    fn newest(&self) -> Option<&RevisionRow> {
        let d = self.drawing;
        let scope = if d.revisions.per_sheet { Some(d.sheets.get(self.sheet)?.id) } else { None };
        d.revisions.newest(scope)
    }

    /// The field `key` of `source`, when it has a value.
    fn field(&self, source: Source, key: &str) -> Option<String> {
        let d = self.drawing;
        let property = |metadata: &Metadata| key.strip_prefix(PROPERTIES).and_then(|name| metadata.text(name)).map(str::to_string);
        match source {
            Source::Drawing => match key {
                "name" => Some(d.name.clone()),
                "number" => d.properties.part_number().or(self.component.component().map(|c| c.name)).map(str::to_string),
                "organization" | "author" | "project" => d.properties.text(key).map(str::to_string),
                _ => property(&d.properties),
            },
            Source::Sheet => {
                let sheet = d.sheets.get(self.sheet)?;
                if key == "scale" {
                    // The sheet's main scale: its base views', as a detail or projected view at its own scale
                    // carries that scale in its label; with no base view, the scale its views draw at.
                    let on_sheet = || d.views.iter().filter(|v| v.sheet == sheet.id);
                    let mut base = on_sheet().filter(|v| matches!(v.kind, ViewKind::Base { .. })).peekable();
                    let scale = if base.peek().is_some() {
                        shared(base.map(|v| v.settings.scale))
                    } else {
                        shared(on_sheet().map(|v| d.resolve_view(v.id).ok().map(|r| r.scale.value)))
                    };
                    return scale.map(|s| s.to_string());
                }
                if sheet.excluded.count {
                    return (key == "name").then(|| sheet.name.clone());
                }
                let counted = |s: &&std::sync::Arc<super::Sheet>| !s.excluded.count;
                match key {
                    "name" => Some(sheet.name.clone()),
                    "number" => Some((d.sheets.iter().take(self.sheet).filter(counted).count() + 1).to_string()),
                    "count" => Some(d.sheets.iter().filter(counted).count().to_string()),
                    _ => None,
                }
            }
            Source::Revision => self.newest()?.field(key).map(str::to_string),
            Source::Component => {
                let c = self.component.component()?;
                match key {
                    "name" => Some(c.name.to_string()),
                    "material" => c.material.map(str::to_string),
                    "mass" => c.mass.map(str::to_string),
                    "part_number" | "description" | "revision" => c.metadata.text(key).map(str::to_string),
                    _ => key.strip_prefix(PROPERTIES).and_then(|name| c.properties?.get(name)).cloned(),
                }
            }
            Source::Prompt | Source::Row => None,
        }
    }
}

/// The one scale all of `scales` are, so neither the order the views were placed in nor their ids
/// decide it: none when there are none, or when one is unknown or differs.
fn shared(mut scales: impl Iterator<Item = Option<Scale>>) -> Option<Scale> {
    let first = scales.next()??;
    scales.all(|scale| scale == Some(first)).then_some(first)
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
