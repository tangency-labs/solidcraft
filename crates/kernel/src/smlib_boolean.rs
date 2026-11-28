//! Booleans through SMLib: each operand's schema 4 record built in SMLib and tagged as operand 0
//! or 1, `SmBrep` boolean, the result exported to schema 4 and decoded as a kernel body. The result's history is SMLib's own (its tags carried through the
//! operation), renumbered from SMLib's face and edge order into the operands' and the result's
//! ([`crate::history::History`]'s contract). A result SMLib's own check or the record's geometry
//! check refuses is an error, never a body. Solids an operand's record keeps apart where they
//! touch each other along an edge go into SMLib sharing it, as SMLib made them
//! (`crate::native::smlib_shared_operand_ordered`); where that fails, the operation is done again
//! with the operands as the record keeps them, as before.

pub(crate) use smlib::{boolean as smlib_boolean, union_all as smlib_union_all};

mod smlib {
    use solidcraft_smlib::{BooleanOp, CheckLevel};

    use crate::body::Body;
    use crate::history::History;
    use crate::native::{body_from_smlib_ordered, smlib_operand_ordered, smlib_operand_to_consume, smlib_shared_operand_ordered};
    use crate::ops::BoolOp;
    use crate::validation::Step;
    use crate::{KernelError, Result, record};

    fn failed(what: &str, e: impl crate::Cause) -> KernelError {
        KernelError::smlib(format!("SMLib boolean: {what}"), e)
    }

    /// An operand in SMLib, with where each of its faces and edges is in the body's order: the
    /// body's own form, shared, or one made for the operation. The operations only read it.
    type Operand = (std::sync::Arc<solidcraft_smlib::Brep>, Option<solidcraft_smlib::history::Order>);

    /// A form made for the operation as an [`Operand`].
    fn made((brep, order): (solidcraft_smlib::Brep, Option<solidcraft_smlib::history::Order>)) -> Operand {
        (std::sync::Arc::new(brep), order)
    }

