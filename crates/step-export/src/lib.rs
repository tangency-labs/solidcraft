//! ISO 10303-21 writer for the neutral [`step_import::Model`].
//!
//! The initial subset is a single-shell planar B-rep with line edges. Unsupported
//! geometry is rejected with an [`ExportError`] instead of being silently approximated.
//! Shared body edges retain one STEP edge identity, with a separate orientation per use.
//! The file is written as AP203 (`CONFIG_CONTROL_DESIGN`) with millimetres.

use std::fmt::{self, Write as _};

use step_import::model::{Body, BodyKind, Bound, Curve, Frame, Model, Surface};

/// Why a model could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    /// Human-readable reason.
    pub message: String,
}

impl ExportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExportError {}

/// A Part 21 entity table with stable ids in insertion order.
struct Entities {
    next: u64,
    rows: Vec<(u64, String)>,
}

impl Entities {
    fn new() -> Self {
        Self {
            next: 1,
            rows: Vec::new(),
        }
    }

    fn add(&mut self, text: String) -> u64 {
        let id = self.next;
        self.next += 1;
        self.rows.push((id, text));
        id
    }
}

fn point(point: [f64; 3]) -> Result<String, ExportError> {
    Ok(format!(
        "CARTESIAN_POINT('',({},{},{}))",
        number(point[0])?,
        number(point[1])?,
        number(point[2])?
    ))
}

fn direction(direction: [f64; 3]) -> Result<String, ExportError> {
    Ok(format!(
        "DIRECTION('',({},{},{}))",
        number(direction[0])?,
        number(direction[1])?,
        number(direction[2])?
    ))
}

