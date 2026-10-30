//! The booleans: two bodies, many bodies, and planar sheets.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_ops.cpp` (`smlib_boolean`,
//! `smlib_boolean_with`, `smlib_boolean2d`, `smlib_boolean_union_all`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API (`SM_API` call) |
//! |------|-------|
//! | [`Smlib::boolean`], [`Smlib::boolean_with`] | `smlib_boolean`, `smlib_boolean_with` (`SmApiBoolean`) |
//! | [`Smlib::boolean_union_all`] | `smlib_boolean_union_all` (`SmApiBoolean`, pairwise) |
//! | [`Smlib::boolean2d`] | `smlib_boolean2d` (`SmApiBoolean2d`) |
//!
//! Deviations: the operands are deep copies (`SmApiBrepCopy`), as in the C API; the
//! port's booleans consume their operands and return the result, so there is no "result
//! is not A" case to guard.

use solidcore::boolean::api_brep::{boolean, boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::Brep;

use crate::session::{finish, Fail, Smlib};
use crate::status::{Result, Status};

/// `SMLIB_BOOLEAN_UNION`.
pub const BOOLEAN_UNION: i32 = 0;
/// `SMLIB_BOOLEAN_SUBTRACT`: a minus b.
pub const BOOLEAN_SUBTRACT: i32 = 1;
/// `SMLIB_BOOLEAN_INTERSECT`.
pub const BOOLEAN_INTERSECT: i32 = 2;
/// `SMLIB_RESULT_NONMANIFOLD`: a result may be closed but not manifold (solids touching
/// along an edge or at a vertex).
pub const RESULT_NONMANIFOLD: i32 = 1;
/// `SMLIB_BOOLEAN_MAX_BREPS`.
pub const BOOLEAN_MAX_BREPS: usize = 100_000;

/// The `SmBooleanOperationType` of an `SMLIB_BOOLEAN_*` code.
fn operation(op: i32) -> Option<BooleanOperation> {
    match op {
        BOOLEAN_UNION => Some(BooleanOperation::Union),
        BOOLEAN_SUBTRACT => Some(BooleanOperation::Difference),
        BOOLEAN_INTERSECT => Some(BooleanOperation::Intersection),
        _ => None,
    }
}

impl Smlib {
    /// `smlib_boolean`: `a` and `b` combined by `op` (`SMLIB_BOOLEAN_*`), with
    /// [`RESULT_NONMANIFOLD`]; both inputs are left unchanged.
    pub fn boolean(&mut self, a: &Brep, b: &Brep, op: i32) -> Result<Brep> {
        self.boolean_with(a, b, op, RESULT_NONMANIFOLD)
    }

    /// `smlib_boolean_with`: a new body, `a` and `b` combined by `op`. The empty set is
    /// `SMLIB_ERR_EMPTY`; solid inputs give a closed result (manifold, or with
    /// [`RESULT_NONMANIFOLD`] in `flags` solids touching along an edge or at a vertex),
    /// anything else is `SMLIB_ERR_NOT_SOLID`. Any other bit in `flags` is `SMLIB_ERR_ARG`.
    pub fn boolean_with(&mut self, a: &Brep, b: &Brep, op: i32, flags: i32) -> Result<Brep> {
        let Some(kind) = operation(op) else {
            return Err(Status::ERR_ARG);
        };
        if flags & !RESULT_NONMANIFOLD != 0 {
            return Err(Status::ERR_ARG);
        }
        let nonmanifold = flags & RESULT_NONMANIFOLD != 0;
        self.run(
            || format!("smlib_boolean(a, b, op={op}, flags={flags})"),
            &[("a", a), ("b", b)],
            |api| {
                let ca = api.copy(a)?;
                let cb = api.copy(b)?;
                let solid_in = brep_is_manifold_solid(&ca)? && brep_is_manifold_solid(&cb)?;
                let r = boolean(api.kernel_mut(), ca, cb, kind)?.result;
                finish(r, solid_in, nonmanifold)
            },
        )
    }

    /// `smlib_boolean_union_all`: the union of `breps` (1 to [`BOOLEAN_MAX_BREPS`]),
    /// inputs left unchanged, pairwise level by level (each body takes part in about
    /// `log2(n)` unions). The result is as [`Smlib::boolean`]'s.
    pub fn boolean_union_all(&mut self, breps: &[&Brep]) -> Result<Brep> {
        if breps.is_empty() || breps.len() > BOOLEAN_MAX_BREPS {
            return Err(Status::ERR_ARG);
        }
        let inputs: Vec<(&str, &Brep)> = breps
            .iter()
            .take(2)
            .enumerate()
            .map(|(i, b)| (if i == 0 { "first" } else { "second" }, *b))
            .collect();
        self.run(
            || format!("smlib_boolean_union_all(n={})", breps.len()),
            &inputs,
            |api| {
                let mut level = Vec::new();
                let mut solid_in = true;
                for b in breps {
                    let copy = api.copy(b)?;
                    solid_in = solid_in && brep_is_manifold_solid(&copy)?;
                    level.push(copy);
                }
                // Pairwise, level by level: the bodies of a level are unioned two by two,
                // an odd one carried to the next level as it is.
                while level.len() > 1 {
                    let mut next = Vec::with_capacity(level.len().div_ceil(2));
                    let mut bodies = level.into_iter();
                    while let Some(a) = bodies.next() {
                        if let Some(b) = bodies.next() {
                            let r = boolean(api.kernel_mut(), a, b, BooleanOperation::Union)?;
                            next.push(r.result);
                        } else {
                            next.push(a);
                        }
                    }
                    level = next;
                }
                let r = level.pop().ok_or(Fail::Status(Status::ERR_EMPTY))?;
                finish(r, solid_in, true)
            },
        )
    }

    /// `smlib_boolean2d`: a new sheet, `a` and `b` (sheets of planar faces on one plane)
    /// combined as regions of that plane by `op` (`SMLIB_BOOLEAN_*`). An input or a result
    /// without faces is `SMLIB_ERR_EMPTY`.
    pub fn boolean2d(&mut self, a: &Brep, b: &Brep, op: i32) -> Result<Brep> {
        let kind = match op {
            BOOLEAN_UNION => Boolean2d::Union,
            BOOLEAN_SUBTRACT => Boolean2d::Difference,
            BOOLEAN_INTERSECT => Boolean2d::Intersection,
            _ => return Err(Status::ERR_ARG),
        };
        self.run(
            || format!("smlib_boolean2d(a, b, op={op})"),
            &[("a", a), ("b", b)],
            |api| {
                let ca = api.copy(a)?;
                let cb = api.copy(b)?;
                // SmApiBoolean2d returns success without a result when an input has no faces.
                if ca.face_list().is_empty() || cb.face_list().is_empty() {
                    return Err(Fail::Status(Status::ERR_EMPTY));
                }
                match boolean_2d(api.kernel_mut(), ca, cb, kind)? {
                    Boolean2dOutcome::Result(r) => finish(r, false, false),
                    Boolean2dOutcome::Unassigned { .. } => Err(Fail::Status(Status::ERR_EMPTY)),
                }
            },
        )
    }
}
