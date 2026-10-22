//! Booleans through SMLib: each operand's schema 4 record built in SMLib and tagged as operand 0
//! or 1, `SmBrep` boolean, the result exported to schema 4 and decoded as a kernel body. The result's history is SMLib's own (its tags carried through the
//! operation), renumbered from SMLib's face and edge order into the operands' and the result's
//! ([`crate::history::History`]'s contract). A result SMLib's own check or the record's geometry
//! check refuses is an error, never a body.

pub(crate) use smlib::boolean as smlib_boolean;

mod smlib {
    use solidcraft_smlib::{BooleanOp, CheckLevel};

    use crate::body::Body;
    use crate::history::History;
    use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
    use crate::ops::BoolOp;
    use crate::{KernelError, Result, record};

    fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib boolean: {what}: {e}"))
    }

    /// `a` and `b` combined by `op` in SMLib, with the history SMLib's tags give (none where a
    /// body's face and edge order cannot be matched to SMLib's). `None`: the result is empty.
    pub(crate) fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
        a.require_brep("a boolean")?;
        b.require_brep("a boolean")?;
        let (sa, oa) = smlib_body_ordered(a)?;
        let (sb, ob) = smlib_body_ordered(b)?;
        let ta = sa.tagged(0).map_err(|e| failed("tag", e))?;
        let tb = sb.tagged(1).map_err(|e| failed("tag", e))?;
        let kind = match op {
            BoolOp::Union => BooleanOp::Union,
            BoolOp::Cut => BooleanOp::Subtract,
            BoolOp::Intersect => BooleanOp::Intersect,
        };
        let Some(r) = ta.boolean(&tb, kind).map_err(|e| failed("operation", e))? else { return Ok(None) };
        // Solids touching each other along an edge or at a vertex come back as one non-manifold
        // body, which the export splits into separate solids: SMLib's check is of those, imported
        // as given (its check of the touching arrangement itself refuses the radial order of
        // faces round an edge they share).
        let manifold = r.is_manifold_solid().map_err(|e| failed("check", e))?;
        let check = if manifold {
            r.check(CheckLevel::All).map_err(|e| failed("check", e))?
        } else {
            let record = r.to_native().map_err(|e| failed("export", e))?;
            let split = solidcraft_smlib::Brep::from_record(&record, true).map_err(|e| failed("the separate solids", e))?;
            split.check(CheckLevel::All).map_err(|e| failed("check", e))?
        };
        if !check.is_valid() {
            // Whether an operand failed it already says whose the fault is.
            let operands: Vec<&str> = [("the first operand", &sa), ("the second operand", &sb)]
                .into_iter()
                .filter(|(_, s)| s.check(CheckLevel::All).is_ok_and(|c| !c.is_valid()))
                .map(|(name, _)| name)
                .collect();
            let what = if operands.is_empty() {
                "SMLib's check refuses the result".to_string()
            } else {
                format!("SMLib's check refuses the result ({} already fails it)", operands.join(" and "))
            };
            return Err(failed(&what, check));
        }
        let (body, or) = body_from_smlib_ordered(&r)?;
        let history = match (oa, ob, or) {
            (Some(oa), Some(ob), Some(or)) => r.history().map_err(|e| failed("history", e))?.history(&[oa, ob], &or).map(Some).unwrap_or_else(|e| {
                log::debug!(target: "smlib", "boolean history: {e}");
                None
            }),
            _ => None,
        };
        Ok(Some(with_history(body, history, a, b)))
    }

    /// `body` with `history` where it describes `body` made from `a` and `b`.
    fn with_history(body: Body, history: Option<History>, a: &Body, b: &Body) -> Body {
        body.with_checked_history(history, &record::counts(&[a, b]))
    }
}