fn number(value: f64) -> Result<String, ExportError> {
    if value == 0.0 {
        Ok("0.".to_string())
    } else if value.is_finite() {
        Ok(format!("{value:.17}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string())
    } else {
        Err(ExportError::new("non-finite number in STEP export"))
    }
}

fn frame_placement(entities: &mut Entities, frame: &Frame) -> Result<u64, ExportError> {
    let origin = entities.add(point(frame.origin)?);
    let axis = entities.add(direction(frame.z)?);
    let ref_direction = entities.add(direction(frame.x)?);
    Ok(entities.add(format!(
        "AXIS2_PLACEMENT_3D('',#{origin},#{axis},#{ref_direction})"
    )))
}

fn shell(entities: &mut Entities, body: &Body) -> Result<u64, ExportError> {
    if body.shells.len() != 1 {
        return Err(ExportError::new(format!(
            "body #{} has {} shells; step-export supports one closed shell",
            body.step_id,
            body.shells.len()
        )));
    }
    let step_shell = body
        .shells
        .first()
        .ok_or_else(|| ExportError::new(format!("body #{} has no shells", body.step_id)))?;
    let vertex_ids = body
        .vertices
        .iter()
        .map(|vertex| {
            let point_id = entities.add(point(vertex.point)?);
            Ok(entities.add(format!("VERTEX_POINT('',#{point_id})")))
        })
        .collect::<Result<Vec<_>, ExportError>>()?;
    let mut edge_ids = vec![None; body.edges.len()];
    let mut face_ids = Vec::with_capacity(step_shell.faces.len());
    for &face_index in &step_shell.faces {
        let face = body.faces.get(face_index).ok_or_else(|| {
            ExportError::new(format!(
                "body #{} references missing face {face_index}",
                body.step_id
            ))
        })?;
        let plane = match &face.surface {
            Surface::Plane { frame } => frame,
            other => {
                return Err(ExportError::new(format!(
                    "face #{} uses unsupported surface {other:?}",
                    face.step_id
                )));
            }
        };
        let placement = frame_placement(entities, plane)?;
        let plane_id = entities.add(format!("PLANE('',#{placement})"));
        let mut bounds = Vec::with_capacity(face.loops.len());
        for loop_ in &face.loops {
            let Bound::Edges(edges) = &loop_.bound else {
                return Err(ExportError::new(format!(
                    "face #{} has a vertex loop; step-export needs edge loops",
                    face.step_id
                )));
            };
            let mut oriented = Vec::with_capacity(edges.len());
            for oriented_edge in edges {
                let cached_id = edge_ids.get_mut(oriented_edge.edge).ok_or_else(|| {
                    ExportError::new(format!("missing edge index {}", oriented_edge.edge))
                })?;
                let edge_id = if let Some(id) = *cached_id {
                    id
                } else {
                    let id = write_edge(entities, body, oriented_edge.edge, &vertex_ids)?;
                    *cached_id = Some(id);
                    id
                };
                let orientation = if oriented_edge.forward { ".T." } else { ".F." };
                oriented
                    .push(entities.add(format!("ORIENTED_EDGE('',*,*,#{edge_id},{orientation})")));
            }
            let loop_id = entities.add(format!(
                "EDGE_LOOP('',({}))",
                oriented
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            let orientation = if loop_.orientation { ".T." } else { ".F." };
            let kind = if loop_.outer {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            bounds.push(entities.add(format!("{kind}('',#{loop_id},{orientation})")));
        }
        let bound_refs = bounds
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        let sense = if face.same_sense { ".T." } else { ".F." };
        face_ids.push(entities.add(format!(
            "ADVANCED_FACE('',({bound_refs}),#{plane_id},{sense})"
        )));
    }
    let refs = face_ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(entities.add(format!("CLOSED_SHELL('',({refs}))")))
}

fn write_edge(
    entities: &mut Entities,
    body: &Body,
    index: usize,
    vertex_ids: &[u64],
) -> Result<u64, ExportError> {
    let edge = body.edges.get(index).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} references missing edge {index}",
            body.step_id
        ))
    })?;
    let Curve::Line {
        origin,
        dir,
        magnitude,
    } = &edge.curve
    else {
        return Err(ExportError::new(format!(
            "edge #{} uses unsupported curve {:?}",
            edge.step_id, edge.curve
        )));
    };
    let start = *vertex_ids.get(edge.start).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} edge #{index} has a missing start vertex",
            body.step_id
        ))
    })?;
    let end = *vertex_ids.get(edge.end).ok_or_else(|| {
        ExportError::new(format!(
            "body #{} edge #{index} has a missing end vertex",
            body.step_id
        ))
    })?;
    let origin_id = entities.add(point(*origin)?);
    let direction_id = entities.add(direction(*dir)?);
    let vector_id = entities.add(format!(
        "VECTOR('',#{direction_id},{})",
        number(*magnitude)?
    ));
    let line_id = entities.add(format!("LINE('',#{origin_id},#{vector_id})"));
    let sense = if edge.same_sense { ".T." } else { ".F." };
    Ok(entities.add(format!("EDGE_CURVE('',#{start},#{end},#{line_id},{sense})")))
}

