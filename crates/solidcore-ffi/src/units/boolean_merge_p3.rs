//! Guarded calls to the five `SmMerge` entry points of `boolean.merge_p3`.
//! The flat program is family, operation, option-mask, has-options, list lengths,
//! shell-vertex points or box origins/dimensions, tree length, tree nodes. Family 0 evaluates
//! trees, 1 folds a list, 2 combines lists, 3 nonmanifold Boolean, 4 piecewise,
//! 5 exports one fixture operand, and 6 runs a nonmanifold Boolean on a context-only
//! `SmMerge` with explicit members and at most one operand (status and controls).
//! Results begin with the native status; successful B-reps include stable input
//! identity and the complete `sc_topology_fixture` graph export.
use crate::{call_with_buffer, reader::Cursor, FfiError};
mod sys {
    extern "C" {
        pub fn sc_boolean_merge_p3_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}
fn bad(message: &str) -> FfiError {
    FfiError::InvalidArgument(message.into())
}
/// Validate all list and tree indices before constructing any native geometry.
/// Out-of-range leaves and binary operations on null operands are UB upstream;
/// repeated leaves without a later dereference, underflow and unknown operators
/// are safe native error paths and remain covered.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Cursor::new(input);
    let family = rd.index(6)?;
    let operation = rd.num()?;
    if !operation.is_finite() || operation.fract() != 0.0 || !(-64.0..=64.0).contains(&operation) {
        return Err(bad("operation outside harness integer range"));
    }
    let options = rd.index(31)?;
    let have_options = rd.flag()?;
    let na = rd.index(16)?;
    let nb = rd.index(16)?;
    if na + nb > 16
        || ((family == 3 || family == 4) && (na != 1 || nb != 1))
        || (family == 0 && nb != 0)
        || ((family == 3 || family == 4) && !(0.0..=10.0).contains(&operation))
        || (family == 5 && (na != 1 || nb != 0))
        || (family == 6 && (na > 1 || nb > 1 || na + nb > 1))
        || (family == 6 && !(0.0..=10.0).contains(&operation))
    {
        return Err(bad("operand list shape or enum"));
    }
    let mut other_has_surfaces = false;
    for operand in 0..na + nb {
        let shape = rd.index(1)?;
        if operand == na {
            other_has_surfaces = shape == 1;
        }
        rd.slice(3)?;
        if shape == 1 && rd.slice(3)?.iter().any(|&d| d <= 0.0) {
            return Err(bad("nonpositive box dimension"));
        }
    }
    // UB-190: boxes have multiple surfaces. The imprint early return deletes
    // the other operand during PARTIAL_MERGE, before the next surface reuses it.
    // A shell vertex has no surfaces, so its per-surface loop is never entered.
    if family == 4 && other_has_surfaces && have_options && options & 2 != 0 && options & 16 == 0 {
        return Err(bad("UB-190: piecewise imprinting requires keep-other"));
    }
    let nt = rd.index(128)?;
    if family != 0 && nt != 0 {
        return Err(bad("unexpected tree"));
    }
    let mut nodes = Vec::new();
    for _ in 0..nt {
        let node = rd.num()?;
        if !node.is_finite() || node.fract() != 0.0 || !(-64.0..=64.0).contains(&node) {
            return Err(bad("invalid tree token"));
        }
        nodes.push(node as i64);
    }
    validate_tree_nodes(na, &nodes)?;
    rd.done()
}
/// Shared native-tree guard: safe repeated leaves and definite early errors
/// remain valid; executed binary operations cannot consume null operands.
pub(crate) fn validate_tree_nodes(count: usize, nodes: &[i64]) -> Result<(), FfiError> {
    if count > 16 || nodes.len() > 128 {
        return Err(bad("tree resource limit"));
    }
    let mut used = vec![false; count];
    let mut stack = Vec::new();
    let mut active = true;
    for &node in nodes {
        if !(-64..=64).contains(&node) {
            return Err(bad("invalid tree token"));
        }
        if active {
            if node >= 0 {
                let flag = used
                    .get_mut(node as usize)
                    .ok_or_else(|| bad("tree leaf out of range"))?;
                stack.push(!*flag);
                *flag = true;
            } else if stack.len() < 2 {
                active = false;
            } else {
                let other = stack.pop().unwrap_or(false);
                let primary = stack.pop().unwrap_or(false);
                if !primary || !other {
                    return Err(bad("binary operation dereferences a null operand"));
                }
                if node < -6 {
                    active = false;
                } else {
                    stack.push(true);
                }
            }
        }
    }
    Ok(())
}
/// Run a validated program on the C++ reference. No native pointers escape.
pub fn eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(16384, |out, len| {
        // SAFETY: validation precedes native construction; every read is also
        // checked by the shim, and the output capacity is respected.
        unsafe {
            sys::sc_boolean_merge_p3_eval(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(nodes: &[f64]) -> Vec<f64> {
        let mut input = vec![
            0.0,
            0.0,
            0.0,
            0.0,
            2.0,
            0.0,
            0.0,
            1.0,
            2.0,
            3.0,
            0.0,
            4.0,
            5.0,
            6.0,
            nodes.len() as f64,
        ];
        input.extend(nodes);
        input
    }

    #[test]
    fn guard_distinguishes_defined_null_outputs_from_null_dereferences() {
        assert!(validate(&tree(&[0.0, 0.0])).is_ok());
        assert!(validate(&tree(&[0.0, 0.0, -1.0])).is_err());
        assert!(validate(&tree(&[64.0])).is_err());
        assert!(validate(&tree(&[-1.0, 64.0])).is_ok());
        assert!(validate(&tree(&[0.0, 1.0, -7.0, 64.0])).is_ok());
    }

    #[test]
    fn piecewise_imprinting_guard_checks_other_surfaces_and_effective_options() {
        for mask in 0..32 {
            for have_options in [false, true] {
                for primary_box in [false, true] {
                    for other_box in [false, true] {
                        let mut input = vec![
                            4.0,
                            0.0,
                            f64::from(mask),
                            f64::from(u8::from(have_options)),
                            1.0,
                            1.0,
                        ];
                        for (is_box, x) in [(primary_box, 0.0), (other_box, 2.0)] {
                            input.extend([f64::from(u8::from(is_box)), x, 0.0, 0.0]);
                            if is_box {
                                input.extend([1.0, 1.0, 1.0]);
                            }
                        }
                        input.push(0.0);
                        assert_eq!(
                            validate(&input).is_err(),
                            other_box && have_options && mask & 2 != 0 && mask & 16 == 0
                        );
                        input[0] = 3.0;
                        assert!(validate(&input).is_ok());
                    }
                }
            }
        }
    }
}
