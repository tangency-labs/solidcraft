//! Figures: the points and curves one creation command made as a unit.
//!
//! A rectangle, polygon or slot is stored as loose points and lines, so a browser that lists
//! every entity cannot show the shape the user drew. A [`Figure`] records which entities the
//! command created, in creation order, and which existing entities it was built from. It is
//! display and history metadata: the solver and the evaluated geometry never read it.
//!
//! The browser hides the entities a figure *created* from the Points/Lines groups and lists them
//! under the figure instead. Entities that existed before the figure (an adopted edge) stay in
//! their groups, because the user drew them as loose geometry.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{Sketch, SketchError};

/// Most figures one sketch may hold.
pub const MAX_FIGURES: usize = 10_000;
/// Longest figure id, kind or command string a sketch accepts.
pub const MAX_FIGURE_TEXT: usize = 512;
/// Most member references (created and adopted, over every figure) a sketch accepts.
pub const MAX_FIGURE_REFERENCES: usize = 1_000_000;

/// One shape a command drew as a unit, with the entities it made and the ones it reused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Figure {
    pub id: String,
    /// What kind of shape it is: `rectangle`, `rectangle.rounded`, `polygon`, `slot`,
    /// `slot.arc`, … A string, not an enum: a newer build's kind stays readable here and falls
    /// back to a generic row, instead of making the whole sketch unreadable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// The command that drew it, for the row's tooltip and the history.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// Curves the command created. The browser hides these from the geometry groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<String>,
    /// Points the command created. Hidden from the Points group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<String>,
    /// Curves that existed before the figure and were built into it. They stay in the geometry
    /// groups: the user drew them as loose geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adopted_curves: Vec<String>,
    /// Points that existed before the figure and were built into it. They stay in Points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adopted_points: Vec<String>,
}

impl Figure {
    /// Every curve id the figure lists, created first.
    pub fn curve_ids(&self) -> impl Iterator<Item = &String> {
        self.curves.iter().chain(self.adopted_curves.iter())
    }

    /// Every point id the figure lists, created first.
    pub fn point_ids(&self) -> impl Iterator<Item = &String> {
        self.points.iter().chain(self.adopted_points.iter())
    }
}

fn text_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_FIGURE_TEXT
}

fn member_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_FIGURE_TEXT
}

impl Sketch {
    /// Record a figure the command `command` just drew: the curves and points it created, and
    /// the existing entities it adopted. Returns the figure's id.
    ///
    /// The caller has already added the geometry; a failure here leaves the sketch unchanged.
    pub fn add_figure(
        &mut self,
        kind: &str,
        command: &str,
        curves: &[String],
        points: &[String],
        adopted_curves: &[String],
        adopted_points: &[String],
    ) -> Result<String, SketchError> {
        if !text_ok(kind) || (command.len() > MAX_FIGURE_TEXT) {
            return Err(SketchError::Invalid("invalid figure kind or command".into()));
        }
        if self.figures.len() >= MAX_FIGURES {
            return Err(SketchError::TooLarge);
        }
        if curves.is_empty() && points.is_empty() {
            return Err(SketchError::Invalid("a figure needs at least one entity".into()));
        }
        let owned = self.figure_created_ids();
        let mut seen = BTreeSet::new();
        for id in curves.iter().chain(points.iter()) {
            if !member_ok(id) || !seen.insert(id.as_str()) {
                return Err(SketchError::Invalid("invalid figure member".into()));
            }
            if self.curve_index(id).is_none() && self.point_index(id).is_none() {
                return Err(SketchError::Unknown(id.clone()));
            }
            if owned.contains(id.as_str()) {
                return Err(SketchError::Invalid(format!("`{id}` already belongs to a figure")));
            }
        }
        for id in adopted_curves.iter().chain(adopted_points.iter()) {
            if !member_ok(id) || !seen.insert(id.as_str()) {
                return Err(SketchError::Invalid("invalid figure member".into()));
            }
            if self.curve_index(id).is_none() && self.point_index(id).is_none() {
                return Err(SketchError::Unknown(id.clone()));
            }
        }
        let id = self.fresh("fig")?;
        self.figures.push(Figure {
            id: id.clone(),
            kind: kind.to_string(),
            command: command.to_string(),
            curves: curves.to_vec(),
            points: points.to_vec(),
            adopted_curves: adopted_curves.to_vec(),
            adopted_points: adopted_points.to_vec(),
        });
        Ok(id)
    }

