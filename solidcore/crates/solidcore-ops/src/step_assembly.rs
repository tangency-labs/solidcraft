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
/// A root definition is placed by the identity. In the row-vector convention a child's world
/// placement is its occurrence transformation followed by its parent's world placement
/// (`local · world`), and each mapped item places the mapped representation by
/// `origin⁻¹ · target`. Refuses cycles and past the record's depth or instance bounds.
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
            self.definition(occurrence.child, concat(&local, &world), depth + 1)?;
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
                    let local = concat(&origin_inverse, &target);
                    let placed = concat(&local, &world);
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
            // The reader keeps `item_1` with `rep_1`, whichever way the file lists them.
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

    /// Minimal synthetic STEP text for the placement cases below: parts, placements, mapped
    /// items and assembly occurrences, written the way CAD exporters write them.
    mod fixtures {
        use std::fmt::Write as _;

        use solidcore_record::step::{p21, product};

        use super::super::{placed_bodies, PlacedBody};

        const HEADER: &str =
            "ISO-10303-21;\nHEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n\
#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n\
#9=APPLICATION_CONTEXT('');\n#100=CLOSED_SHELL('',());\n";

        /// Expand `data` (the DATA section) and return its placed bodies.
        pub fn placed(data: &str) -> Vec<PlacedBody> {
            let text = format!("{HEADER}{data}\nENDSEC;\nEND-ISO-10303-21;\n");
            let exchange = p21::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
            let structure = product::read(&exchange).unwrap_or_else(|e| panic!("{e}\n{text}"));
            placed_bodies(&structure).unwrap_or_else(|e| panic!("{e}\n{text}"))
        }

        /// `AXIS2_PLACEMENT_3D` #k (its point and directions are #k+1 to #k+3).
        pub fn axis(k: u64, origin: [f64; 3], z: [f64; 3], x: [f64; 3]) -> String {
            format!(
                "#{k}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n\
#{}=CARTESIAN_POINT('',({:?},{:?},{:?}));\n\
#{}=DIRECTION('',({:?},{:?},{:?}));\n\
#{}=DIRECTION('',({:?},{:?},{:?}));\n",
                k + 1,
                k + 2,
                k + 3,
                k + 1,
                origin[0],
                origin[1],
                origin[2],
                k + 2,
                z[0],
                z[1],
                z[2],
                k + 3,
                x[0],
                x[1],
                x[2]
            )
        }

        /// A placement at `origin` with the global axes.
        pub fn at(k: u64, origin: [f64; 3]) -> String {
            axis(k, origin, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
        }

        /// A product definition #pd with its shape representation #rep listing `items`:
        /// `PRODUCT` #pd+1, formation #pd+2, `PRODUCT_DEFINITION_SHAPE` #pd+3, SDR #pd+4.
        pub fn part(pd: u64, rep: u64, items: &str) -> String {
            format!(
                "#{}=PRODUCT('p{pd}','p{pd}','',());\n\
#{}=PRODUCT_DEFINITION_FORMATION('','',#{});\n\
#{pd}=PRODUCT_DEFINITION('design','',#{},#8);\n\
#{}=PRODUCT_DEFINITION_SHAPE('','',#{pd});\n\
#{rep}=SHAPE_REPRESENTATION('',({items}),$);\n\
#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{rep});\n",
                pd + 1,
                pd + 2,
                pd + 1,
                pd + 2,
                pd + 3,
                pd + 4,
                pd + 3
            )
        }

        /// A solid body #id in the shell #100.
        pub fn solid(id: u64) -> String {
            format!("#{id}=MANIFOLD_SOLID_BREP('',#100);\n")
        }

        /// An occurrence #n of definition `child` in `parent`, placed by the transformation
        /// with items `(item_1, item_2)` between representations `(rep_1, rep_2)`: PDS #n+1,
        /// transformation #n+2, relationship #n+3, CDSR #n+4.
        pub fn occurrence(
            n: u64,
            (parent, child): (u64, u64),
            (rep_1, rep_2): (u64, u64),
            (item_1, item_2): (u64, u64),
        ) -> String {
            let mut t = String::new();
            let _ = write!(
                t,
                "#{n}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{n}','','',#{parent},#{child},$);\n\
#{}=PRODUCT_DEFINITION_SHAPE('','',#{n});\n\
#{}=ITEM_DEFINED_TRANSFORMATION('','',#{item_1},#{item_2});\n\
#{}=(REPRESENTATION_RELATIONSHIP('','',#{rep_1},#{rep_2})\
REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{})SHAPE_REPRESENTATION_RELATIONSHIP());\n\
#{}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n",
                n + 1,
                n + 2,
                n + 3,
                n + 2,
                n + 4,
                n + 3,
                n + 1
            );
            t
        }
    }

    const TOLERANCE: f64 = 1e-9;

    fn assert_matrix(got: Matrix4, want: Matrix4) {
        for (row, (got, want)) in got.iter().zip(&want).enumerate() {
            for (column, (got, want)) in got.iter().zip(want).enumerate() {
                assert!(
                    (got - want).abs() < TOLERANCE,
                    "entry [{row}][{column}]: got {got}, want {want}\ngot {got:?}\nwant {want:?}"
                );
            }
        }
    }

    fn translated(x: f64, y: f64, z: f64) -> Matrix4 {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [x, y, z, 1.0],
        ]
    }

    fn body_transform(placed: &[PlacedBody], entity: u64) -> Matrix4 {
        placed
            .iter()
            .find(|body| body.entity == entity)
            .unwrap_or_else(|| panic!("no body #{entity} in {placed:?}"))
            .transform
    }

    /// A two-part assembly: the root (definition #10, representation #1000, one placement)
    /// and a child holding body #101 (definition #20, representation #2000, one placement).
    /// The child sits 754 along +X of the root. `rep_1_is_child` and `item_1_in_parent` pick
    /// how the relationship and its transformation are written.
    fn two_parts(rep_1_is_child: bool, item_1_in_parent: bool) -> Vec<PlacedBody> {
        use fixtures::{at, occurrence, part, placed, solid};
        // Placements: #300 in the root (at 754), #400 in the child (at the origin).
        let items = if item_1_in_parent {
            (300, 400)
        } else {
            (400, 300)
        };
        let reps = if rep_1_is_child {
            (2000, 1000)
        } else {
            (1000, 2000)
        };
        let data = [
            solid(101),
            at(300, [754.0, 0.0, 0.0]),
            at(400, [0.0, 0.0, 0.0]),
            part(10, 1000, "#300"),
            part(20, 2000, "#101,#400"),
            occurrence(500, (10, 20), reps, items),
        ]
        .concat();
        placed(&data)
    }

    /// Conformant writing (ISO 10303-43, and the `Transformation::Items` docs): `rep_1` is the
    /// parent and `item_1` lives in it, `item_2` lives in the child. The child sits at +754.
    #[test]
    fn a_conformant_transformation_places_the_child_at_the_parent_placement() {
        let placed = two_parts(false, true);
        assert_eq!(placed.len(), 1);
        assert_matrix(body_transform(&placed, 101), translated(754.0, 0.0, 0.0));
    }

    /// NON-conformant writing, yet found in real exporter output: `rep_1` is the child, but
    /// `item_1` lives in the parent's representation and `item_2` in the child's (the standard
    /// wants `item_1` in `rep_1`). The order of the representations is therefore unreliable;
    /// where the items actually live decides. The child is carried to the parent's placement,
    /// so it sits at +754, not -754.
    #[test]
    fn a_child_is_placed_at_the_parent_placement_when_item_1_is_in_the_parent() {
        let placed = two_parts(true, true);
        assert_eq!(placed.len(), 1);
        assert_matrix(body_transform(&placed, 101), translated(754.0, 0.0, 0.0));
    }

    /// The writing with `item_1` in the child and `rep_1` the child (conformant) is the same
    /// assembly: +754.
    #[test]
    fn a_child_is_placed_at_the_parent_placement_when_item_1_is_in_the_child() {
        let placed = two_parts(true, false);
        assert_eq!(placed.len(), 1);
        assert_matrix(body_transform(&placed, 101), translated(754.0, 0.0, 0.0));
    }

    /// Root, then A turned a quarter turn about Z (child placement identity, parent placement
    /// with `z = +Z`, `x = +Y`), then B moved 10 along X inside A. Row vectors: `p·F` takes a
    /// point from a part's frame to its parent's, so a point `p` of B is at
    /// `p · T_AB · R_RA` in the world, i.e. B's matrix is `local · world`, local first.
    /// `(x, y, z)·R_RA = (-y, x, z)`, so B's origin `(0,0,0)` goes to `(10,0,0)` in A and to
    /// `(0,10,0)` in the world; B's matrix is `[[0,1,0],[-1,0,0],[0,0,1]]` with row 3 `(0,10,0)`.
    /// Composing in the other order would put B at `(10,0,0)`.
    #[test]
    fn nested_placements_compose_the_child_transform_before_the_parent_world() {
        use fixtures::{at, axis, occurrence, part, placed, solid};
        let quarter_turn = axis(300, [0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        let data = [
            solid(101),
            solid(102),
            quarter_turn,
            at(310, [0.0; 3]),
            at(320, [10.0, 0.0, 0.0]),
            at(330, [0.0; 3]),
            part(10, 1000, "#300"),
            part(20, 2000, "#101,#310,#320"),
            part(30, 3000, "#102,#330"),
            // Child's placement first: A in the root, B in A.
            occurrence(500, (10, 20), (2000, 1000), (310, 300)),
            occurrence(510, (20, 30), (3000, 2000), (330, 320)),
        ]
        .concat();
        let placed = placed(&data);
        assert_eq!(placed.len(), 2);
        let turn = [
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_matrix(body_transform(&placed, 101), turn);
        let mut want = turn;
        want[3] = [0.0, 10.0, 0.0, 1.0];
        assert_matrix(body_transform(&placed, 102), want);
    }

    /// A mapped item places the map origin onto its target: `origin⁻¹ · target` for row
    /// vectors. The origin is a translation by (1,0,0) and the target a quarter turn about Z
    /// at (5,0,0), so a point `p` goes to `(p - (1,0,0)) · R + (5,0,0)`: the map origin
    /// lands on (5,0,0) and the translation row is `(-1,0,0)·R + (5,0,0) = (5,-1,0)`.
    /// The other order, `target · origin⁻¹`, would give (4,0,0).
    #[test]
    fn a_mapped_item_places_its_origin_onto_its_target_with_the_origin_undone_first() {
        use fixtures::{at, axis, part, placed, solid};
        let data = [
            solid(101),
            at(300, [1.0, 0.0, 0.0]),
            axis(310, [5.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            "#400=SHAPE_REPRESENTATION('',(#101,#300),$);\n".to_string(),
            "#401=REPRESENTATION_MAP(#300,#400);\n".to_string(),
            "#402=MAPPED_ITEM('',#401,#310);\n".to_string(),
            part(10, 1000, "#402,#310"),
        ]
        .concat();
        let placed = placed(&data);
        assert_eq!(placed.len(), 1);
        assert_matrix(
            body_transform(&placed, 101),
            [
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [5.0, -1.0, 0.0, 1.0],
            ],
        );
    }
}
