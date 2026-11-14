//! Compose a STEP product structure into placed body instances.
//!
//! The record reader keeps the placements exactly as the file writes them: assembly
//! occurrences, `MAPPED_ITEM` targets, representation maps and transformation operators.
//! This module turns those raw placements into kernel matrices, walks the product tree and
//! returns every body with the transform that places it in the file's world frame.
//!
//! It computes no geometry of its own: frame and operator matrices, their products and
//! inverses come from `solidcore::geom::step_matrix`. Body construction is the caller's:
//! each returned [`PlacedBody::entity`] is a STEP entity id and every occurrence of a shared
//! body is emitted separately with its own transform.

use std::collections::BTreeSet;

use solidcore::geom::step_matrix::{
    concat, frame_matrix, inverse, operator_matrix, Matrix4, IDENTITY,
};
use solidcore_record::step::geom::Placement;
use solidcore_record::step::product::{
    Frame, ItemKind, Link, Occurrence, ProductStructure, Transformation, MAX_ASSEMBLY_DEPTH,
    MAX_VISITS,
};

/// One body as it appears in the expanded product structure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedBody {
    /// The STEP entity id of the body (`MANIFOLD_SOLID_BREP`, …).
    pub entity: u64,
    /// The body's placement in the file's world frame, in millimetres.
    pub transform: Matrix4,
}

struct Walk<'a> {
    structure: &'a ProductStructure,
    /// Representations reachable from each definition's own shapes through plain
    /// representation relationships.
    closure: Vec<Vec<usize>>,
    /// Child occurrences of each definition, in file order.
    children: Vec<Vec<usize>>,
    /// The definitions on the current path, to refuse a cycle.
    path: Vec<usize>,
    visits: usize,
    out: Vec<PlacedBody>,
}

/// Expand the product structure and return every body with its world placement.
///
/// A root definition is placed by the identity. Each occurrence multiplies the child's
/// placement by the occurrence transformation, and each mapped item places the mapped
/// representation by `target · origin⁻¹`. Refuses cycles and past the record's depth or
/// instance bounds.
pub fn placed_bodies(structure: &ProductStructure) -> Result<Vec<PlacedBody>, String> {
    let mut children = vec![Vec::new(); structure.definitions.len()];
    for (index, occurrence) in structure.occurrences.iter().enumerate() {
        if let Some(list) = children.get_mut(occurrence.parent) {
            list.push(index);
        }
    }
    let closure = (0..structure.definitions.len())
        .map(|definition| representation_closure(structure, definition))
        .collect();
    let mut walk = Walk {
        structure,
        closure,
        children,
        path: Vec::new(),
        visits: 0,
        out: Vec::new(),
    };
    for &root in &structure.roots {
        walk.definition(root, IDENTITY, 0)?;
    }
    Ok(walk.out)
}

fn representation_closure(structure: &ProductStructure, definition: usize) -> Vec<usize> {
    let Some(definition) = structure.definitions.get(definition) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut stack = definition.shapes.clone();
    while let Some(representation) = stack.pop() {
        if !seen.insert(representation) {
            continue;
        }
        out.push(representation);
        for link in &structure.links {
            if link.transformation.is_some() {
                continue;
            }
            if link.rep_1 == representation {
                stack.push(link.rep_2);
            } else if link.rep_2 == representation {
                stack.push(link.rep_1);
            }
        }
    }
    out
}

