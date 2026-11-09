use crate::Result;
use crate::body::Body;

/// STEP (ISO 10303-21, AP242) text for the given bodies as one part (`Body1`…, with the
/// bodies' colours).
pub fn step_export(bodies: &[&Body], system: &str) -> Result<String> {
    let list: Vec<crate::ExportBody> =
        bodies.iter().enumerate().map(|(i, b)| crate::ExportBody { name: format!("Body{}", i + 1), body: b, color: b.color() }).collect();
    let header = crate::StepHeader { organization: system.to_string(), ..Default::default() };
    crate::step_out::step_export_bodies(&list, &header)
}
