//! FFI wrappers for unit `boolean.api_brep`.
//!
//! Shim: `harness/cpp/shim/boolean_api_brep.cpp` (`sc_boolean_api_brep_run`).
//! Upstream: `source/SM_API/src/SmApiBrep.cpp` (`SmApiBoolean`, `SmApiBooleanUnion`,
//! `SmApiBooleanDifference`, `SmApiBooleanIntersection`, `SmApiBooleanMerge`,
//! `SmApiMergeBreps`, `SmApiBooleanWithOptions`, `SmApiBooleanLists`,
//! `SmApiBooleanTrees`).
//!
//! A [`Call`] is encoded as `op args...`. An operand block ([`BrepSpec`]) is `kind`
//! then its data: `1 O[3] length width height` (`SmApiCreateBox`), `2 C[3] radius`
//! (`SmApiCreateSphere`), `3 O[3] x y` (`SmApiCreatePlane`, the `SmBrep*&` overload),
//! `4 operation brep brep` (the `SmApiBoolean` result of two operand blocks) or
//! `5 C[3] major minor` (`SmApiCreateTorus`, the `SmBrep*&` overload). Nested
//! blocks are at most [`MAX_DEPTH`] deep.
//!
//! | [`Call`]        | op | args                                   | output after the status (on success)            |
//! |-----------------|----|----------------------------------------|-------------------------------------------------|
//! | `Boolean`       | 1  | brep brep `operation`                  | two-B-rep record                                |
//! | `Union`         | 2  | brep brep                              | two-B-rep record                                |
//! | `Difference`    | 3  | brep brep                              | two-B-rep record                                |
//! | `Intersection`  | 4  | brep brep                              | two-B-rep record                                |
//! | `Merge`         | 5  | brep brep                              | two-B-rep record                                |
//! | `WithOptions`   | 6  | brep brep `operation` 5 flags          | two-B-rep record                                |
//! | `MergeBreps`    | 7  | `n`, `n` breps, `operation`            | `list_len has [brep]`                           |
//! | `Lists`         | 8  | `n1`, breps, `n2`, breps, `operation`  | `nb`, `nb` x `has [brep]`, `ns`                 |
//! | `Trees`         | 9  | `n`, breps, `t`, `t` nodes             | `nb`, `nb` x `has [brep]`                       |
//! | `Measured`      | 10 | brep brep `operation` `post_process`   | two-B-rep record, then the measures             |
//!
//! The two-B-rep record is `has [is_first kept brep [brep]]` (the second B-rep when
//! upstream keeps it); a B-rep is `faces edges vertices regions manifold_solid`, then per
//! region (`GetRegions` order) `is_void is_infinite shells` and per shell (`GetShells` order)
//! `faceuses mate_region`, the `GetRegions` index of the region behind the shell's first
//! faceuse's mate (`-1` without one). A
//! rejected `MergeBreps` (`SM_ERR_INVALID_INPUT`) is followed by `cleared list_len`.
//! A failing `SmApiCreateTorus` operand (in any call) makes the whole record its status
//! then `2`, so both sides' radius guards are compared.
//! `Measured` is `SmApiBooleanWithOptions` with only `bBooleanPostProcess` chosen; on
//! success its record ends with the result's measures `volume_status volume faces areas...`
//! (`SmApiBrepComputeVolume` at [`VOLUME_ACCURACY`], then each face's `SmApiFaceComputeArea`
//! at [`AREA_ACCURACY`] in ascending order, a failed face as minus its status) and the
//! number of those values last ([`split_measures`]).
//!
//! [`run`] accepts only what upstream handles without undefined behaviour: finite
//! operand data with positive box and plane dimensions and a nonzero sphere radius (torus
//! radii are not limited: `SmTorus::CreateCanonical` fails cleanly for a radius
//! `<= SM_EFF_ZERO` and accepts a spindle torus, `minor >= major`; any other failing
//! constructor or operand Boolean is `SC_ERR_ARG`); operand Booleans that are union, intersection, difference
//! or slice (upstream deletes their second operand); operation codes of `SmBooleanOperationType` (0 to 10);
//! at most [`MAX_LIST`] B-reps per list and [`MAX_TREE`] tree nodes in -64..=64; tree
//! executed B-rep nodes in range and binary operands non-null (`SmMerge::BooleanTrees`
//! indexes unchecked and clears each leaf slot). Safe repeated leaves remain accepted.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest B-rep list accepted by [`run`] (per list for [`Call::Lists`]).
pub const MAX_LIST: usize = 8;
/// Largest postfix tree accepted by [`run`].
pub const MAX_TREE: usize = 64;
/// Deepest operand block accepted by [`run`] (a plain primitive is depth 1).
pub const MAX_DEPTH: usize = 4;
/// `dRelativeAccuracy` of the [`Call::Measured`] volume (`SmApiBrepComputeVolume`).
pub const VOLUME_ACCURACY: f64 = 1e-6;
/// `dRelativeAccuracy` of the [`Call::Measured`] face areas (`SmApiFaceComputeArea`).
pub const AREA_ACCURACY: f64 = 1e-4;