impl Walk<'_> {
    fn definition(
        &mut self,
        definition: usize,
        world: Matrix4,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_ASSEMBLY_DEPTH {
            return Err("assembly nested too deeply".into());
        }
        if self.path.contains(&definition) {
            return Err("assembly cycle".into());
        }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            return Err(format!("more than {MAX_VISITS} assembly instances"));
        }
        self.path.push(definition);
        let reps = self.closure.get(definition).cloned().unwrap_or_default();
        let mut walked = BTreeSet::new();
        for representation in reps {
            self.representation(representation, world, &mut walked, depth)?;
        }
        let child_definitions: Vec<usize> =
            self.children.get(definition).cloned().unwrap_or_default();
        for occurrence_index in child_definitions {
            let Some(occurrence) = self.structure.occurrences.get(occurrence_index) else {
                continue;
            };
            let child_reps = self
                .closure
                .get(occurrence.child)
                .cloned()
                .unwrap_or_default();
            let local = occurrence_matrix(self.structure, occurrence, &child_reps)?;
            self.definition(occurrence.child, concat(&world, &local), depth + 1)?;
        }
        self.path.pop();
        Ok(())
    }

    fn representation(
        &mut self,
        representation: usize,
        world: Matrix4,
        walked: &mut BTreeSet<usize>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_ASSEMBLY_DEPTH {
            return Err("mapped items nested too deeply".into());
        }
        if !walked.insert(representation) {
            return Ok(());
        }
        let Some(rep) = self.structure.representations.get(representation) else {
            return Err(format!("missing representation #{representation}"));
        };
        for item in &rep.items {
            match &item.kind {
                ItemKind::Body(_) => self.out.push(PlacedBody {
                    entity: item.entity,
                    transform: world,
                }),
                ItemKind::Mapped { map, target } => {
                    let Some(map) = self.structure.maps.get(*map) else {
                        return Err(format!("missing representation map #{}", item.entity));
                    };
                    let target = frame(target)?;
                    let origin = frame(&map.origin)?;
                    let origin_inverse = inverse(&origin)
                        .ok_or_else(|| format!("singular map origin #{}", map.entity))?;
                    let local = concat(&target, &origin_inverse);
                    let placed = concat(&world, &local);
                    let mapped = map.representation;
                    let mut mapped_walked = BTreeSet::new();
                    self.representation(mapped, placed, &mut mapped_walked, depth + 1)?;
                    for link in &self.structure.links {
                        if link.transformation.is_some() {
                            continue;
                        }
                        let other = if link.rep_1 == mapped {
                            Some(link.rep_2)
                        } else if link.rep_2 == mapped {
                            Some(link.rep_1)
                        } else {
                            None
                        };
                        if let Some(other) = other {
                            self.representation(other, placed, &mut mapped_walked, depth + 1)?;
                        }
                    }
                }
                ItemKind::Other(_) => {}
            }
        }
        Ok(())
    }
}

fn occurrence_matrix(
    structure: &ProductStructure,
    occurrence: &Occurrence,
    child_reps: &[usize],
) -> Result<Matrix4, String> {
    let Some(&link_index) = occurrence.placements.first() else {
        return Ok(IDENTITY);
    };
    let Some(link) = structure.links.get(link_index) else {
        return Err(format!("missing placement link #{link_index}"));
    };
    link_matrix(link, child_reps)
}

fn link_matrix(link: &Link, child_reps: &[usize]) -> Result<Matrix4, String> {
    let Some(transformation) = &link.transformation else {
        return Ok(IDENTITY);
    };
    match transformation {
        Transformation::Frame(frame_value) => frame(frame_value),
        Transformation::Items { item_1, item_2, .. } => {
            let first = frame(item_1)?;
            let second = frame(item_2)?;
            let child_is_first =
                child_reps.contains(&link.rep_1) || !child_reps.contains(&link.rep_2);
            // Row-vector convention: a child in rep_1 is placed by
            // `item_1⁻¹ · item_2`; a child in rep_2 by `item_2⁻¹ · item_1`.
            let (from, to) = if child_is_first {
                (&first, &second)
            } else {
                (&second, &first)
            };
            let inverse_from = inverse(from)
                .ok_or_else(|| format!("singular occurrence placement #{}", link.entity))?;
            Ok(concat(&inverse_from, to))
        }
    }
}

