//! FFI wrappers for unit `boolean.api_brep`.
//!
//! Shim: `harness/cpp/shim/boolean_api_brep.cpp` (`sc_boolean_api_brep_run`).
//! Upstream: `source/SM_API/src/SmApiBrep.cpp` (`SmApiBoolean`, `SmApiBooleanUnion`,
//! `SmApiBooleanDifference`, `SmApiBooleanIntersection`, `SmApiBooleanMerge`,
//! `SmApiMergeBreps`, `SmApiBooleanWithOptions`, `SmApiBooleanLists`,
//! `SmApiBooleanTrees`).
//!
//! A [`Call`] is encoded as `op args...`. An operand block ([`BrepSpec`]) is `kind`
//! then its data: `1 O[3] length width height` (`SmApiCreateBox`) or `2 C[3] radius`
//! (`SmApiCreateSphere`).
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
//!
//! The two-B-rep record is `has [is_first kept brep [brep]]` (the second B-rep when
//! upstream keeps it); a B-rep is `faces edges vertices regions manifold_solid`. A
//! rejected `MergeBreps` (`SM_ERR_INVALID_INPUT`) is followed by `cleared list_len`.
//!
//! [`run`] accepts only what upstream handles without undefined behaviour: finite
//! operand data with positive box dimensions and a nonzero sphere radius (a failing
//! constructor is `SC_ERR_ARG`); operation codes of `SmBooleanOperationType` (0 to 10);
//! at most [`MAX_LIST`] B-reps per list and [`MAX_TREE`] tree nodes in -64..=64; tree
//! B-rep nodes in range and used at most once (`SmMerge::BooleanTrees` indexes the list
//! unchecked and clears each entry it takes).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest B-rep list accepted by [`run`] (per list for [`Call::Lists`]).
pub const MAX_LIST: usize = 8;
/// Largest postfix tree accepted by [`run`].
pub const MAX_TREE: usize = 64;

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
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

fn check_spec(s: &BrepSpec) -> Result<(), FfiError> {
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
    check_spec(s)?;
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
    }
    Ok(())
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
            let mut used = vec![false; breps.len()];
            for &node in tree {
                if !(-64..=64).contains(&node) {
                    return Err(bad("tree node out of -64..=64"));
                }
                if let Ok(i) = usize::try_from(node) {
                    match used.get_mut(i) {
                        Some(u) if !*u => *u = true,
                        Some(_) => return Err(bad("tree uses a B-rep twice")),
                        None => return Err(bad("tree B-rep index out of range")),
                    }
                }
            }
            o.push(9.0);
            push_list(&mut o, breps, 2 * MAX_LIST)?;
            o.push(tree.len() as f64);
            o.extend(tree.iter().map(|&n| n as f64));
        }
    }
    Ok(o)
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