    /// `a` and `b` combined by `op` in SMLib, with the history SMLib's tags give (none where a
    /// body's face and edge order cannot be matched to SMLib's). `None`: the result is empty.
    pub(crate) fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
        a.require_brep("a boolean")?;
        b.require_brep("a boolean")?;
        // Solids an operand keeps apart where they touch along an edge go in sharing it (#14419);
        // where that fails, the operands go in as the record keeps them, as before.
        let (shared_a, shared_b) = (smlib_shared_operand_ordered(a)?.map(made), smlib_shared_operand_ordered(b)?.map(made));
        if shared_a.is_some() || shared_b.is_some() {
            let sa = match shared_a {
                Some(s) => s,
                None => smlib_operand_ordered(a)?,
            };
            let sb = match shared_b {
                Some(s) => s,
                None => smlib_operand_ordered(b)?,
            };
            match combine(a, b, sa, sb, op) {
                Ok(r) => return Ok(r),
                Err(e) => log::debug!(target: "smlib", "boolean with touching solids shared: {e}"),
            }
        }
        combine(a, b, smlib_operand_to_consume(a)?, smlib_operand_to_consume(b)?, op)
    }

    /// [`boolean`] of `a` and `b` as SMLib's `sa` and `sb`, each with its numbering.
    fn combine(a: &Body, b: &Body, (sa, oa): Operand, (sb, ob): Operand, op: BoolOp) -> Result<Option<Body>> {
        let kind = match op {
            BoolOp::Union => BooleanOp::Union,
            BoolOp::Cut => BooleanOp::Subtract,
            BoolOp::Intersect => BooleanOp::Intersect,
        };
        // The operands tagged and combined, the tagged copies consumed (solidcore#3359). Where the
        // step is not checked now, an operand made for this operation alone (not a body's kept
        // form) is handed over and tagged itself, not copied first: nothing reads it after.
        let step = crate::validation::step();
        let (r, check_operands) = if step == Step::Check {
            let Some(r) = sa.boolean_tagged(&sb, kind).map_err(|e| failed("operation", e))? else { return Ok(None) };
            (r, Some((sa, sb)))
        } else {
            let taken = |s: &mut Option<std::sync::Arc<solidcraft_smlib::Brep>>| match s.take().map(std::sync::Arc::try_unwrap) {
                Some(Ok(owned)) => Some(Err(owned)),
                Some(Err(shared)) => {
                    *s = Some(shared);
                    None
                }
                None => None,
            };
            let (mut ka, mut kb) = (Some(sa), Some(sb));
            let (oa, ob) = (taken(&mut ka), taken(&mut kb));
            let a_in = match (oa, ka.as_deref()) {
                (Some(owned), _) => owned,
                (None, Some(shared)) => Ok(shared),
                (None, None) => return Err(KernelError::Internal("SMLib boolean: an operand went missing".into())),
            };
            let b_in = match (ob, kb.as_deref()) {
                (Some(owned), _) => owned,
                (None, Some(shared)) => Ok(shared),
                (None, None) => return Err(KernelError::Internal("SMLib boolean: an operand went missing".into())),
            };
            let first = solidcraft_smlib::Brep::boolean_tagged_owned(a_in, b_in, kind);
            let outcome = match first {
                Ok(r) => Ok(r),
                Err(ref err)
                    if op == BoolOp::Union
                        && (ka.is_none() || kb.is_none())
                        && matches!(
                            err,
                            solidcraft_smlib::Error::Solidcore(msg)
                                if msg.contains("boolean::merge::p2::make_faceuse") && msg.contains("no radial edge use")
                        ) =>
                {
                    let retry_a = match ka {
                        Some(s) => Ok(s),
                        None => smlib_operand_ordered(a).map(|(s, _)| s),
                    };
                    let retry_b = match kb {
                        Some(s) => Ok(s),
                        None => smlib_operand_ordered(b).map(|(s, _)| s),
                    };
                    match (retry_a, retry_b) {
                        (Ok(ra), Ok(rb)) => match ra.boolean_tagged(&rb, kind) {
                            ok @ Ok(Some(_)) => ok,
                            Err(e @ solidcraft_smlib::Error::Cancelled(_)) => Err(e),
                            _ => first,
                        },
                        _ => first,
                    }
                }
                err => err,
            };
            let Some(r) = outcome.map_err(|e| failed("operation", e))? else {
                return Ok(None);
            };
            (r, None)
        };
        let (body, or) = body_from_smlib_ordered(&r)?;
        if let Some((sa, sb)) = check_operands {
            checked_decoded(body.record(), &[&sa, &sb])?;
        }
        let history = match (oa, ob, or) {
            (Some(oa), Some(ob), Some(or)) => r.history().map_err(|e| failed("history", e))?.history(&[oa, ob], &or).map(Some).unwrap_or_else(|e| {
                log::debug!(target: "smlib", "boolean history: {e}");
                None
            }),
            _ => None,
        };
        Ok(Some(left(with_history(body, history, a, b), step, &[a, b])))
    }

    /// The union of `bodies` in one SMLib call, each tagged as its own operand, checked once:
    /// the result's history is over `bodies` in order. At least one body.
    pub(crate) fn union_all(bodies: &[&Body]) -> Result<Body> {
        if bodies.len() > solidcraft_smlib::MAX_UNION_BREPS {
            return Err(KernelError::Invalid(format!("a union takes at most {} bodies", solidcraft_smlib::MAX_UNION_BREPS)));
        }
        let mut shared = Vec::with_capacity(bodies.len());
        for body in bodies {
            body.require_brep("a union")?;
            shared.push(smlib_shared_operand_ordered(body)?.map(made));
        }
        // As in [`boolean`]: shared where an operand has touching solids, else as before.
        if shared.iter().any(Option::is_some) {
            let mut operands = Vec::with_capacity(bodies.len());
            for (body, s) in bodies.iter().zip(shared) {
                operands.push(match s {
                    Some(s) => s,
                    None => smlib_operand_ordered(body)?,
                });
            }
            match unite(bodies, operands) {
                Ok(r) => return Ok(r),
                Err(e) => log::debug!(target: "smlib", "union with touching solids shared: {e}"),
            }
        }
        let operands = bodies.iter().map(|&body| smlib_operand_ordered(body)).collect::<Result<Vec<_>>>()?;
        unite(bodies, operands)
    }

    /// [`union_all`] of `bodies` as SMLib's `given`, in order, each with its numbering.
    fn unite(bodies: &[&Body], given: Vec<Operand>) -> Result<Body> {
        let mut operands = Vec::with_capacity(given.len());
        let mut orders = Some(Vec::with_capacity(given.len()));
        for (s, order) in given {
            operands.push(s);
            orders = orders.zip(order).map(|(mut v, o)| {
                v.push(o);
                v
            });
        }
        let operand_refs: Vec<&solidcraft_smlib::Brep> = operands.iter().map(|s| &**s).collect();
        // The operands tagged each as its own and united, the tagged copies consumed (solidcore#3359).
        let r = solidcraft_smlib::Brep::union_all_tagged(&operand_refs).map_err(|e| failed("operation", e))?;
        let step = crate::validation::step();
        let (body, or) = body_from_smlib_ordered(&r)?;
        if step == Step::Check {
            checked_decoded(body.record(), &operand_refs)?;
        }
        let history = match (orders, or) {
            (Some(orders), Some(or)) => r.history().map_err(|e| failed("history", e))?.history(&orders, &or).map(Some).unwrap_or_else(|e| {
                log::debug!(target: "smlib", "union history: {e}");
                None
            }),
            _ => None,
        };
        Ok(left(body.with_checked_history(history, &record::counts(bodies)), step, bodies))
    }

    /// `body`, the result of combining `operands`, marking its check for the end of its feature
    /// when the step left it there.
    fn left(body: Body, step: Step, operands: &[&Body]) -> Body {
        if step == Step::Defer { body.left_unchecked("SMLib boolean", operands) } else { body }
    }

    /// An error when SMLib's check refuses `record` as [`Body::smlib`] imports it (for a
    /// non-manifold result, already the separate solids export split it into; #22138).
    fn checked_decoded(record: &solidcore_record::brep::Brep, operands: &[&solidcraft_smlib::Brep]) -> Result<()> {
        let crate::validation::DecodedCheck::Refused(detail) = crate::validation::check_decoded("SMLib boolean", record)? else {
            return Ok(());
        };
        // Each operand is checked as a copy: a check edits the body it is given, and an operand may
        // be its body's own form, shared (`crate::native::smlib_operand_ordered`).
        Err(failed(&crate::validation::refuses(operands.iter().map(|s| s.try_clone().and_then(|c| c.check_owned(CheckLevel::All)))), detail))
    }

    /// `body` with `history` where it describes `body` made from `a` and `b`.
    fn with_history(body: Body, history: Option<History>, a: &Body, b: &Body) -> Body {
        body.with_checked_history(history, &record::counts(&[a, b]))
    }
}