fn frame(value: &Frame) -> Result<Matrix4, String> {
    match value {
        Frame::Placement(Placement {
            origin,
            axis,
            ref_direction,
        }) => frame_matrix(*origin, *axis, *ref_direction)
            .ok_or_else(|| "degenerate placement".to_string()),
        Frame::Operator(operator) => operator_matrix(
            operator.axis1,
            operator.axis2,
            operator.axis3,
            operator.origin,
            operator.scale,
            operator.scale2,
            operator.scale3,
        )
        .ok_or_else(|| "degenerate transformation operator".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::step::geom::Placement;
    use solidcore_record::step::product::{
        BodyKind, Definition, Item, OccurrenceKind, Representation,
    };
    use solidcore_record::step::units::{Context, Units};

    fn translation(x: f64, y: f64, z: f64) -> Frame {
        Frame::Placement(Placement {
            origin: [x, y, z],
            axis: None,
            ref_direction: None,
        })
    }

    fn body(entity: u64) -> Item {
        Item {
            entity,
            name: String::new(),
            kind: ItemKind::Body(BodyKind::ManifoldSolidBrep),
        }
    }

    fn representation(entity: u64, items: Vec<Item>) -> Representation {
        Representation {
            entity,
            kind: "SHAPE_REPRESENTATION".into(),
            name: String::new(),
            items,
            context: None,
            units: Context {
                units: Units { len: 1.0, ang: 1.0 },
                uncertainty: None,
            },
        }
    }

    fn definition(entity: u64, shape: usize) -> Definition {
        Definition {
            entity,
            id: format!("def{entity}"),
            description: String::new(),
            formation: 0,
            product: 0,
            shapes: vec![shape],
        }
    }

    fn structure() -> ProductStructure {
        ProductStructure {
            products: Vec::new(),
            definitions: vec![definition(1, 0), definition(2, 1)],
            representations: vec![
                representation(10, Vec::new()),
                representation(20, vec![body(200)]),
            ],
            maps: Vec::new(),
            links: Vec::new(),
            occurrences: Vec::new(),
            roots: vec![0],
            styles: Vec::new(),
        }
    }

    #[test]
    fn a_placed_child_body_is_transformed_by_its_occurrence() {
        let mut structure = structure();
        structure.representations[0] = representation(10, Vec::new());
        structure.occurrences.push(Occurrence {
            entity: 30,
            kind: OccurrenceKind::NextAssemblyUsage,
            id: "occ".into(),
            name: String::new(),
            description: String::new(),
            parent: 0,
            child: 1,
            reference_designator: None,
            placements: vec![0],
        });
        structure.links.push(Link {
            entity: 40,
            rep_1: 1,
            rep_2: 0,
            transformation: Some(Transformation::Items {
                entity: 41,
                item_1: translation(0.0, 0.0, 0.0),
                item_2: translation(2.0, 3.0, 0.0),
            }),
        });
        let placed = placed_bodies(&structure).unwrap();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].entity, 200);
        assert_eq!(
            placed[0].transform,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [2.0, 3.0, 0.0, 1.0],
            ]
        );
    }

    #[test]
    fn mapped_items_place_the_map_origin_onto_their_target() {
        let mut structure = structure();
        structure.representations[0] = representation(10, Vec::new());
        structure.representations[0].items.push(Item {
            entity: 100,
            name: String::new(),
            kind: ItemKind::Mapped {
                map: 0,
                target: translation(5.0, 0.0, 0.0),
            },
        });
        structure
            .maps
            .push(solidcore_record::step::product::RepresentationMap {
                entity: 50,
                origin: translation(1.0, 0.0, 0.0),
                representation: 1,
            });
        let placed = placed_bodies(&structure).unwrap();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].transform[3], [4.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn nested_occurrence_placements_compose() {
        let mut structure = structure();
        structure.definitions.push(definition(3, 2));
        structure
            .representations
            .push(representation(30, vec![body(300)]));
        structure.occurrences.extend([
            Occurrence {
                entity: 31,
                kind: OccurrenceKind::NextAssemblyUsage,
                id: "a".into(),
                name: String::new(),
                description: String::new(),
                parent: 0,
                child: 1,
                reference_designator: None,
                placements: vec![0],
            },
            Occurrence {
                entity: 32,
                kind: OccurrenceKind::NextAssemblyUsage,
                id: "b".into(),
                name: String::new(),
                description: String::new(),
                parent: 1,
                child: 2,
                reference_designator: None,
                placements: vec![1],
            },
        ]);
        structure.links.extend([
            Link {
                entity: 40,
                rep_1: 1,
                rep_2: 0,
                transformation: Some(Transformation::Items {
                    entity: 41,
                    item_1: translation(0.0, 0.0, 0.0),
                    item_2: translation(1.0, 0.0, 0.0),
                }),
            },
            Link {
                entity: 42,
                rep_1: 2,
                rep_2: 1,
                transformation: Some(Transformation::Items {
                    entity: 43,
                    item_1: translation(0.0, 0.0, 0.0),
                    item_2: translation(0.0, 2.0, 0.0),
                }),
            },
        ]);
        let placed = placed_bodies(&structure).unwrap();
        let grandchild = placed
            .iter()
            .find(|body| body.entity == 300)
            .expect("grandchild body");
        assert_eq!(grandchild.transform[3], [1.0, 2.0, 0.0, 1.0]);
    }

    #[test]
    fn a_cycle_is_refused() {
        let mut structure = structure();
        structure.occurrences.push(Occurrence {
            entity: 30,
            kind: OccurrenceKind::NextAssemblyUsage,
            id: "occ".into(),
            name: String::new(),
            description: String::new(),
            parent: 0,
            child: 0,
            reference_designator: None,
            placements: Vec::new(),
        });
        assert!(placed_bodies(&structure).is_err());
    }
}
