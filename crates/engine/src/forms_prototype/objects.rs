//! Additional Forms stay staged until Finish; editor Cancel never mutates the document.
use super::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    pub definition: Arc<crate::doc::forms::Definition>,
}
impl Form {
    pub fn from_import(definitions: Vec<(String, crate::doc::forms::Definition, usize)>) -> Result<Self> {
        if definitions.is_empty() || definitions.len() > 16 {
            return Err(error("import 1..16 Forms at a time"));
        }
        let mut definitions = definitions.into_iter();
        let (_, definition, levels) = definitions.next().ok_or_else(|| error("no imported Form"))?;
        let levels = definition.cage.preview_level(levels).map_err(error)?;
        let mut form = Self::from_definition(definition, levels, None)?;
        for (name, definition, _) in definitions {
            let id = form.next_part;
            form.next_part += 1;
            form.parts.insert(id, Part { name, definition: Arc::new(definition) });
        }
        Ok(form)
    }
    pub fn replace_import(&mut self, mut next: Self) {
        let parts = std::mem::take(&mut next.parts);
        let next_part = next.next_part;
        self.replace_primitive(next);
        self.parts = parts;
        self.next_part = next_part;
    }
    pub fn adopt_pipe(&mut self, definition: crate::doc::forms::Definition) -> Result<()> {
        if definition.pipe.is_none() || !self.parts.is_empty() || !self.consumed.is_empty() {
            return Err(error("finish staged Forms before adopting a pipe source"));
        }
        self.adopt_definition(definition, true)
    }
    pub fn adopt_profile(&mut self, definition: crate::doc::forms::Definition) -> Result<()> {
        if !self.parts.is_empty() || !self.consumed.is_empty() {
            return Err(error("finish the staged Forms before creating a Form Surface"));
        }
        self.adopt_definition(definition, true)
    }
    pub fn consumed_features(&self) -> impl Iterator<Item = u64> + '_ {
        self.consumed.keys().copied()
    }
    pub fn consumes(&self, feature: u64) -> bool {
        self.consumed.contains_key(&feature)
    }
    pub(super) fn adopt_definition(&mut self, definition: crate::doc::forms::Definition, checkpoint: bool) -> Result<()> {
        let levels = definition.cage.preview_level(self.levels).map_err(error)?;
        let mut candidate = Self::from_definition(definition, levels, self.editing_feature)?;
        if candidate.pipe.as_ref().zip(self.pipe.as_ref()).is_some_and(|(a, b)| a.source == b.source) {
            candidate.pipe_segments = self.pipe_segments.clone();
        }
        if checkpoint {
            self.checkpoint();
        }
        candidate.undo = std::mem::take(&mut self.undo);
        candidate.parts = std::mem::take(&mut self.parts);
        candidate.consumed = std::mem::take(&mut self.consumed);
        candidate.next_part = self.next_part;
        candidate.authoring_component = self.authoring_component;
        candidate.authoring_path = self.authoring_path.clone();
        candidate.tool_original = self.tool_original.clone();
        candidate.revision = self.revision.wrapping_add(1);
        *self = candidate;
        Ok(())
    }
    pub fn unweld_parts(&mut self, edges: &[[usize; 2]], separate: bool, name: &str) -> Result<Option<u64>> {
        if self.hierarchy.is_some() {
            return Err(error("Unweld requires a standalone base cage"));
        }
        if self.parts.len() >= 16 || name.is_empty() || name.len() > 256 || name.contains('\0') {
            return Err(error("Unweld requires a bounded part count and name"));
        }
        let [a, b] = self.cage.split_seam(edges).map_err(error)?;
        let mut definition = self.definition();
        if let Some(pipe) = &mut definition.pipe {
            pipe.follow_path = false;
        }
        definition.symmetry = None;
        if !separate {
            definition.cage = a.disjoint_union(&b).map_err(error)?;
            self.adopt_definition(definition, true)?;
            return Ok(None);
        }
        let id = self.next_part;
        let next = id.checked_add(1).ok_or_else(|| error("Form part identity budget exceeded"))?;
        let mut other = definition.clone();
        other.cage = b;
        definition.cage = a;
        self.adopt_definition(definition, true)?;
        self.parts.insert(id, Part { name: name.to_owned(), definition: Arc::new(other) });
        self.next_part = next;
        Ok(Some(id))
    }
    pub fn weld_definition(&mut self, other: &crate::doc::forms::Definition, first: [usize; 2], second: [usize; 2]) -> Result<()> {
        if self.hierarchy.is_some() || other.hierarchy.is_some() {
            return Err(error("Weld requires standalone base cages without local refinement"));
        }
        let mut cage = other.cage.clone();
        other.placement.validate()?;
        for p in &mut cage.points {
            *p = self.placement.local_point(other.placement.point(*p));
        }
        let cage = self.cage.weld_openings(&cage, first, second).map_err(error)?;
        let mut definition = self.definition();
        if let Some(pipe) = &mut definition.pipe {
            pipe.follow_path = false;
        }
        definition.cage = cage;
        definition.symmetry = None;
        self.adopt_definition(definition, true)
    }
}