    /// The ids of every entity a figure created. The browser skips these in the geometry groups.
    pub fn figure_created_ids(&self) -> BTreeSet<&str> {
        self.figures.iter().flat_map(|f| f.curves.iter().chain(f.points.iter())).map(String::as_str).collect()
    }

    /// The figure that created `id`, if one does.
    pub fn figure_of(&self, id: &str) -> Option<&Figure> {
        self.figures.iter().find(|f| f.curves.iter().chain(f.points.iter()).any(|m| m == id))
    }

    /// Check the saved figures: ids, lengths, members and exclusivity. Hostile input and stale
    /// member ids are refused; a design with a damaged figure does not open.
    pub fn validate_figures(&self) -> Result<(), SketchError> {
        if self.figures.len() > MAX_FIGURES {
            return Err(SketchError::TooLarge);
        }
        let curves: BTreeSet<&str> = self.curves.iter().map(|c| c.id.as_str()).collect();
        let points: BTreeSet<&str> = self.points.iter().map(|p| p.id.as_str()).collect();
        let mut ids = BTreeSet::new();
        let mut owned = BTreeSet::new();
        let mut references = 0usize;
        for figure in &self.figures {
            if !text_ok(&figure.id) || !ids.insert(&figure.id) || figure.kind.len() > MAX_FIGURE_TEXT || figure.command.len() > MAX_FIGURE_TEXT {
                return Err(SketchError::Invalid("invalid sketch figure".into()));
            }
            references = references
                .saturating_add(figure.curves.len())
                .saturating_add(figure.points.len())
                .saturating_add(figure.adopted_curves.len())
                .saturating_add(figure.adopted_points.len());
            if references > MAX_FIGURE_REFERENCES {
                return Err(SketchError::TooLarge);
            }
            let mut seen = BTreeSet::new();
            for id in figure.curves.iter().chain(figure.points.iter()) {
                if !member_ok(id) || !seen.insert(id.as_str()) || !(curves.contains(id.as_str()) || points.contains(id.as_str())) {
                    return Err(SketchError::Invalid("invalid sketch figure member".into()));
                }
                if !owned.insert(id.as_str()) {
                    return Err(SketchError::Invalid(format!("`{id}` belongs to two figures")));
                }
            }
            for id in figure.adopted_curves.iter().chain(figure.adopted_points.iter()) {
                if !member_ok(id) || !seen.insert(id.as_str()) || !(curves.contains(id.as_str()) || points.contains(id.as_str())) {
                    return Err(SketchError::Invalid("invalid sketch figure member".into()));
                }
            }
        }
        Ok(())
    }

