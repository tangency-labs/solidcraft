//! Structural validation of a mapped STEP model.
//!
//! The mapper constructs the model, so this is a defence-in-depth check for callers
//! that build or edit a [`Model`] themselves. It does not replace schema validation;
//! it checks internal consistency and references that the parser can leave outside
//! mapped bodies.

use std::fmt;

use crate::model::{Body, Model};
use crate::part21::{Instance, Param, Record, StepFile};

/// One validation problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Entity or owning body the problem belongs to, when known.
    pub step_id: Option<u64>,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.step_id {
            Some(id) => write!(f, "#{id}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ValidationError {}

fn visit(param: &Param, missing: &mut Vec<u64>) {
    match param {
        Param::Ref(id) => missing.push(*id),
        Param::List(items) => {
            for item in items {
                visit(item, missing);
            }
        }
        Param::Typed(_, value) => visit(value, missing),
        _ => {}
    }
}

fn dangling_references(file: &StepFile) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for (id, instance) in &file.entities {
        let records: &[Record] = match instance {
            Instance::Simple(record) => std::slice::from_ref(record),
            Instance::Complex(records) => records,
        };
        for record in records {
            for param in &record.params {
                let mut missing = Vec::new();
                visit(param, &mut missing);
                for target in missing {
                    if !file.entities.contains_key(&target) {
                        errors.push(ValidationError {
                            step_id: Some(*id),
                            message: format!("dangling reference #{target}"),
                        });
                    }
                }
            }
        }
    }
    errors
}

fn validate_body(body: &Body) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    if !(body.unit_mm.is_finite() && body.unit_mm > 0.0) {
        errors.push(ValidationError {
            step_id: Some(body.step_id),
            message: format!("invalid body unit factor {}", body.unit_mm),
        });
    }
    if body.shells.is_empty() {
        errors.push(ValidationError {
            step_id: Some(body.step_id),
            message: "body has no shells".into(),
        });
    }
    for shell in &body.shells {
        for &face in &shell.faces {
            if face >= body.faces.len() {
                errors.push(ValidationError {
                    step_id: Some(body.step_id),
                    message: format!("shell references missing face {face}"),
                });
            }
        }
    }
    for face in &body.faces {
        for loop_ in &face.loops {
            if let crate::model::Bound::Edges(edges) = &loop_.bound {
                for edge in edges {
                    if edge.edge >= body.edges.len() {
                        errors.push(ValidationError {
                            step_id: Some(body.step_id),
                            message: format!(
                                "face #{} references missing edge {}",
                                face.step_id, edge.edge
                            ),
                        });
                    }
                }
            }
        }
    }
    for (index, edge) in body.edges.iter().enumerate() {
        if edge.start >= body.vertices.len() || edge.end >= body.vertices.len() {
            errors.push(ValidationError {
                step_id: Some(body.step_id),
                message: format!("edge {index} references a missing vertex"),
            });
        }
    }
    errors
}

/// Validates internal model references and raw-file reference targets.
pub fn validate(file: &StepFile, model: &Model) -> Vec<ValidationError> {
    let mut errors = dangling_references(file);
    for body in &model.bodies {
        errors.extend(validate_body(body));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::map_file;
    use crate::part21::{parse, Limits};

    #[test]
    fn reports_dangling_references() {
        let text = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n#1=PRODUCT('p','p','',(#9));\nENDSEC;\nEND-ISO-10303-21;\n";
        let file = parse(text.as_bytes(), &Limits::default()).unwrap();
        let model = map_file(&file, &Limits::default());
        assert!(validate(&file, &model)
            .iter()
            .any(|error| error.message.contains("dangling reference #9")));
    }

    #[test]
    fn accepts_a_consistent_model() {
        let file = parse(b"ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;", &Limits::default()).unwrap();
        let model = map_file(&file, &Limits::default());
        assert!(validate(&file, &model).is_empty());
    }
}