/// Writes a model as an AP203 Part 21 exchange file.
///
/// # Errors
///
/// A body containing anything outside the supported single-shell planar/line subset,
/// or a non-finite number in an emitted STEP value.
pub fn write(model: &Model) -> Result<String, ExportError> {
    let mut entities = Entities::new();
    let unit = entities.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".into());
    let context = entities.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unit})) REPRESENTATION_CONTEXT('',''))"
    ));
    let mut body_ids = Vec::new();
    for body in &model.bodies {
        match body.kind {
            BodyKind::ManifoldSolid => {}
            other => {
                return Err(ExportError::new(format!(
                    "body #{} is {:?}; step-export currently writes MANIFOLD_SOLID_BREP only",
                    body.step_id, other
                )));
            }
        }
        let shell_id = shell(&mut entities, body)?;
        body_ids.push(entities.add(format!("MANIFOLD_SOLID_BREP('',#{shell_id})")));
    }
    if body_ids.is_empty() {
        return Err(ExportError::new("model has no bodies"));
    }
    let refs = body_ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    entities.add(format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('',({refs}),#{context})"
    ));

    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('step-export'),'2;1');\n");
    out.push_str("FILE_NAME('','',(''),(''),'step-export','step-export','');\n");
    out.push_str("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n");
    for (id, row) in &entities.rows {
        let _ = writeln!(out, "#{id}={row};");
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use step_import::model::{Edge, Face, Loop, OrientedEdge, Shell, Vertex};
    use step_import::part21::Header;

    fn line(origin: [f64; 3], dir: [f64; 3], magnitude: f64) -> Curve {
        Curve::Line {
            origin,
            dir,
            magnitude,
        }
    }

    fn triangle() -> Model {
        let z = [0.0, 0.0, 1.0];
        let frame = Frame {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z,
        };
        let vertices = vec![
            Vertex {
                step_id: 1,
                point: [0.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [10.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 3,
                point: [0.0, 10.0, 0.0],
            },
        ];
        let edges = vec![
            Edge {
                step_id: 1,
                start: 0,
                end: 1,
                curve: line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
            Edge {
                step_id: 2,
                start: 1,
                end: 2,
                curve: line([10.0, 0.0, 0.0], [-1.0, 1.0, 0.0], 10.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
            Edge {
                step_id: 3,
                start: 2,
                end: 0,
                curve: line([0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 10.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
        ];
        let face = Face {
            step_id: 1,
            surface: Surface::Plane { frame },
            same_sense: true,
            loops: vec![Loop {
                outer: true,
                orientation: true,
                bound: Bound::Edges(vec![
                    OrientedEdge {
                        edge: 0,
                        forward: true,
                    },
                    OrientedEdge {
                        edge: 1,
                        forward: true,
                    },
                    OrientedEdge {
                        edge: 2,
                        forward: true,
                    },
                ]),
            }],
        };
        let body = Body {
            step_id: 10,
            name: Some("triangle".into()),
            kind: BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: None,
            shells: vec![Shell {
                step_id: 1,
                closed: true,
                orientation: true,
                faces: vec![0],
            }],
            faces: vec![face],
            edges,
            vertices,
        };
        Model {
            header: Header::default(),
            schemas: vec!["CONFIG_CONTROL_DESIGN".into()],
            bodies: vec![body],
            skipped: Vec::new(),
            diagnostics: Vec::new(),
            assembly_instances: 0,
        }
    }

    fn tetrahedron() -> Model {
        let points: [[f64; 3]; 4] = [
            [3.0, -5.0, 7.0],
            [5.0, -5.0, 7.0],
            [3.0, -2.0, 7.0],
            [3.0, -5.0, 11.0],
        ];
        let unit = |v: [f64; 3]| {
            let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v.map(|component| component / length)
        };
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let vertices = points
            .iter()
            .enumerate()
            .map(|(index, &point)| Vertex {
                step_id: index as u64 + 1,
                point,
            })
            .collect();
        let edges = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
            .into_iter()
            .enumerate()
            .map(|(index, (start, end))| {
                let vector = std::array::from_fn(|axis| points[end][axis] - points[start][axis]);
                let magnitude =
                    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
                Edge {
                    step_id: index as u64 + 1,
                    start,
                    end,
                    curve: line(points[start], unit(vector), magnitude),
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 1.0,
                }
            })
            .collect();
        // Outward-facing loops: every edge occurs on two distinct faces, in opposite directions.
        let face_specs = [
            ([0, 2, 1], [(1, true), (3, false), (0, false)]),
            ([0, 1, 3], [(0, true), (4, true), (2, false)]),
            ([0, 3, 2], [(2, true), (5, false), (1, false)]),
            ([1, 2, 3], [(3, true), (5, true), (4, false)]),
        ];
        let faces = face_specs
            .into_iter()
            .enumerate()
            .map(|(index, ([a, b, c], uses))| {
                let x = unit(std::array::from_fn(|axis| {
                    points[b][axis] - points[a][axis]
                }));
                let other = std::array::from_fn(|axis| points[c][axis] - points[a][axis]);
                let z = unit(cross(x, other));
                Face {
                    step_id: index as u64 + 1,
                    surface: Surface::Plane {
                        frame: Frame {
                            origin: points[a],
                            x,
                            y: cross(z, x),
                            z,
                        },
                    },
                    same_sense: true,
                    loops: vec![Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(
                            uses.into_iter()
                                .map(|(edge, forward)| OrientedEdge { edge, forward })
                                .collect(),
                        ),
                    }],
                }
            })
            .collect();
        Model {
            header: Header::default(),
            schemas: vec!["CONFIG_CONTROL_DESIGN".into()],
            bodies: vec![Body {
                step_id: 10,
                name: Some("translated tetrahedron".into()),
                kind: BodyKind::ManifoldSolid,
                unit_mm: 1.0,
                uncertainty_mm: None,
                shells: vec![Shell {
                    step_id: 1,
                    closed: true,
                    orientation: true,
                    faces: vec![0, 1, 2, 3],
                }],
                faces,
                edges,
                vertices,
            }],
            skipped: Vec::new(),
            diagnostics: Vec::new(),
            assembly_instances: 0,
        }
    }

    #[test]
    fn round_trip_preserves_tetrahedron_shared_edge_incidence() {
        let text = write(&tetrahedron()).expect("write tetrahedron");
        let model = step_import::import(text.as_bytes(), &step_import::Limits::default())
            .expect("re-import tetrahedron");
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.diagnostics.is_empty(), "{:?}", model.diagnostics);
        assert_eq!(model.bodies.len(), 1);
        let body = &model.bodies[0];
        assert_eq!(body.kind, BodyKind::ManifoldSolid);
        assert_eq!(body.shells.len(), 1);
        assert!(body.shells[0].closed);
        let mut shell_faces = body.shells[0].faces.clone();
        shell_faces.sort_unstable();
        assert_eq!(shell_faces, vec![0, 1, 2, 3]);

        let expected_points = [
            [3.0, -5.0, 7.0],
            [5.0, -5.0, 7.0],
            [3.0, -2.0, 7.0],
            [3.0, -5.0, 11.0],
        ];
        let vertex_labels: Vec<_> = body
            .vertices
            .iter()
            .map(|vertex| {
                expected_points
                    .iter()
                    .position(|point| *point == vertex.point)
                    .expect("expected world-space vertex")
            })
            .collect();
        let mut labels = vertex_labels.clone();
        labels.sort_unstable();
        assert_eq!(labels, vec![0, 1, 2, 3]);
        let mut edge_endpoints: Vec<_> = body
            .edges
            .iter()
            .map(|edge| {
                assert!(edge.same_sense);
                (vertex_labels[edge.start], vertex_labels[edge.end])
            })
            .collect();
        edge_endpoints.sort_unstable();
        assert_eq!(
            edge_endpoints,
            vec![(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        );

        let mut incidences = vec![Vec::new(); body.edges.len()];
        let mut face_boundaries = Vec::new();
        for (face_index, face) in body.faces.iter().enumerate() {
            assert!(face.same_sense);
            assert_eq!(face.loops.len(), 1);
            let loop_ = &face.loops[0];
            assert!(loop_.outer);
            assert!(loop_.orientation);
            let Bound::Edges(uses) = &loop_.bound else {
                panic!("tetrahedron face must have an edge loop");
            };
            assert_eq!(uses.len(), 3);
            let mut directed = Vec::new();
            for oriented in uses {
                let edge = &body.edges[oriented.edge];
                let (start, end) = (vertex_labels[edge.start], vertex_labels[edge.end]);
                directed.push(if oriented.forward {
                    (start, end)
                } else {
                    (end, start)
                });
                incidences[oriented.edge].push((face_index, oriented.forward));
            }
            for pair in directed.windows(2) {
                assert_eq!(pair[0].1, pair[1].0, "face loop must be connected");
            }
            assert_eq!(directed[2].1, directed[0].0, "face loop must close");
            directed.sort_unstable();
            face_boundaries.push(directed);
        }
        let mut expected_boundaries = vec![
            vec![(0, 2), (2, 1), (1, 0)],
            vec![(0, 1), (1, 3), (3, 0)],
            vec![(0, 3), (3, 2), (2, 0)],
            vec![(1, 2), (2, 3), (3, 1)],
        ];
        for boundary in &mut expected_boundaries {
            boundary.sort_unstable();
        }
        face_boundaries.sort_unstable();
        expected_boundaries.sort_unstable();
        assert_eq!(face_boundaries, expected_boundaries);
        for uses in incidences {
            assert_eq!(uses.len(), 2, "each boundary must be shared by two faces");
            assert_ne!(
                uses[0].0, uses[1].0,
                "shared uses must be on distinct faces"
            );
            assert_ne!(uses[0].1, uses[1].1, "shared uses must oppose each other");
        }
    }

    #[test]
    fn writes_a_round_trippable_triangle() {
        let text = write(&triangle()).expect("write");
        let file = step_import::import(text.as_bytes(), &step_import::Limits::default())
            .expect("re-import");
        assert_eq!(file.bodies.len(), 1);
        assert_eq!(file.bodies[0].faces.len(), 1);
        assert_eq!(file.bodies[0].edges.len(), 3);
        assert_eq!(file.bodies[0].vertices.len(), 3);
    }

    #[test]
    fn rejects_non_planar_geometry() {
        let mut model = triangle();
        model.bodies[0].faces[0].surface = Surface::Cylinder {
            frame: Frame {
                origin: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                z: [0.0, 0.0, 1.0],
            },
            radius: 1.0,
        };
        assert!(write(&model).is_err());
    }

    #[test]
    fn rejects_non_finite_emitted_numbers() {
        let values = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
        let mut accepted = Vec::new();

        // vertex point component
        for &v in &values {
            let mut model = triangle();
            model.bodies[0].vertices[0].point[0] = v;
            if write(&model).is_ok() {
                accepted.push(format!("vertex point component: {v}"));
            }
        }
        // line origin
        for &v in &values {
            let mut model = triangle();
            if let Curve::Line { origin, .. } = &mut model.bodies[0].edges[0].curve {
                origin[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("line origin: {v}"));
            }
        }
        // line dir
        for &v in &values {
            let mut model = triangle();
            if let Curve::Line { dir, .. } = &mut model.bodies[0].edges[0].curve {
                dir[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("line dir: {v}"));
            }
        }
        // line magnitude
        for &v in &values {
            let mut model = triangle();
            if let Curve::Line { magnitude, .. } = &mut model.bodies[0].edges[0].curve {
                *magnitude = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("line magnitude: {v}"));
            }
        }
        // plane origin
        for &v in &values {
            let mut model = triangle();
            if let Surface::Plane { frame } = &mut model.bodies[0].faces[0].surface {
                frame.origin[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("plane origin: {v}"));
            }
        }
        // plane X
        for &v in &values {
            let mut model = triangle();
            if let Surface::Plane { frame } = &mut model.bodies[0].faces[0].surface {
                frame.x[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("plane X: {v}"));
            }
        }
        // plane Z
        for &v in &values {
            let mut model = triangle();
            if let Surface::Plane { frame } = &mut model.bodies[0].faces[0].surface {
                frame.z[0] = v;
            }
            if write(&model).is_ok() {
                accepted.push(format!("plane Z: {v}"));
            }
        }

        assert!(accepted.is_empty(), "wrongly accepted: {accepted:?}");
    }
}