    /// Drop a figure whose created geometry is no longer there, and forget adopted members that
    /// were deleted. Called after curves or points are removed.
    pub fn prune_figures(&mut self) {
        if self.figures.is_empty() {
            return;
        }
        let curves: BTreeSet<&str> = self.curves.iter().map(|c| c.id.as_str()).collect();
        let points: BTreeSet<&str> = self.points.iter().map(|p| p.id.as_str()).collect();
        let exists = |id: &String| curves.contains(id.as_str()) || points.contains(id.as_str());
        self.figures.retain(|figure| figure.curves.iter().chain(figure.points.iter()).all(exists));
        for figure in &mut self.figures {
            figure.adopted_curves.retain(exists);
            figure.adopted_points.retain(exists);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec2;

    fn rectangle(sk: &mut Sketch) -> (Vec<String>, Vec<String>) {
        let pts: Vec<usize> =
            [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)].into_iter().map(|(x, y)| sk.add_point(Vec2::new(x, y), None).unwrap()).collect();
        let curves = (0..4)
            .map(|i| {
                let line = sk.add_line_pts(pts[i], pts[(i + 1) % 4], None).unwrap();
                sk.curves[line].id.clone()
            })
            .collect();
        let points = pts.iter().map(|i| sk.points[*i].id.clone()).collect();
        (curves, points)
    }

    #[test]
    fn a_figure_records_the_entities_its_command_made() {
        let mut sk = Sketch::new();
        let (curves, points) = rectangle(&mut sk);
        let id = sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves, &points, &[], &[]).unwrap();
        assert_eq!(id, "fig1");
        let figure = sk.figure_of(&curves[0]).unwrap();
        assert_eq!(figure.id, "fig1");
        assert_eq!(figure.curves, curves);
        assert_eq!(figure.points, points);
        assert!(sk.validate_figures().is_ok());
    }

    #[test]
    fn a_created_entity_belongs_to_one_figure_and_ids_are_never_reused() {
        let mut sk = Sketch::new();
        let (curves, points) = rectangle(&mut sk);
        sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves, &points, &[], &[]).unwrap();
        assert!(sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves[..1], &[], &[], &[]).is_err());
        // Deleting the figure's geometry drops it; a new figure still gets the next id.
        let indices: Vec<usize> = curves.iter().filter_map(|c| sk.curve_index(c)).collect();
        sk.remove_curves(&indices);
        assert!(sk.figures.is_empty());
        let (curves, points) = rectangle(&mut sk);
        let id = sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves, &points, &[], &[]).unwrap();
        assert_eq!(id, "fig2");
    }

    #[test]
    fn deleting_one_member_drops_the_figure_and_keeps_the_rest() {
        let mut sk = Sketch::new();
        let (curves, points) = rectangle(&mut sk);
        sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves, &points, &[], &[]).unwrap();
        let one = sk.curve_index(&curves[0]).unwrap();
        sk.remove_curves(&[one]);
        assert!(sk.figures.is_empty());
        assert_eq!(sk.curves.len(), 3, "the other sides stay as loose geometry");
    }

    #[test]
    fn an_adopted_member_stays_when_it_is_deleted_from_the_figure() {
        let mut sk = Sketch::new();
        let (curves, points) = rectangle(&mut sk);
        sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves[..3], &points, &curves[3..], &[]).unwrap();
        let adopted = sk.curve_index(&curves[3]).unwrap();
        sk.remove_curves(&[adopted]);
        assert_eq!(sk.figures.len(), 1);
        assert!(sk.figures[0].adopted_curves.is_empty());
    }

    #[test]
    fn a_figure_round_trips_and_an_old_sketch_without_it_loads() {
        let mut sk = Sketch::new();
        let (curves, points) = rectangle(&mut sk);
        sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves, &points, &[], &[]).unwrap();
        let text = serde_json::to_string(&sk).unwrap();
        let back: Sketch = serde_json::from_str(&text).unwrap();
        assert_eq!(back, sk);

        let mut old = serde_json::to_value(Sketch::new()).unwrap();
        old.as_object_mut().unwrap().remove("figures");
        let read: Sketch = serde_json::from_value(old).unwrap();
        assert!(read.figures.is_empty());
    }

    #[test]
    fn validation_refuses_stale_and_duplicate_members() {
        let mut sk = Sketch::new();
        let (curves, points) = rectangle(&mut sk);
        sk.add_figure("rectangle", "sketch.rectangle.two_point", &curves, &points, &[], &[]).unwrap();
        let mut broken = sk.clone();
        broken.figures[0].curves.push("l999".into());
        assert!(broken.validate_figures().is_err());
        let mut twice = sk.clone();
        let mut second = twice.figures[0].clone();
        second.id = "fig9".into();
        twice.figures.push(second);
        assert!(twice.validate_figures().is_err());
    }
}
