//! ISO 10303-21 writer for the neutral [`step_import::Model`].
//!
//! The initial subset is a single-shell planar B-rep with line edges. Unsupported
//! geometry is rejected with an [`ExportError`] instead of being silently approximated.
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

fn point(point: [f64; 3]) -> String {
    format!(
        "CARTESIAN_POINT('',({},{},{}))",
        number(point[0]),
        number(point[1]),
        number(point[2])
    )
}

fn direction(direction: [f64; 3]) -> String {
    format!(
        "DIRECTION('',({},{},{}))",
        number(direction[0]),
        number(direction[1]),
        number(direction[2])
    )
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0.".to_string()
    } else if value.is_finite() {
        format!("{value:.17}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        "0.".to_string()
    }
}

fn frame_placement(entities: &mut Entities, frame: &Frame) -> u64 {
    let origin = entities.add(point(frame.origin));
    let axis = entities.add(direction(frame.z));
    let ref_direction = entities.add(direction(frame.x));
    entities.add(format!(
        "AXIS2_PLACEMENT_3D('',#{origin},#{axis},#{ref_direction})"
    ))
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
            let point_id = entities.add(point(vertex.point));
            entities.add(format!("VERTEX_POINT('',#{point_id})"))
        })
        .collect::<Vec<_>>();
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
        let placement = frame_placement(entities, plane);
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
                let edge_id = write_edge(
                    entities,
                    body,
                    edge_index(body, oriented_edge.edge)?,
                    &vertex_ids,
                )?;
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

fn edge_index(body: &Body, index: usize) -> Result<usize, ExportError> {
    if index < body.edges.len() {
        Ok(index)
    } else {
        Err(ExportError::new(format!("missing edge index {index}")))
    }
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
    let origin_id = entities.add(point(*origin));
    let direction_id = entities.add(direction(*dir));
    let vector_id = entities.add(format!("VECTOR('',#{direction_id},{})", number(*magnitude)));
    let line_id = entities.add(format!("LINE('',#{origin_id},#{vector_id})"));
    let sense = if edge.same_sense { ".T." } else { ".F." };
    Ok(entities.add(format!("EDGE_CURVE('',#{start},#{end},#{line_id},{sense})")))
}

/// Writes a model as an AP203 Part 21 exchange file.
///
/// # Errors
///
/// A body containing anything outside the supported single-shell planar/line subset.
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
            schemas: vec!["CONFIG_CONTROL_DESIGN".into()],
            bodies: vec![body],
            skipped: Vec::new(),
            assembly_instances: 0,
        }
    }

    #[test]
    fn writes_a_round_trippable_triangle() {
        let text = write(&triangle()).expect("write");
        assert!(text.contains("MANIFOLD_SOLID_BREP('"));
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
}