mod sys {
    extern "C" {
        pub fn sc_boolean_api_brep_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// How an operand B-rep is built on the C++ side.
#[derive(Debug, Clone, PartialEq)]
pub enum BrepSpec {
    /// `SmApiCreateBox(origin, length, width, height)`.
    Box {
        /// Lower-left corner.
        origin: [f64; 3],
        /// Extents along x, y, z.
        size: [f64; 3],
    },
    /// `SmApiCreateSphere(center, radius)`.
    Sphere {
        /// Center.
        center: [f64; 3],
        /// Radius (nonzero; negative is accepted upstream).
        radius: f64,
    },
    /// `SmApiCreatePlane(origin, x, y)`: a rectangular sheet in the XY plane.
    Plane {
        /// Lower-left corner.
        origin: [f64; 3],
        /// Extents along x and y.
        size: [f64; 2],
    },
    /// `SmApiCreateTorus(center, major, minor)` (axis +Z).
    Torus {
        /// Center.
        center: [f64; 3],
        /// Major (ring) radius.
        major: f64,
        /// Minor (tube) radius.
        minor: f64,
    },
    /// `SmApiBoolean(a, b, operation)`'s result.
    Boolean {
        /// `SmBooleanOperationType` value: union, intersection, difference or slice.
        operation: i32,
        /// First operand (modified into the result).
        a: Box<BrepSpec>,
        /// Second operand (deleted by upstream).
        b: Box<BrepSpec>,
    },
}

/// The merge option flags of `SmApiBooleanWithOptions`, in upstream argument order.
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean arguments
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    /// `bCookieCutter`.
    pub cookie_cutter: bool,
    /// `bImprinting`.
    pub imprinting: bool,
    /// `bBooleanPostProcess`.
    pub boolean_post_process: bool,
    /// `bImprintAndClassifyFaces`.
    pub imprint_and_classify_faces: bool,
    /// `bKeepOtherBrep`.
    pub keep_other_brep: bool,
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiBoolean`.
    Boolean {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
    },
    /// `SmApiBooleanUnion`.
    Union {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanDifference`.
    Difference {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanIntersection`.
    Intersection {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanMerge`.
    Merge {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanWithOptions`.
    WithOptions {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
        /// Merge flags.
        flags: Flags,
    },
    /// `SmApiMergeBreps`.
    MergeBreps {
        /// The list.
        breps: Vec<BrepSpec>,
        /// `SmBooleanOperationType` value.
        operation: i32,
    },
    /// `SmApiBooleanLists` (empty surface lists).
    Lists {
        /// First list.
        breps1: Vec<BrepSpec>,
        /// Second list.
        breps2: Vec<BrepSpec>,
        /// `0` to `4` (any `int` is accepted upstream).
        operation: i32,
    },
    /// `SmApiBooleanTrees`.
    Trees {
        /// The B-reps.
        breps: Vec<BrepSpec>,
        /// Postfix nodes: `>= 0` a B-rep index, `< 0` an operation.
        tree: Vec<i64>,
    },
    /// `SmApiBooleanWithOptions` with only `bBooleanPostProcess` chosen, followed by the
    /// result's volume and face areas.
    Measured {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
        /// `bBooleanPostProcess`.
        post_process: bool,
    },
}

fn check_spec(s: &BrepSpec, depth: usize) -> Result<(), FfiError> {
    if depth >= MAX_DEPTH {
        return Err(bad("operand nested too deep"));
    }
    match s {
        BrepSpec::Box { origin, size } => {
            if !origin.iter().chain(size).all(|x| x.is_finite()) {
                return Err(bad("box: non-finite value"));
            }
            if size.iter().any(|&x| x <= 0.0) {
                return Err(bad("box: dimensions must be positive"));
            }
        }
        BrepSpec::Sphere { center, radius } => {
            if !center.iter().all(|x| x.is_finite()) || !radius.is_finite() {
                return Err(bad("sphere: non-finite value"));
            }
            if *radius == 0.0 {
                return Err(bad("sphere: zero radius"));
            }
        }
        BrepSpec::Plane { origin, size } => {
            if !origin.iter().chain(size).all(|x| x.is_finite()) {
                return Err(bad("plane: non-finite value"));
            }
            if size.iter().any(|&x| x <= 0.0) {
                return Err(bad("plane: dimensions must be positive"));
            }
        }
        BrepSpec::Torus {
            center,
            major,
            minor,
        } => {
            if !center.iter().all(|x| x.is_finite()) || !major.is_finite() || !minor.is_finite()
            {
                return Err(bad("torus: non-finite value"));
            }
        }
        BrepSpec::Boolean { operation, a, b } => {
            if ![1, 2, 3, 10].contains(operation) {
                return Err(bad(
                    "operand boolean: not union, intersection, difference or slice",
                ));
            }
            check_spec(a, depth + 1)?;
            check_spec(b, depth + 1)?;
        }
    }
    Ok(())
}

fn check_op(op: i32) -> Result<(), FfiError> {
    if (0..=10).contains(&op) {
        Ok(())
    } else {
        Err(bad("operation is not an SmBooleanOperationType value"))
    }
}

fn push_spec(o: &mut Vec<f64>, s: &BrepSpec) -> Result<(), FfiError> {
    check_spec(s, 0)?;
    push_checked(o, s);
    Ok(())
}

fn push_checked(o: &mut Vec<f64>, s: &BrepSpec) {
    match s {
        BrepSpec::Box { origin, size } => {
            o.push(1.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(size);
        }
        BrepSpec::Sphere { center, radius } => {
            o.push(2.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        BrepSpec::Plane { origin, size } => {
            o.push(3.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(size);
        }
        BrepSpec::Torus {
            center,
            major,
            minor,
        } => {
            o.push(5.0);
            o.extend_from_slice(center);
            o.push(*major);
            o.push(*minor);
        }
        BrepSpec::Boolean { operation, a, b } => {
            o.push(4.0);
            o.push(f64::from(*operation));
            push_checked(o, a);
            push_checked(o, b);
        }
    }
}

fn push_list(o: &mut Vec<f64>, l: &[BrepSpec], max: usize) -> Result<(), FfiError> {
    if l.len() > max {
        return Err(bad(format!("more than {max} B-reps")));
    }
    o.push(l.len() as f64);
    for s in l {
        push_spec(o, s)?;
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Boolean { a, b, operation } => {
            check_op(*operation)?;
            o.push(1.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
        }
        Call::Union { a, b }
        | Call::Difference { a, b }
        | Call::Intersection { a, b }
        | Call::Merge { a, b } => {
            let op = match call {
                Call::Union { .. } => 2.0,
                Call::Difference { .. } => 3.0,
                Call::Intersection { .. } => 4.0,
                _ => 5.0,
            };
            o.push(op);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
        }
        Call::WithOptions {
            a,
            b,
            operation,
            flags,
        } => {
            check_op(*operation)?;
            o.push(6.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
            o.extend([
                flag(flags.cookie_cutter),
                flag(flags.imprinting),
                flag(flags.boolean_post_process),
                flag(flags.imprint_and_classify_faces),
                flag(flags.keep_other_brep),
            ]);
        }
        Call::MergeBreps { breps, operation } => {
            check_op(*operation)?;
            o.push(7.0);
            push_list(&mut o, breps, 2 * MAX_LIST)?;
            o.push(f64::from(*operation));
        }
        Call::Lists {
            breps1,
            breps2,
            operation,
        } => {
            o.push(8.0);
            push_list(&mut o, breps1, MAX_LIST)?;
            push_list(&mut o, breps2, MAX_LIST)?;
            o.push(f64::from(*operation));
        }
        Call::Trees { breps, tree } => {
            if tree.len() > MAX_TREE {
                return Err(bad("tree too long"));
            }
            super::boolean_merge_p3::validate_tree_nodes(breps.len(), tree)?;
            o.push(9.0);
            push_list(&mut o, breps, 2 * MAX_LIST)?;
            o.push(tree.len() as f64);
            o.extend(tree.iter().map(|&n| n as f64));
        }
        Call::Measured {
            a,
            b,
            operation,
            post_process,
        } => {
            check_op(*operation)?;
            o.push(10.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
            o.push(flag(*post_process));
        }
    }
    Ok(o)
}

/// Splits a successful [`Call::Measured`] record into the integer part before the
/// measures and the measures themselves (`volume_status volume faces areas...`), using the
/// count the shim appends last. `None` for a record too short for its count.
#[must_use]
pub fn split_measures(record: &[f64]) -> Option<(&[f64], &[f64])> {
    let (&n, rest) = record.split_last()?;
    if !(n >= 0.0 && n.fract() == 0.0) {
        return None;
    }
    let n = n as usize;
    let at = rest.len().checked_sub(n)?;
    Some(rest.split_at(at))
}

/// Runs one API call on the C++ side. Returns the flat output record (status first,
/// see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs upstream cannot take safely;
/// [`FfiError::Status`] for a shim failure (`SC_ERR_ARG`: an operand constructor
/// failed).
pub fn run(call: &Call) -> Result<Vec<f64>, FfiError> {
    let input = encode(call)?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: input is valid for input.len() reads; buf for buf.len() writes; len is
        // a valid i64. The shim bounds-checks every read and copies at most buf.len().
        unsafe {
            sys::sc_boolean_api_brep_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trees_accept_defined_null_slots_and_guard_only_reachable_dereferences() {
        let shape = BrepSpec::Box {
            origin: [0.0; 3],
            size: [1.0; 3],
        };
        let program = |tree| Call::Trees {
            breps: vec![shape.clone(), shape.clone()],
            tree,
        };
        assert!(encode(&program(vec![0, 0])).is_ok());
        assert!(encode(&program(vec![0, 0, -1])).is_err());
        assert!(encode(&program(vec![-1, 64])).is_ok());
    }

    #[test]
    fn operand_blocks_nest_only_to_max_depth() {
        let mut spec = BrepSpec::Box {
            origin: [0.0; 3],
            size: [1.0; 3],
        };
        let call = |a: &BrepSpec| Call::Union {
            a: a.clone(),
            b: a.clone(),
        };
        for depth in 1..=MAX_DEPTH {
            assert!(encode(&call(&spec)).is_ok(), "depth {depth}");
            spec = BrepSpec::Boolean {
                operation: 1,
                a: Box::new(spec.clone()),
                b: Box::new(spec),
            };
        }
        assert!(encode(&call(&spec)).is_err());
        let slice = |operation| BrepSpec::Boolean {
            operation,
            a: Box::new(BrepSpec::Sphere {
                center: [0.0; 3],
                radius: 1.0,
            }),
            b: Box::new(BrepSpec::Plane {
                origin: [0.0; 3],
                size: [1.0, 1.0],
            }),
        };
        assert!(encode(&call(&slice(10))).is_ok());
        assert!(encode(&call(&slice(6))).is_err());
    }
}
